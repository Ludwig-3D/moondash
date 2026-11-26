use serde_json::Value;

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

pub fn mesh_heatmap(matrix: &[Vec<f64>], range: (f64, f64)) -> slint::Image {
    const LONG_EDGE: usize = 384;
    let rows = matrix.len();
    let columns = matrix[0].len();
    let long_side = rows.max(columns);
    let width = (LONG_EDGE * columns / long_side).max(2);
    let height = (LONG_EDGE * rows / long_side).max(2);
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(width as u32, height as u32);

    let axis = |pixel: usize, pixels: usize, samples: usize| {
        let at = pixel as f64 * (samples - 1) as f64 / (pixels - 1) as f64;
        let first = at as usize;
        (first, (first + 1).min(samples - 1), at - first as f64)
    };
    let x_coordinates: Vec<_> = (0..width).map(|x| axis(x, width, columns)).collect();
    let mix = |a: f64, b: f64, weight: f64| a + (b - a) * weight;
    for (row, pixels) in pixels.make_mut_slice().chunks_exact_mut(width).enumerate() {
        // Klipper lists the lowest Y row first. Keep the front of the bed at the bottom.
        let (y0, y1, fy) = axis(height - 1 - row, height, rows);
        for (pixel, &(x0, x1, fx)) in pixels.iter_mut().zip(&x_coordinates) {
            let value = mix(
                mix(matrix[y0][x0], matrix[y0][x1], fx),
                mix(matrix[y1][x0], matrix[y1][x1], fx),
                fy,
            );
            let (r, g, b) = mesh_color(value, Some(range));
            *pixel = slint::Rgb8Pixel { r, g, b };
        }
    }
    slint::Image::from_rgb8(pixels)
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

#[cfg(test)]
mod tests {
    use super::{
        append_pid_digit, bed_mesh_matrix, mesh_color, mesh_profiles, mesh_range, pid_target,
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
}
