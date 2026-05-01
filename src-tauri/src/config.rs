use notify::{RecursiveMode, Watcher};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{mpsc::channel, Mutex},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppConfig(pub Mutex<Value>);
pub struct WatchedConfigPath(pub Mutex<Option<String>>);
pub struct IdleWatcherGeneration(pub Mutex<u64>);

#[tauri::command]
pub fn get_config(state: State<AppConfig>) -> Result<Value, String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "failed to lock config state".to_string())?;

    Ok(guard.clone())
}

#[tauri::command]
pub fn load_config_file(
    config_path: String,
    app: AppHandle,
    config_state: State<AppConfig>,
    watched_path_state: State<WatchedConfigPath>,
) -> Result<Value, String> {
    let merged = load_and_merge_config(&config_path)?;

    {
        let mut guard = config_state
            .0
            .lock()
            .map_err(|_| "failed to lock config state".to_string())?;
        *guard = merged.clone();
    }

    bump_idle_generation(&app);

    let _ = app.emit("config-loaded", merged.clone());
    crate::theme::emit_theme_assets(&app, &config_path);

    ensure_config_watcher(&app, &config_path, &watched_path_state);

    Ok(merged)
}

#[tauri::command]
pub fn save_editable_config(
    editable_config: Value,
    app: AppHandle,
    config_state: State<AppConfig>,
    watched_path_state: State<WatchedConfigPath>,
) -> Result<Value, String> {
    let config_path = {
        let guard = watched_path_state
            .0
            .lock()
            .map_err(|_| "failed to lock watched path state".to_string())?;

        guard
            .clone()
            .or_else(get_default_project_config_path)
            .ok_or_else(|| "no writable config path available".to_string())?
    };

    let mut current = load_and_merge_config(&config_path).unwrap_or_else(|_| default_config());

    apply_editable_config(&mut current, &editable_config);

    let serialized = serialize_cfg(&current);
    fs::write(&config_path, serialized)
        .map_err(|e| format!("failed to write config '{}': {}", config_path, e))?;

    {
        let mut guard = config_state
            .0
            .lock()
            .map_err(|_| "failed to lock config state".to_string())?;
        *guard = current.clone();
    }

    bump_idle_generation(&app);

    let _ = app.emit("config-loaded", current.clone());
    crate::theme::emit_theme_assets(&app, &config_path);

    Ok(current)
}

pub fn bump_idle_generation(app: &AppHandle) {
    if let Some(state) = app.try_state::<IdleWatcherGeneration>() {
        if let Ok(mut guard) = state.0.lock() {
            *guard += 1;
            eprintln!("idle watcher generation bumped to {}", *guard);
        }
    }
}

pub fn get_idle_generation(app: &AppHandle) -> u64 {
    app.try_state::<IdleWatcherGeneration>()
        .and_then(|state| state.0.lock().ok().map(|guard| *guard))
        .unwrap_or(0)
}

pub fn read_idle_config(app: &AppHandle) -> Option<(bool, u64)> {
    let state = app.try_state::<AppConfig>()?;
    let config = state.0.lock().ok()?;
    let system = config.get("system").and_then(Value::as_object);

    let enabled = system
        .and_then(|s| s.get("use_idle_timeout"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let timeout = system
        .and_then(|s| s.get("idle_timeout"))
        .and_then(Value::as_u64)
        .unwrap_or(900);

    Some((enabled, timeout.max(1)))
}

pub fn read_idle_unlock_delay(app: &AppHandle) -> u64 {
    app.try_state::<AppConfig>()
        .and_then(|state| {
            state.0.lock().ok().and_then(|config| {
                config
                    .get("system")
                    .and_then(Value::as_object)
                    .and_then(|system| system.get("idle_unlock"))
                    .and_then(Value::as_u64)
            })
        })
        .unwrap_or(500)
}

pub fn default_config() -> Value {
    json!({
        "websocket": {
            "ip": "127.0.0.1:7125"
        },
        "styling": {
            "zoom": 1.0,
            "darkmode": true,
            "primary": "",
            "secondary": ""
        },
        "dev": {
            "debug": false
        },
        "system": {
            "language": "en",
            "idle_timeout": 900,
            "idle_unlock": 500,
            "use_idle_timeout": true
        }
    })
}

pub fn load_and_merge_config(config_path: &str) -> Result<Value, String> {
    let content = fs::read_to_string(config_path)
        .map_err(|e| format!("failed to read config '{}': {}", config_path, e))?;

    let parsed = parse_cfg_to_json(&content)?;
    Ok(merge_json(default_config(), parsed))
}

pub fn get_app_config_arg() -> Option<String> {
    std::env::args().find_map(|arg| {
        arg.strip_prefix("--app-config=")
            .map(|s| s.to_string())
    })
}

pub fn get_default_project_config_path() -> Option<String> {
    let path = PathBuf::from("../config.cfg");
    if path.exists() {
        Some(path.to_string_lossy().to_string())
    } else {
        None
    }
}

pub fn ensure_config_watcher(
    app: &AppHandle,
    config_path: &str,
    watched_path_state: &State<WatchedConfigPath>,
) {
    let mut guard = match watched_path_state.0.lock() {
        Ok(g) => g,
        Err(_) => {
            eprintln!("failed to lock watched path state");
            return;
        }
    };

    if guard.as_deref() == Some(config_path) {
        return;
    }

    *guard = Some(config_path.to_string());
    start_config_watcher(app.clone(), config_path.to_string());
}

fn start_config_watcher(app: AppHandle, config_path: String) {
    thread::spawn(move || {
        let path = PathBuf::from(config_path.clone());
        let (tx, rx) = channel();

        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(err) => {
                eprintln!("failed to create config watcher: {}", err);
                return;
            }
        };

        if let Err(err) = watcher.watch(&path, RecursiveMode::NonRecursive) {
            eprintln!("failed to watch config file '{}': {}", config_path, err);
            return;
        }

        let mut last_emitted: Option<Value> = None;

        loop {
            match rx.recv() {
                Ok(Ok(_event)) => {
                    thread::sleep(Duration::from_millis(200));

                    while rx.try_recv().is_ok() {}

                    match load_and_merge_config(&config_path) {
                        Ok(config_json) => {
                            if last_emitted.as_ref() == Some(&config_json) {
                                continue;
                            }

                            let mut changed = true;

                            if let Some(state) = app.try_state::<AppConfig>() {
                                if let Ok(mut guard) = state.0.lock() {
                                    if *guard == config_json {
                                        changed = false;
                                    } else {
                                        *guard = config_json.clone();
                                    }
                                }
                            }

                            if !changed {
                                last_emitted = Some(config_json);
                                continue;
                            }

                            bump_idle_generation(&app);

                            last_emitted = Some(config_json.clone());

                            let _ = app.emit("config-loaded", config_json);
                            crate::theme::emit_theme_assets(&app, &config_path);
                        }
                        Err(err) => {
                            eprintln!("failed to reload config after file change: {}", err);
                        }
                    }
                }
                Ok(Err(err)) => {
                    eprintln!("config watcher event error: {}", err);
                }
                Err(err) => {
                    eprintln!("config watcher channel error: {}", err);
                    break;
                }
            }
        }
    });
}

fn apply_editable_config(target: &mut Value, patch: &Value) {
    let Some(target_obj) = target.as_object_mut() else {
        return;
    };

    let Some(patch_obj) = patch.as_object() else {
        return;
    };

    if let Some(styling_patch) = patch_obj.get("styling").and_then(Value::as_object) {
        let styling = target_obj
            .entry("styling".to_string())
            .or_insert_with(|| Value::Object(Map::new()));

        let Some(styling_obj) = styling.as_object_mut() else {
            return;
        };

        if let Some(value) = styling_patch.get("darkmode") {
            if value.is_boolean() {
                styling_obj.insert("darkmode".to_string(), value.clone());
            }
        }

        if let Some(value) = styling_patch.get("primary") {
            styling_obj.insert("primary".to_string(), normalize_nullable_string(value));
        }

        if let Some(value) = styling_patch.get("secondary") {
            styling_obj.insert("secondary".to_string(), normalize_nullable_string(value));
        }
    }

    if let Some(system_patch) = patch_obj.get("system").and_then(Value::as_object) {
        let system = target_obj
            .entry("system".to_string())
            .or_insert_with(|| Value::Object(Map::new()));

        let Some(system_obj) = system.as_object_mut() else {
            return;
        };

        if let Some(value) = system_patch.get("language") {
            system_obj.insert("language".to_string(), normalize_nullable_string(value));
        }

        if let Some(value) = system_patch.get("idle_timeout") {
            if value.is_number() {
                system_obj.insert("idle_timeout".to_string(), value.clone());
            }
        }

        if let Some(value) = system_patch.get("idle_unlock") {
            if value.is_number() {
                system_obj.insert("idle_unlock".to_string(), value.clone());
            }
        }

        if let Some(value) = system_patch.get("use_idle_timeout") {
            if value.is_boolean() {
                system_obj.insert("use_idle_timeout".to_string(), value.clone());
            }
        }
    }

    if let Some(shortcutbuttons_patch) = patch_obj.get("shortcutbuttons").and_then(Value::as_array) {
        target_obj.insert(
            "shortcutbuttons".to_string(),
            Value::Array(shortcutbuttons_patch.clone()),
        );

        let keys_to_remove: Vec<String> = target_obj
            .keys()
            .filter(|key| key.to_lowercase().starts_with("shortcutbutton "))
            .cloned()
            .collect();

        for key in keys_to_remove {
            target_obj.remove(&key);
        }

        for button in shortcutbuttons_patch {
            let Some(button_obj) = button.as_object() else {
                continue;
            };

            let Some(name) = button_obj.get("name").and_then(Value::as_str) else {
                continue;
            };

            let Some(macro_inactive) = button_obj.get("macro_inactive").and_then(Value::as_str) else {
                continue;
            };

            let Some(icon) = button_obj.get("icon").and_then(Value::as_str) else {
                continue;
            };

            let mut section = Map::new();

            if let Some(value) = button_obj.get("position") {
                if value.is_number() {
                    section.insert("position".to_string(), value.clone());
                }
            }

            section.insert(
                "macro_inactive".to_string(),
                Value::String(macro_inactive.trim().to_string()),
            );
            section.insert("icon".to_string(), Value::String(icon.trim().to_string()));

            for key in ["macro_active", "active_config", "active_type"] {
                if let Some(value) = button_obj.get(key).and_then(Value::as_str) {
                    if !value.trim().is_empty() {
                        section.insert(key.to_string(), Value::String(value.trim().to_string()));
                    }
                }
            }

            if let Some(value) = button_obj.get("active_threshold") {
                if value.is_number() {
                    section.insert("active_threshold".to_string(), value.clone());
                }
            }

            target_obj.insert(
                format!("shortcutbutton {}", name.trim()),
                Value::Object(section),
            );
        }
    }
}

fn normalize_nullable_string(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Value::Null
            } else {
                Value::String(trimmed.to_string())
            }
        }
        _ => Value::Null,
    }
}

fn serialize_cfg(config: &Value) -> String {
    let Some(root) = config.as_object() else {
        return String::new();
    };

    let preferred_order = ["websocket", "styling", "dev", "system"];
    let mut sections: Vec<String> = preferred_order
        .iter()
        .filter(|key| root.contains_key(**key))
        .map(|key| key.to_string())
        .collect();

    for key in root.keys() {
        if key == "shortcutbuttons" {
            continue;
        }

        if key.to_lowercase().starts_with("shortcutbutton ") {
            continue;
        }

        if !sections.iter().any(|existing| existing == key) {
            sections.push(key.clone());
        }
    }

    let mut out = String::new();
    let mut wrote_any = false;

    for section_name in sections {
        let Some(section_value) = root.get(&section_name) else {
            continue;
        };

        if wrote_any {
            out.push('\n');
        }

        match section_value {
            Value::Object(section_obj) => {
                out.push('[');
                out.push_str(&section_name);
                out.push_str("]\n");

                let mut keys: Vec<_> = section_obj.keys().cloned().collect();
                keys.sort();

                for key in keys {
                    let value = section_obj.get(&key).unwrap_or(&Value::Null);
                    out.push_str(&key);
                    out.push_str(": ");
                    out.push_str(&serialize_scalar(value));
                    out.push('\n');
                }
            }
            other => {
                out.push_str(&section_name);
                out.push_str(": ");
                out.push_str(&serialize_scalar(other));
                out.push('\n');
            }
        }

        wrote_any = true;
    }

    if let Some(shortcutbuttons) = root.get("shortcutbuttons").and_then(Value::as_array) {
        for button in shortcutbuttons {
            let Some(button_obj) = button.as_object() else {
                continue;
            };

            let Some(name) = button_obj.get("name").and_then(Value::as_str) else {
                continue;
            };

            if wrote_any {
                out.push('\n');
            }

            out.push_str("[shortcutbutton ");
            out.push_str(name.trim());
            out.push_str("]\n");

            let mut keys: Vec<String> = button_obj
                .keys()
                .filter(|k| k.as_str() != "name")
                .cloned()
                .collect();

            keys.sort();

            if let Some(pos_index) = keys.iter().position(|k| k == "position") {
                let pos = keys.remove(pos_index);
                keys.insert(0, pos);
            }

            for key in keys {
                let value = button_obj.get(&key).unwrap_or(&Value::Null);

                match value {
                    Value::Null => continue,
                    Value::String(s) if s.trim().is_empty() => continue,
                    _ => {}
                }

                out.push_str(&key);
                out.push_str(": ");
                out.push_str(&serialize_scalar(value));
                out.push('\n');
            }

            wrote_any = true;
        }
    }

    out
}

fn serialize_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => String::new(),
    }
}

fn parse_cfg_to_json(input: &str) -> Result<Value, String> {
    let mut root = Map::new();
    let mut current_section: Option<String> = None;

    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            let section_name = line[1..line.len() - 1].trim();

            if section_name.is_empty() {
                return Err(format!("empty section name at line {}", line_no));
            }

            current_section = Some(section_name.to_string());
            root.entry(section_name.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            continue;
        }

        let (key_raw, value_raw) = if let Some(parts) = line.split_once('=') {
            parts
        } else if let Some(parts) = line.split_once(':') {
            parts
        } else {
            return Err(format!(
                "invalid line {}: expected format 'key=value' or 'key: value'",
                line_no
            ));
        };

        let key = key_raw.trim();
        let value_str = value_raw.trim();

        if key.is_empty() {
            return Err(format!("empty key at line {}", line_no));
        }

        let value = parse_scalar(value_str);

        match &current_section {
            Some(section) => {
                let Some(section_obj) = root.get_mut(section).and_then(Value::as_object_mut) else {
                    return Err(format!("invalid section '{}'", section));
                };

                section_obj.insert(key.to_string(), value);
            }
            None => {
                root.insert(key.to_string(), value);
            }
        }
    }

    let mut shortcutbuttons: Vec<Value> = Vec::new();

    for (key, value) in &root {
        if !key.to_lowercase().starts_with("shortcutbutton ") {
            continue;
        }

        let Some(section_obj) = value.as_object() else {
            continue;
        };

        let name = key["shortcutbutton ".len()..].trim();

        if name.is_empty() {
            continue;
        }

        let macro_inactive = section_obj
            .get("macro_inactive")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        let icon = section_obj
            .get("icon")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        if macro_inactive.is_empty() || icon.is_empty() {
            continue;
        }

        let mut button = Map::new();
        button.insert("name".to_string(), Value::String(name.to_string()));
        button.insert("macro_inactive".to_string(), Value::String(macro_inactive));
        button.insert("icon".to_string(), Value::String(icon));

        if let Some(value) = section_obj.get("position") {
            if value.is_number() {
                button.insert("position".to_string(), value.clone());
            }
        }

        for key in ["macro_active", "active_config", "active_type"] {
            if let Some(value) = section_obj.get(key).and_then(Value::as_str) {
                if !value.trim().is_empty() {
                    button.insert(key.to_string(), Value::String(value.trim().to_string()));
                }
            }
        }

        if let Some(value) = section_obj.get("active_threshold") {
            if value.is_number() {
                button.insert("active_threshold".to_string(), value.clone());
            }
        }

        shortcutbuttons.push(Value::Object(button));
    }

    if !shortcutbuttons.is_empty() {
        shortcutbuttons.sort_by(|a, b| {
            let a_pos = a.get("position").and_then(Value::as_i64).unwrap_or(i64::MAX);
            let b_pos = b.get("position").and_then(Value::as_i64).unwrap_or(i64::MAX);
            a_pos.cmp(&b_pos)
        });

        root.insert("shortcutbuttons".to_string(), Value::Array(shortcutbuttons));
    }

    Ok(Value::Object(root))
}

fn parse_scalar(s: &str) -> Value {
    if s.eq_ignore_ascii_case("true") {
        Value::Bool(true)
    } else if s.eq_ignore_ascii_case("false") {
        Value::Bool(false)
    } else if s.eq_ignore_ascii_case("null") {
        Value::Null
    } else if let Ok(i) = s.parse::<i64>() {
        json!(i)
    } else if let Ok(f) = s.parse::<f64>() {
        json!(f)
    } else {
        json!(s)
    }
}

fn merge_json(defaults: Value, overrides: Value) -> Value {
    match (defaults, overrides) {
        (Value::Object(mut default_map), Value::Object(override_map)) => {
            for (key, override_value) in override_map {
                let merged_value = match default_map.remove(&key) {
                    Some(default_value) => merge_json(default_value, override_value),
                    None => override_value,
                };

                default_map.insert(key, merged_value);
            }

            Value::Object(default_map)
        }
        (_, override_value) => override_value,
    }
}