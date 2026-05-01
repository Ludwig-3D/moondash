use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct ThemeAssets(pub Mutex<Value>);

const FALLBACK_LOGO: &[u8] = include_bytes!("../../images/logo.png");

const LOGO_CANDIDATES: [(&str, &str); 4] = [
    ("moondash_logo.png", "image/png"),
    ("moondash_logo.webp", "image/webp"),
    ("moondash_logo.jpg", "image/jpeg"),
    ("moondash_logo.gif", "image/gif"),
];

#[tauri::command]
pub fn get_theme_assets(state: State<ThemeAssets>) -> Result<Value, String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "failed to lock theme assets state".to_string())?;

    Ok(guard.clone())
}

pub fn default_theme_assets() -> Value {
    let logo_base64 = general_purpose::STANDARD.encode(FALLBACK_LOGO);

    json!({
        "css": "",
        "logo": format!("data:image/png;base64,{logo_base64}")
    })
}

pub fn load_theme_assets(config_path: &str) -> Value {
    eprintln!("[theme] load_theme_assets called");
    eprintln!("[theme] config_path: {config_path}");

    let config_path_ref = Path::new(config_path);

    let base_dir = config_path_ref.parent().unwrap_or_else(|| Path::new("."));

    eprintln!("[theme] base_dir: {}", base_dir.display());

    let css_path = base_dir.join("moondash.css");

    let css = match fs::read_to_string(&css_path) {
        Ok(content) => {
            eprintln!("[theme] loaded css: {} chars", content.len());
            content
        }
        Err(err) => {
            eprintln!(
                "[theme] failed to load css '{}', using empty string: {err}",
                css_path.display()
            );
            String::new()
        }
    };

    let mut loaded_logo: Option<(Vec<u8>, &'static str, String)> = None;

    for (file_name, mime_type) in LOGO_CANDIDATES {
        let logo_path = base_dir.join(file_name);

        eprintln!("[theme] checking logo: {}", logo_path.display());

        match fs::read(&logo_path) {
            Ok(bytes) => {
                eprintln!(
                    "[theme] loaded custom logo '{}', mime={}, bytes={}",
                    logo_path.display(),
                    mime_type,
                    bytes.len()
                );

                loaded_logo = Some((bytes, mime_type, logo_path.display().to_string()));
                break;
            }
            Err(err) => {
                eprintln!(
                    "[theme] failed to read logo '{}': {err}",
                    logo_path.display()
                );
            }
        }
    }

    let (logo_bytes, mime_type, source) = loaded_logo.unwrap_or_else(|| {
        eprintln!(
            "[theme] using compiled fallback logo, bytes={}",
            FALLBACK_LOGO.len()
        );

        (
            FALLBACK_LOGO.to_vec(),
            "image/png",
            "compiled fallback images/logo.png".to_string(),
        )
    });

    let logo_base64 = general_purpose::STANDARD.encode(&logo_bytes);

    eprintln!("[theme] selected logo source: {source}");
    eprintln!("[theme] selected logo mime: {mime_type}");
    eprintln!("[theme] selected logo bytes: {}", logo_bytes.len());

    json!({
        "css": css,
        "logo": format!("data:{mime_type};base64,{logo_base64}")
    })
}

pub fn emit_theme_assets(app: &AppHandle, config_path: &str) {
    eprintln!("[theme] emit_theme_assets called");

    let payload = load_theme_assets(config_path);

    if let Some(state) = app.try_state::<ThemeAssets>() {
        match state.0.lock() {
            Ok(mut guard) => {
                *guard = payload.clone();
                eprintln!("[theme] stored theme assets in state");
            }
            Err(_) => {
                eprintln!("[theme] failed to lock theme assets state");
            }
        }
    } else {
        eprintln!("[theme] ThemeAssets state not available");
    }

    match app.emit("theme-loaded", payload) {
        Ok(()) => eprintln!("[theme] emitted theme-loaded"),
        Err(err) => eprintln!("[theme] failed to emit theme-loaded: {err}"),
    }
}