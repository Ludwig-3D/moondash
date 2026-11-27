use std::borrow::Cow;

pub(super) fn screen(page: &str, tab: &str) -> &'static str {
    match (page, tab) {
        ("home", _) => "Dashboard",
        ("files", _) => "Print List",
        ("tune", "print") => "Print Options",
        ("tune", "afc") => "Filament",
        ("tune", "objects") => "Print Objects",
        ("tune", "queue") => "Job Queue",
        ("tune", _) => "Controls",
        ("settings", "network") => "Network",
        ("settings", "config") => "Settings",
        ("settings", "tools") => "Tools",
        ("settings", "host") => "Resources",
        ("settings", "shortcuts") => "Shortcuts",
        ("settings", "updates") => "Updates",
        ("settings", _) => "Settings",
        ("debug", _) => "Diagnostics",
        _ => "Unknown screen",
    }
}

pub(super) fn pressed(action: &str, argument: &str, page: &str, tab: &str) {
    let description = match action {
        // These actions are recorded by State::navigate at the destination.
        "show-afc" | "show-objects" | "show-queue" | "show-updates" => return,
        "file" => "Opened print file details",
        "folder" if argument.is_empty() => "Opened the root print folder",
        "folder" => "Opened a print folder",
        "up" => "Went up one print folder",
        "file-context" => "Opened print file or folder actions",
        "file-page-next" => "Opened the next page of print files",
        "file-page-previous" => "Opened the previous page of print files",
        "file-filter" => "Opened the print file filter",
        "file-filter-done" => "Finished editing the print file filter",
        "file-sort" => "Changed the print file sort order",
        "print-file" => "Requested to start a print",
        "queue-file" => "Requested to add a print to the queue",
        "move-selected" => "Opened the move and rename dialog",
        "delete-selected" => "Opened the delete confirmation",
        "apply-delete" => "Confirmed deletion",
        "apply-move" => "Requested to move or rename the selection",
        "tool-mesh" => "Opened Bed Mesh",
        "mesh-open-3d" => "Opened the 3D bed mesh",
        "mesh-close-3d" => "Closed the 3D bed mesh",
        "tool-pid" => "Opened PID Tuning",
        "pid-digit" | "pid-heater" | "pid-run" => return,
        "tool-camera" => "Opened Webcam",
        "tools-home" => "Returned to the tools list",
        "heater" | "temperature-fan" => "Opened temperature controls",
        "speed" => "Opened print speed controls",
        "fan" => "Opened fan controls",
        "toggle-filament-sensor" => "Requested to toggle the filament sensor",
        "pause" => "Requested to pause the print",
        "resume" => "Requested to resume the print",
        "cancel" => "Opened the print cancellation confirmation",
        "power" => "Opened Power Options",
        "power-back" => "Returned to Power Options",
        "power-confirm" => "Confirmed the selected power action",
        "update" => "Requested to install an update",
        "update-all" => "Requested to install all updates",
        "update-refresh" => "Checked for updates",
        "update-info" => "Opened update details",
        "update-close" => "Closed the update dialog",
        "wifi-scan" => "Scanned for Wi-Fi networks",
        "wifi-submit" => "Submitted the Wi-Fi connection form",
        "add-shortcut" => "Opened the new shortcut editor",
        "edit-shortcut" => "Opened the shortcut editor",
        "shortcut-save" => "Requested to save the shortcut",
        _ => {
            log::info!(target: "moondash::ui", "Pressed {} ({})", action.replace('-', " "), screen(page, tab));
            return;
        }
    };
    let location = screen(page, tab);
    if matches!(
        action,
        "heater" | "temperature-fan" | "fan" | "toggle-filament-sensor"
    ) && crate::tools::public_device(argument)
    {
        log::info!(target: "moondash::ui", "{description} for {argument} ({location})");
    } else {
        log::info!(target: "moondash::ui", "{description} ({location})");
    }
}

fn dialog_name(mode: &str, action: &str) -> Cow<'static, str> {
    match (mode, action) {
        ("power", _) => "Power Options".into(),
        ("temperature", _) => "Temperature".into(),
        ("speed", _) => "Print Speed".into(),
        ("fan", _) => "Fan".into(),
        ("print", _) => "Print Details".into(),
        ("file-context", _) => "File Actions".into(),
        ("confirm", "cancel") => "Cancel Print".into(),
        ("confirm", "update") => "Install Update".into(),
        ("confirm", "update-all") => "Install All Updates".into(),
        (_, _) if !action.is_empty() => action.replace('-', " ").into(),
        (_, "") if !mode.is_empty() => mode.replace('-', " ").into(),
        _ => "dialog".into(),
    }
}

pub(super) fn submitted(mode: &str, action: &str, argument: &str) {
    let name = dialog_name(mode, action);
    if matches!(mode, "temperature" | "fan") && crate::tools::public_device(argument) {
        log::info!(target: "moondash::ui", "Submitted {name} for {argument}");
    } else {
        log::info!(target: "moondash::ui", "Submitted {name}");
    }
}

pub(super) fn closed(mode: &str) {
    log::info!(target: "moondash::ui", "Closed {}", dialog_name(mode, ""));
}

pub(super) fn setting_name(path: &str) -> &str {
    match path {
        "styling.darkmode" => "Dark mode",
        "styling.advanced_states" => "Advanced states",
        "styling.primary" => "Primary theme color",
        "styling.secondary" => "Secondary theme color",
        "system.language" => "Language",
        "system.display_profile" => "Display profile",
        "system.use_idle_timeout" => "Display idle timeout",
        "system.idle_timeout" => "Idle timeout duration",
        "websocket.ip" => "Moonraker address",
        _ => "Setting",
    }
}

pub(super) fn setting_value(path: &str, config: &serde_json::Value) -> Option<String> {
    let (section, key) = path.split_once('.')?;
    let value = &config[section][key];
    let toggle = |default| value.as_bool().unwrap_or(default).to_string();
    Some(match path {
        "styling.darkmode" | "system.use_idle_timeout" => toggle(true),
        "styling.advanced_states" => toggle(false),
        "system.idle_timeout" => format!("{} s", value.as_u64().unwrap_or(900)),
        "system.language" => match value.as_str() {
            Some("en") => "English".into(),
            Some("de") => "German".into(),
            _ => "System default".into(),
        },
        "system.display_profile" => {
            match value.as_str().or_else(|| config["styling"][key].as_str()) {
                Some(profile @ ("small" | "normal" | "highres")) => profile.into(),
                _ => "Auto".into(),
            }
        }
        "styling.primary" | "styling.secondary" => {
            let defaults = crate::theme::theme_defaults(
                config["styling"]["darkmode"].as_bool().unwrap_or(true),
            );
            let rgb = value
                .as_str()
                .and_then(crate::theme::parse_rgb)
                .unwrap_or(defaults[usize::from(key == "secondary")]);
            format!("#{rgb:06X}")
        }
        _ => return None,
    })
}
