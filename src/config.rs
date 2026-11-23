use serde_json::{Map, Value, json};

pub fn default_config() -> Value {
    json!({
        "websocket": {
            "ip": "127.0.0.1:7125"
        },
        "styling": {
            "zoom": 1.0,
            "darkmode": true,
            "primary": "",
            "secondary": "",
            "advanced_states": false,
            "printfiles_grid_row": null,
            "printfiles_grid_columns": null
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

        let Some((key_raw, value_raw)) = line.split_once(['=', ':']) else {
            return Err(format!(
                "invalid line {line_no}: expected key=value or key: value",
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

        if let Some(value) = section_obj
            .get("position")
            .filter(|value| value.is_number())
        {
            button.insert("position".to_string(), value.clone());
        }

        for key in ["macro_active", "active_config", "active_type"] {
            if let Some(value) = section_obj
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                button.insert(key.to_string(), Value::String(value.to_string()));
            }
        }

        if let Some(value) = section_obj
            .get("active_threshold")
            .filter(|value| value.is_number())
        {
            button.insert("active_threshold".to_string(), value.clone());
        }

        shortcutbuttons.push(Value::Object(button));
    }

    if !shortcutbuttons.is_empty() {
        shortcutbuttons.sort_by(|a, b| {
            let a_pos = a
                .get("position")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            let b_pos = b
                .get("position")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
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
pub fn parse(text: &str) -> Result<Value, String> {
    Ok(merge_json(default_config(), parse_cfg_to_json(text)?))
}

/// Update a single setting while preserving unrelated text and comments.
pub fn with_value(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut insertion = None;
    let mut changed = false;

    for (index, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']'))
        {
            if insertion.is_some() {
                break;
            }
            if name.eq_ignore_ascii_case(section) {
                insertion = Some(index + 1);
            }
        } else if insertion.is_some() {
            if let Some((name, _)) = trimmed.split_once([':', '='])
                && name.trim() == key
            {
                *line = format!("{key}: {value}");
                changed = true;
                break;
            }
            if !trimmed.is_empty() {
                insertion = Some(index + 1);
            }
        }
    }

    if !changed {
        let entry = format!("{key}: {value}");
        if let Some(index) = insertion {
            lines.insert(index, entry);
        } else {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(entry);
        }
    }

    format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_one_setting_without_rewriting_other_sections() {
        let file = "# Printer config\n[styling]\ndarkmode = true\n\n[system]\nlanguage: de\n";
        let changed = with_value(file, "styling", "darkmode", "false");
        assert_eq!(
            changed,
            "# Printer config\n[styling]\ndarkmode: false\n\n[system]\nlanguage: de\n"
        );
        let changed = with_value(&changed, "styling", "secondary", "#48a9a6");
        assert_eq!(parse(&changed).unwrap()["styling"]["secondary"], "#48a9a6");
    }

    #[test]
    fn reads_existing_config_and_shortcuts_with_both_delimiters() {
        let cfg = parse(
            "[websocket]\nip: wss://printer.local/websocket?token=example\n\
            [styling]\ndarkmode=false\n\
            [shortcutbutton Light]\nicon=light\nmacro_inactive: SET_PIN PIN=light VALUE=1\n",
        )
        .unwrap();
        assert_eq!(
            cfg["websocket"]["ip"],
            "wss://printer.local/websocket?token=example"
        );
        assert_eq!(cfg["styling"]["darkmode"], false);
        assert_eq!(cfg["system"]["idle_timeout"], 900);
        assert_eq!(
            cfg["shortcutbuttons"][0]["macro_inactive"],
            "SET_PIN PIN=light VALUE=1"
        );
    }
}
