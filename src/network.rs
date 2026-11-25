use get_if_addrs::{IfAddr, get_if_addrs};
use nmrs::{Device, DeviceState, DeviceType, NetworkManager};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::net::UdpSocket;
use std::path::Path;
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Instant;

#[cfg(target_os = "linux")]
mod dbus;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiInterface {
    interface_name: String,
    connected: bool,
    ip: Option<String>,
}

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
pub struct WifiScan {
    scanned: Vec<WifiNetwork>,
    saved: Vec<WifiNetwork>,
    cached: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiSettings {
    enabled: bool,
    connected_ssid: Option<String>,
    connected_ip: Option<String>,
    interface_name: Option<String>,
    interfaces: Vec<WifiInterface>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanbusInterface {
    interface_name: String,
    connected: bool,
    bitrate: Option<u64>,
    rx_bytes: Option<u64>,
    tx_bytes: Option<u64>,
    rx_packets: Option<u64>,
    tx_packets: Option<u64>,
    rx_bytes_per_second: Option<f64>,
    tx_bytes_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanbusSettings {
    interfaces: Vec<CanbusInterface>,
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
    device.device_type == DeviceType::Wifi
}

#[cfg(target_os = "linux")]
fn is_wired_device(device: &Device) -> bool {
    device.device_type == DeviceType::Ethernet
}

#[cfg(target_os = "linux")]
fn is_active_device(device: &Device) -> bool {
    device.state == DeviceState::Activated
}

#[cfg(target_os = "linux")]
fn wifi_interfaces_from_devices(devices: &[Device]) -> Vec<WifiInterface> {
    let mut interfaces: Vec<WifiInterface> = devices
        .iter()
        .filter(|device| is_wifi_device(device))
        .filter(|device| !device.interface.trim().is_empty())
        .map(|device| {
            let interface_name = device.interface.clone();

            WifiInterface {
                connected: is_active_device(device),
                ip: ipv4_for_device(device),
                interface_name,
            }
        })
        .collect();

    interfaces.sort_by(|a, b| a.interface_name.cmp(&b.interface_name));
    interfaces
}

#[cfg(target_os = "linux")]
fn ipv4_for_interface(interface_name: &str) -> Option<String> {
    let interfaces = match get_if_addrs() {
        Ok(interfaces) => interfaces,
        Err(_err) => {
            return None;
        }
    };

    for iface in interfaces {
        if iface.name != interface_name {
            continue;
        }

        match iface.addr {
            IfAddr::V4(v4) if !v4.ip.is_loopback() => {
                let ip = v4.ip.to_string();
                return Some(ip);
            }
            _ => {}
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn ipv4_for_device(device: &Device) -> Option<String> {
    device
        .ip4_address
        .as_deref()
        .map(|address| address.split('/').next().unwrap_or(address).to_owned())
        .or_else(|| ipv4_for_interface(&device.interface))
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
fn read_u64_file(path: impl AsRef<Path>) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse::<u64>().ok()
}

#[cfg(target_os = "linux")]
fn read_operstate(interface_name: &str) -> String {
    fs::read_to_string(format!("/sys/class/net/{}/operstate", interface_name))
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct CanbusCounterSnapshot {
    captured_at: Instant,
    rx_bytes: Option<u64>,
    tx_bytes: Option<u64>,
}

#[cfg(target_os = "linux")]
static CANBUS_COUNTER_SNAPSHOTS: OnceLock<Mutex<HashMap<String, CanbusCounterSnapshot>>> =
    OnceLock::new();

#[cfg(target_os = "linux")]
fn canbus_counter_snapshots() -> &'static Mutex<HashMap<String, CanbusCounterSnapshot>> {
    CANBUS_COUNTER_SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "linux")]
fn bytes_per_second(
    previous: Option<u64>,
    current: Option<u64>,
    elapsed_seconds: f64,
) -> Option<f64> {
    if elapsed_seconds <= 0.0 {
        return None;
    }

    match (previous, current) {
        (Some(previous), Some(current)) if current >= previous => {
            Some((current - previous) as f64 / elapsed_seconds)
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn canbus_interface_from_name(interface_name: String) -> CanbusInterface {
    let base = format!("/sys/class/net/{}", interface_name);
    let state = read_operstate(&interface_name);
    let now = Instant::now();
    let rx_bytes = read_u64_file(format!("{}/statistics/rx_bytes", base));
    let tx_bytes = read_u64_file(format!("{}/statistics/tx_bytes", base));

    let (rx_bytes_per_second, tx_bytes_per_second) = canbus_counter_snapshots()
        .lock()
        .ok()
        .map(|mut snapshots| {
            let previous = snapshots.get(&interface_name).cloned();

            snapshots.insert(
                interface_name.clone(),
                CanbusCounterSnapshot {
                    captured_at: now,
                    rx_bytes,
                    tx_bytes,
                },
            );

            previous
                .map(|previous| {
                    let elapsed_seconds = now.duration_since(previous.captured_at).as_secs_f64();

                    (
                        bytes_per_second(previous.rx_bytes, rx_bytes, elapsed_seconds),
                        bytes_per_second(previous.tx_bytes, tx_bytes, elapsed_seconds),
                    )
                })
                .unwrap_or((None, None))
        })
        .unwrap_or((None, None));

    CanbusInterface {
        connected: state == "up" || state == "unknown",
        bitrate: read_u64_file(format!("{}/can_bittiming/bitrate", base)),
        rx_bytes,
        tx_bytes,
        rx_packets: read_u64_file(format!("{}/statistics/rx_packets", base)),
        tx_packets: read_u64_file(format!("{}/statistics/tx_packets", base)),
        rx_bytes_per_second,
        tx_bytes_per_second,
        interface_name,
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

        if let Some(current) = nm.current_network().await.ok().flatten() {
            if ethernet_connected {
                return Ok(ethernet_and_wifi(current.ssid, current.strength));
            }

            return Ok(wifi_only(current.ssid, current.strength));
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

        let wifi_interfaces = wifi_interfaces_from_devices(&devices);

        let connected_interface = wifi_interfaces
            .iter()
            .find(|iface| iface.connected)
            .or_else(|| wifi_interfaces.first());

        let interface_name = connected_interface.map(|iface| iface.interface_name.clone());

        let connected_ip = connected_interface.and_then(|iface| iface.ip.clone());

        let enabled = nm.wifi_enabled().await.map_err(|e| e.to_string())?;
        let connected_ssid = nm.current_ssid().await;
        let networks = dbus::wifi_lists(&devices, enabled, false)
            .await
            .map_err(|e| e.to_string())?;

        Ok(WifiSettings {
            enabled,
            connected_ssid,
            connected_ip,
            interface_name,
            interfaces: wifi_interfaces,
            saved_networks: networks.saved,
            scanned_networks: networks.scanned,
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
            .filter(is_wired_device)
            .map(|device| {
                let interface_name = device.interface.clone();

                WiredInterface {
                    connected: is_active_device(&device),
                    ip: ipv4_for_device(&device),
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

async fn get_canbus_settings_inner() -> Result<CanbusSettings, String> {
    #[cfg(target_os = "linux")]
    {
        let mut interfaces = Vec::new();

        for entry in fs::read_dir("/sys/class/net").map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let interface_name = entry.file_name().to_string_lossy().to_string();

            if interface_name.starts_with("can") {
                interfaces.push(canbus_interface_from_name(interface_name));
            }
        }

        interfaces.sort_by(|a, b| a.interface_name.cmp(&b.interface_name));

        Ok(CanbusSettings { interfaces })
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
            .map_err(|e| e.to_string())?;

        if enabled {
            nm.wait_for_wifi_ready().await.map_err(|e| e.to_string())?;
        }

        Ok(())
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
        let device = nm()
            .await?
            .list_devices()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|device| is_wired_device(device) && device.interface == interface_name)
            .ok_or_else(|| format!("Wired interface {interface_name} not found"))?;
        dbus::set_wired_enabled(&device, enabled)
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (interface_name, enabled);
        Err("Unsupported platform".to_string())
    }
}

async fn scan_wifi_networks_inner() -> Result<WifiScan, String> {
    #[cfg(target_os = "linux")]
    {
        let nm = nm().await?;
        let enabled = nm.wifi_enabled().await.map_err(|e| e.to_string())?;
        let devices = nm
            .list_wireless_devices()
            .await
            .map_err(|e| e.to_string())?;
        dbus::wifi_lists(&devices, enabled, true)
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("Unsupported platform".to_string())
    }
}

async fn connect_wifi_inner(
    ssid: String,
    password: Option<String>,
    hidden: bool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err("SSID must be between 1 and 32 bytes".into());
        }
        let devices = nm()
            .await?
            .list_wireless_devices()
            .await
            .map_err(|e| e.to_string())?;
        dbus::connect_wifi(&devices, &ssid, password, hidden)
            .await
            .map_err(|e| e.to_string())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (ssid, password, hidden);
        Err("Unsupported platform".to_string())
    }
}

async fn forget_saved_wifi_inner(ssid: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if ssid.is_empty() {
            return Err("SSID cannot be empty".to_string());
        }

        dbus::forget_wifi(&ssid).await.map_err(|e| e.to_string())
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
    GetCanbusSettings {
        reply: mpsc::Sender<Result<CanbusSettings, String>>,
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
        reply: mpsc::Sender<Result<WifiScan, String>>,
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
                    WorkerRequest::GetCanbusSettings { reply } => {
                        let _ = reply.send(runtime.block_on(get_canbus_settings_inner()));
                    }
                    WorkerRequest::SetWifiEnabled { enabled, reply } => {
                        let _ = reply.send(runtime.block_on(set_wifi_enabled_inner(enabled)));
                    }
                    WorkerRequest::SetWiredInterfaceEnabled {
                        interface_name,
                        enabled,
                        reply,
                    } => {
                        let _ =
                            reply.send(runtime.block_on(set_wired_interface_enabled_inner(
                                interface_name,
                                enabled,
                            )));
                    }
                    WorkerRequest::ScanWifiNetworks { reply } => {
                        let _ = reply.send(runtime.block_on(scan_wifi_networks_inner()));
                    }
                    WorkerRequest::ConnectToWifi {
                        ssid,
                        password,
                        reply,
                    } => {
                        let _ =
                            reply.send(runtime.block_on(connect_wifi_inner(ssid, password, false)));
                    }
                    WorkerRequest::ConnectHiddenWifi {
                        ssid,
                        password,
                        reply,
                    } => {
                        let _ =
                            reply.send(runtime.block_on(connect_wifi_inner(ssid, password, true)));
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

pub async fn get_network_status() -> Result<NetworkStatus, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetNetworkStatus { reply }).await
}

pub async fn get_wifi_settings() -> Result<WifiSettings, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetWifiSettings { reply }).await
}

pub async fn get_wired_settings() -> Result<WiredSettings, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetWiredSettings { reply }).await
}

pub async fn get_canbus_settings() -> Result<CanbusSettings, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::GetCanbusSettings { reply }).await
}

pub async fn set_wifi_enabled(enabled: bool) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::SetWifiEnabled {
        enabled,
        reply,
    })
    .await
}

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

pub async fn scan_wifi_networks() -> Result<WifiScan, String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, |reply| WorkerRequest::ScanWifiNetworks { reply }).await
}

pub async fn connect_to_wifi(ssid: String, password: Option<String>) -> Result<(), String> {
    let tx = worker().sender()?;

    dispatch_blocking(tx, move |reply| WorkerRequest::ConnectToWifi {
        ssid,
        password,
        reply,
    })
    .await
}

pub async fn connect_hidden_wifi(ssid: String, password: Option<String>) -> Result<(), String> {
    let tx = worker().sender()?;

    dispatch_blocking(tx, move |reply| WorkerRequest::ConnectHiddenWifi {
        ssid,
        password,
        reply,
    })
    .await
}

pub async fn forget_saved_wifi(ssid: String) -> Result<(), String> {
    let tx = worker().sender()?;
    dispatch_blocking(tx, move |reply| WorkerRequest::ForgetSavedWifi {
        ssid,
        reply,
    })
    .await
}

pub async fn get_primary_ip_address() -> Result<String, String> {
    tokio::task::spawn_blocking(get_primary_ip_address_inner)
        .await
        .map_err(|e| e.to_string())?
}
