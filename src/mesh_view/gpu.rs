use super::{GpuVertex, MeshGeometry, SceneHandle};
use glow::HasContext;
use slint::RenderingState;
use std::{ffi::CStr, num::NonZeroU32};

const WIDTH: u32 = 760;
const HEIGHT: u32 = 412;
const DEPTH_FORMAT: slint::wgpu_30::wgpu::TextureFormat =
    slint::wgpu_30::wgpu::TextureFormat::Depth24Plus;

pub struct GpuRenderer {
    backend: Option<Backend>,
    last_revision: Option<u64>,
    first_draw_logged: bool,
}

enum Backend {
    OpenGl(OpenGlRenderer),
    Wgpu(WgpuRenderer),
}

impl Backend {
    fn device_label(&self) -> &str {
        match self {
            Self::OpenGl(renderer) => renderer.device_label(),
            Self::Wgpu(renderer) => &renderer.device_label,
        }
    }
}

impl Default for GpuRenderer {
    fn default() -> Self {
        Self {
            backend: None,
            last_revision: None,
            first_draw_logged: false,
        }
    }
}

impl GpuRenderer {
    pub fn notify(
        &mut self,
        state: RenderingState,
        graphics_api: &slint::GraphicsAPI,
        scene: &SceneHandle,
        ui: &slint::Weak<crate::Moondash>,
    ) {
        match state {
            RenderingState::RenderingSetup => {
                self.initialize(graphics_api, ui);
            }
            RenderingState::BeforeRendering => self.render(scene, ui),
            RenderingState::RenderingTeardown => self.teardown(ui),
            _ => {}
        }
    }

    fn initialize(&mut self, graphics_api: &slint::GraphicsAPI, ui: &slint::Weak<crate::Moondash>) {
        self.backend = None;
        self.last_revision = None;
        self.first_draw_logged = false;
        let initialized = match graphics_api {
            slint::GraphicsAPI::NativeOpenGL { get_proc_address } => unsafe {
                OpenGlRenderer::new(*get_proc_address)
                    .map(|(renderer, image)| (Backend::OpenGl(renderer), image))
            },
            #[allow(unreachable_patterns)]
            slint::GraphicsAPI::WGPU30 {
                instance,
                device,
                queue,
                ..
            } => WgpuRenderer::new(instance, device, queue)
                .map(|(renderer, image)| (Backend::Wgpu(renderer), image)),
            _ => Err("Slint provided an unsupported graphics context".into()),
        };

        match initialized {
            Ok((backend, image)) => {
                let device_label = backend.device_label().to_owned();
                self.backend = Some(backend);
                if let Some(ui) = ui.upgrade() {
                    ui.set_tool_mesh_3d(image);
                    ui.set_tool_mesh_3d_available(true);
                }
                log::info!("GPU mesh rendering is available on {device_label}");
            }
            Err(error) => {
                log::warn!("GPU mesh rendering unavailable: {error}");
                if let Some(ui) = ui.upgrade() {
                    ui.set_tool_mesh_3d_available(false);
                    ui.set_tool_mesh_3d_open(false);
                }
            }
        }
    }

    fn render(&mut self, scene: &SceneHandle, ui: &slint::Weak<crate::Moondash>) {
        let Some(backend) = &mut self.backend else {
            return;
        };
        let (revision, geometry) = {
            let scene = scene.borrow();
            (scene.revision, scene.geometry.clone())
        };
        if self.last_revision == Some(revision) {
            return;
        }
        let Some(geometry) = geometry else {
            self.last_revision = Some(revision);
            return;
        };
        let result = match backend {
            Backend::OpenGl(renderer) => renderer.render(&geometry),
            Backend::Wgpu(renderer) => renderer.render(&geometry),
        };
        let vertex_counts = (geometry.triangles.len(), geometry.strokes.len());
        match result {
            Ok(()) => {
                self.last_revision = Some(revision);
                if !self.first_draw_logged {
                    match backend {
                        Backend::OpenGl(renderer) => {
                            renderer.finish();
                            log::info!(
                                "first GPU mesh draw completed ({} surface vertices, {} grid/axis vertices) on {}",
                                vertex_counts.0,
                                vertex_counts.1,
                                renderer.device_label()
                            );
                        }
                        Backend::Wgpu(renderer) => {
                            let device_label = renderer.device_label.clone();
                            let vertex_counts = vertex_counts;
                            renderer.queue.on_submitted_work_done(move || {
                                log::info!("first GPU mesh draw completed ({} surface vertices, {} grid/axis vertices) on {device_label}",
                                    vertex_counts.0,
                                    vertex_counts.1
                                );
                            });
                        }
                    }
                    self.first_draw_logged = true;
                }
            }
            Err(error) => {
                log::warn!("GPU mesh rendering failed: {error}");
                self.teardown(ui);
            }
        }
    }

    fn teardown(&mut self, ui: &slint::Weak<crate::Moondash>) {
        if let Some(Backend::OpenGl(renderer)) = &mut self.backend {
            unsafe { renderer.destroy() };
        }
        self.backend = None;
        self.last_revision = None;
        if let Some(ui) = ui.upgrade() {
            ui.set_tool_mesh_3d_open(false);
            ui.set_tool_mesh_3d(slint::Image::default());
            ui.set_tool_mesh_3d_available(false);
        }
    }
}

fn is_software_renderer(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "llvmpipe",
        "softpipe",
        "swiftshader",
        "software rasterizer",
        "software renderer",
        "microsoft basic render driver",
    ]
    .iter()
    .any(|software| name.contains(software))
}

fn vertex_bytes(vertices: &[GpuVertex]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(vertices.as_ptr().cast(), std::mem::size_of_val(vertices)) }
}

struct WgpuRenderer {
    device: slint::wgpu_30::wgpu::Device,
    queue: slint::wgpu_30::wgpu::Queue,
    device_label: String,
    color: slint::wgpu_30::wgpu::TextureView,
    depth: slint::wgpu_30::wgpu::TextureView,
    triangles: slint::wgpu_30::wgpu::RenderPipeline,
    lines: slint::wgpu_30::wgpu::RenderPipeline,
}

impl WgpuRenderer {
    fn new(
        instance: &slint::wgpu_30::wgpu::Instance,
        device: &slint::wgpu_30::wgpu::Device,
        queue: &slint::wgpu_30::wgpu::Queue,
    ) -> Result<(Self, slint::Image), String> {
        use slint::wgpu_30::wgpu;

        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
        let hardware_adapters = adapters
            .iter()
            .map(|adapter| adapter.get_info())
            .filter(|info| {
                info.device_type != wgpu::DeviceType::Cpu && !is_software_renderer(&info.name)
            })
            .map(|info| info.name)
            .collect::<Vec<_>>();
        if hardware_adapters.is_empty() {
            return Err("no hardware Vulkan adapter is available".into());
        }
        let device_label = format!(
            "Slint's active WGPU device; hardware Vulkan adapter(s) available: {}",
            hardware_adapters.join(", ")
        );

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Moondash 3D bed mesh"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let image = slint::Image::try_from(texture)
            .map_err(|error| format!("could not import Vulkan texture: {error:?}"))?;
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Moondash 3D bed mesh depth"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Moondash bed mesh shader"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color.rgb * input.color.a, input.color.a);
}
"#
                .into(),
            ),
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<GpuVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Moondash bed mesh pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = |topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Moondash bed mesh pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let triangles = pipeline(wgpu::PrimitiveTopology::TriangleList);
        let lines = pipeline(wgpu::PrimitiveTopology::TriangleList);

        Ok((
            Self {
                device: device.clone(),
                queue: queue.clone(),
                device_label,
                color,
                depth,
                triangles,
                lines,
            },
            image,
        ))
    }

    fn render(&mut self, geometry: &MeshGeometry) -> Result<(), String> {
        use slint::wgpu_30::wgpu;
        use wgpu::util::DeviceExt;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Moondash bed mesh render"),
            });
        let triangle_buffer = (!geometry.triangles.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Moondash bed mesh triangles"),
                    contents: vertex_bytes(&geometry.triangles),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let line_buffer = (!geometry.strokes.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Moondash bed mesh strokes"),
                    contents: vertex_bytes(&geometry.strokes),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Moondash bed mesh render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(buffer) = triangle_buffer {
                pass.set_pipeline(&self.triangles);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..geometry.triangles.len() as u32, 0..1);
            }
            if let Some(buffer) = line_buffer {
                pass.set_pipeline(&self.lines);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..geometry.strokes.len() as u32, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}

struct OpenGlRenderer {
    gl: glow::Context,
    renderer_name: String,
    program: glow::NativeProgram,
    vertex_array: glow::NativeVertexArray,
    vertex_buffer: glow::NativeBuffer,
    texture: glow::NativeTexture,
    framebuffer: glow::NativeFramebuffer,
    depth_buffer: glow::NativeRenderbuffer,
}

impl OpenGlRenderer {
    unsafe fn new(
        get_proc_address: &dyn Fn(&CStr) -> *const std::ffi::c_void,
    ) -> Result<(Self, slint::Image), String> {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) };
        let renderer_name = unsafe { gl.get_parameter_string(glow::RENDERER) };
        if is_software_renderer(&renderer_name) {
            return Err(format!(
                "software OpenGL renderer detected: {renderer_name}"
            ));
        }
        let state = unsafe { GlState::capture(&gl) };
        let initialized = unsafe { Self::create(&gl) };
        unsafe { state.restore(&gl) };
        let (resources, image) = initialized?;
        Ok((
            Self {
                gl,
                renderer_name,
                program: resources.program,
                vertex_array: resources.vertex_array,
                vertex_buffer: resources.vertex_buffer,
                texture: resources.texture,
                framebuffer: resources.framebuffer,
                depth_buffer: resources.depth_buffer,
            },
            image,
        ))
    }

    unsafe fn create(gl: &glow::Context) -> Result<(OpenGlResources, slint::Image), String> {
        let (vertex_shader, fragment_shader) = if unsafe { gl.get_parameter_string(glow::VERSION) }
            .to_ascii_lowercase()
            .contains("opengl es")
        {
            (
                "#version 300 es\nprecision highp float;\nlayout(location=0) in vec3 position;\nlayout(location=1) in vec4 color;\nout vec4 vertex_color;\nvoid main() { gl_Position = vec4(position, 1.0); vertex_color = color; }",
                "#version 300 es\nprecision highp float;\nin vec4 vertex_color;\nout vec4 output_color;\nvoid main() { output_color = vec4(vertex_color.rgb * vertex_color.a, vertex_color.a); }",
            )
        } else {
            (
                "#version 330 core\nlayout(location=0) in vec3 position;\nlayout(location=1) in vec4 color;\nout vec4 vertex_color;\nvoid main() { gl_Position = vec4(position, 1.0); vertex_color = color; }",
                "#version 330 core\nin vec4 vertex_color;\nout vec4 output_color;\nvoid main() { output_color = vec4(vertex_color.rgb * vertex_color.a, vertex_color.a); }",
            )
        };
        let vertex = unsafe { compile_shader(gl, glow::VERTEX_SHADER, vertex_shader) }?;
        let fragment = unsafe { compile_shader(gl, glow::FRAGMENT_SHADER, fragment_shader) }?;
        let program = unsafe { gl.create_program() }?;
        unsafe {
            gl.attach_shader(program, vertex);
            gl.attach_shader(program, fragment);
            gl.link_program(program);
            gl.delete_shader(vertex);
            gl.delete_shader(fragment);
        }
        if !unsafe { gl.get_program_link_status(program) } {
            let error = unsafe { gl.get_program_info_log(program) };
            unsafe { gl.delete_program(program) };
            return Err(format!("could not link mesh shader: {error}"));
        }

        let texture = unsafe { gl.create_texture() }?;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                WIDTH as i32,
                HEIGHT as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
        }
        let framebuffer = unsafe { gl.create_framebuffer() }?;
        let depth_buffer = unsafe { gl.create_renderbuffer() }?;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth_buffer));
            gl.renderbuffer_storage(
                glow::RENDERBUFFER,
                glow::DEPTH_COMPONENT24,
                WIDTH as i32,
                HEIGHT as i32,
            );
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(depth_buffer),
            );
        }
        let status = unsafe { gl.check_framebuffer_status(glow::FRAMEBUFFER) };
        if status != glow::FRAMEBUFFER_COMPLETE {
            return Err(format!("mesh framebuffer is incomplete: {status:#x}"));
        }
        let vertex_array = unsafe { gl.create_vertex_array() }?;
        let vertex_buffer = unsafe { gl.create_buffer() }?;
        let imported_texture = unsafe {
            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                texture.0,
                (WIDTH, HEIGHT).into(),
            )
            .build()
        };
        Ok((
            OpenGlResources {
                program,
                vertex_array,
                vertex_buffer,
                texture,
                framebuffer,
                depth_buffer,
            },
            imported_texture,
        ))
    }

    fn render(&mut self, geometry: &MeshGeometry) -> Result<(), String> {
        let gl = &self.gl;
        let state = unsafe { GlState::capture(gl) };
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
            gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.enable(glow::BLEND);
            gl.blend_equation_separate(glow::FUNC_ADD, glow::FUNC_ADD);
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.clear_color(0., 0., 0., 0.);
            gl.clear_depth_f32(1.);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vertex_buffer));
            gl.enable_vertex_attrib_array(0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 28, 0);
            gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, 28, 12);
            let mut vertices =
                Vec::with_capacity(geometry.triangles.len() + geometry.strokes.len());
            vertices.extend_from_slice(&geometry.triangles);
            vertices.extend_from_slice(&geometry.strokes);
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                vertex_bytes(&vertices),
                glow::DYNAMIC_DRAW,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, geometry.triangles.len() as i32);
            gl.draw_arrays(
                glow::TRIANGLES,
                geometry.triangles.len() as i32,
                geometry.strokes.len() as i32,
            );
        }
        unsafe { state.restore(gl) };
        Ok(())
    }

    fn device_label(&self) -> &str {
        &self.renderer_name
    }

    fn finish(&self) {
        unsafe { self.gl.finish() };
    }

    unsafe fn destroy(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_buffer(self.vertex_buffer);
            self.gl.delete_texture(self.texture);
            self.gl.delete_framebuffer(self.framebuffer);
            self.gl.delete_renderbuffer(self.depth_buffer);
        }
    }
}

struct OpenGlResources {
    program: glow::NativeProgram,
    vertex_array: glow::NativeVertexArray,
    vertex_buffer: glow::NativeBuffer,
    texture: glow::NativeTexture,
    framebuffer: glow::NativeFramebuffer,
    depth_buffer: glow::NativeRenderbuffer,
}

unsafe fn compile_shader(
    gl: &glow::Context,
    kind: u32,
    source: &str,
) -> Result<glow::NativeShader, String> {
    let shader = unsafe { gl.create_shader(kind) }?;
    unsafe {
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
    }
    if !unsafe { gl.get_shader_compile_status(shader) } {
        let error = unsafe { gl.get_shader_info_log(shader) };
        unsafe { gl.delete_shader(shader) };
        return Err(format!("could not compile mesh shader: {error}"));
    }
    Ok(shader)
}

struct GlState {
    draw_framebuffer: Option<glow::NativeFramebuffer>,
    read_framebuffer: Option<glow::NativeFramebuffer>,
    renderbuffer: Option<glow::NativeRenderbuffer>,
    texture: Option<glow::NativeTexture>,
    active_texture: i32,
    program: Option<glow::NativeProgram>,
    vertex_array: Option<glow::NativeVertexArray>,
    array_buffer: Option<glow::NativeBuffer>,
    viewport: [i32; 4],
    depth_test: bool,
    depth_write: bool,
    blend: bool,
    scissor: bool,
    cull: bool,
    depth_function: i32,
    blend_src_rgb: i32,
    blend_dst_rgb: i32,
    blend_src_alpha: i32,
    blend_dst_alpha: i32,
    blend_equation_rgb: i32,
    blend_equation_alpha: i32,
    clear_color: [f32; 4],
    clear_depth: f32,
}

impl GlState {
    unsafe fn capture(gl: &glow::Context) -> Self {
        let mut viewport = [0; 4];
        let mut clear_color = [0.; 4];
        unsafe {
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear_color);
        }
        let native_id = |parameter| unsafe { gl.get_parameter_i32(parameter) } as u32;
        Self {
            draw_framebuffer: NonZeroU32::new(native_id(glow::DRAW_FRAMEBUFFER_BINDING))
                .map(glow::NativeFramebuffer),
            read_framebuffer: NonZeroU32::new(native_id(glow::READ_FRAMEBUFFER_BINDING))
                .map(glow::NativeFramebuffer),
            renderbuffer: NonZeroU32::new(native_id(glow::RENDERBUFFER_BINDING))
                .map(glow::NativeRenderbuffer),
            texture: NonZeroU32::new(native_id(glow::TEXTURE_BINDING_2D)).map(glow::NativeTexture),
            active_texture: unsafe { gl.get_parameter_i32(glow::ACTIVE_TEXTURE) },
            program: NonZeroU32::new(native_id(glow::CURRENT_PROGRAM)).map(glow::NativeProgram),
            vertex_array: NonZeroU32::new(native_id(glow::VERTEX_ARRAY_BINDING))
                .map(glow::NativeVertexArray),
            array_buffer: NonZeroU32::new(native_id(glow::ARRAY_BUFFER_BINDING))
                .map(glow::NativeBuffer),
            viewport,
            depth_test: unsafe { gl.is_enabled(glow::DEPTH_TEST) },
            depth_write: unsafe { gl.get_parameter_i32(glow::DEPTH_WRITEMASK) != 0 },
            blend: unsafe { gl.is_enabled(glow::BLEND) },
            scissor: unsafe { gl.is_enabled(glow::SCISSOR_TEST) },
            cull: unsafe { gl.is_enabled(glow::CULL_FACE) },
            depth_function: unsafe { gl.get_parameter_i32(glow::DEPTH_FUNC) },
            blend_src_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_SRC_RGB) },
            blend_dst_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_DST_RGB) },
            blend_src_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_SRC_ALPHA) },
            blend_dst_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_DST_ALPHA) },
            blend_equation_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_EQUATION_RGB) },
            blend_equation_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA) },
            clear_color,
            clear_depth: unsafe { gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE) },
        }
    }

    unsafe fn restore(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_framebuffer);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_framebuffer);
            gl.bind_renderbuffer(glow::RENDERBUFFER, self.renderbuffer);
            gl.active_texture(self.active_texture as u32);
            gl.bind_texture(glow::TEXTURE_2D, self.texture);
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vertex_array);
            gl.bind_buffer(glow::ARRAY_BUFFER, self.array_buffer);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            set_enabled(gl, glow::DEPTH_TEST, self.depth_test);
            gl.depth_mask(self.depth_write);
            set_enabled(gl, glow::BLEND, self.blend);
            set_enabled(gl, glow::SCISSOR_TEST, self.scissor);
            set_enabled(gl, glow::CULL_FACE, self.cull);
            gl.depth_func(self.depth_function as u32);
            gl.blend_func_separate(
                self.blend_src_rgb as u32,
                self.blend_dst_rgb as u32,
                self.blend_src_alpha as u32,
                self.blend_dst_alpha as u32,
            );
            gl.blend_equation_separate(
                self.blend_equation_rgb as u32,
                self.blend_equation_alpha as u32,
            );
            gl.clear_color(
                self.clear_color[0],
                self.clear_color[1],
                self.clear_color[2],
                self.clear_color[3],
            );
            gl.clear_depth_f32(self.clear_depth);
        }
    }
}

unsafe fn set_enabled(gl: &glow::Context, capability: u32, enabled: bool) {
    unsafe {
        if enabled {
            gl.enable(capability);
        } else {
            gl.disable(capability);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_software_renderer;

    #[test]
    fn detects_common_software_graphics_drivers() {
        for renderer in ["llvmpipe (LLVM 17.0)", "Softpipe", "Google SwiftShader"] {
            assert!(is_software_renderer(renderer));
        }
        assert!(!is_software_renderer("Mesa Intel(R) UHD Graphics"));
    }
}
