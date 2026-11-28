mod app;
mod config;
mod icons;
mod input_idle;
mod locale;
mod logging;
mod mesh_view;
mod moonraker;
mod network;
mod print_files;
mod renderer;
mod resources;
mod shortcuts;
mod theme;
mod tools;
mod updates;
mod version;
mod wayland_power;
mod webcam;

use renderer::Renderer;
use slint::ComponentHandle;
use std::{cell::Cell, path::PathBuf, process::ExitCode, rc::Rc};

slint::include_modules!();

const RETRY_RENDERER: &str = "MOONDASH_STARTUP_RENDERER";

struct Options {
    fullscreen: bool,
    config: PathBuf,
    renderer: Option<Renderer>,
}

fn options() -> Result<Option<Options>, String> {
    let mut options = Options {
        fullscreen: false,
        config: if PathBuf::from("config.cfg").exists() {
            PathBuf::from("config.cfg")
        } else {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("printer_data/config/moondash.cfg")
        },
        renderer: None,
    };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let (key, value) = arg.to_string_lossy().split_once('=').map_or_else(
            || (arg.to_string_lossy().into_owned(), None),
            |(key, value)| (key.to_owned(), Some(value.to_owned())),
        );
        match key.as_str() {
            "-h" | "--help" => {
                println!(
                    "Moondash {}\n\nUsage: moondash [--fullscreen] [--app-config PATH] [--renderer vulkan|opengl|software]\n\nBy default Moondash tries Vulkan, OpenGL, then software.",
                    version::current_version()
                );
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("Moondash {}", version::current_version());
                return Ok(None);
            }
            "--fullscreen" if value.is_none() => options.fullscreen = true,
            "--app-config" => {
                options.config = value
                    .map(Into::into)
                    .or_else(|| args.next())
                    .filter(|path| !path.is_empty())
                    .ok_or("--app-config requires a path")?
                    .into();
            }
            "--renderer" => {
                let name: std::ffi::OsString = value
                    .map(Into::into)
                    .or_else(|| args.next())
                    .ok_or("--renderer requires vulkan, opengl, or software")?;
                options.renderer = Some(
                    Renderer::parse(&name.to_string_lossy())
                        .ok_or("--renderer requires vulkan, opengl, or software")?,
                );
            }
            _ => return Err(format!("Unknown argument {key}. See --help.")),
        }
    }
    Ok(Some(options))
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(Some(options)) => options,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("moondash: {error}");
            return ExitCode::FAILURE;
        }
    };

    let _logging = logging::init();
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        if let Some(location) = panic.location() {
            log::error!("application panic at {location}");
        } else {
            log::error!("application panic");
        }
        log::logger().flush();
        default_panic_hook(panic);
    }));
    log::info!("starting Moondash {}", version::current_version());

    let fallback = std::env::var(RETRY_RENDERER)
        .ok()
        .and_then(|name| Renderer::parse(&name));
    let (renderer, source) = match (options.renderer, fallback) {
        (Some(renderer), _) => (renderer, "argument"),
        (None, Some(renderer)) => (renderer, "fallback"),
        (None, None) => (Renderer::Vulkan, "default"),
    };
    renderer::record_selection(renderer, source);
    let started = Rc::new(Cell::new(false));
    log::info!(target: "moondash::renderer", "trying {}", renderer.name());

    match run(&options, renderer, started.clone()) {
        Ok(()) => {
            log::info!("application exited");
            ExitCode::SUCCESS
        }
        Err(error) => {
            log::error!(target: "moondash::renderer", "{} failed: {error}", renderer.name());
            if options.renderer.is_none()
                && !started.get()
                && let Some(next) = renderer.next()
            {
                // Slint installs its platform once and Winit cannot recreate its event loop.
                // exec starts the next attempt with a clean graphics stack under the same PID.
                use std::os::unix::process::CommandExt;
                log::info!(target: "moondash::renderer", "falling back to {}", next.name());
                log::logger().flush();
                match std::env::current_exe() {
                    Ok(executable) => {
                        let error = std::process::Command::new(executable)
                            .args(std::env::args_os().skip(1))
                            .env(RETRY_RENDERER, next.name())
                            .exec();
                        log::error!(target: "moondash::renderer", "could not start {}: {error}", next.name());
                    }
                    Err(error) => {
                        log::error!(target: "moondash::renderer", "could not resolve executable: {error}")
                    }
                }
            }
            ExitCode::FAILURE
        }
    }
}

fn run(
    options: &Options,
    renderer: Renderer,
    started: Rc<Cell<bool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    renderer.select(options.fullscreen)?;
    let ui = Moondash::new()?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _app = app::start(&ui, options.config.clone(), runtime.handle().clone())?;
    renderer.watch_first_frame(&ui, started, _app.mesh_scene.clone())?;
    ui.run()?;
    Ok(())
}
