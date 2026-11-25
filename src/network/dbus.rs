use super::{WifiNetwork, WifiScan};
use futures_util::{StreamExt, stream};
use nmrs::{Device, DeviceState, builders::WifiConnectionBuilder};
use std::collections::{HashMap, HashSet};
use tokio::time::{Duration, timeout};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, Error, Proxy, Result, fdo::PropertiesProxy, names::InterfaceName};

const SERVICE: &str = "org.freedesktop.NetworkManager";
const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const PROFILE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const WIFI: &str = "802-11-wireless";
const ROOT: &str = "/";
const SCAN_WAIT: Duration = Duration::from_secs(12);
const SCAN_BUSY_WAIT: Duration = Duration::from_secs(2);

type ProfileSettings = HashMap<String, HashMap<String, OwnedValue>>;
type Settings<'a> = HashMap<&'a str, HashMap<&'a str, Value<'a>>>;

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait Wireless {
    fn get_all_access_points(&self) -> Result<Vec<OwnedObjectPath>>;
    fn request_scan(&self, options: HashMap<&str, Value<'_>>) -> Result<()>;

    #[zbus(property)]
    fn last_scan(&self) -> Result<i64>;
}

pub(super) async fn system_bus() -> Result<&'static Connection> {
    static BUS: tokio::sync::OnceCell<Connection> = tokio::sync::OnceCell::const_new();
    BUS.get_or_try_init(Connection::system).await
}

struct SavedProfile {
    ssid: String,
    path: OwnedObjectPath,
    secured: bool,
    priority: i32,
}

async fn wifi_profiles(bus: &Connection) -> Result<Vec<SavedProfile>> {
    let settings = Proxy::new(
        bus,
        SERVICE,
        SETTINGS_PATH,
        "org.freedesktop.NetworkManager.Settings",
    )
    .await?;
    let paths: Vec<OwnedObjectPath> = settings.call("ListConnections", &()).await?;
    let mut profiles = stream::iter(paths)
        .map(|path| async move { read_wifi_profile(bus, path).await })
        .buffer_unordered(12)
        .filter_map(|profile| async { profile.ok().flatten() })
        .collect::<Vec<_>>()
        .await;
    profiles.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(profiles)
}

async fn read_wifi_profile(
    bus: &Connection,
    path: OwnedObjectPath,
) -> Result<Option<SavedProfile>> {
    let profile = Proxy::new(bus, SERVICE, path.as_str(), PROFILE).await?;
    let settings: ProfileSettings = profile.call("GetSettings", &()).await?;
    Ok(saved_profile(path, &settings))
}

fn saved_profile(path: OwnedObjectPath, settings: &ProfileSettings) -> Option<SavedProfile> {
    let ssid = settings
        .get(WIFI)
        .and_then(|wifi| wifi.get("ssid"))
        .and_then(ssid_from_value)?;
    Some(SavedProfile {
        ssid,
        path,
        secured: settings.contains_key("802-11-wireless-security"),
        priority: settings
            .get("connection")
            .and_then(|setting| setting.get("autoconnect-priority"))
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or_default(),
    })
}

fn ssid_from_value(value: &OwnedValue) -> Option<String> {
    let bytes: Vec<u8> = value.try_clone().ok()?.try_into().ok()?;
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

async fn access_point(bus: &Connection, path: OwnedObjectPath) -> Result<Option<WifiNetwork>> {
    let properties = PropertiesProxy::builder(bus)
        .destination(SERVICE)?
        .path(path)?
        .build()
        .await?;
    let values = properties
        .get_all(InterfaceName::from_static_str_unchecked(ACCESS_POINT))
        .await?;
    let ssid = values.get("Ssid").and_then(ssid_from_value);
    let number = |name| {
        values
            .get(name)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_default()
    };
    Ok(ssid.map(|ssid| WifiNetwork {
        ssid,
        secured: number("Flags") & 1 != 0 || number("WpaFlags") != 0 || number("RsnFlags") != 0,
        saved: false,
        signal_percent: values
            .get("Strength")
            .and_then(|value| u8::try_from(value).ok()),
    }))
}

async fn scan_device(wifi: &WirelessProxy<'_>) -> Result<bool> {
    // Install the listener before RequestScan so fast scans cannot be missed.
    let mut changes = wifi.receive_last_scan_changed().await;
    let baseline = wifi.last_scan().await?;
    let wait = match wifi.request_scan(HashMap::new()).await {
        Ok(()) => SCAN_WAIT,
        Err(Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.NetworkManager.Device.NotAllowed"
                && baseline >= 0 =>
        {
            SCAN_BUSY_WAIT
        }
        Err(error) => return Err(error),
    };
    timeout(wait, async {
        while let Some(change) = changes.next().await {
            if change.get().await? > baseline {
                return Ok(true);
            }
        }
        Ok(false)
    })
    .await
    .unwrap_or(Ok(false))
}

async fn access_points(
    bus: &Connection,
    wifi: &WirelessProxy<'_>,
) -> Result<Vec<(OwnedObjectPath, WifiNetwork)>> {
    let paths = wifi.get_all_access_points().await?;
    Ok(stream::iter(paths)
        .map(|path| async move {
            access_point(bus, path.clone())
                .await
                .ok()
                .flatten()
                .map(|network| (path, network))
        })
        .buffer_unordered(16)
        .filter_map(|ap| async { ap })
        .collect()
        .await)
}

pub(super) async fn wifi_lists(
    devices: &[Device],
    enabled: bool,
    request_scan: bool,
) -> Result<WifiScan> {
    let bus = system_bus().await?;
    let profiles = wifi_profiles(bus).await?;
    let results = stream::iter(devices.iter().filter(|device| {
        super::is_wifi_device(device)
            && device.managed != Some(false)
            && !matches!(
                device.state,
                DeviceState::Unmanaged | DeviceState::Unavailable
            )
    }))
    .filter(|_| async { enabled })
    .map(|device| async move {
        let wifi = WirelessProxy::builder(bus)
            .path(device.path.as_str())?
            .build()
            .await?;
        let fresh = if request_scan {
            scan_device(&wifi).await?
        } else {
            true
        };
        Ok::<_, Error>((fresh, access_points(bus, &wifi).await?))
    })
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;

    let mut fresh = false;
    let mut networks = Vec::new();
    let mut first_error = None;
    for result in results {
        match result {
            Ok((completed, access_points)) => {
                fresh |= completed;
                networks.extend(access_points.into_iter().map(|(_, network)| network));
            }
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if request_scan && networks.is_empty() && !fresh {
        return Err(first_error.unwrap_or_else(|| {
            Error::Failure(
                "No Wi-Fi adapter completed the scan. Check that Wi-Fi is enabled and try again."
                    .into(),
            )
        }));
    }
    Ok(with_saved_profiles(
        networks,
        profiles,
        request_scan && !fresh,
    ))
}

fn with_saved_profiles(
    networks: Vec<WifiNetwork>,
    profiles: Vec<SavedProfile>,
    cached: bool,
) -> WifiScan {
    let saved_ssids: HashSet<_> = profiles
        .iter()
        .map(|profile| profile.ssid.as_str())
        .collect();
    let mut by_ssid = HashMap::<String, WifiNetwork>::new();
    for mut network in networks {
        network.saved = saved_ssids.contains(network.ssid.as_str());
        let best = by_ssid
            .entry(network.ssid.clone())
            .or_insert_with(|| network.clone());
        if network.signal_percent > best.signal_percent {
            *best = network;
        }
    }
    let mut saved = Vec::new();
    let mut seen = HashSet::new();
    for profile in profiles {
        if seen.insert(profile.ssid.clone()) {
            saved.push(by_ssid.get(&profile.ssid).cloned().unwrap_or(WifiNetwork {
                ssid: profile.ssid,
                secured: profile.secured,
                saved: true,
                signal_percent: None,
            }));
        }
    }
    saved.sort_by_cached_key(|network| (network.ssid.to_lowercase(), network.ssid.clone()));
    let mut scanned: Vec<_> = by_ssid.into_values().collect();
    scanned.sort_by_cached_key(|network| {
        (
            std::cmp::Reverse(network.signal_percent),
            network.ssid.to_lowercase(),
            network.ssid.clone(),
        )
    });
    WifiScan {
        scanned,
        saved,
        cached,
    }
}

async fn manager(bus: &Connection) -> Result<Proxy<'_>> {
    Proxy::new(bus, SERVICE, MANAGER_PATH, SERVICE).await
}

async fn activate(
    bus: &Connection,
    profile: &ObjectPath<'_>,
    device: &ObjectPath<'_>,
) -> Result<()> {
    let root = ObjectPath::from_static_str_unchecked(ROOT);
    let active: OwnedObjectPath = manager(bus)
        .await?
        .call("ActivateConnection", &(profile, device, root))
        .await?;
    wait_for_activation(bus, active).await
}

async fn add_and_activate(
    bus: &Connection,
    settings: Settings<'_>,
    device: &ObjectPath<'_>,
    specific: &ObjectPath<'_>,
) -> Result<()> {
    let (_, active): (OwnedObjectPath, OwnedObjectPath) = manager(bus)
        .await?
        .call("AddAndActivateConnection", &(settings, device, specific))
        .await?;
    wait_for_activation(bus, active).await
}

async fn wait_for_activation(bus: &Connection, active: OwnedObjectPath) -> Result<()> {
    let proxy = Proxy::new(bus, SERVICE, active, ACTIVE).await?;
    let mut changes = proxy.receive_property_changed::<u32>("State").await;
    let mut state: u32 = proxy.get_property("State").await?;
    timeout(Duration::from_secs(25), async {
        loop {
            // NMActiveConnectionState. ACTIVATED = 2, DEACTIVATING = 3, DEACTIVATED = 4.
            match state {
                2 => return Ok(()),
                3 | 4 => break,
                _ => {}
            }
            match changes.next().await {
                Some(change) => state = change.get().await?,
                None => break,
            }
        }
        Err(Error::Failure(
            "NetworkManager could not activate the connection".into(),
        ))
    })
    .await
    .unwrap_or_else(|_| Err(Error::Failure("NetworkManager activation timed out".into())))
}

fn available_wifi_devices(devices: &[Device]) -> impl Iterator<Item = &Device> {
    devices.iter().filter(|device| {
        super::is_wifi_device(device)
            && device.managed != Some(false)
            && matches!(
                device.state,
                DeviceState::Disconnected | DeviceState::Activated
            )
    })
}

pub(super) async fn connect_wifi(
    devices: &[Device],
    ssid: &str,
    password: Option<String>,
    hidden: bool,
) -> Result<()> {
    let bus = system_bus().await?;
    let adapter = available_wifi_devices(devices)
        .max_by_key(|device| super::is_active_device(device))
        .ok_or_else(|| Error::Failure("No available Wi-Fi adapter".into()))?;
    let device = ObjectPath::try_from(adapter.path.as_str())?;
    let root = ObjectPath::from_static_str_unchecked(ROOT);
    if password.is_none() {
        let profiles = wifi_profiles(bus).await?;
        if let Some(preferred) = profiles.iter().find(|profile| profile.ssid == ssid) {
            for adapter in available_wifi_devices(devices) {
                let path = ObjectPath::try_from(adapter.path.as_str())?;
                let proxy = Proxy::new(bus, SERVICE, path.clone(), DEVICE).await?;
                let available: Vec<OwnedObjectPath> =
                    proxy.get_property("AvailableConnections").await?;
                if let Some(profile) = profiles
                    .iter()
                    .find(|profile| profile.ssid == ssid && available.contains(&profile.path))
                {
                    return activate(bus, &profile.path, &path).await;
                }
            }
            return activate(bus, &preferred.path, &device).await;
        }
    }

    let mut builder = WifiConnectionBuilder::new(ssid)
        .hidden(hidden)
        .ipv4_auto()
        .ipv6_auto();
    if let Some(password) = password {
        builder = builder.wpa_psk(password);
    }
    if hidden {
        return add_and_activate(bus, builder.build(), &device, &root).await;
    }
    let mut strongest = None;
    for adapter in available_wifi_devices(devices) {
        let path = ObjectPath::try_from(adapter.path.as_str())?;
        let wifi = WirelessProxy::builder(bus)
            .path(path.clone())?
            .build()
            .await?;
        for (ap, network) in access_points(bus, &wifi).await? {
            if network.ssid == ssid
                && strongest
                    .as_ref()
                    .is_none_or(|(_, _, strength)| network.signal_percent > *strength)
            {
                strongest = Some((path.clone(), ap, network.signal_percent));
            }
        }
    }
    let (device, ap, _) =
        strongest.ok_or_else(|| Error::Failure("Wi-Fi network is no longer in range".into()))?;
    add_and_activate(bus, builder.build(), &device, &ap).await
}

pub(super) async fn forget_wifi(ssid: &str) -> Result<()> {
    let bus = system_bus().await?;
    for profile in wifi_profiles(bus)
        .await?
        .into_iter()
        .filter(|profile| profile.ssid == ssid)
    {
        let proxy = Proxy::new(bus, SERVICE, profile.path, PROFILE).await?;
        proxy.call::<_, _, ()>("Delete", &()).await?;
    }
    Ok(())
}

pub(super) async fn set_wired_enabled(device: &Device, enabled: bool) -> Result<()> {
    let bus = system_bus().await?;
    let path = ObjectPath::try_from(device.path.as_str())?;
    let proxy = Proxy::new(bus, SERVICE, path.clone(), DEVICE).await?;
    if !enabled {
        return proxy.call("Disconnect", &()).await;
    }
    if super::is_active_device(device) {
        return Ok(());
    }
    let available: Vec<OwnedObjectPath> = proxy.get_property("AvailableConnections").await?;
    let root = ObjectPath::from_static_str_unchecked(ROOT);
    if available.is_empty() {
        add_and_activate(bus, HashMap::new(), &path, &root).await
    } else {
        activate(bus, &root, &path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_profiles_use_the_on_air_ssid_instead_of_the_profile_label() {
        let path = OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Settings/1").unwrap();
        let mut settings = ProfileSettings::from([(
            "connection".into(),
            HashMap::from([(
                "id".into(),
                OwnedValue::try_from(Value::from("Home Wi-Fi")).unwrap(),
            )]),
        )]);
        assert!(saved_profile(path.clone(), &settings).is_none());

        let actual_ssid = "  Test:Wi-Fi  ";
        settings.insert(
            WIFI.into(),
            HashMap::from([(
                "ssid".into(),
                OwnedValue::try_from(Value::from(actual_ssid.as_bytes().to_vec())).unwrap(),
            )]),
        );
        let profile = saved_profile(path, &settings).unwrap();
        assert_eq!(profile.ssid, actual_ssid);
    }

    #[test]
    fn scan_lists_keep_the_strongest_access_point_and_mark_saved_ssids() {
        let access_point = |ssid: &str, signal| WifiNetwork {
            ssid: ssid.into(),
            secured: true,
            saved: false,
            signal_percent: Some(signal),
        };
        let networks = vec![
            access_point("Home", 35),
            access_point("Guest", 50),
            access_point("Home", 80),
        ];
        let profile = SavedProfile {
            ssid: "Home".into(),
            path: OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/Settings/1").unwrap(),
            secured: true,
            priority: 0,
        };
        let scan = with_saved_profiles(networks, vec![profile], false);
        assert_eq!(scan.scanned.len(), 2);
        assert_eq!(scan.scanned[0].ssid, "Home");
        assert_eq!(scan.scanned[0].signal_percent, Some(80));
        assert!(scan.scanned[0].saved);
        assert!(!scan.scanned[1].saved);
        assert_eq!(scan.saved[0].signal_percent, Some(80));
    }
}
