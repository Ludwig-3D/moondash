use std::path::Path;

pub fn custom_logo(config_path: &Path) -> Option<slint::Image> {
    ["png", "webp", "jpg", "gif"]
        .into_iter()
        .find_map(|extension| {
            let path = config_path.with_file_name(format!("moondash_logo.{extension}"));
            path.is_file()
                .then(|| slint::Image::load_from_path(&path).ok())
                .flatten()
        })
}

pub const SWATCHES: [u32; 33] = [
    0x000000, 0xffffff, 0x1a1818, 0x434343, 0x6d4c41, 0x8d6e63, 0xe53935, 0xf44336, 0xf4511e,
    0xff7043, 0xe66100, 0xfb8c00, 0xffb74d, 0xfdd835, 0xfff176, 0xc0ca33, 0x9ccc65, 0x7cb342,
    0x43a047, 0x66bb6a, 0x00897b, 0x26a69a, 0x00acc1, 0x4dd0e1, 0x039be5, 0x64b5f6, 0x1e88e5,
    0x3949ab, 0x5e35b1, 0x9575cd, 0x8e24aa, 0xd81b60, 0xf06292,
];

pub fn theme_defaults(dark: bool) -> [u32; 2] {
    if dark {
        [0x2196f3, 0x54b6b2]
    } else {
        [0x1867c0, 0x48a9a6]
    }
}

pub fn parse_rgb(input: &str) -> Option<u32> {
    let input = input.trim();
    let hex = input.strip_prefix('#').unwrap_or(input);
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(value),
        3 => {
            let red = (value >> 8) * 17;
            let green = ((value >> 4) & 0xf) * 17;
            let blue = (value & 0xf) * 17;
            Some((red << 16) | (green << 8) | blue)
        }
        _ => None,
    }
}
