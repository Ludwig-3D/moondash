use crate::config;
use serde_json::{Map, Value};

pub const ICONS: [&str; 6] = [
    "mdi-lightbulb",
    "mdi-fan",
    "mdi-printer-3d",
    "mdi-printer-3d-nozzle-heat",
    "mdi-printer-3d-nozzle",
    "mdi-led-strip-variant",
];

pub fn section(name: &str) -> String {
    format!("shortcutbutton {name}")
}

pub fn alter_section(text: &str, name: &str, new_name: Option<&str>) -> String {
    let mut removing = false;
    let mut result = String::new();
    for line in text.lines() {
        if let Some(header) = line
            .trim()
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
        {
            removing = false;
            if header.trim().eq_ignore_ascii_case(&section(name)) {
                if let Some(new_name) = new_name {
                    result.push_str(&format!("[{}]\n", section(new_name)));
                } else {
                    removing = true;
                }
                continue;
            }
        }
        if !removing || line.trim().is_empty() || line.trim().starts_with(['#', ';']) {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

pub fn order(mut text: String, names: &[String]) -> String {
    for (index, name) in names.iter().enumerate() {
        text = config::with_value(&text, &section(name), "position", &index.to_string());
    }
    text
}

pub fn threshold(kind: &str) -> Option<&'static str> {
    match kind {
        "output_pin" | "led" => Some("0.5"),
        "fan" | "fan_generic" | "temperature_fan" => Some("0.2"),
        _ => None,
    }
}

pub fn suggestions(status: &Map<String, Value>) -> (Vec<String>, Vec<String>) {
    let configuration = &status.get("configfile").unwrap_or(&Value::Null);
    let sections = configuration["settings"]
        .as_object()
        .or_else(|| configuration["config"].as_object());
    let keys = status
        .keys()
        .chain(sections.into_iter().flat_map(Map::keys));
    let mut macros = Vec::new();
    let mut sensors = Vec::new();
    for key in keys {
        let Some((kind, name)) = key.split_once(' ') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.starts_with('_') {
            continue;
        }
        match kind.to_ascii_lowercase().as_str() {
            "gcode_macro" => macros.push(name.to_owned()),
            "output_pin" | "fan_generic" | "temperature_fan" => sensors.push(key.clone()),
            "led" => {
                if let Some(settings) = sections.and_then(|sections| sections.get(key)) {
                    for channel in ["red", "green", "blue", "white"] {
                        if settings.get(format!("{channel}_pin")).is_some() {
                            sensors.push(format!("{key} {channel}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for values in [&mut macros, &mut sensors] {
        values.sort_by_cached_key(|value| value.to_ascii_lowercase());
        values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    }
    (macros, sensors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_reordering_and_removing_preserves_other_config_text() {
        let text = "[shortcutbutton Light]\nicon: lightbulb\nmacro_inactive: LIGHT_ON\n# shortcut comment\n\n[system]\nlanguage: de\n";
        let renamed = alter_section(text, "Light", Some("Lamp"));
        let positioned = order(renamed, &["Lamp".into()]);
        let parsed = config::parse(&positioned).unwrap();
        assert_eq!(parsed["shortcutbuttons"][0]["name"], "Lamp");
        assert_eq!(parsed["shortcutbuttons"][0]["position"], 0);
        let removed = alter_section(&positioned, "Lamp", None);
        assert_eq!(removed, "# shortcut comment\n\n[system]\nlanguage: de\n");
    }
}
