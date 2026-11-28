//! Data dumps for the developer page (enabled with `[dev] debug: true`).

use super::{State, system};
use crate::{InfoCard, JsonRow, Moondash};
use serde_json::{Map, Value, json};
use slint::{Model, ModelRc, VecModel};
use std::{collections::HashSet, path::PathBuf, time::Instant};

pub(super) const TABS: [&str; 5] = ["other", "moonraker", "afc", "full", "system"];

pub(super) fn tab_index(tab: &str) -> usize {
    TABS.iter().position(|name| *name == tab).unwrap_or(0)
}

/// Objects reported by the Armored Turtle AFC Klipper add-on.
pub(super) fn is_afc_object(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower == "afc"
        || lower.starts_with("afc ")
        || lower.starts_with("afc_")
        || lower.contains(" afc ")
        || lower.contains("filament_switch_sensor afc")
}

fn is_fan_object(lower: &str) -> bool {
    lower == "fan"
        || [
            "fan_generic ",
            "controller_fan ",
            "heater_fan ",
            "temperature_fan ",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn is_heater_object(lower: &str, value: &Value) -> bool {
    lower.starts_with("heater_generic ")
        || value.get("temperature").is_some() && value.get("target").is_some()
}

impl State {
    fn dev_other(&self) -> Map<String, Value> {
        let mut other = Map::new();
        other.insert(
            "darkmode".into(),
            self.config["styling"]["darkmode"].clone(),
        );
        other.insert("zoom".into(), self.config["styling"]["zoom"].clone());
        other.insert("debug".into(), self.config["dev"]["debug"].clone());
        other.insert(
            "websocket".into(),
            json!({"ip": self.config["websocket"]["ip"], "connected": self.online}),
        );
        other.insert(
            "moonrakerReady".into(),
            json!(self.object("webhooks")["state"] == "ready"),
        );
        other
    }

    fn dev_proc_stats(&self) -> Value {
        let mut stats = self.host["stats"].clone();
        if let Some(stats) = stats.as_object_mut() {
            stats.remove("throttled_state");
        }
        stats
    }

    fn dev_moonraker(&self) -> Map<String, Value> {
        let mut heaters = Map::new();
        let mut fans = Map::new();
        for (key, value) in &self.status {
            let lower = key.to_lowercase();
            if !value.is_object() || matches!(lower.as_str(), "extruder" | "heater_bed") {
                continue;
            }
            if lower.starts_with("afc") {
                continue;
            }
            if is_fan_object(&lower) {
                fans.insert(key.clone(), value.clone());
            } else if is_heater_object(&lower, value) {
                heaters.insert(key.clone(), value.clone());
            }
        }

        let mut moonraker = Map::new();
        for (label, object) in [
            ("webhooks", "webhooks"),
            ("extruder", "extruder"),
            ("heaterBed", "heater_bed"),
            ("toolhead", "toolhead"),
            ("gcodeMove", "gcode_move"),
            ("printStats", "print_stats"),
            ("virtualSdcard", "virtual_sdcard"),
        ] {
            moonraker.insert(label.into(), self.object(object).clone());
        }
        moonraker.insert("procStats".into(), self.dev_proc_stats());
        moonraker.insert(
            "throttle".into(),
            self.host["stats"]["throttled_state"].clone(),
        );
        moonraker.insert(
            "displayStatus".into(),
            self.object("display_status").clone(),
        );
        moonraker.insert("dynamicHeaters".into(), Value::Object(heaters));
        moonraker.insert("dynamicFans".into(), Value::Object(fans));
        moonraker
    }

    fn dev_afc(&self) -> Value {
        let objects: Map<String, Value> = self
            .status
            .iter()
            .filter(|(key, _)| is_afc_object(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        json!({"available": !objects.is_empty(), "objects": objects})
    }

    fn dev_full(&self) -> Value {
        let mut moonraker = self.status.clone();
        moonraker.insert("procStats".into(), self.dev_proc_stats());
        moonraker.insert(
            "throttle".into(),
            self.host["stats"]["throttled_state"].clone(),
        );
        let mut full = self.dev_other();
        full.insert("moonraker".into(), Value::Object(moonraker));
        Value::Object(full)
    }

    pub(super) fn dev_value(&self, ui: &Moondash, tab: &str) -> Value {
        match tab {
            "moonraker" => Value::Object(self.dev_moonraker()),
            "afc" => self.dev_afc(),
            "full" => self.dev_full(),
            "system" => system::report(ui, &self.config_path),
            _ => Value::Object(self.dev_other()),
        }
    }

    /// The data currently shown: the frozen snapshot while paused, else live data.
    fn dev_current(&self, ui: &Moondash) -> Value {
        match &self.dev_frozen {
            Some(frozen) => frozen[tab_index(&self.dev_tab)].clone(),
            None => self.dev_value(ui, &self.dev_tab),
        }
    }

    /// Pretty-printed JSON of the current tab, for Copy and Save.
    pub(super) fn dev_text(&self, ui: &Moondash) -> String {
        serde_json::to_string_pretty(&self.dev_current(ui)).unwrap_or_default()
    }

    /// Push the current tab to the UI: cards for the system tab, otherwise the
    /// visible rows of the JSON tree. Rows update in place so the list keeps
    /// its scroll position and only changed rows redraw.
    pub(super) fn render_dev(&mut self, ui: &Moondash) {
        self.dev_dirty = false;
        self.dev_rendered_at = Instant::now();
        let value = self.dev_current(ui);
        if value == self.dev_shown && !self.dev_tree_changed {
            return;
        }
        self.dev_tree_changed = false;
        if self.dev_tab == "system" {
            let (left, right) = balance(system::cards(&value));
            update_cards(&ui.get_dev_cards_left(), left, |model| {
                ui.set_dev_cards_left(model)
            });
            update_cards(&ui.get_dev_cards_right(), right, |model| {
                ui.set_dev_cards_right(model)
            });
        } else {
            let mut rows = Vec::new();
            flatten(&value, &self.dev_tab, 0, &self.dev_expanded, &mut rows);
            update_rows(&ui.get_dev_rows(), rows, |model| ui.set_dev_rows(model));
        }
        self.dev_shown = value;
    }

    pub(super) fn toggle_dev_node(&mut self, path: &str) {
        if !self.dev_expanded.remove(path) {
            self.dev_expanded.insert(path.to_owned());
        }
        self.dev_tree_changed = true;
    }

    /// Open (or close) every object and array in the current tab.
    pub(super) fn expand_dev_tree(&mut self, ui: &Moondash, expand: bool) {
        let prefix = format!("{}{SEPARATOR}", self.dev_tab);
        self.dev_expanded.retain(|path| !path.starts_with(&prefix));
        if expand {
            collect_containers(&self.dev_current(ui), &self.dev_tab, &mut self.dev_expanded);
        }
        self.dev_tree_changed = true;
    }

    pub(super) fn toggle_dev_pause(&mut self, ui: &Moondash) {
        self.dev_frozen = match self.dev_frozen {
            Some(_) => None,
            None => Some(TABS.map(|tab| self.dev_value(ui, tab))),
        };
    }
}

/// Joins keys into node paths; a control character cannot clash with JSON keys in practice.
const SEPARATOR: char = '\u{1f}';

fn child_path(parent: &str, key: &str) -> String {
    format!("{parent}{SEPARATOR}{key}")
}

fn children(value: &Value) -> Vec<(String, &Value)> {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| (key.clone(), value))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => Vec::new(),
    }
}

fn collect_containers(value: &Value, path: &str, out: &mut HashSet<String>) {
    for (key, child) in children(value) {
        if child.is_object() || child.is_array() {
            let path = child_path(path, &key);
            collect_containers(child, &path, out);
            out.insert(path);
        }
    }
}

/// One tree row: the key, a short value (or child count) and how to colour it.
fn tree_row(key: &str, value: &Value, path: String, depth: usize, expanded: bool) -> JsonRow {
    let (kind, preview) = match value {
        Value::Object(map) => ("object", format!("{{{}}}", map.len())),
        Value::Array(items) => ("array", format!("[{}]", items.len())),
        Value::String(text) => {
            // Rows are single-line; Copy/Save have the full value.
            let short: String = text.chars().take(300).collect();
            ("string", format!("\"{}\"", short.replace('\n', "\\n")))
        }
        Value::Number(number) => ("number", number.to_string()),
        Value::Bool(flag) => ("bool", flag.to_string()),
        Value::Null => ("null", "null".to_owned()),
    };
    JsonRow {
        depth: depth as i32,
        key: key.into(),
        value: preview.into(),
        kind: kind.into(),
        expandable: matches!(value, Value::Object(map) if !map.is_empty())
            || matches!(value, Value::Array(items) if !items.is_empty()),
        expanded,
        path: path.into(),
    }
}

/// The rows currently visible: children of expanded nodes only.
fn flatten(
    value: &Value,
    path: &str,
    depth: usize,
    expanded: &HashSet<String>,
    out: &mut Vec<JsonRow>,
) {
    for (key, child) in children(value) {
        let path = child_path(path, &key);
        let open = expanded.contains(&path);
        out.push(tree_row(&key, child, path.clone(), depth, open));
        if open {
            flatten(child, &path, depth + 1, expanded, out);
        }
    }
}

fn update_rows(
    current: &ModelRc<JsonRow>,
    rows: Vec<JsonRow>,
    replace: impl FnOnce(ModelRc<JsonRow>),
) {
    let Some(model) = current.as_any().downcast_ref::<VecModel<JsonRow>>() else {
        replace(ModelRc::new(VecModel::from(rows)));
        return;
    };
    let shared = model.row_count().min(rows.len());
    for (index, row) in rows.iter().take(shared).enumerate() {
        if model.row_data(index).as_ref() != Some(row) {
            model.set_row_data(index, row.clone());
        }
    }
    while model.row_count() > rows.len() {
        model.remove(model.row_count() - 1);
    }
    model.extend(rows.into_iter().skip(shared));
}

/// Split cards into two columns of similar height, keeping their order within each column.
fn balance(cards: Vec<InfoCard>) -> (Vec<InfoCard>, Vec<InfoCard>) {
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let (mut left_height, mut right_height) = (0, 0);
    for card in cards {
        // Header and divider count roughly as two rows.
        let height = card.rows.row_count() + 2;
        if left_height <= right_height {
            left_height += height;
            left.push(card);
        } else {
            right_height += height;
            right.push(card);
        }
    }
    (left, right)
}

/// Update cards in place so values refresh every second without rebuilding the views.
fn update_cards(
    current: &ModelRc<InfoCard>,
    next: Vec<InfoCard>,
    replace: impl FnOnce(ModelRc<InfoCard>),
) {
    let Some(model) = current.as_any().downcast_ref::<VecModel<InfoCard>>() else {
        replace(ModelRc::new(VecModel::from(next)));
        return;
    };
    while model.row_count() > next.len() {
        model.remove(model.row_count() - 1);
    }
    for (index, card) in next.into_iter().enumerate() {
        let Some(existing) = model.row_data(index) else {
            model.push(card);
            continue;
        };
        let rows = existing
            .rows
            .as_any()
            .downcast_ref::<VecModel<crate::InfoRow>>();
        match rows {
            Some(rows)
                if existing.title == card.title
                    && existing.icon == card.icon
                    && rows.row_count() == card.rows.row_count() =>
            {
                for (row_index, row) in card.rows.iter().enumerate() {
                    if rows.row_data(row_index).as_ref() != Some(&row) {
                        rows.set_row_data(row_index, row);
                    }
                }
            }
            _ => model.set_row_data(index, card),
        }
    }
}

/// Write a dump next to the Moondash logs so it can be downloaded from the
/// Mainsail/Fluidd log browser, falling back to the config directory.
pub(super) fn save_dump(text: &str, config_path: &std::path::Path) -> std::io::Result<PathBuf> {
    let directory = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("printer_data/logs"))
        .filter(|directory| directory.is_dir())
        .or_else(|| config_path.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let stamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
    let path = directory.join(format!("dump_{stamp}.json"));
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_afc_and_device_objects() {
        assert!(is_afc_object("AFC"));
        assert!(is_afc_object("AFC_stepper lane1"));
        assert!(is_afc_object("filament_switch_sensor AFC_prep"));
        assert!(!is_afc_object("extruder"));
        assert!(is_fan_object("heater_fan hotend_fan"));
        assert!(!is_fan_object("fanatic"));
        assert!(is_heater_object(
            "temperature_sensor x",
            &json!({"temperature": 1, "target": 0})
        ));
        assert!(!is_heater_object(
            "temperature_sensor x",
            &json!({"temperature": 1})
        ));
        assert_eq!(tab_index("afc"), 2);
        assert_eq!(tab_index("unknown"), 0);
    }

    #[test]
    fn flattens_only_expanded_nodes() {
        let value = json!({"a": {"b": 1, "c": [true, null]}, "d": "x\ny"});
        let mut expanded = HashSet::new();
        let mut rows = Vec::new();
        flatten(&value, "t", 0, &expanded, &mut rows);
        let keys: Vec<_> = rows.iter().map(|row| row.key.to_string()).collect();
        assert_eq!(keys, ["a", "d"]);
        assert_eq!(rows[0].value, "{2}");
        assert!(rows[0].expandable && !rows[0].expanded);
        assert_eq!(rows[1].value, "\"x\\ny\"");

        collect_containers(&value, "t", &mut expanded);
        rows.clear();
        flatten(&value, "t", 0, &expanded, &mut rows);
        let shown: Vec<_> = rows
            .iter()
            .map(|row| (row.depth, row.key.to_string()))
            .collect();
        assert_eq!(
            shown,
            [
                (0, "a".into()),
                (1, "b".into()),
                (1, "c".into()),
                (2, "0".into()),
                (2, "1".into()),
                (0, "d".into())
            ]
        );
    }
}
