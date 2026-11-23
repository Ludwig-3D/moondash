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
