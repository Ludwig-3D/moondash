use crate::{Row, icons};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

pub fn filename(file: &Value) -> Option<&str> {
    ["path", "filename", "display"]
        .into_iter()
        .find_map(|key| file[key].as_str().filter(|path| !path.is_empty()))
        .map(|path| path.trim_start_matches('/'))
        .filter(|path| valid_path(path))
}

pub fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn display_name(path: &str) -> &str {
    let name = leaf(path);
    if let Some((title, extension)) = name.rsplit_once('.')
        && extension.eq_ignore_ascii_case("gcode")
    {
        return title;
    }
    name
}

pub fn valid_path(path: &str) -> bool {
    !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

pub fn files(response: &Value) -> &[Value] {
    response
        .as_array()
        .map(Vec::as_slice)
        .or_else(|| response["files"].as_array().map(Vec::as_slice))
        .unwrap_or_else(|| array(&response["result"]))
}

pub fn entries(response: &Value, folder: &str, filter: &str, sort: u8) -> Vec<Row> {
    let filter = filter.trim().to_lowercase();
    let prefix = if folder.is_empty() {
        String::new()
    } else {
        format!("{folder}/")
    };
    let mut folders = BTreeMap::<&str, f64>::new();
    let mut entries = Vec::new();
    for file in files(response) {
        let Some(path) = filename(file) else {
            continue;
        };
        let Some(relative) = path.strip_prefix(&prefix) else {
            continue;
        };
        let modified = file["modified"].as_f64().unwrap_or_default();
        if let Some((name, _)) = relative.split_once('/') {
            if !name.starts_with('.') && name.to_lowercase().contains(&filter) {
                folders
                    .entry(name)
                    .and_modify(|current| *current = current.max(modified))
                    .or_insert(modified);
            }
        } else {
            let title = display_name(
                file["display"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .unwrap_or(path),
            );
            if title.to_lowercase().contains(&filter) {
                let writable = file["permissions"]
                    .as_str()
                    .is_none_or(|permissions| permissions.contains('w'));
                entries.push((
                    modified,
                    Row {
                        title: title.into(),
                        argument: path.into(),
                        action: "file".into(),
                        alternate: if writable {
                            "remove-file".into()
                        } else {
                            "".into()
                        },
                        ..Default::default()
                    },
                ));
            }
        }
    }
    for (name, modified) in folders {
        entries.push((
            modified,
            Row {
                title: name.into(),
                argument: format!("{prefix}{name}").into(),
                action: "folder".into(),
                alternate: "remove-dir".into(),
                ..Default::default()
            },
        ));
    }
    entries.sort_by(|(first_time, first), (second_time, second)| {
        let first_name = first.title.to_lowercase();
        let second_name = second.title.to_lowercase();
        (first.action != "folder")
            .cmp(&(second.action != "folder"))
            .then_with(|| match sort {
                0 => second_time
                    .total_cmp(first_time)
                    .then_with(|| first_name.cmp(&second_name)),
                1 => first_time
                    .total_cmp(second_time)
                    .then_with(|| first_name.cmp(&second_name)),
                3 => second_name.cmp(&first_name),
                _ => first_name.cmp(&second_name),
            })
    });
    entries.into_iter().map(|(_, row)| row).collect()
}

pub fn estimated_time(metadata: &Value) -> Option<String> {
    let seconds = metadata["estimated_time"].as_f64()?;
    if !seconds.is_finite() || seconds < 0. {
        return None;
    }
    let minutes = (seconds / 60.) as u64;
    let mut label = String::new();
    for (value, unit) in [
        (minutes / 1440, 'd'),
        (minutes / 60 % 24, 'h'),
        (minutes % 60, 'm'),
    ] {
        if value != 0 {
            label.push_str(&format!("{value}{unit}"));
        }
    }
    Some(if label.is_empty() { "0m".into() } else { label })
}

pub fn filament_weight(metadata: &Value) -> Option<String> {
    [
        &metadata["filament_weight_total"],
        &metadata["filament_weight"],
        &metadata["filament"]["weight_total"],
        &metadata["filament"]["weight"],
    ]
    .into_iter()
    .find_map(Value::as_f64)
    .map(|grams| format!("{grams:.1}g"))
}

pub fn history(row: &mut Row, jobs: &[Value]) {
    let path = row.argument.as_str();
    let normalized = |job: &Value| {
        let name = job["filename"].as_str()?.replace('\\', "/");
        Some(
            name.rsplit_once("/gcodes/")
                .map_or(name.as_str(), |(_, suffix)| suffix)
                .trim_start_matches('/')
                .trim()
                .to_owned(),
        )
    };
    let (mut exact_count, mut leaf_count) = (0, 0);
    let (mut exact_status, mut leaf_status) = (None, None);
    for job in jobs {
        let Some(name) = normalized(job) else {
            continue;
        };
        let status = job["status"].as_str().unwrap_or_default();
        if name == path {
            exact_count += 1;
            exact_status.get_or_insert(status);
        }
        if leaf(&name) == leaf(path) {
            leaf_count += 1;
            leaf_status.get_or_insert(status);
        }
    }
    let (status, count) = if exact_count > 0 {
        (exact_status, exact_count)
    } else {
        (leaf_status, leaf_count)
    };
    if let Some(status) = status.filter(|status| !status.is_empty()) {
        let (icon, tone) = match status {
            "completed" => ("check-circle-outline", 0x4caf50),
            "failed" | "error" => ("close-circle-outline", 0xcf6679),
            "cancelled" | "klippy_shutdown" => ("alert-outline", 0xfb8c00),
            _ => ("history", 0xbdbdbd),
        };
        row.icon = icons::glyph(icon);
        row.tone = slint::Color::from_rgb_u8((tone >> 16) as u8, (tone >> 8) as u8, tone as u8);
        row.button = count.to_string().into();
    }
}

pub fn thumbnail_path(filename: &str, thumbnails: &[Value]) -> Option<String> {
    let thumbnail = thumbnails.iter().max_by_key(|item| {
        item["width"]
            .as_u64()
            .unwrap_or_default()
            .saturating_mul(item["height"].as_u64().unwrap_or(1))
    })?;
    if let Some(path) = thumbnail["thumbnail_path"]
        .as_str()
        .filter(|path| valid_path(path))
    {
        return Some(path.into());
    }
    let path = thumbnail["relative_path"]
        .as_str()
        .filter(|path| valid_path(path))?;
    Some(match filename.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{path}"),
        None => path.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn derives_folders_and_file_titles_from_the_recursive_listing() {
        let listing = json!([
            {"path":"Parts/Bolt.gcode", "modified":100},
            {"path":"Parts/Nut.gcode", "modified":200},
            {"path":"Tests/First.gcode", "modified":80},
            {"path":"Parts.GCODE", "permissions":"r"}
        ]);
        let rows = entries(&listing, "", "parts", 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].argument, "Parts");
        assert_eq!(rows[1].title, "Parts");
        assert_eq!(rows[1].argument, "Parts.GCODE");
        assert!(rows[1].alternate.is_empty());
        assert_eq!(entries(&listing, "Parts", "", 0)[0].title, "Nut");
    }

    #[test]
    fn resolves_both_moonraker_thumbnail_path_formats() {
        let thumbnails = json!([
            {"thumbnail_path":"Parts/.thumbs/file-300.png", "width":300, "height":200},
            {"relative_path":".thumbs/file-32.png", "width":32, "height":32}
        ]);
        assert_eq!(
            thumbnail_path("Parts/file.gcode", array(&thumbnails)).as_deref(),
            Some("Parts/.thumbs/file-300.png")
        );
        assert_eq!(
            thumbnail_path("Parts/file.gcode", &array(&thumbnails)[1..]).as_deref(),
            Some("Parts/.thumbs/file-32.png")
        );
    }
}
