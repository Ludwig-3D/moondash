//! Host, session, display and graphics details for the developer page.

use crate::{I18n, InfoCard, InfoRow, Moondash, icons, renderer, version};
use serde_json::{Map, Value, json};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{fs, path::Path, sync::OnceLock, time::Instant};

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Call once at startup so the page can show how long Moondash has been running.
pub(super) fn mark_start() {
    STARTED.get_or_init(Instant::now);
}

/// Compositors, window managers and display servers worth naming when found running.
const COMPOSITORS: &[&str] = &[
    "labwc",
    "cage",
    "sway",
    "weston",
    "wayfire",
    "Hyprland",
    "niri",
    "river",
    "cosmic-comp",
    "gamescope",
    "kwin_wayland",
    "kwin_x11",
    "gnome-shell",
    "mutter",
    "openbox",
    "xfwm4",
    "Xorg",
    "Xwayland",
];

/// Environment variables that affect the session or which graphics stack is used.
const ENVIRONMENT: &[&str] = &[
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DESKTOP_SESSION",
    "XDG_SESSION_TYPE",
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "WINIT_UNIX_BACKEND",
    "SLINT_BACKEND",
    "WGPU_BACKEND",
    "WGPU_ADAPTER_NAME",
    "__GLX_VENDOR_LIBRARY_NAME",
    "__EGL_VENDOR_LIBRARY_FILENAMES",
    "LIBGL_ALWAYS_SOFTWARE",
    "MESA_LOADER_DRIVER_OVERRIDE",
    "LANG",
    "LC_ALL",
];

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.trim_matches(|c: char| c.is_whitespace() || c == '\0')
                .to_owned()
        })
        .filter(|text| !text.is_empty())
}

/// First `key: value` (or `key=value`) entry with this key in a /proc or /etc file.
fn field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once([':', '='])?;
        (name.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn duration(seconds: u64) -> String {
    let (days, hours, minutes) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    match days {
        0 => format!("{hours}h {minutes:02}m {:02}s", seconds % 60),
        _ => format!("{days}d {hours}h {minutes:02}m"),
    }
}

fn kibibytes_to_mib(value: Option<String>) -> Value {
    value
        .and_then(|value| value.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(Value::Null, |kib| {
            let mib = kib / 1024.;
            json!(if mib >= 1024. {
                format!("{:.1} GiB", mib / 1024.)
            } else {
                format!("{mib:.1} MiB")
            })
        })
}

fn vendor_name(id: &str) -> &'static str {
    match id {
        "0x10de" => "NVIDIA",
        "0x1002" => "AMD",
        "0x8086" => "Intel",
        "0x14e4" => "Broadcom",
        "0x5143" => "Qualcomm",
        _ => "",
    }
}

fn gpus() -> Value {
    let mut cards: Vec<_> = fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("card"))
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    cards.sort();
    let gpus: Vec<Value> = cards
        .iter()
        .map(|card| {
            let device = card.join("device");
            let vendor = read(device.join("vendor"));
            let driver = fs::read_link(device.join("driver"))
                .ok()
                .and_then(|link| Some(link.file_name()?.to_string_lossy().into_owned()));
            json!({
                "card": card.file_name().map(|name| name.to_string_lossy().into_owned()),
                "vendor": vendor.as_deref().map(|id| match vendor_name(id) {
                    "" => id.to_owned(),
                    name => format!("{name} ({id})"),
                }),
                "device": read(device.join("device")),
                "driver": driver,
                "primary": read(device.join("boot_vga")).map(|flag| flag == "1"),
            })
        })
        .collect();
    json!(gpus)
}

fn running_compositors() -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|entry| read(entry.path().join("comm")))
        .filter(|name| COMPOSITORS.contains(&name.as_str()))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Details that cannot change while Moondash runs, gathered once.
fn static_info() -> &'static Value {
    static INFO: OnceLock<Value> = OnceLock::new();
    INFO.get_or_init(|| {
        let os_release = read("/etc/os-release").unwrap_or_default();
        let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
        let meminfo = read("/proc/meminfo").unwrap_or_default();
        json!({
            "host": {
                "hostname": read("/proc/sys/kernel/hostname"),
                "model": read("/proc/device-tree/model")
                    .or_else(|| read("/sys/class/dmi/id/product_name")),
                "os": field(&os_release, "PRETTY_NAME"),
                "kernel": read("/proc/sys/kernel/osrelease"),
                "architecture": std::env::consts::ARCH,
                "cpu": field(&cpuinfo, "model name")
                    .or_else(|| field(&cpuinfo, "Model"))
                    .or_else(|| field(&cpuinfo, "Hardware")),
                "cpuCores": std::thread::available_parallelism().map(usize::from).ok(),
                "memoryTotal": kibibytes_to_mib(field(&meminfo, "MemTotal")),
            },
            "gpus": gpus(),
            "nvidiaDriver": read("/proc/driver/nvidia/version")
                .and_then(|text| {
                text.lines()
                    .next()
                    .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            }),
            "compositors": running_compositors(),
        })
    })
}

fn window(ui: &Moondash) -> Value {
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let window = ui.window();
    let size = window.size();
    let mut info = Map::new();
    info.insert(
        "size".into(),
        json!(format!("{}x{} px", size.width, size.height)),
    );
    info.insert("scaleFactor".into(), json!(window.scale_factor()));
    info.insert("fullscreen".into(), json!(window.is_fullscreen()));
    let native = window.with_winit_window(|winit| {
        let platform = match winit.window_handle().map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Wayland(_)) => "Wayland".to_owned(),
            Ok(RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_)) => "X11".to_owned(),
            Ok(other) => format!("{other:?}"),
            Err(error) => format!("unknown ({error})"),
        };
        let monitor = winit.current_monitor().map(|monitor| {
            let size = monitor.size();
            json!({
                "name": monitor.name(),
                "resolution": format!("{}x{}", size.width, size.height),
                "refreshRate": monitor
                    .refresh_rate_millihertz()
                    .map(|mhz| format!("{:.2} Hz", f64::from(mhz) / 1000.)),
                "scaleFactor": monitor.scale_factor(),
            })
        });
        (platform, monitor)
    });
    if let Some((platform, monitor)) = native {
        info.insert("platform".into(), json!(platform));
        info.insert("monitor".into(), monitor.unwrap_or(Value::Null));
    }
    Value::Object(info)
}

pub(super) fn report(ui: &Moondash, config_path: &Path) -> Value {
    let fixed = static_info();
    let status = read("/proc/self/status").unwrap_or_default();
    let uptime = read("/proc/uptime")
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|seconds| duration(seconds as u64));
    let environment: Map<String, Value> = ENVIRONMENT
        .iter()
        .map(|key| {
            (
                key.to_string(),
                std::env::var(key).map_or(Value::Null, Value::String),
            )
        })
        .collect();
    let mut host = fixed["host"].clone();
    host["uptime"] = json!(uptime);
    host["load"] = json!(read("/proc/loadavg").map(|text| {
        text.split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    }));
    json!({
        "moondash": {
            "version": version::current_version(),
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "pid": std::process::id(),
            "executable": std::env::current_exe().ok(),
            "config": config_path,
            "language": ui.global::<I18n>().get_language().as_str(),
            "uptime": STARTED.get().map(|started| duration(started.elapsed().as_secs())),
            "memory": kibibytes_to_mib(field(&status, "VmRSS")),
            "threads": field(&status, "Threads"),
        },
        "renderer": renderer::report(),
        "session": {
            "compositors": fixed["compositors"],
            "environment": environment,
        },
        "window": window(ui),
        "host": host,
        "gpus": fixed["gpus"],
        "nvidiaDriver": fixed["nvidiaDriver"],
    })
}

/// Human-readable form of a report value for a card row.
fn display(value: &Value) -> String {
    match value {
        Value::Null => "—".into(),
        Value::Bool(true) => "Yes".into(),
        Value::Bool(false) => "No".into(),
        Value::String(text) if text.is_empty() => "—".into(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Array(items) if items.is_empty() => "none found".into(),
        Value::Array(items) => items.iter().map(display).collect::<Vec<_>>().join(", "),
        Value::Object(_) => value.to_string(),
    }
}

fn card(title: impl Into<String>, icon: &str, rows: Vec<(String, String)>) -> InfoCard {
    let rows: Vec<InfoRow> = rows
        .into_iter()
        .map(|(label, value)| InfoRow {
            label: label.into(),
            value: value.into(),
        })
        .collect();
    InfoCard {
        title: title.into().into(),
        icon: icons::glyph(icon),
        rows: ModelRc::new(VecModel::from(rows)),
    }
}

/// Rows for `(label, key)` pairs of a report object.
fn rows(object: &Value, fields: &[(&str, &str)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(label, key)| (label.to_string(), display(&object[*key])))
        .collect()
}

/// The version number out of NVIDIA's `NVRM version: ... 620.73.04 Release Build ...` line.
fn nvidia_version(line: &str) -> Option<&str> {
    line.split_whitespace().find(|word| {
        word.contains('.')
            && word
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Turn the system report (live, or a paused snapshot) into cards for the dev page.
pub(super) fn cards(report: &Value) -> Vec<InfoCard> {
    let mut cards = Vec::new();

    cards.push(card(
        "Moondash",
        "information-outline",
        rows(
            &report["moondash"],
            &[
                ("Version", "version"),
                ("Build", "build"),
                ("Process ID", "pid"),
                ("Uptime", "uptime"),
                ("Memory", "memory"),
                ("Threads", "threads"),
                ("Language", "language"),
                ("Config", "config"),
                ("Executable", "executable"),
            ],
        ),
    ));

    let renderer = &report["renderer"];
    let mut renderer_rows = vec![
        (
            "Active".to_owned(),
            match renderer["active"].as_str() {
                Some("vulkan") => "Vulkan".to_owned(),
                Some("opengl") => "OpenGL".to_owned(),
                Some("software") => "Software".to_owned(),
                _ => display(&renderer["active"]),
            },
        ),
        (
            "Selected by".to_owned(),
            match renderer["selectedBy"].as_str() {
                Some("argument") => "--renderer argument".to_owned(),
                Some("fallback") => "Fallback after a failed start".to_owned(),
                Some("default") => "Default".to_owned(),
                _ => display(&renderer["selectedBy"]),
            },
        ),
    ];
    renderer_rows.extend(rows(
        renderer,
        &[
            ("Slint renderer", "slintRenderer"),
            ("Slint backend", "slintBackend"),
            ("First frame", "firstFramePresented"),
        ],
    ));
    let graphics = &renderer["graphics"];
    renderer_rows.extend(match graphics["api"].as_str() {
        Some("OpenGL") => rows(
            graphics,
            &[
                ("Graphics API", "api"),
                ("Vendor", "vendor"),
                ("GPU", "renderer"),
                ("Version", "version"),
                ("Shading language", "shadingLanguage"),
            ],
        ),
        Some("WGPU") => rows(
            graphics,
            &[
                ("Graphics API", "backend"),
                ("GPU", "adapter"),
                ("Device type", "deviceType"),
                ("Driver", "driver"),
                ("Driver info", "driverInfo"),
                ("Vendor ID", "vendorId"),
                ("Device ID", "deviceId"),
            ],
        ),
        _ if renderer["active"] == "software" => {
            vec![("Graphics API".to_owned(), "None (CPU rendering)".to_owned())]
        }
        _ => vec![("Graphics API".to_owned(), display(&graphics["api"]))],
    });
    cards.push(card("Renderer", "chip", renderer_rows));

    cards.push(card(
        "Display",
        "monitor",
        [
            rows(
                &report["window"],
                &[
                    ("Platform", "platform"),
                    ("Window", "size"),
                    ("Scale factor", "scaleFactor"),
                    ("Fullscreen", "fullscreen"),
                ],
            ),
            rows(
                &report["window"]["monitor"],
                &[
                    ("Monitor", "name"),
                    ("Resolution", "resolution"),
                    ("Refresh rate", "refreshRate"),
                    ("Monitor scale", "scaleFactor"),
                ],
            ),
        ]
        .concat(),
    ));

    let session = &report["session"];
    let environment = &session["environment"];
    let mut session_rows = vec![
        ("Compositor".to_owned(), display(&session["compositors"])),
        (
            "Desktop".to_owned(),
            display(&environment["XDG_CURRENT_DESKTOP"]),
        ),
        (
            "Session type".to_owned(),
            display(&environment["XDG_SESSION_TYPE"]),
        ),
    ];
    if let Some(variables) = environment.as_object() {
        session_rows.extend(
            variables
                .iter()
                .filter(|(key, value)| {
                    !value.is_null()
                        && !matches!(key.as_str(), "XDG_CURRENT_DESKTOP" | "XDG_SESSION_TYPE")
                })
                .map(|(key, value)| (key.clone(), display(value))),
        );
    }
    cards.push(card("Session", "application-outline", session_rows));

    cards.push(card(
        "Host",
        "server",
        rows(
            &report["host"],
            &[
                ("Hostname", "hostname"),
                ("Model", "model"),
                ("OS", "os"),
                ("Kernel", "kernel"),
                ("Architecture", "architecture"),
                ("CPU", "cpu"),
                ("Cores", "cpuCores"),
                ("Memory", "memoryTotal"),
                ("Uptime", "uptime"),
                ("Load", "load"),
            ],
        ),
    ));

    let gpus = report["gpus"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for gpu in gpus {
        let mut gpu_rows = rows(
            gpu,
            &[
                ("Vendor", "vendor"),
                ("Device ID", "device"),
                ("Kernel driver", "driver"),
                ("Primary", "primary"),
            ],
        );
        if gpu["driver"] == "nvidia"
            && let Some(line) = report["nvidiaDriver"].as_str()
        {
            gpu_rows.push((
                "Driver version".to_owned(),
                nvidia_version(line).unwrap_or(line).to_owned(),
            ));
        }
        cards.push(card(
            format!("GPU · {}", display(&gpu["card"])),
            "expansion-card",
            gpu_rows,
        ));
    }
    if gpus.is_empty() {
        cards.push(card(
            "GPU",
            "expansion-card",
            vec![("Devices".to_owned(), "none found".to_owned())],
        ));
    }
    cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_style_fields_and_durations() {
        let text = "Name:\tmoondash\nVmRSS:\t  20480 kB\n";
        assert_eq!(kibibytes_to_mib(field(text, "VmRSS")), json!("20.0 MiB"));
        assert_eq!(
            field("PRETTY_NAME=\"Debian GNU/Linux 12\"\n", "PRETTY_NAME").as_deref(),
            Some("Debian GNU/Linux 12")
        );
        assert_eq!(duration(3_725), "1h 02m 05s");
        assert_eq!(duration(90_061), "1d 1h 01m");
        assert_eq!(vendor_name("0x10de"), "NVIDIA");
        assert_eq!(
            nvidia_version(
                "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  620.73.04  Release Build"
            ),
            Some("620.73.04")
        );
        assert_eq!(display(&json!([])), "none found");
        assert_eq!(display(&json!(["labwc", "Xwayland"])), "labwc, Xwayland");
        assert_eq!(display(&Value::Null), "—");
    }
}
