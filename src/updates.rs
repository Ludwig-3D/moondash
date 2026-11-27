use serde_json::Value;

pub fn name<'a>(key: &'a str, service: &'a Value) -> &'a str {
    text(service, "name").unwrap_or(key)
}

pub fn text<'a>(service: &'a Value, field: &str) -> Option<&'a str> {
    service[field].as_str().filter(|value| !value.is_empty())
}

pub fn current_version(service: &Value) -> &str {
    text(service, "full_version_string")
        .or_else(|| text(service, "version"))
        .or_else(|| text(service, "current_hash"))
        .unwrap_or("—")
}

pub fn remote_version(service: &Value) -> &str {
    text(service, "remote_version")
        .or_else(|| text(service, "remote_hash"))
        .unwrap_or("—")
}

pub fn is_system(service: &Value) -> bool {
    text(service, "configured_type") == Some("system")
}

pub fn dirty(service: &Value) -> bool {
    service["is_dirty"].as_bool() == Some(true)
        || service["dirty"].as_bool() == Some(true)
        || ["version", "remote_version", "full_version_string"]
            .into_iter()
            .filter_map(|field| text(service, field))
            .any(|value| value.to_ascii_lowercase().contains("dirty"))
}

pub fn available(service: &Value) -> bool {
    if is_system(service) {
        return service["package_count"].as_u64().unwrap_or_default() > 0;
    }
    service["commits_behind_count"].as_u64().unwrap_or_default() > 0
        || [
            ("current_hash", "remote_hash"),
            ("version", "remote_version"),
        ]
        .into_iter()
        .any(|(current, remote)| {
            text(service, current)
                .zip(text(service, remote))
                .is_some_and(|(a, b)| a != b)
        })
}

pub fn installable(service: &Value) -> bool {
    available(service) && !dirty(service) && service["is_valid"].as_bool() != Some(false)
}

pub fn packages(service: &Value) -> impl Iterator<Item = &str> {
    service["package_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|package| !package.is_empty())
}

pub fn warnings(service: &Value) -> impl Iterator<Item = &str> {
    ["warnings", "anomalies"]
        .into_iter()
        .flat_map(|field| service[field].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .filter(|message| !message.is_empty())
}

pub fn includes_moondash(service: &Value) -> bool {
    is_system(service)
        && packages(service).any(|package| {
            package
                .trim()
                .split(['=', ':', ' ', '/'])
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("moondash"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_updates_without_enabling_dirty_or_invalid_repositories() {
        let repo = json!({"current_hash": "a", "remote_hash": "b"});
        assert!(installable(&repo));

        for props in [
            json!({"commits_behind_count": 1, "full_version_string": "v1-dirty"}),
            json!({"version": "v1", "remote_version": "v2", "is_valid": false}),
        ] {
            assert!(available(&props));
            assert!(!installable(&props));
        }

        assert!(!available(&json!({
            "configured_type": "system", "package_count": 0, "version": "a", "remote_version": "b"
        })));
    }
}
