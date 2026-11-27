use crate::{DashboardStats, StatsAxis, StatsUsage, resources};
use serde_json::{Map, Value};

use super::number;

pub(super) fn dashboard(status: &Map<String, Value>, host: &Value, german: bool) -> DashboardStats {
    let get = |name: &str| status.get(name).unwrap_or(&Value::Null);
    let toolhead = get("toolhead");
    let gcode = get("gcode_move");
    let motion = get("motion_report");
    let config = get("configfile");
    let proc_stats = &host["stats"];
    let position = gcode["gcode_position"]
        .as_array()
        .or_else(|| toolhead["position"].as_array());
    let axis = |index| {
        let value = position
            .and_then(|position| position.get(index))
            .and_then(number);
        let min = number(&toolhead["axis_minimum"][index]);
        let max = number(&toolhead["axis_maximum"][index]);
        let fraction = value.zip(min).zip(max).and_then(|((value, min), max)| {
            let travel = max - min;
            (travel.is_finite() && travel > 0.)
                .then(|| ((value - min) / travel).clamp(0., 1.) as f32)
        });
        StatsAxis {
            value: quantity(value, 2, "", german).into(),
            fraction: fraction.unwrap_or(0.5),
            travel: (max.unwrap_or(350.) - min.unwrap_or(0.)).clamp(1., f32::MAX as f64) as f32,
            bounded: fraction.is_some(),
        }
    };

    let diameter = number(&config["config"]["extruder"]["filament_diameter"])
        .or_else(|| number(&config["settings"]["extruder"]["filament_diameter"]))
        .unwrap_or(1.75);
    let flow = number(&motion["live_extruder_velocity"])
        .map(|speed| speed.max(0.) * std::f64::consts::PI * (diameter / 2.).powi(2));
    let filament = number(&get("print_stats")["filament_used"]);
    let filament = if filament.is_some_and(|length| length >= 1000.) {
        quantity(filament.map(|length| length / 1000.), 2, " m", german)
    } else {
        quantity(filament, 2, " mm", german)
    };

    let metrics =
        resources::host_metrics(&host["info"]["system_info"], proc_stats, &host["storage"]);
    let cpu = number(&proc_stats["system_cpu_usage"])
        .or_else(|| number(&proc_stats["system_cpu_usage"]["cpu"]));
    let temperature = metrics.cpu_temperature;
    DashboardStats {
        x: axis(0),
        y: axis(1),
        z: axis(2),
        speed: quantity(
            number(&motion["live_velocity"]).map(|speed| speed.max(0.)),
            1,
            " mm/s",
            german,
        )
        .into(),
        max_speed: quantity(number(&toolhead["max_velocity"]), 0, " mm/s", german).into(),
        acceleration: quantity(number(&toolhead["max_accel"]), 0, " mm/s²", german).into(),
        flow: quantity(flow, 2, " mm³/s", german).into(),
        filament: filament.into(),
        cpu_temperature: quantity(temperature, 0, " °C", german).into(),
        cpu_temperature_level: match temperature {
            Some(value) if value >= 85. => 2,
            Some(value) if value >= 70. => 1,
            _ => 0,
        },
        cpu: usage(cpu, german),
        ram: usage(metrics.memory_usage, german),
        storage: usage(metrics.storage_usage, german),
        uptime: number(&proc_stats["system_uptime"])
            .map_or_else(|| "—".into(), |time| uptime(time).into()),
        websockets: number(&proc_stats["websocket_connections"])
            .map_or_else(|| "—".into(), |count| format!("{count:.0}").into()),
    }
}

fn usage(percent: Option<f64>, german: bool) -> StatsUsage {
    let percent = percent.map(resources::normalize_percent);
    StatsUsage {
        value: quantity(percent, 0, " %", german).into(),
        fraction: (percent.unwrap_or(0.) / 100.) as f32,
    }
}

fn quantity(value: Option<f64>, precision: usize, unit: &str, german: bool) -> String {
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return "—".into();
    };
    let text = format!("{value:.precision$}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let (sign, digits) = whole
        .strip_prefix('-')
        .map_or(("", whole), |digits| ("-", digits));
    let (group_separator, decimal_separator) = if german { ('.', ',') } else { (',', '.') };
    let mut result = String::from(sign);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(group_separator);
        }
        result.push(digit);
    }
    if !fraction.is_empty() {
        result.push(decimal_separator);
        result.push_str(fraction);
    }
    result.push_str(unit);
    result
}

fn uptime(seconds: f64) -> String {
    let seconds = seconds as u64;
    let (days, hours, minutes) = (seconds / 86400, seconds / 3600 % 24, seconds / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_motion_geometry_and_host_values_with_missing_data() {
        let status = json!({
            "gcode_move": {"gcode_position": [45, 400, null]},
            "toolhead": {"axis_minimum": [-5, 0, 0], "axis_maximum": [95, 300, 400], "max_accel": 5000},
            "motion_report": {"live_velocity": 40, "live_extruder_velocity": -1},
            "print_stats": {"filament_used": 1250},
        }).as_object().unwrap().clone();
        let stats = dashboard(
            &status,
            &json!({
                "stats": {"cpu_temp": 73, "system_cpu_usage": {"cpu": 25}, "system_uptime": 90600,
                    "websocket_connections": 3, "system_memory": {"total": 1024, "used": 512}},
                "storage": {"disk_usage": {"total": 4000, "used": 1000}},
            }),
            true,
        );
        assert_eq!(stats.x.fraction, 0.5);
        assert_eq!(stats.x.travel, 100.);
        assert_eq!(stats.y.fraction, 1.);
        assert!(!stats.z.bounded);
        assert_eq!(stats.z.value, "—");
        assert_eq!(stats.acceleration, "5.000 mm/s²");
        assert_eq!(stats.flow, "0,00 mm³/s");
        assert_eq!(stats.filament, "1,25 m");
        assert_eq!(stats.cpu_temperature_level, 1);
        assert_eq!(stats.cpu.value, "25 %");
        assert_eq!(stats.ram.fraction, 0.5);
        assert_eq!(stats.storage.fraction, 0.25);
        assert_eq!(stats.websockets, "3");
        assert_eq!(stats.uptime, "1d 1h");
    }
}
