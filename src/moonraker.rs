use crate::logging::Change;
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde_json::{Map, Value, json};
use std::{collections::HashMap, sync::mpsc, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{mpsc as async_mpsc, watch},
    time::{self, Instant},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct Call {
    pub method: &'static str,
    pub params: Value,
    pub tag: String,
    changes: Vec<Change>,
}

impl Call {
    pub fn new(method: &'static str, params: Value, tag: impl Into<String>) -> Self {
        Self {
            method,
            params,
            tag: tag.into(),
            changes: Vec::new(),
        }
    }

    pub fn with_changes(mut self, changes: Vec<Change>) -> Self {
        self.changes = changes;
        self
    }
}

struct Pending {
    tag: String,
    method: &'static str,
    sent: Instant,
    deadline: Instant,
    changes: Vec<Change>,
}

fn log_address(url: &Url) -> String {
    let host = url.host_str().unwrap_or("unknown");
    match url.port_or_known_default() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

fn connection_error(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Io(error) => format!("I/O: {}", error.kind()),
        Error::Http(response) => format!("HTTP {}", response.status().as_u16()),
        Error::Tls(_) => "TLS handshake failed".into(),
        Error::Protocol(_) => "WebSocket protocol error".into(),
        _ => "WebSocket connection failed".into(),
    }
}

fn session_error(error: &str) -> &str {
    match error {
        "Moonraker stopped responding" => "server stopped responding",
        "Moonraker closed the connection" => "server closed the connection",
        _ => "WebSocket session lost",
    }
}

fn record_success(method: &str) -> bool {
    method.starts_with("printer.")
        && !matches!(method, "printer.objects.list" | "printer.objects.subscribe")
        || method.starts_with("machine.update.") && method != "machine.update.status"
        || matches!(
            method,
            "server.files.delete_file"
                | "server.files.delete_directory"
                | "server.files.move"
                | "server.files.post_directory"
                | "server.job_queue.post_job"
                | "server.job_queue.delete_job"
                | "server.job_queue.start"
                | "server.job_queue.pause"
                | "machine.reboot"
                | "machine.shutdown"
        )
}

pub enum Event {
    Connected,
    Disconnected(String),
    Status(Map<String, Value>),
    Reply(String, Result<Value, String>),
    Notification(String, Value),
}

pub fn endpoint(host: &str) -> Result<Url, String> {
    let host = host.trim();
    let value = if host.contains("://") {
        host.to_owned()
    } else {
        format!("ws://{host}")
    };
    let mut url = Url::parse(&value).map_err(|e| e.to_string())?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        _ => return Err("Moonraker URL must use HTTP, HTTPS, WS, or WSS".into()),
    };
    url.set_scheme(scheme)
        .map_err(|_| "Invalid Moonraker URL")?;
    let path = url.path().trim_end_matches('/');
    let path = if path.ends_with("/websocket") {
        path.to_owned()
    } else {
        format!("{path}/websocket")
    };
    url.set_path(&path);
    Ok(url)
}

pub fn http_endpoint(host: &str, segments: &[&str]) -> Result<Url, String> {
    let mut url = endpoint(host)?;
    let scheme = if url.scheme() == "wss" {
        "https"
    } else {
        "http"
    };
    url.set_scheme(scheme)
        .map_err(|_| "Invalid Moonraker URL")?;
    url.path_segments_mut()
        .map_err(|_| "Invalid Moonraker URL")?
        .pop()
        .extend(segments);
    Ok(url)
}

pub async fn run(
    mut endpoint_rx: watch::Receiver<String>,
    mut calls: async_mpsc::Receiver<Call>,
    output: mpsc::Sender<Event>,
) {
    let mut last_failure = None::<(String, Instant)>;
    loop {
        let host = endpoint_rx.borrow_and_update().clone();
        let mut failure = String::new();
        let mut address = None;
        let connection = async {
            let url = endpoint(&host).inspect_err(|_| failure = "invalid endpoint".into())?;
            let label = log_address(&url);
            if last_failure.is_none() {
                log::info!("connecting to {label}");
            }
            address = Some(label);
            let (socket, _) = time::timeout(
                Duration::from_secs(8),
                tokio_tungstenite::connect_async(url.as_str()),
            )
            .await
            .map_err(|_| {
                failure = "connection timed out".into();
                "Moonraker connection timed out".to_owned()
            })?
            .map_err(|error| {
                failure = connection_error(&error);
                error.to_string()
            })?;
            last_failure = None;
            session(
                socket,
                &mut calls,
                &output,
                address.as_deref().unwrap_or("unknown"),
            )
            .await
            .inspect_err(|error| failure = session_error(error).into())
        };
        let reason = tokio::select! {
            reason = connection => reason,
            changed = endpoint_rx.changed() => {
                if changed.is_err() { return; }
                failure.clear();
                log::info!("connection interrupted by reconnect request or endpoint change");
                last_failure = None;
                Err("Moonraker address changed".to_owned())
            }
        };
        if !failure.is_empty()
            && last_failure.as_ref().is_none_or(|(previous, since)| {
                previous != &failure || since.elapsed() >= Duration::from_secs(60)
            })
        {
            log::warn!(
                "disconnected from {}: {failure}; retrying",
                address.as_deref().unwrap_or("unconfigured endpoint")
            );
            last_failure = Some((failure, Instant::now()));
        }
        while calls.try_recv().is_ok() {}
        if output
            .send(Event::Disconnected(reason.err().unwrap_or_default()))
            .is_err()
        {
            return;
        }

        let retry = time::sleep(Duration::from_secs(3));
        tokio::pin!(retry);
        loop {
            tokio::select! {
                _ = &mut retry => break,
                changed = endpoint_rx.changed() => {
                    if changed.is_err() { return; }
                    last_failure = None;
                    break;
                }
                call = calls.recv() => {
                    let Some(call) = call else { return; };
                    let _ = output.send(Event::Reply(call.tag, Err("Moonraker is disconnected".into())));
                }
            }
        }
    }
}

async fn session(
    mut socket: Socket,
    calls: &mut async_mpsc::Receiver<Call>,
    output: &mpsc::Sender<Event>,
    address: &str,
) -> Result<(), String> {
    let mut id = 0u64;
    let mut pending = HashMap::new();
    let mut ping = time::interval(Duration::from_secs(5));
    let mut last_received = Instant::now();
    let mut status = Map::new();
    let mut connected = false;

    send(
        &mut socket,
        &mut id,
        &mut pending,
        Call::new("server.info", json!({}), "@server"),
    )
    .await?;
    loop {
        tokio::select! {
            call = calls.recv() => {
                let Some(call) = call else { return Ok(()); };
                if connected || call.method == "printer.emergency_stop" {
                    send(&mut socket, &mut id, &mut pending, call).await?;
                } else {
                    let _ = output.send(Event::Reply(call.tag, Err("Moonraker is connecting".into())));
                }
            }
            _ = ping.tick() => {
                let now = Instant::now();
                if now.duration_since(last_received) > Duration::from_secs(20) {
                    return Err("Moonraker stopped responding".into());
                }
                let mut retry_subscription = false;
                pending.retain(|_, request| {
                    if now < request.deadline { return true; }
                    log::warn!("request method={} timed out", request.method);
                    if request.tag.starts_with('@') { retry_subscription = true; }
                    else { let _ = output.send(Event::Reply(request.tag.clone(), Err("Moonraker request timed out".into()))); }
                    false
                });
                let awaiting_klippy = connected && matches!(
                    status.get("webhooks").and_then(|webhooks: &Value| webhooks["state"].as_str()),
                    Some("startup" | "disconnected")
                );
                if (retry_subscription || awaiting_klippy)
                    && !pending.values().any(|request| request.tag == "@server")
                {
                    send(&mut socket, &mut id, &mut pending, Call::new("server.info", json!({}), "@server")).await?;
                }
                socket.send(Message::Ping(Default::default())).await.map_err(|e| e.to_string())?;
            }
            message = socket.next() => {
                let message = message.ok_or("Moonraker closed the connection")?.map_err(|e| e.to_string())?;
                last_received = Instant::now();
                let Message::Text(text) = message else {
                    if matches!(message, Message::Close(_)) { return Err("Moonraker closed the connection".into()); }
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue; };
                if let Some(request) = value["id"].as_u64().and_then(|id| pending.remove(&id)) {
                    let Pending { tag, method, sent, changes, .. } = request;
                    let elapsed = sent.elapsed().as_millis();
                    let result = if let Some(error) = value["error"]["message"].as_str() {
                        Err(error.to_owned())
                    } else {
                        Ok(value["result"].clone())
                    };
                    if result.is_err() {
                        let code = value["error"]["code"].as_i64();
                        log::warn!("request method={method} failed code={code:?} elapsed_ms={elapsed}");
                        for change in changes {
                            log::warn!(target: "moondash::settings", "Could not confirm {} change", change.label);
                        }
                    } else {
                        if record_success(method) {
                            log::info!("request method={method} completed elapsed_ms={elapsed}");
                        }
                        for change in changes {
                            change.record();
                        }
                    }
                    match tag.as_str() {
                        "@server" => {
                            let result = result?;
                            if !connected {
                                connected = true;
                                log::info!("connected to {address}");
                                let _ = output.send(Event::Connected);
                            }
                            let klippy_state = result["klippy_state"].as_str().unwrap_or("disconnected");
                            if status.get("webhooks").and_then(|webhooks| webhooks["state"].as_str()) != Some(klippy_state) {
                                status.insert("webhooks".into(), json!({"state": klippy_state}));
                            }
                            let _ = output.send(Event::Status(status.clone()));
                            if klippy_state != "disconnected" {
                                send(&mut socket, &mut id, &mut pending, Call::new("printer.objects.list", json!({}), "@objects")).await?;
                            }
                        }
                        "@objects" => {
                            if let Ok(objects) = result {
                                let objects = objects["objects"].as_array().into_iter().flatten()
                                    .filter_map(Value::as_str).map(|name| (name.to_owned(), Value::Null)).collect::<Map<_, _>>();
                                send(&mut socket, &mut id, &mut pending, Call::new("printer.objects.subscribe", json!({"objects": objects}), "@subscribe")).await?;
                            }
                        }
                        "@subscribe" => {
                            if let Ok(result) = result {
                                update_status(&mut status, &result["status"]);
                                let _ = output.send(Event::Status(status.clone()));
                            }
                        }
                        _ => { let _ = output.send(Event::Reply(tag, result)); }
                    }
                } else if let Some(method) = value["method"].as_str() {
                    let payload = &value["params"][0];
                    match method {
                        "notify_status_update" => {
                            update_status(&mut status, payload);
                            let _ = output.send(Event::Status(status.clone()));
                        }
                        "notify_klippy_ready" => {
                            status.clear();
                            send(&mut socket, &mut id, &mut pending, Call::new("server.info", json!({}), "@server")).await?;
                        }
                        "notify_klippy_disconnected" | "notify_klippy_shutdown" => {
                            status.clear();
                            status.insert("webhooks".into(), json!({
                                "state": if method.ends_with("shutdown") { "shutdown" } else { "disconnected" }
                            }));
                            let _ = output.send(Event::Status(status.clone()));
                        }
                        _ => { let _ = output.send(Event::Notification(method.to_owned(), payload.clone())); }
                    }
                }
            }
        }
    }
}

async fn send(
    socket: &mut Socket,
    id: &mut u64,
    pending: &mut HashMap<u64, Pending>,
    call: Call,
) -> Result<(), String> {
    *id += 1;
    let text = json!({"jsonrpc": "2.0", "id": *id, "method": call.method, "params": call.params});
    socket
        .send(Message::Text(text.to_string().into()))
        .await
        .map_err(|e| e.to_string())?;
    let timeout = match call.method {
        "machine.update.refresh" => 60,
        "machine.update.upgrade" | "machine.update.full" | "printer.gcode.script" => 900,
        _ => 20,
    };
    let sent = Instant::now();
    pending.insert(
        *id,
        Pending {
            tag: call.tag,
            method: call.method,
            sent,
            deadline: sent + Duration::from_secs(timeout),
            changes: call.changes,
        },
    );
    Ok(())
}

fn update_status(status: &mut Map<String, Value>, delta: &Value) {
    let Some(objects) = delta.as_object() else {
        return;
    };
    for (name, object) in objects {
        let previous = status.entry(name).or_insert_with(|| json!({}));
        if let (Some(previous), Some(fields)) = (previous.as_object_mut(), object.as_object()) {
            previous.extend(fields.clone());
        } else {
            *previous = object.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_and_http_urls_preserve_reverse_proxy_prefix() {
        let host = "https://printer.local/moonraker/websocket/";
        assert_eq!(
            endpoint(host).unwrap().as_str(),
            "wss://printer.local/moonraker/websocket"
        );
        assert_eq!(
            http_endpoint(host, &["server", "files", "gcodes", "part #1.png"])
                .unwrap()
                .as_str(),
            "https://printer.local/moonraker/server/files/gcodes/part%20%231.png"
        );
    }

    #[test]
    fn diagnostic_address_omits_url_credentials_path_and_query() {
        let url = endpoint("https://operator:secret@printer.local/proxy?token=secret").unwrap();
        assert_eq!(log_address(&url), "wss://printer.local:443");
    }

    #[tokio::test]
    async fn subscribes_to_status_and_sends_user_commands() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut subscribed = false;
            loop {
                let message = socket.next().await.unwrap().unwrap();
                let Message::Text(text) = message else {
                    continue;
                };
                let request: Value = serde_json::from_str(&text).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "server.info" => json!({"klippy_state": "ready"}),
                    "printer.objects.list" => {
                        json!({"objects": ["print_stats", "AFC_stepper lane1"]})
                    }
                    "printer.objects.subscribe" => {
                        assert_eq!(
                            request["params"]["objects"],
                            json!({"print_stats": null, "AFC_stepper lane1": null})
                        );
                        subscribed = true;
                        json!({"status": {"print_stats": {"state": "printing", "filename": "part.gcode"}}})
                    }
                    "printer.print.pause" => {
                        assert!(subscribed);
                        json!("ok")
                    }
                    method => panic!("Unexpected RPC method {method}"),
                };
                let response = json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
                if request["method"] == "printer.objects.subscribe" {
                    let update = json!({"jsonrpc": "2.0", "method": "notify_status_update", "params": [
                        {"print_stats": {"state": "paused"}}, 12.5
                    ]});
                    socket
                        .send(Message::Text(update.to_string().into()))
                        .await
                        .unwrap();
                }
                if request["method"] == "printer.print.pause" {
                    return;
                }
            }
        });

        let (_endpoint, endpoint_rx) = watch::channel(address.to_string());
        let (requests, requests_rx) = async_mpsc::channel(4);
        let (events, events_rx) = mpsc::channel();
        let client = tokio::spawn(run(endpoint_rx, requests_rx, events));
        tokio::task::spawn_blocking(move || {
            loop {
                match events_rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                    Event::Status(status)
                        if status
                            .get("print_stats")
                            .is_some_and(|stats| stats["state"] == "paused") =>
                    {
                        assert_eq!(status["print_stats"]["filename"], "part.gcode");
                        requests
                            .try_send(Call::new("printer.print.pause", json!({}), "pause"))
                            .unwrap();
                    }
                    Event::Reply(tag, result) => {
                        assert_eq!(tag, "pause");
                        assert_eq!(result.unwrap(), json!("ok"));
                        break;
                    }
                    Event::Disconnected(reason) => panic!("{reason}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        server.await.unwrap();
        client.abort();
    }
}
