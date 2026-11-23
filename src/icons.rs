use slint::SharedString;
use std::{collections::HashMap, sync::OnceLock};

pub fn glyph(name: &str) -> SharedString {
    static GLYPHS: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    let name = name.trim().strip_prefix("mdi-").unwrap_or(name.trim());
    let glyphs = GLYPHS.get_or_init(|| {
        include_str!("../ui/fonts/mdi-glyphs.tsv")
            .lines()
            .filter_map(|line| {
                let (name, code) = line.split_once('\t')?;
                Some((name, char::from_u32(u32::from_str_radix(code, 16).ok()?)?))
            })
            .collect()
    });
    glyphs
        .get(name)
        .copied()
        .map(|glyph| glyph.to_string().into())
        .unwrap_or_default()
}
