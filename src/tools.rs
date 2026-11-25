use crate::moonraker;
use reqwest::Url;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Webcam {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshProfile {
    pub name: String,
    pub variance: Option<f64>,
}

pub fn bed_mesh_matrix(mesh: &Value) -> Vec<Vec<f64>> {
    let matrix = [
        "mesh_matrix",
        "probed_matrix",
        "matrix",
        "points",
        "z_values",
    ]
    .into_iter()
    .find_map(|key| mesh.get(key).filter(|value| !value.is_null()));
    let Some(rows) = matrix.and_then(Value::as_array) else {
        return Vec::new();
    };

    let parsed: Vec<Vec<f64>> = rows
        .iter()
        .filter_map(|row| {
            let values = row.as_array()?;
            let numbers: Option<Vec<_>> = values.iter().map(Value::as_f64).collect();
            numbers.filter(|numbers| !numbers.is_empty())
        })
        .collect();
    let columns = parsed.first().map_or(0, Vec::len);
    if columns == 0 || parsed.iter().any(|row| row.len() != columns) {
        Vec::new()
    } else {
        parsed
    }
}

pub fn mesh_range(matrix: &[Vec<f64>]) -> Option<(f64, f64)> {
    let mut values = matrix.iter().flatten().copied();
    let first = values.next()?;
    let (min, max) = values.fold((first, first), |(min, max), value| {
        (min.min(value), max.max(value))
    });
    Some((min, max))
}

pub fn mesh_color(value: f64, range: Option<(f64, f64)>) -> (u8, u8, u8) {
    let (minimum, maximum) = range.unwrap_or((value, value));
    if maximum <= minimum {
        return (200, 200, 200);
    }

    let position = ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0);
    let (start, end, fraction) = if position < 0.33 {
        ((50, 100, 255), (80, 200, 120), position / 0.33)
    } else if position < 0.66 {
        ((80, 200, 120), (255, 220, 80), (position - 0.33) / 0.33)
    } else {
        ((255, 220, 80), (255, 80, 70), (position - 0.66) / 0.34)
    };
    let channel = |start: u8, end: u8| {
        (f64::from(start) + (f64::from(end) - f64::from(start)) * fraction).round() as u8
    };
    (
        channel(start.0, end.0),
        channel(start.1, end.1),
        channel(start.2, end.2),
    )
}

pub fn mesh_profiles(profiles: &Value) -> Vec<MeshProfile> {
    let Some(profiles) = profiles.as_object() else {
        return Vec::new();
    };

    let mut profiles: Vec<_> = profiles
        .iter()
        .filter_map(|(name, profile)| {
            profile.as_object()?;
            Some(MeshProfile {
                name: name.clone(),
                variance: profile_variance(profile),
            })
        })
        .collect();
    profiles.sort_by_cached_key(|profile| profile.name.to_lowercase());
    profiles
}

fn profile_variance(profile: &Value) -> Option<f64> {
    [
        "variance",
        "saved_variance",
        "profile_variance",
        "mesh_variance",
    ]
    .into_iter()
    .find_map(|key| json_number(&profile[key]))
    .or_else(|| number_range(&profile["points"]))
}

fn json_number(value: &Value) -> Option<f64> {
    let number = value.as_f64().or_else(|| value.as_str()?.parse().ok())?;
    number.is_finite().then_some(number)
}

fn number_range(value: &Value) -> Option<f64> {
    fn collect(value: &Value, numbers: &mut Vec<f64>) {
        if let Some(number) = json_number(value) {
            numbers.push(number);
        } else if let Some(values) = value.as_array() {
            for value in values {
                collect(value, numbers);
            }
        }
    }

    let mut numbers = Vec::new();
    collect(value, &mut numbers);
    let minimum = numbers.iter().copied().reduce(f64::min)?;
    let maximum = numbers.into_iter().reduce(f64::max)?;
    Some(maximum - minimum)
}

pub fn pid_target(value: &str) -> Option<u16> {
    let target = value.trim().parse::<u16>().ok()?;
    (target <= 999).then_some(target)
}

pub fn append_pid_digit(value: &str, digit: &str) -> String {
    if digit.len() != 1 || !digit.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.to_owned();
    }
    let current = value.parse::<u16>().unwrap_or_default();
    let next = format!("{current}{digit}")
        .parse::<u32>()
        .unwrap_or(999)
        .min(999);
    next.to_string()
}

pub fn webcams(result: &Value, endpoint: &str) -> Vec<Webcam> {
    let Some(cameras) = result["webcams"]
        .as_array()
        .or_else(|| result["result"]["webcams"].as_array())
    else {
        return Vec::new();
    };

    cameras
        .iter()
        .enumerate()
        .filter(|(_, camera)| camera["enabled"].as_bool() != Some(false))
        .filter_map(|(index, camera)| {
            let raw_url = [
                "snapshot_url",
                "url_snapshot",
                "stream_url",
                "url_stream",
                "mjpg_url",
                "webrtc_url",
                "url",
            ]
            .into_iter()
            .find_map(|key| camera[key].as_str().filter(|url| !url.trim().is_empty()))?;
            let url = normalize_webcam_url(raw_url, endpoint)?;
            let name = camera["name"]
                .as_str()
                .or_else(|| camera["title"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Webcam {}", index + 1));
            let id = camera["uid"]
                .as_str()
                .or_else(|| camera["id"].as_str())
                .or_else(|| camera["name"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| index.to_string());
            Some(Webcam { id, name, url })
        })
        .collect()
}

pub fn normalize_webcam_url(raw: &str, endpoint: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let base = Url::parse(moonraker::http_endpoint(endpoint, &[]).ok()?.as_str()).ok()?;
    let mut url = Url::parse(raw).or_else(|_| base.join(raw)).ok()?;

    if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")) {
        url.set_host(base.host_str()).ok()?;
    }
    if url.path().contains("/webrtc") {
        url.set_path(&url.path().replace("/webrtc", "/"));
        url.set_query(Some("action=snapshot"));
    }
    Some(url.to_string())
}

pub fn cache_bust_webcam_url(raw: &str, nonce: u64) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    url.query_pairs_mut().append_pair("_ts", &nonce.to_string());
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        append_pid_digit, bed_mesh_matrix, cache_bust_webcam_url, mesh_color, mesh_profiles,
        mesh_range, normalize_webcam_url, pid_target, webcams,
    };
    use serde_json::json;

    #[test]
    fn parses_only_rectangular_numeric_bed_meshes_and_computes_range() {
        let mesh = json!({"mesh_matrix": [[-0.1, 0.2], [0.0, 0.35]]});
        let matrix = bed_mesh_matrix(&mesh);
        assert_eq!(matrix, [[-0.1, 0.2], [0.0, 0.35]]);
        assert_eq!(mesh_range(&matrix), Some((-0.1, 0.35)));
        assert!(bed_mesh_matrix(&json!({"mesh_matrix": [[0.1, 0.2], [0.2]]})).is_empty());
        assert_eq!(mesh_range(&[]), None);
    }

    #[test]
    fn matches_archived_bed_mesh_colors_and_profile_variance_fallbacks() {
        assert_eq!(mesh_color(0.0, Some((0.0, 1.0))), (50, 100, 255));
        assert_eq!(mesh_color(0.33, Some((0.0, 1.0))), (80, 200, 120));
        assert_eq!(mesh_color(0.66, Some((0.0, 1.0))), (255, 220, 80));
        assert_eq!(mesh_color(1.0, Some((0.0, 1.0))), (255, 80, 70));
        assert_eq!(mesh_color(1.0, Some((1.0, 1.0))), (200, 200, 200));

        let profiles = mesh_profiles(&json!({
            "zeta": {"points": [[0.1, -0.2], [0.3, 0.0]]},
            "alpha": {"variance": 0.125, "points": [[-1.0, 1.0]]},
            "beta": {"saved_variance": "0.456"},
            "invalid": {"points": [["bad"]]},
            "not a profile": "ignored"
        }));
        assert_eq!(profiles.len(), 4);
        assert_eq!(profiles[0].name, "alpha");
        assert_eq!(profiles[0].variance, Some(0.125));
        assert_eq!(profiles[1].name, "beta");
        assert_eq!(profiles[1].variance, Some(0.456));
        assert_eq!(profiles[2].name, "invalid");
        assert_eq!(profiles[2].variance, None);
        assert_eq!(profiles[3].name, "zeta");
        assert_eq!(profiles[3].variance, Some(0.5));
    }

    #[test]
    fn bounds_pid_keypad_values_and_rejects_invalid_targets() {
        assert_eq!(pid_target(" 200 "), Some(200));
        assert_eq!(pid_target("1000"), None);
        assert_eq!(pid_target(""), None);
        assert_eq!(append_pid_digit("", "2"), "2");
        assert_eq!(append_pid_digit("99", "9"), "999");
        assert_eq!(append_pid_digit("999", "9"), "999");
        assert_eq!(append_pid_digit("12", "x"), "12");
    }

    #[test]
    fn discovers_enabled_webcams_and_normalizes_snapshot_urls() {
        let cameras = webcams(
            &json!({
                "result": {"webcams": [
                    {"name": "Front", "uid": "front", "enabled": true, "snapshot_url": "http://localhost:8080/webcam/?action=stream"},
                    {"name": "Disabled", "enabled": false, "snapshot_url": "/off"},
                    {"name": "No URL", "enabled": true}
                ]}
            }),
            "http://printer.local:7125",
        );
        assert_eq!(cameras.len(), 1);
        assert_eq!(cameras[0].id, "front");
        assert_eq!(cameras[0].name, "Front");
        assert_eq!(
            cameras[0].url,
            "http://printer.local:8080/webcam/?action=stream"
        );
        assert_eq!(
            normalize_webcam_url("/webcam/webrtc", "http://printer.local:7125").as_deref(),
            Some("http://printer.local:7125/webcam/?action=snapshot")
        );
        assert_eq!(
            cache_bust_webcam_url("http://printer.local/snapshot?action=snapshot", 7).as_deref(),
            Some("http://printer.local/snapshot?action=snapshot&_ts=7")
        );
        assert!(webcams(&json!({"webcams": []}), "printer.local").is_empty());
    }
}
