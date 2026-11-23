use evdev::{Device, EventType};
use std::{fs, path::Path};

pub fn open_input_devices() -> Result<Vec<Device>, String> {
    let mut devices = Vec::new();

    let entries = fs::read_dir("/dev/input")
        .map_err(|e| format!("[idle watcher] failed to read /dev/input: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if !is_event_device(&path) {
            continue;
        }

        if let Ok(device) = Device::open(path)
            && should_watch_device(&device)
            && device.set_nonblocking(true).is_ok()
        {
            devices.push(device);
        }
    }

    if devices.is_empty() {
        return Err("[idle watcher] no readable /dev/input/event* devices found".to_string());
    }

    Ok(devices)
}

fn should_watch_device(device: &Device) -> bool {
    device
        .supported_keys()
        .is_some_and(|keys| keys.iter().next().is_some())
        || device
            .supported_absolute_axes()
            .is_some_and(|axes| axes.iter().next().is_some())
        || device
            .supported_relative_axes()
            .is_some_and(|axes| axes.iter().next().is_some())
}

fn is_event_device(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("event"))
        .unwrap_or(false)
}

pub fn is_real_input_event(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::KEY | EventType::RELATIVE | EventType::ABSOLUTE | EventType::SWITCH
    )
}
