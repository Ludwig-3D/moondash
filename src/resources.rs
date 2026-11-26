use serde_json::{Map, Value};
use std::collections::HashMap;

pub fn ring_chart(
    tracks: &[f64],
    arcs: &[(f64, f64, f64, slint::Color)],
    track: slint::Color,
) -> slint::Image {
    use std::fmt::Write;

    let css = |color: slint::Color| {
        format!(
            "#{:02x}{:02x}{:02x}",
            color.red(),
            color.green(),
            color.blue()
        )
    };
    let mut svg = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="144" height="144" viewBox="0 0 144 144"><g fill="none" stroke-width="10" transform="rotate(-90 72 72)">"#,
    );
    for radius in tracks {
        let _ = write!(
            svg,
            r#"<circle cx="72" cy="72" r="{radius}" stroke="{}"/>"#,
            css(track)
        );
    }
    for &(radius, start, fraction, color) in arcs {
        if fraction <= 0. {
            continue;
        }
        let length = std::f64::consts::TAU * radius;
        let _ = write!(
            svg,
            r#"<circle cx="72" cy="72" r="{radius}" stroke="{}" stroke-dasharray="{} {}" stroke-dashoffset="{}"/>"#,
            css(color),
            length * fraction,
            length * (1. - fraction),
            -length * start
        );
    }
    svg.push_str("</g></svg>");
    slint::Image::load_from_svg_data(svg.as_bytes()).unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HostMetrics {
    pub cpu_usage: Option<f64>,
    pub cpu_temperature: Option<f64>,
    pub memory_usage: Option<f64>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub storage_usage: Option<f64>,
    pub storage_used: Option<u64>,
    pub storage_total: Option<u64>,
    pub operating_system: Option<String>,
    pub processor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct McuMetrics {
    pub key: String,
    pub name: String,
    pub version: Option<String>,
    pub chipset: Option<String>,
    pub load_ms: Option<f64>,
    pub usage_percent: Option<f64>,
    pub frequency_hz: Option<f64>,
    pub temperature: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryStats {
    pub total_jobs: Option<u64>,
    pub total_print_time: Option<f64>,
    pub longest_job: Option<f64>,
    pub total_filament_used: Option<f64>,
    pub statuses: Vec<(String, u64)>,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryPages {
    pub totals: Value,
    pub jobs: Vec<Value>,
    expected: Option<usize>,
}

impl HistoryPages {
    pub fn set_totals(&mut self, totals: Value) -> bool {
        let Some(job_totals) = totals["job_totals"].as_object() else {
            return false;
        };
        self.expected = job_totals
            .get("total_jobs")
            .and_then(Value::as_u64)
            .and_then(|count| usize::try_from(count).ok());
        self.totals = totals;
        true
    }

    pub fn expected(&self) -> Option<usize> {
        self.expected
    }

    pub fn push_page(&mut self, result: &Value) -> Result<bool, ()> {
        let Some(jobs) = result["jobs"].as_array() else {
            return Err(());
        };
        self.jobs.extend(jobs.iter().cloned());
        Ok(!jobs.is_empty())
    }
}

pub fn host_metrics(system_info: &Value, proc_stats: &Value, storage: &Value) -> HostMetrics {
    let system_memory = &proc_stats["system_memory"];
    let memory_unit = system_memory["units"]
        .as_str()
        .or_else(|| system_memory["unit"].as_str())
        .unwrap_or("kb");
    let memory_total = first_number(&[
        &system_memory["total"],
        &system_memory["total_memory"],
        &system_memory["mem_total"],
    ])
    .map(|value| memory_bytes(value, memory_unit))
    .or_else(|| {
        let cpu_info = &system_info["cpu_info"];
        first_number(&[&cpu_info["total_memory"]])
            .map(|value| memory_bytes(value, cpu_info["memory_units"].as_str().unwrap_or("kB")))
    });
    let explicit_used = first_number(&[
        &system_memory["used"],
        &system_memory["used_memory"],
        &system_memory["mem_used"],
    ])
    .map(|value| memory_bytes(value, memory_unit));
    let available = first_number(&[
        &system_memory["available"],
        &system_memory["available_memory"],
        &system_memory["mem_available"],
    ])
    .map(|value| memory_bytes(value, memory_unit));
    let free = first_number(&[&system_memory["free"], &system_memory["mem_free"]]);
    let buffers = first_number(&[&system_memory["buffers"]]).unwrap_or_default();
    let cached = first_number(&[&system_memory["cached"]]).unwrap_or_default();
    let memory_used = explicit_used.or_else(|| {
        let total = memory_total?;
        if let Some(available) = available {
            return Some(total.saturating_sub(available));
        }
        free.map(|free| total.saturating_sub(memory_bytes(free + buffers + cached, memory_unit)))
    });
    let storage_used = first_number(&[&storage["disk_usage"]["used"], &storage["used"]])
        .map(|value| value.max(0.) as u64);
    let storage_free = first_number(&[&storage["disk_usage"]["free"], &storage["free"]])
        .map(|value| value.max(0.) as u64);
    let storage_total = first_number(&[&storage["disk_usage"]["total"], &storage["total"]])
        .map(|value| value.max(0.) as u64)
        .or_else(|| Some(storage_used?.saturating_add(storage_free?)));

    HostMetrics {
        cpu_usage: first_number(&[
            &proc_stats["system_cpu_usage"]["cpu"],
            &proc_stats["moonraker_stats"]["cpu_usage"],
            &proc_stats["moonrakerStats"]["cpuUsage"],
        ])
        .or_else(|| {
            first_number(&[&proc_stats["moonraker_stats"].as_array()?.last()?["cpu_usage"]])
        })
        .map(normalize_percent),
        cpu_temperature: first_number(&[
            &proc_stats["cpu_temp"],
            &proc_stats["cpuTemp"],
            &system_info["cpu_info"]["temperature"],
        ]),
        memory_usage: first_number(&[
            &system_memory["percent"],
            &system_memory["usage"],
            &system_memory["used_percent"],
        ])
        .map(normalize_percent)
        .or_else(|| {
            memory_total
                .filter(|total| *total > 0)
                .zip(memory_used)
                .map(|(total, used)| normalize_percent(used as f64 / total as f64 * 100.))
        }),
        memory_used,
        memory_total,
        storage_usage: storage_total
            .filter(|total| *total > 0)
            .zip(storage_used)
            .map(|(total, used)| normalize_percent(used as f64 / total as f64 * 100.)),
        storage_used,
        storage_total,
        operating_system: nonempty_string(&system_info["distribution"]["pretty_name"]),
        processor: nonempty_string(&system_info["cpu_info"]["cpu_desc"]),
    }
}

pub fn mcu_metrics(status: &Map<String, Value>) -> Vec<McuMetrics> {
    let mut temperatures = HashMap::new();
    let config = &status.get("configfile").unwrap_or(&Value::Null)["config"];
    for (key, raw) in status {
        if !key.starts_with("temperature_sensor ") && !key.starts_with("temperature_fan ") {
            continue;
        }
        let sensor_config = &config[key];
        if sensor_config["sensor_type"].as_str() != Some("temperature_mcu") {
            continue;
        }
        let configured_mcu = sensor_config["sensor_mcu"].as_str().unwrap_or("mcu");
        let mcu_name = configured_mcu
            .strip_prefix("mcu ")
            .unwrap_or(configured_mcu);
        if let Some(temperature) = first_number(&[&raw["temperature"]]) {
            temperatures.insert(mcu_name.to_owned(), temperature);
        }
    }

    let mut mcus: Vec<_> = status
        .iter()
        .filter(|(key, _)| key.as_str() == "mcu" || key.starts_with("mcu "))
        .map(|(key, raw)| {
            let name = key.strip_prefix("mcu ").unwrap_or("mcu");
            let average = first_number(&[&raw["last_stats"]["mcu_task_avg"]]);
            let deviation = first_number(&[&raw["last_stats"]["mcu_task_stddev"]]);
            let load_ms = (average.is_some() || deviation.is_some())
                .then(|| (average.unwrap_or_default() + deviation.unwrap_or_default()) * 1000.);
            McuMetrics {
                key: key.clone(),
                name: name.to_owned(),
                version: {
                    let app = raw["app"].as_str().filter(|value| !value.is_empty());
                    let version = raw["mcu_version"]
                        .as_str()
                        .filter(|value| !value.is_empty());
                    match (app, version) {
                        (Some(app), Some(version)) => Some(format!("{app} {version}")),
                        (Some(app), None) => Some(app.to_owned()),
                        (None, Some(version)) => Some(version.to_owned()),
                        (None, None) => None,
                    }
                },
                chipset: nonempty_string(&raw["mcu_constants"]["MCU"]),
                load_ms,
                usage_percent: first_number(&[&raw["last_stats"]["mcu_awake"]])
                    .map(|value| normalize_percent(value * 100.)),
                frequency_hz: first_number(&[
                    &raw["last_stats"]["freq"],
                    &raw["mcu_constants"]["CLOCK_FREQ"],
                ]),
                temperature: first_number(&[&raw["temperature"]])
                    .or_else(|| temperatures.get(name).copied()),
            }
        })
        .collect();
    mcus.sort_by(
        |left, right| match (left.key == "mcu", right.key == "mcu") {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.name.cmp(&right.name),
        },
    );
    mcus
}

pub fn history_stats(totals: &Value, jobs: &[Value]) -> HistoryStats {
    let mut statuses = HashMap::<String, u64>::new();
    for job in jobs {
        let status = job["status"]
            .as_str()
            .filter(|status| !status.is_empty())
            .unwrap_or("other");
        let status = match status {
            "completed" | "cancelled" | "error" | "klippy_shutdown" | "interrupted"
            | "in_progress" => status,
            _ => "other",
        };
        *statuses.entry(status.to_owned()).or_default() += 1;
    }
    let mut statuses: Vec<_> = statuses.into_iter().collect();
    statuses.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let counted_jobs = statuses.iter().map(|(_, count)| count).sum();
    let totals = &totals["job_totals"];
    HistoryStats {
        total_jobs: totals["total_jobs"]
            .as_u64()
            .or_else(|| (!jobs.is_empty()).then_some(counted_jobs)),
        total_print_time: first_number(&[&totals["total_print_time"]]),
        longest_job: first_number(&[&totals["longest_job"]]),
        total_filament_used: first_number(&[&totals["total_filament_used"]]),
        statuses,
    }
}

pub fn format_bytes(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "—".into();
    };
    let units = ["B", "kB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut index = 0;
    while size >= 1024. && index < units.len() - 1 {
        size /= 1024.;
        index += 1;
    }
    let digits = if index >= 3 || size < 10. { 1 } else { 0 };
    format!("{size:.digits$} {}", units[index])
}

pub fn format_duration(value: Option<f64>) -> String {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return "—".into();
    };
    let seconds = value.max(0.) as u64;
    let hours = seconds / 3600;
    let minutes = seconds % 3600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

pub fn format_filament(value_mm: Option<f64>) -> String {
    value_mm
        .filter(|value| value.is_finite())
        .map(|value| format!("{:.2} m", value.max(0.) / 1000.))
        .unwrap_or_else(|| "—".into())
}

pub fn normalize_percent(value: f64) -> f64 {
    value.clamp(0., 100.)
}

fn memory_bytes(value: f64, unit: &str) -> u64 {
    let value = if unit.to_ascii_lowercase().contains('g') {
        value * 1024_f64.powi(3)
    } else if unit.to_ascii_lowercase().contains('m') {
        value * 1024_f64.powi(2)
    } else if unit.to_ascii_lowercase().contains('k') {
        value * 1024.
    } else {
        value
    };
    value.max(0.).min(u64::MAX as f64) as u64
}

fn first_number(values: &[&Value]) -> Option<f64> {
    values.iter().find_map(|value| {
        value
            .as_f64()
            .filter(|number| number.is_finite())
            .or_else(|| value.as_i64().map(|number| number as f64))
            .or_else(|| value.as_u64().map(|number| number as f64))
    })
}

fn nonempty_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ring_chart_loads_multiple_arcs_and_empty_tracks() {
        let color = slint::Color::from_rgb_u8(255, 0, 0);
        let image = ring_chart(
            &[60., 40.],
            &[(60., 0., 0.25, color), (60., 0.25, 0.75, color)],
            color,
        );
        assert_eq!((image.size().width, image.size().height), (144, 144));
    }

    #[test]
    fn host_metrics_convert_kilobytes_and_ignore_missing_values() {
        let system_info = json!({
            "distribution": {"pretty_name": "Test Linux"},
            "cpu_info": {"cpu_desc": "Test CPU", "temperature": 47.5}
        });
        let proc_stats = json!({
            "system_cpu_usage": {"cpu": 24.5},
            "system_memory": {"units": "kB", "total": 1024, "available": 256}
        });
        let host = host_metrics(&system_info, &proc_stats, &Value::Null);
        assert_eq!(host.cpu_usage, Some(24.5));
        assert_eq!(host.cpu_temperature, Some(47.5));
        assert_eq!(host.memory_total, Some(1_048_576));
        assert_eq!(host.memory_used, Some(786_432));
        assert_eq!(host.memory_usage, Some(75.));
        assert_eq!(host.storage_usage, None);
        assert_eq!(host.operating_system.as_deref(), Some("Test Linux"));
    }

    #[test]
    fn mcu_metrics_match_configured_temperature_sensor_and_sort_main_first() {
        let status: Map<String, Value> = serde_json::from_value(json!({
            "mcu z": {
                "mcu_version": "v1",
                "mcu_constants": {"MCU": "chip-z", "CLOCK_FREQ": 16000000},
                "last_stats": {"mcu_task_avg": 0.0001, "mcu_task_stddev": 0.0002, "mcu_awake": 0.25, "freq": 15900000}
            },
            "mcu": {
                "app": "Klipper",
                "mcu_version": "v2",
                "mcu_constants": {"MCU": "chip-main"},
                "last_stats": {"mcu_task_avg": 0.0002, "mcu_task_stddev": 0.0001}
            },
            "temperature_sensor board": {"temperature": 42.0},
            "configfile": {"config": {
                "temperature_sensor board": {"sensor_type": "temperature_mcu", "sensor_mcu": "mcu z"}
            }}
        }))
        .unwrap();
        let mcus = mcu_metrics(&status);
        assert_eq!(
            mcus.iter().map(|mcu| mcu.key.as_str()).collect::<Vec<_>>(),
            ["mcu", "mcu z"]
        );
        assert_eq!(mcus[0].version.as_deref(), Some("Klipper v2"));
        assert_eq!(mcus[1].temperature, Some(42.));
        assert_eq!(mcus[1].frequency_hz, Some(15_900_000.));
        assert!(
            mcus[1]
                .load_ms
                .is_some_and(|value| (value - 0.3).abs() < 1e-9)
        );
        assert!(
            mcus[1]
                .usage_percent
                .is_some_and(|value| (value - 25.).abs() < 1e-9)
        );
        assert_eq!(mcus[0].usage_percent, None);
    }

    #[test]
    fn mcu_usage_keeps_zero_and_missing_values_distinct() {
        let status: Map<String, Value> = serde_json::from_value(json!({
            "mcu": {"last_stats": {"mcu_awake": 0.0}},
            "mcu auxiliary": {"last_stats": {}}
        }))
        .unwrap();
        let mcus = mcu_metrics(&status);
        assert_eq!(mcus[0].usage_percent, Some(0.));
        assert_eq!(mcus[1].usage_percent, None);
    }

    #[test]
    fn history_pages_fetch_complete_pages_before_publication() {
        let mut pages = HistoryPages::default();
        assert!(pages.set_totals(json!({"job_totals": {"total_jobs": 3}})));
        assert_eq!(pages.expected(), Some(3));
        assert!(
            pages
                .push_page(&json!({"jobs": [{"id": 1}, {"id": 2}]}))
                .unwrap()
        );
        assert_eq!(pages.jobs.len(), 2);
        assert!(pages.push_page(&json!({"jobs": [{"id": 3}]})).unwrap());
        assert_eq!(pages.jobs.len(), 3);
        assert!(!pages.push_page(&json!({"jobs": []})).unwrap());
    }

    #[test]
    fn history_pages_continue_after_short_pages_and_ignore_page_count() {
        let mut pages = HistoryPages::default();
        assert!(pages.set_totals(json!({"job_totals": {"total_jobs": 10}})));
        assert!(
            pages
                .push_page(&json!({"count": 1, "jobs": [{"id": 1}]}))
                .unwrap()
        );
        assert_eq!(pages.expected(), Some(10));
        assert_eq!(pages.jobs.len(), 1);
        assert!(
            pages
                .push_page(&json!({"count": 1, "jobs": [{"id": 2}]}))
                .unwrap()
        );
        assert!(!pages.push_page(&json!({"count": 0, "jobs": []})).unwrap());
        assert_eq!(pages.jobs.len(), 2);

        let mut without_totals = HistoryPages::default();
        assert!(
            without_totals
                .push_page(&json!({"count": 1, "jobs": [{"id": 1}]}))
                .unwrap()
        );
        assert_eq!(without_totals.expected(), None);
    }

    #[test]
    fn history_pages_continue_despite_zero_or_stale_totals() {
        let mut zero = HistoryPages::default();
        assert!(zero.set_totals(json!({"job_totals": {"total_jobs": 0}})));
        assert!(
            zero.push_page(&json!({"count": 1, "jobs": [{"id": 1}]}))
                .unwrap()
        );
        assert!(!zero.push_page(&json!({"count": 0, "jobs": []})).unwrap());
        assert_eq!(zero.jobs.len(), 1);

        let mut stale = HistoryPages::default();
        assert!(stale.set_totals(json!({"job_totals": {"total_jobs": 1}})));
        assert!(
            stale
                .push_page(&json!({"count": 2, "jobs": [{"id": 1}, {"id": 2}]}))
                .unwrap()
        );
        assert!(!stale.push_page(&json!({"count": 0, "jobs": []})).unwrap());
        assert_eq!(stale.jobs.len(), 2);
    }

    #[test]
    fn history_pages_fetch_more_than_the_old_limit() {
        let mut pages = HistoryPages::default();
        let jobs = (0..10_001).map(|id| json!({"id": id})).collect::<Vec<_>>();
        assert!(
            pages
                .push_page(&json!({"count": jobs.len(), "jobs": jobs}))
                .unwrap()
        );
        assert_eq!(pages.jobs.len(), 10_001);
        assert!(!pages.push_page(&json!({"count": 0, "jobs": []})).unwrap());
    }

    #[test]
    fn history_pages_distinguish_empty_from_unavailable() {
        let mut pages = HistoryPages::default();
        assert!(!pages.set_totals(json!({"error": "unsupported"})));

        assert!(pages.set_totals(json!({"job_totals": {"total_jobs": 0}})));
        assert_eq!(pages.expected(), Some(0));
        assert!(pages.jobs.is_empty());
    }

    #[test]
    fn history_stats_aggregate_job_statuses_and_preserve_missing_totals() {
        let totals = json!({"job_totals": {
            "total_jobs": 8,
            "total_print_time": 3600.0,
            "longest_job": 1800.0,
            "total_filament_used": 12500.0
        }});
        let jobs = [
            json!({"status": "completed"}),
            json!({"status": "error"}),
            json!({"status": "completed"}),
            json!({}),
            json!({"status": "paused"}),
            json!({"status": "vendor_status"}),
        ];
        let history = history_stats(&totals, &jobs);
        assert_eq!(history.total_jobs, Some(8));
        assert_eq!(history.total_print_time, Some(3600.));
        assert_eq!(
            history.statuses,
            [
                ("other".into(), 3),
                ("completed".into(), 2),
                ("error".into(), 1)
            ]
        );
        assert_eq!(format_duration(Some(3661.)), "1h 1m");
        assert_eq!(format_filament(Some(12500.)), "12.50 m");
        assert_eq!(history_stats(&Value::Null, &[]).total_print_time, None);
        assert_eq!(history_stats(&Value::Null, &[]).total_jobs, None);
        assert_eq!(
            history_stats(&json!({"job_totals": {"total_jobs": 0}}), &[]).total_jobs,
            Some(0)
        );
    }
}
