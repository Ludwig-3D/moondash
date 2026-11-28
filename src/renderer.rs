use serde_json::{Map, Value, json};
use slint::{ComponentHandle, RenderingState};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Renderer choice and what the graphics driver reported, shown on the dev page.
static SELECTION: Mutex<Option<(Renderer, &'static str)>> = Mutex::new(None);
static GRAPHICS: Mutex<Option<Value>> = Mutex::new(None);
static PRESENTED: AtomicBool = AtomicBool::new(false);

/// Remember which renderer runs and why (`source` is "argument", "fallback" or "default").
pub fn record_selection(renderer: Renderer, source: &'static str) {
    if let Ok(mut selection) = SELECTION.lock() {
        *selection = Some((renderer, source));
    }
}

fn record_graphics_api(api: &slint::GraphicsAPI) {
    let info = match api {
        slint::GraphicsAPI::NativeOpenGL { get_proc_address } => {
            use glow::HasContext;
            // Called from RenderingSetup, where Slint's GL context is current.
            let gl =
                unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) };
            let read = |name| unsafe { gl.get_parameter_string(name) };
            json!({
                "api": "OpenGL",
                "vendor": read(glow::VENDOR),
                "renderer": read(glow::RENDERER),
                "version": read(glow::VERSION),
                "shadingLanguage": read(glow::SHADING_LANGUAGE_VERSION),
            })
        }
        #[allow(unreachable_patterns)]
        slint::GraphicsAPI::WGPU30 { device, .. } => {
            let adapter = device.adapter_info();
            json!({
                "api": "WGPU",
                "backend": format!("{:?}", adapter.backend),
                "adapter": adapter.name,
                "deviceType": format!("{:?}", adapter.device_type),
                "vendorId": format!("{:#06x}", adapter.vendor),
                "deviceId": format!("{:#06x}", adapter.device),
                "driver": adapter.driver,
                "driverInfo": adapter.driver_info,
            })
        }
        _ => json!({"api": "unknown"}),
    };
    if let Ok(mut graphics) = GRAPHICS.lock() {
        *graphics = Some(info);
    }
}

pub fn report() -> Value {
    let mut report = Map::new();
    if let Some((renderer, source)) = SELECTION.lock().ok().and_then(|selection| *selection) {
        report.insert("active".into(), json!(renderer.name()));
        report.insert(
            "slintRenderer".into(),
            json!(match renderer {
                Renderer::Vulkan => "femtovg-wgpu",
                Renderer::OpenGl => "femtovg",
                Renderer::Software => "software",
            }),
        );
        report.insert("selectedBy".into(), json!(source));
    }
    report.insert("slintBackend".into(), json!("winit"));
    report.insert(
        "firstFramePresented".into(),
        json!(PRESENTED.load(Ordering::Relaxed)),
    );
    if let Some(graphics) = GRAPHICS.lock().ok().and_then(|graphics| graphics.clone()) {
        report.insert("graphics".into(), graphics);
    }
    Value::Object(report)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    Vulkan,
    OpenGl,
    Software,
}

impl Renderer {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "vulkan" => Some(Self::Vulkan),
            "opengl" => Some(Self::OpenGl),
            "software" => Some(Self::Software),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Vulkan => "vulkan",
            Self::OpenGl => "opengl",
            Self::Software => "software",
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            Self::Vulkan => Some(Self::OpenGl),
            Self::OpenGl => Some(Self::Software),
            Self::Software => None,
        }
    }

    pub fn select(self, fullscreen: bool) -> Result<(), slint::PlatformError> {
        use slint::winit_030::winit::window::Fullscreen;

        let backend = slint::BackendSelector::new()
            .backend_name("winit".into())
            .with_winit_window_attributes_hook(move |attributes| {
                if fullscreen {
                    attributes
                        .with_decorations(false)
                        .with_fullscreen(Some(Fullscreen::Borderless(None)))
                } else {
                    attributes
                }
            });

        match self {
            Self::Vulkan => backend
                .renderer_name("femtovg-wgpu".into())
                .require_vulkan()
                .select(),
            Self::OpenGl => backend.renderer_name("femtovg".into()).select(),
            Self::Software => backend.renderer_name("software".into()).select(),
        }
    }

    pub fn watch_first_frame(
        self,
        ui: &crate::Moondash,
        started: Rc<Cell<bool>>,
        mesh_scene: crate::mesh_view::SceneHandle,
    ) -> Result<(), String> {
        if self == Self::Software {
            PRESENTED.store(true, Ordering::Relaxed);
            log::info!("using {}", self.name());
            ui.set_tool_mesh_3d_available(false);
            return Ok(());
        }

        // Slint can substitute a software adapter at construction time. It doesn't
        // support render notifications, so continue to the next candidate if it does.
        let scheduled = Cell::new(false);
        let weak = ui.as_weak();
        let mut mesh_renderer = crate::mesh_view::gpu::GpuRenderer::default();
        ui.window()
            .set_rendering_notifier(move |state, api: &slint::GraphicsAPI| {
                let after_rendering = matches!(&state, RenderingState::AfterRendering);
                if matches!(&state, RenderingState::RenderingSetup) {
                    record_graphics_api(api);
                }
                mesh_renderer.notify(state, api, &mesh_scene, &weak);
                if after_rendering && !scheduled.replace(true) {
                    let started = started.clone();
                    // The notifier runs just before the surface is presented. Confirm
                    // startup on the next event-loop turn, after presentation succeeds.
                    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                        started.set(true);
                        PRESENTED.store(true, Ordering::Relaxed);
                        log::info!("using {}", self.name());
                    });
                }
            })
            .map_err(|error| format!("GPU renderer unavailable: {error:?}"))
    }
}
