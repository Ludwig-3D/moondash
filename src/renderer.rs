use slint::{ComponentHandle, RenderingState};
use std::{cell::Cell, rc::Rc};

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
                mesh_renderer.notify(state, api, &mesh_scene, &weak);
                if after_rendering && !scheduled.replace(true) {
                    let started = started.clone();
                    // The notifier runs just before the surface is presented. Confirm
                    // startup on the next event-loop turn, after presentation succeeds.
                    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                        started.set(true);
                        log::info!("using {}", self.name());
                    });
                }
            })
            .map_err(|error| format!("GPU renderer unavailable: {error:?}"))
    }
}
