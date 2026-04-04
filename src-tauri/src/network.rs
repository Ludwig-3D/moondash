use get_if_addrs::{get_if_addrs, IfAddr};
use nmrs::{Device, Network as NmNetwork, NetworkManager, WifiSecurity};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::process::Command;
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    ethernet_connected: bool,
    wifi_connected: bool,
    ssid: Option<String>,
    signal_percent: Option<u8>,
    quality: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    ssid: String,
    secured: bool,
    saved: bool,
    signal_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiSettings {
    enabled: bool,
    connected_ssid: Option<String>,
    connected_ip: Option<String>,
    saved_networks: Vec<WifiNetwork>,
    scanned_networks: Vec<WifiNetwork>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WiredInterface {
    interface_name: String,
    connected: bool,
    ip: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WiredSettings {
    interfaces: Vec<WiredInterface>,
}

fn quality_label(percent: u8) -> String {
    match percent {
        80..=100 => "excellent",
        60..=79 => "good",
        40..=59 => "ok",
        1..=39 => "weak",
        _ => "none",
    }
    .to_string()
}

fn disconnected() -> NetworkStatus {
    NetworkStatus {
        ethernet_connected: false,
        wifi_connected: false,
        ssid: None,
        signal_percent: None,
        quality: None,
    }
}

fn ethernet_only() -> NetworkStatus {
    NetworkStatus {
        ethernet_connected: true,
        wifi_connected: false,
        ssid: None,
        signal_percent: None,
        quality: None,
    }
}

fn wifi_only(ssid: String, signal_percent: Option<u8>) -> NetworkStatus {
    NetworkStatus {
        ethernet_connected: false,
        wifi_connected: true,
        ssid: Some(ssid),
        signal_percent,
        quality: signal_percent.map(quality_label),
    }
}

fn ethernet_and_wifi(ssid: String, signal_percent: Option<u8>) -> NetworkStatus {
    NetworkStatus {
        ethernet_connected: true,
        wifi_connected: true,
        ssid: Some(ssid),
        signal_percent,
        quality: signal_percent.map(quality_label),
    }
}

#[cfg(target_os = "linux")]
async fn nm() -> Result<NetworkManager, String> {
    NetworkManager::new().await.map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn is_wifi_device(device: &Device) -> bool {
    let kind = format!("{}", device.device_type).to_lowercase();
    kind.contains("wifi") || kind.contains("wireless")
}

#[cfg(target_os = "linux")]
fn is_wired_device(device: &Device) -> bool {
    let kind = format!("{}", device.device_type).to_lowercase();
    kind.contains("ethernet") || kind.contains("wired")
}

#[cfg(target_os = "linux")]
fn is_active_device(device: &Device) -> bool {
    let state = format!("{}", device.state).to_lowercase();
    state.contains("activated") || state.contains("connected")
}

#[cfg(target_os = "linux")]
fn active_wifi_interface_name(devices: &[Device]) -> Option<String> {
    devices
        .iter()
        .find(|device| {
            is_wifi_device(device)
                && is_active_device(device)
                && !device.interface.trim().is_empty()
        })
        .map(|device| device.interface.clone())
        .or_else(|| {
            devices
                .iter()
                .find(|device| is_wifi_device(device) && !device.interface.trim().is_empty())
                .map(|device| device.interface.clone())
        })
}

#[cfg(target_os = "linux")]
fn ipv4_for_interface(interface_name: &str) -> Option<String> {
    let interfaces = get_if_addrs().ok()?;

    interfaces.into_iter().find_map(|iface| {
        if iface.name != interface_name {
            return None;
        }

        match iface.addr {
            IfAddr::V4(v4) if !v4.ip.is_loopback() => Some(v4.ip.to_string()),
            _ => None,
        }
    })
}

#[cfg(target_os = "linux")]
fn primary_ip_from_udp() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;

    let ip = socket.local_addr().ok()?.ip();

    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

#[cfg(target_os = "linux")]
fn dedupe_nm_networks(networks: Vec<NmNetwork>) -> Vec<NmNetwork> {
    let mut by_ssid: HashMap<String, NmNetwork> = HashMap::new();

    for network in networks {
        let ssid = network.ssid.trim().to_string();
        if ssid.is_empty() {
            continue;
        }

        match by_ssid.get(&ssid) {
            Some(existing) => {
                let existing_strength = existing.strength.unwrap_or(0);
                let next_strength = network.strength.unwrap_or(0);

                if next_strength > existing_strength {
                    by_ssid.insert(ssid, network);
                }
            }
            None => {
                by_ssid.insert(ssid, network);
            }
        }
    }

    let mut result: Vec<NmNetwork> = by_ssid.into_values().collect();
    result.sort_by(|a, b| {
        b.strength
            .unwrap_or(0)
            .cmp(&a.strength.unwrap_or(0))
            .then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
    result
}

#[cfg(target_os = "linux")]
fn map_visible_network(net: &NmNetwork, saved_set: &HashSet<String>) -> WifiNetwork {
    WifiNetwork {
        ssid: net.ssid.clone(),
        secured: net.secured,
        saved: saved_set.contains(&net.ssid),
        signal_percent: net.strength,
    }
}

#[cfg(target_os = "linux")]
fn fallback_saved_network(ssid: String) -> WifiNetwork {
    WifiNetwork {
        ssid,
        secured: true,
        saved: true,
        signal_percent: None,
    }
}

#[cfg(target_os = "linux")]
fn sort_networks_by_name(networks: &mut [WifiNetwork]) {
    networks.sort_by(|a, b| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()));
}

#[cfg(target_os = "linux")]
fn wired_ipv4_for_interface(interface_name: &str) -> Option<String> {
    ipv4_for_interface(interface_name)
}

#[cfg(target_os = "linux")]
fn run_nmcli_device_toggle(interface_name: &str, enabled: bool) -> Result<(), String> {
    let action = if enabled { "up" } else { "down" };

    let output = Command::new("nmcli")
        .args(["--wait", "10", "device", action, interface_name])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

        if !stderr.is_empty() {
            Err(stderr)
        } else if !stdout.is_empty() {
            Err(stdout)
        } else {
            Err(format!("nmcli device {} failed for {}", action, interface_name))
        }
    }
}

async fn get_network_status_inner() -> Result<NetworkStatus, String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;
        let devices = nm.list_devices().await.map_err(|e| e.to_string())?;

        let ethernet_connected = devices
            .iter()
            .any(|device| is_wired_device(device) && is_active_device(device));

        let wifi_ssid = nm.current_ssid().await;

        if let Some(current_ssid) = wifi_ssid {
            let visible = dedupe_nm_networks(nm.list_networks().await.unwrap_or_default());
            let signal = visible
                .iter()
                .find(|network| network.ssid == current_ssid)
                .and_then(|network| network.strength);

            if ethernet_connected {
                return Ok(ethernet_and_wifi(current_ssid, signal));
            }

            return Ok(wifi_only(current_ssid, signal));
        }

        if ethernet_connected {
            return Ok(ethernet_only());
        }

        Ok(disconnected())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Unsupported platform".to_string())
    }
}

async fn get_wifi_settings_inner() -> Result<WifiSettings, String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;
        let devices = nm.list_devices().await.map_err(|e| e.to_string())?;

        let enabled = nm.wifi_enabled().await.map_err(|e| e.to_string())?;
        let connected_ssid = nm.current_ssid().await;

        let connected_ip = active_wifi_interface_name(&devices)
            .as_deref()
            .and_then(ipv4_for_interface)
            .or_else(|| {
                if connected_ssid.is_some() {
                    primary_ip_from_udp()
                } else {
                    None
                }
            });

        let all_saved_names = nm
            .list_saved_connections()
            .await
            .map_err(|e| e.to_string())?;

        let visible_networks = if enabled {
            dedupe_nm_networks(nm.list_networks().await.unwrap_or_default())
        } else {
            Vec::new()
        };

        let visible_ssids: HashSet<String> = visible_networks
            .iter()
            .map(|network| network.ssid.clone())
            .collect();

        let mut wifi_saved_names: Vec<String> = all_saved_names
            .into_iter()
            .filter(|name| {
                visible_ssids.contains(name)
                    || connected_ssid
                        .as_ref()
                        .map(|ssid| ssid == name)
                        .unwrap_or(false)
            })
            .collect();

        wifi_saved_names.sort();
        wifi_saved_names.dedup();

        let saved_set: HashSet<String> = wifi_saved_names.iter().cloned().collect();

        let scanned_networks: Vec<WifiNetwork> = visible_networks
            .iter()
            .map(|network| map_visible_network(network, &saved_set))
            .collect();

        let visible_by_ssid: HashMap<String, WifiNetwork> = scanned_networks
            .iter()
            .cloned()
            .map(|network| (network.ssid.clone(), network))
            .collect();

        let mut saved_networks: Vec<WifiNetwork> = wifi_saved_names
            .into_iter()
            .map(|ssid| {
                visible_by_ssid
                    .get(&ssid)
                    .cloned()
                    .unwrap_or_else(|| fallback_saved_network(ssid))
            })
            .collect();

        sort_networks_by_name(&mut saved_networks);

        Ok(WifiSettings {
            enabled,
            connected_ssid,
            connected_ip,
            saved_networks,
            scanned_networks,
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Unsupported platform".to_string())
    }
}

async fn get_wired_settings_inner() -> Result<WiredSettings, String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;
        let devices = nm.list_devices().await.map_err(|e| e.to_string())?;

        let mut interfaces: Vec<WiredInterface> = devices
            .into_iter()
            .filter(|device| is_wired_device(device))
            .map(|device| {
                let interface_name = device.interface.clone();
                WiredInterface {
                    connected: is_active_device(&device),
                    ip: wired_ipv4_for_interface(&interface_name),
                    interface_name,
                }
            })
            .collect();

        interfaces.sort_by(|a, b| a.interface_name.cmp(&b.interface_name));

        Ok(WiredSettings { interfaces })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Unsupported platform".to_string())
    }
}

async fn set_wifi_enabled_inner(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;
        nm.set_wifi_enabled(enabled)
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = enabled;
        Err("Unsupported platform".to_string())
    }
}

async fn set_wired_interface_enabled_inner(
    interface_name: String,
    enabled: bool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        run_nmcli_device_toggle(&interface_name, enabled)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interface_name, enabled);
        Err("Unsupported platform".to_string())
    }
}

async fn scan_wifi_networks_inner() -> Result<Vec<WifiNetwork>, String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;

        if !nm.wifi_enabled().await.map_err(|e| e.to_string())? {
            return Ok(Vec::new());
        }

        nm.scan_networks().await.map_err(|e| e.to_string())?;
        sleep(Duration::from_millis(1200)).await;

        let saved_names = nm
            .list_saved_connections()
            .await
            .map_err(|e| e.to_string())?;
        let saved_set: HashSet<String> = saved_names.into_iter().collect();

        let networks = dedupe_nm_networks(
            nm.list_networks()
                .await
                .map_err(|e| e.to_string())?,
        );

        Ok(networks
            .iter()
            .map(|network| map_visible_network(network, &saved_set))
            .collect())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Unsupported platform".to_string())
    }
}

async fn connect_to_wifi_inner(ssid: String, password: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;

        let security = match password {
            Some(psk) if !psk.trim().is_empty() => WifiSecurity::WpaPsk { psk },
            _ => WifiSecurity::Open,
        };

        nm.connect(&ssid, security)
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (ssid, password);
        Err("Unsupported platform".to_string())
    }
}

async fn connect_hidden_wifi_inner(ssid: String, password: Option<String>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        connect_to_wifi_inner(ssid, password).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (ssid, password);
        Err("Unsupported platform".to_string())
    }
}

async fn forget_saved_wifi_inner(ssid: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;
        nm.forget(&ssid).await.map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = ssid;
        Err("Unsupported platform".to_string())
    }
}

fn get_primary_ip_address_inner() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        primary_ip_from_udp().ok_or_else(|| "No primary IP address found".to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Unsupported platform".to_string())
    }
}

enum WorkerRequest {
    GetNetworkStatus {
        reply: mpsc::Sender<Result<NetworkStatus, String>>,
    },
    GetWifiSettings {
        reply: mpsc::Sender<Result<WifiSettings, String>>,
    },
    GetWiredSettings {
        reply: mpsc::Sender<Result<WiredSettings, String>>,
    },
    SetWifiEnabled {
        enabled: bool,
        reply: mpsc::Sender<Result<(), String>>,
    },
    SetWiredInterfaceEnabled {
        interface_name: String,
        enabled: bool,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ScanWifiNetworks {
        reply: mpsc::Sender<Result<Vec<WifiNetwork>, String>>,
    },
    ConnectToWifi {
        ssid: String,
        password: Option<String>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ConnectHiddenWifi {
        ssid: String,
        password: Option<String>,
        reply: mpsc::Sender<Result<(), String>>,
    },
    ForgetSavedWifi {
        ssid: String,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

pub struct NetworkWorkerState {
    tx: Mutex<mpsc::Sender<WorkerRequest>>,
}

impl NetworkWorkerState {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<WorkerRequest>();

        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build network worker runtime");

            for request in rx {
                match request {
                    WorkerRequest::GetNetworkStatus { reply } => {
                        let _ = reply.send(runtime.block_on(get_network_status_inner()));
                    }
                    WorkerRequest::GetWifiSettings { reply } => {
                        let _ = reply.send(runtime.block_on(get_wifi_settings_inner()));
                    }
                    WorkerRequest::GetWiredSettings { reply } => {
                        let _ = reply.send(runtime.block_on(get_wired_settings_inner()));
                    }
                    WorkerRequest::SetWifiEnabled { enabled, reply } => {
                        let _ = reply.send(runtime.block_on(set_wifi_enabled_inner(enabled)));
                    }
                    WorkerRequest::SetWiredInterfaceEnabled {
                        interface_name,
                        enabled,
                        reply,
                    } => {
                        let _ = reply.send(
                            runtime.block_on(set_wired_interface_enabled_inner(interface_name, enabled))
                        );
                    }
                    WorkerRequest::ScanWifiNetworks { reply } => {
                        let _ = reply.send(runtime.block_on(scan_wifi_networks_inner()));
                    }
                    WorkerRequest::ConnectToWifi {
                        ssid,
                        password,
                        reply,
                    } => {
                        let _ = reply.send(runtime.block_on(connect_to_wifi_inner(ssid, password)));
                    }
                    WorkerRequest::ConnectHiddenWifi {
                        ssid,
                        password,
                        reply,
                    } => {
                        let _ = reply.send(runtime.block_on(connect_hidden_wifi_inner(ssid, password)));
                    }
                    WorkerRequest::ForgetSavedWifi { ssid, reply } => {
                        let _ = reply.send(runtime.block_on(forget_saved_wifi_inner(ssid)));
                    }
                }
            }
        });

        Self { tx: Mutex::new(tx) }
    }

    fn sender(&self) -> Result<mpsc::Sender<WorkerRequest>, String> {
        self.tx
            .lock()
            .map_err(|_| "network worker lock poisoned".to_string())
            .map(|tx| tx.clone())
    }
}

static NETWORK_WORKER: OnceLock<NetworkWorkerState> = OnceLock::new();

fn worker() -> &'static NetworkWorkerState {
    NETWORK_WORKER.get_or_init(NetworkWorkerState::new)
}

async fn dispatch_blocking<R: Send + 'static>(
    tx: mpsc::Sender<WorkerRequest>,
    build: impl FnOnce(mpsc::Sender<Result<R, String>>) -> WorkerRequest + Send + 'static,
) -> Result<R, String> {
    tokio::task::spawn_blocking(move || {
        let (reply_tx, reply_rx) = mpsc::channel::<Result<R, String>>();

        tx.send(build(reply_tx))
            .map_err(|_| "network worker unavailable".to_string())?;

        reply_rx
            .recv()
            .map_err(|_| "network worker dropped response".to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_network_status() -> Result<NetworkStatus, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetNetworkStatus { reply }).await
}

#[tauri::command]
pub async fn get_wifi_settings() -> Result<WifiSettings, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetWifiSettings { reply }).await
}

#[tauri::command]
pub async fn get_wired_settings() -> Result<WiredSettings, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetWiredSettings { reply }).await
}

#[tauri::command]
pub async fn set_wifi_enabled(enabled: bool) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::SetWifiEnabled { enabled, reply }).await
}

#[tauri::command]
pub async fn set_wired_interface_enabled(
    interface_name: String,
    enabled: bool,
) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::SetWiredInterfaceEnabled {
        interface_name,
        enabled,
        reply,
    })
    .await
}

#[tauri::command]
pub async fn scan_wifi_networks() -> Result<Vec<WifiNetwork>, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::ScanWifiNetworks { reply }).await
}

#[tauri::command]
pub async fn connect_to_wifi(ssid: String, password: Option<String>) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::ConnectToWifi {
        ssid,
        password,
        reply,
    })
    .await
}

#[tauri::command]
pub async fn connect_hidden_wifi(ssid: String, password: Option<String>) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::ConnectHiddenWifi {
        ssid,
        password,
        reply,
    })
    .await
}

#[tauri::command]
pub async fn forget_saved_wifi(ssid: String) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::ForgetSavedWifi { ssid, reply }).await
}

#[tauri::command]
pub async fn get_primary_ip_address() -> Result<String, String> {
    tokio::task::spawn_blocking(get_primary_ip_address_inner)
        .await
        .map_err(|e| e.to_string())?
}