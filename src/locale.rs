use std::{collections::HashMap, sync::OnceLock};

type Catalog = HashMap<String, String>;

static ENGLISH: OnceLock<Catalog> = OnceLock::new();
static GERMAN: OnceLock<Catalog> = OnceLock::new();

fn english() -> &'static Catalog {
    ENGLISH.get_or_init(|| {
        serde_json::from_str(include_str!("locales/en.json")).expect("valid English catalog")
    })
}

fn german() -> &'static Catalog {
    GERMAN.get_or_init(|| {
        serde_json::from_str(include_str!("locales/de.json")).expect("valid German catalog")
    })
}

pub fn text(key: &str, in_german: bool) -> &str {
    if in_german && let Some(value) = german().get(key) {
        return value;
    }
    english().get(key).map_or(key, String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_catalogs_have_the_same_keys() {
        assert_eq!(english().len(), german().len());
        assert!(english().keys().all(|key| german().contains_key(key)));
        assert_eq!(text("system_starting", false), "System starting…");
        assert_eq!(text("system_starting", true), "System wird gestartet…");
    }
}
