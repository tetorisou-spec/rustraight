use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use pollster::block_on;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton as WMouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{WindowAttributes, WindowId};

use crate::draw::{Color, ColorVert, verts_circle, verts_fill, verts_line, verts_pixel, verts_rectangle, verts_triangle};
use crate::gamepad::{GamepadManager, PadAxis, PadButton};
use crate::graphics::{BlendMode, DrawSpriteParams};
use crate::input::{KeyCode, MouseButton};

// ── Shaders ───────────────────────────────────────────────────────────────────

// Blits screen_texture (straight alpha) to swap chain as pre-multiplied alpha.
const BLIT_SHADER: &str = r#"
struct Vout { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) vi: u32) -> Vout {
    var p = array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(-1.,1.),vec2(1.,-1.),vec2(1.,1.),vec2(-1.,1.));
    var u = array<vec2<f32>,6>(vec2(0.,1.),vec2(1.,1.),vec2(0.,0.),vec2(1.,1.),vec2(1.,0.),vec2(0.,0.));
    return Vout(vec4(p[vi],0.,1.), u[vi]);
}
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> {
    let c = textureSample(t, s, in.uv);
    return vec4(c.rgb * c.a, c.a); // straight → pre-multiplied
}
"#;

// Renders a textured sprite quad with optional mask modulation and per-vertex alpha.
const SPRITE_SHADER: &str = r#"
struct Vin {
    @location(0) pos:       vec2<f32>,
    @location(1) uv:        vec2<f32>,
    @location(2) screen_xy: vec2<f32>,
    @location(3) mask_ox:   f32,
    @location(4) mask_oy:   f32,
    @location(5) mask_w:    f32,
    @location(6) mask_h:    f32,
    @location(7) mask_on:   f32,
    @location(8) alpha:     f32,
}
struct Vout {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:        vec2<f32>,
    @location(1) screen_xy: vec2<f32>,
    @location(2) mask_ox:   f32,
    @location(3) mask_oy:   f32,
    @location(4) mask_w:    f32,
    @location(5) mask_h:    f32,
    @location(6) mask_on:   f32,
    @location(7) alpha:     f32,
}
@group(0) @binding(0) var t_sprite: texture_2d<f32>;
@group(0) @binding(1) var t_mask:   texture_2d<f32>;
@group(0) @binding(2) var s_samp:   sampler;
@vertex fn vs(v: Vin) -> Vout {
    return Vout(vec4(v.pos, 0., 1.), v.uv, v.screen_xy, v.mask_ox, v.mask_oy, v.mask_w, v.mask_h, v.mask_on, v.alpha);
}
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> {
    var c = textureSample(t_sprite, s_samp, in.uv);
    if in.mask_on > 0.5 {
        let mx = (in.screen_xy.x - in.mask_ox) / in.mask_w;
        let my = (in.screen_xy.y - in.mask_oy) / in.mask_h;
        if mx >= 0. && mx <= 1. && my >= 0. && my <= 1. {
            c.a *= textureSample(t_mask, s_samp, vec2(mx, my)).a;
        } else {
            c.a = 0.;
        }
    }
    c.a *= in.alpha;
    return c;
}
"#;

// Renders colored vertex geometry (fill, lines, shapes).
const COLOR_SHADER: &str = r#"
struct Vin  { @location(0) pos: vec2<f32>, @location(1) color: vec4<f32> }
struct Vout { @builtin(position) clip: vec4<f32>, @location(0) color: vec4<f32> }
@vertex fn vs(v: Vin) -> Vout { return Vout(vec4(v.pos, 0., 1.), v.color); }
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> { return in.color; }
"#;

// ── Vertex types ──────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct SpriteVertex {
    pos:       [f32; 2], // NDC
    uv:        [f32; 2],
    screen_xy: [f32; 2], // screen pixel position (interpolated, for mask sampling)
    mask_ox:   f32,
    mask_oy:   f32,
    mask_w:    f32,
    mask_h:    f32,
    mask_on:   f32,      // 1.0 = apply mask, 0.0 = no mask
    alpha:     f32,
}
// stride: 48 bytes

fn slice_as_bytes<T>(data: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data)) }
}

// ── GPU sprite cache ──────────────────────────────────────────────────────────

struct SpriteGpuData {
    _texture:   Option<wgpu::Texture>, // CPU-loaded sprites only; None = gpu_native (owned by Screen)
    view:       wgpu::TextureView,
    width:      u32,
    height:     u32,
    gpu_native: bool,
}

fn ensure_sprite(
    handle: u32,
    device: &wgpu::Device,
    queue:  &wgpu::Queue,
    cache:  &mut HashMap<u32, SpriteGpuData>,
) {
    // gpu_native screens upload themselves in Screen::sprite(); skip here
    if cache.get(&handle).map(|e| e.gpu_native).unwrap_or(false) { return; }

    crate::graphics::with_sprite(handle, |w, h, rgba| {
        let entry = cache.entry(handle).or_insert_with(|| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label:             None,
                size:              wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count:   1,
                sample_count:      1,
                dimension:         wgpu::TextureDimension::D2,
                format:            wgpu::TextureFormat::Rgba8UnormSrgb,
                usage:             wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats:      &[],
            });
            let view = tex.create_view(&Default::default());
            SpriteGpuData { _texture: Some(tex), view, width: w, height: h, gpu_native: false }
        });
        let tex = entry._texture.as_ref().expect("non-native sprite must have texture");
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture:   tex,
                mip_level: 0,
                origin:    wgpu::Origin3d::ZERO,
                aspect:    wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset:         0,
                bytes_per_row:  Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    });
}

// ── Draw command queue ────────────────────────────────────────────────────────

#[derive(Clone)]
enum DrawCommand {
    Polys  { verts: Vec<ColorVert> },
    Sprite { x: i32, y: i32, handle: u32, mask_handle: Option<u32>, mask_ox: i32, mask_oy: i32, params: DrawSpriteParams, blend: BlendMode },
    Text   { x: i32, y: i32, width: u32, height: u32, rgba: Vec<u8> },
}

// ── winit event handler ───────────────────────────────────────────────────────

struct AppHandler {
    should_close:       bool,
    key_events:         Vec<(KeyCode, bool)>,
    resize_event:       Option<(u32, u32)>,
    cursor_moved:       Option<(f64, f64)>,
    mouse_btn_events:   Vec<(MouseButton, bool)>,
}

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, _el: &ActiveEventLoop) {}

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.should_close = true,
            WindowEvent::Resized(size) => {
                self.resize_event = Some((size.width, size.height));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.key_events.push((code, event.state == ElementState::Pressed));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_moved = Some((position.x, position.y));
            }
            WindowEvent::MouseInput { button, state, .. } => {
                let btn = match button {
                    WMouseButton::Left   => Some(MouseButton::Left),
                    WMouseButton::Right  => Some(MouseButton::Right),
                    WMouseButton::Middle => Some(MouseButton::Middle),
                    _                    => None,
                };
                if let Some(btn) = btn {
                    self.mouse_btn_events.push((btn, state == ElementState::Pressed));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {}
}

// ── Runtime state ─────────────────────────────────────────────────────────────

struct WindowInner {
    winit_window:        Arc<winit::window::Window>,
    event_loop:          EventLoop<()>,
    app_handler:         AppHandler,
    device:              wgpu::Device,
    queue:               wgpu::Queue,
    surface:             wgpu::Surface<'static>,
    surface_config:      wgpu::SurfaceConfiguration,
    screen_width:        u32,
    screen_height:       u32,
    // Screen render target (RENDER_ATTACHMENT + TEXTURE_BINDING)
    screen_texture:      wgpu::Texture,
    screen_texture_view: wgpu::TextureView,
    // Final blit to swap chain
    blit_pipeline:       wgpu::RenderPipeline,
    blit_bind_group:     wgpu::BindGroup,
    // Sprite pipelines (Normal / Add / Mul blend modes)
    sprite_pipeline:     Arc<wgpu::RenderPipeline>,
    sprite_pipeline_add: Arc<wgpu::RenderPipeline>,
    sprite_pipeline_mul: Arc<wgpu::RenderPipeline>,
    sprite_bgl:          Arc<wgpu::BindGroupLayout>,
    // Color geometry pipeline (fills, lines, shapes)
    color_pipeline:      std::sync::Arc<wgpu::RenderPipeline>,
    // Shared sampler + dummy 1x1 white texture for unmasked draws
    sampler:             wgpu::Sampler,
    dummy_texture:       wgpu::Texture,
    dummy_view:          wgpu::TextureView,
    // Pre-allocated sprite vertex buffer
    sprite_vbuf:         wgpu::Buffer, // 1024 sprites * 6 verts * 44 bytes
    // Per-frame draw queue
    draw_queue:          Vec<DrawCommand>,
    sprite_cache:        HashMap<u32, SpriteGpuData>,
    sprite_bg_cache:     HashMap<(u32, Option<u32>), Arc<wgpu::BindGroup>>,
    mask:                Option<(i32, i32, u32)>,
    blend:               BlendMode,
    transparent:         bool,
    gamepad:             Option<GamepadManager>,
    default_font:        Option<u32>,
}

// ── Public Window struct ──────────────────────────────────────────────────────

pub struct Window {
    title:             String,
    win_width:         u16,
    win_height:        u16,
    screen_width:      u16,
    screen_height:     u16,
    resizable:         bool,
    vsync_enabled:     bool,
    decorations:       bool,
    transparent:       bool,
    default_font_path: Option<String>,
    default_font_size: u32,
    inner:             Option<Box<WindowInner>>,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            title:         String::from("Window"),
            win_width:     800,
            win_height:    600,
            screen_width:  800,
            screen_height: 600,
            resizable:         true,
            vsync_enabled:     true,
            decorations:       true,
            transparent:       false,
            default_font_path: None,
            default_font_size: 16,
            inner:             None,
        }
    }
}

impl Window {
    pub fn title(&mut self, t: &str)             { self.title = t.to_string(); }
    pub fn size(&mut self, w: u16, h: u16)       { self.win_width = w; self.win_height = h; }
    pub fn screen_size(&mut self, w: u16, h: u16){ self.screen_width = w; self.screen_height = h; }
    pub fn resizable(&mut self, v: bool)         { self.resizable = v; }
    pub fn vsync(&mut self, v: bool)             { self.vsync_enabled = v; }
    pub fn decorations(&mut self, v: bool)       { self.decorations = v; }
    pub fn transparent(&mut self, v: bool)       { self.transparent = v; }

    pub fn init(&mut self) {
        let event_loop = EventLoop::new().expect("failed to create event loop");

        let win_attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::PhysicalSize::new(self.win_width as u32, self.win_height as u32))
            .with_resizable(self.resizable)
            .with_decorations(self.decorations)
            .with_transparent(self.transparent);

        #[allow(deprecated)]
        let winit_window = Arc::new(
            event_loop.create_window(win_attrs).expect("failed to create window"),
        );

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface  = instance.create_surface(winit_window.clone()).expect("failed to create surface");

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference:   wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
        })).expect("no suitable GPU adapter");

        let (device, queue) = block_on(adapter.request_device(
            &wgpu::DeviceDescriptor::default(), None,
        )).expect("failed to create wgpu device");

        let caps    = surface.get_capabilities(&adapter);
        let fmt     = caps.formats[0];
        let present = if self.vsync_enabled { wgpu::PresentMode::Fifo } else { wgpu::PresentMode::Immediate };
        let alpha   = if self.transparent {
            caps.alpha_modes.iter().find(|&&m| m == wgpu::CompositeAlphaMode::PreMultiplied)
                .or_else(|| caps.alpha_modes.iter().find(|&&m| m == wgpu::CompositeAlphaMode::PostMultiplied))
                .copied().unwrap_or(caps.alpha_modes[0])
        } else {
            caps.alpha_modes.iter().find(|&&m| m == wgpu::CompositeAlphaMode::Opaque)
                .copied().unwrap_or(caps.alpha_modes[0])
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage:                          wgpu::TextureUsages::RENDER_ATTACHMENT,
            format:                         fmt,
            width:                          self.win_width as u32,
            height:                         self.win_height as u32,
            present_mode:                   present,
            alpha_mode:                     alpha,
            view_formats:                   vec![],
            desired_maximum_frame_latency:  2,
        };
        surface.configure(&device, &surface_config);

        // ── Screen render target ──────────────────────────────────────────────
        let sw = self.screen_width  as u32;
        let sh = self.screen_height as u32;
        let screen_texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("screen"),
            size:            wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Rgba8Unorm,
            usage:           wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats:    &[],
        });
        let screen_texture_view = screen_texture.create_view(&Default::default());

        // ── Shared sampler ────────────────────────────────────────────────────
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter:       wgpu::FilterMode::Nearest,
            min_filter:       wgpu::FilterMode::Nearest,
            address_mode_u:   wgpu::AddressMode::ClampToEdge,
            address_mode_v:   wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        // ── Dummy 1×1 white texture (used as placeholder mask) ────────────────
        let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("dummy"),
            size:            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Rgba8Unorm,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &dummy_texture, mip_level: 0,
                origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let dummy_view = dummy_texture.create_view(&Default::default());

        // ── Blit pipeline (screen_texture → swap chain) ───────────────────────
        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("blit_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding:    0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Texture {
                        sample_type:    wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled:   false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding:    1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty:         wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count:      None,
                },
            ],
        });
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label:   Some("blit_bg"),
            layout:  &blit_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&screen_texture_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit_layout"), bind_group_layouts: &[&blit_bgl], push_constant_ranges: &[],
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("blit"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState { module: &blit_shader, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: fmt, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive:     wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        // ── Sprite pipeline ───────────────────────────────────────────────────
        let sprite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label:   Some("sprite_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite"), source: wgpu::ShaderSource::Wgsl(SPRITE_SHADER.into()),
        });
        let sprite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite_layout"), bind_group_layouts: &[&sprite_bgl], push_constant_ranges: &[],
        });
        let sprite_attrs = [
            wgpu::VertexAttribute { shader_location: 0, offset:  0, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { shader_location: 1, offset:  8, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { shader_location: 2, offset: 16, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { shader_location: 3, offset: 24, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { shader_location: 4, offset: 28, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { shader_location: 5, offset: 32, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { shader_location: 6, offset: 36, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { shader_location: 7, offset: 40, format: wgpu::VertexFormat::Float32   },
            wgpu::VertexAttribute { shader_location: 8, offset: 44, format: wgpu::VertexFormat::Float32   },
        ];
        let sprite_vbl = wgpu::VertexBufferLayout {
            array_stride: 48,
            step_mode:    wgpu::VertexStepMode::Vertex,
            attributes:   &sprite_attrs,
        };
        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("sprite"),
            layout: Some(&sprite_layout),
            vertex: wgpu::VertexState { module: &sprite_shader, entry_point: Some("vs"), buffers: &[sprite_vbl.clone()], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive:     wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });
        let sprite_pipeline_add = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("sprite_add"),
            layout: Some(&sprite_layout),
            vertex: wgpu::VertexState { module: &sprite_shader, entry_point: Some("vs"), buffers: &[sprite_vbl.clone()], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One,      dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive:     wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });
        let sprite_pipeline_mul = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("sprite_mul"),
            layout: Some(&sprite_layout),
            vertex: wgpu::VertexState { module: &sprite_shader, entry_point: Some("vs"), buffers: &[sprite_vbl], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst,  dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One,  dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive:     wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        });

        let sprite_pipeline     = Arc::new(sprite_pipeline);
        let sprite_pipeline_add = Arc::new(sprite_pipeline_add);
        let sprite_pipeline_mul = Arc::new(sprite_pipeline_mul);
        let sprite_bgl          = Arc::new(sprite_bgl);

        // ── Color geometry pipeline (fills, lines, shapes) ────────────────────
        // ColorVert: pos[2] @ offset 0, color[4] @ offset 8 — stride 24 bytes
        let color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("color"), source: wgpu::ShaderSource::Wgsl(COLOR_SHADER.into()),
        });
        let color_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("color_layout"), bind_group_layouts: &[], push_constant_ranges: &[],
        });
        let color_vbl = wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode:    wgpu::VertexStepMode::Vertex,
            attributes:   &[
                wgpu::VertexAttribute { shader_location: 0, offset:  0, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { shader_location: 1, offset:  8, format: wgpu::VertexFormat::Float32x4 },
            ],
        };
        let color_pipeline = std::sync::Arc::new(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("color"),
            layout: Some(&color_layout),
            vertex: wgpu::VertexState { module: &color_shader, entry_point: Some("vs"), buffers: &[color_vbl], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &color_shader, entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive:     wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample:   wgpu::MultisampleState::default(),
            multiview:     None,
            cache:         None,
        }));

        // ── Pre-allocated sprite vertex buffer ────────────────────────────────
        let sprite_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label:              Some("sprite_vbuf"),
            size:               1024 * 6 * 48,
            usage:              wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.inner = Some(Box::new(WindowInner {
            winit_window, event_loop,
            app_handler: AppHandler {
                should_close:     false,
                key_events:       Vec::new(),
                resize_event:     None,
                cursor_moved:     None,
                mouse_btn_events: Vec::new(),
            },
            device, queue, surface, surface_config,
            screen_width: sw, screen_height: sh,
            screen_texture, screen_texture_view,
            blit_pipeline, blit_bind_group,
            sprite_pipeline, sprite_pipeline_add, sprite_pipeline_mul, sprite_bgl,
            color_pipeline,
            sampler, dummy_texture, dummy_view,
            sprite_vbuf,
            draw_queue:      Vec::new(),
            sprite_cache:    HashMap::new(),
            sprite_bg_cache: HashMap::new(),
            mask:         None,
            blend:        BlendMode::Normal,
            transparent:  self.transparent,
            gamepad:      GamepadManager::try_new(),
            default_font: None,
        }));
    }

    fn inner_mut(&mut self) -> &mut WindowInner {
        self.inner.as_mut().expect("Window not initialized — call window.init() first")
    }

    fn inner_ref(&self) -> &WindowInner {
        self.inner.as_ref().expect("Window not initialized — call window.init() first")
    }

    // ── Per-frame ─────────────────────────────────────────────────────────────

    pub fn advance_frame(&mut self) -> bool {
        let inner = self.inner_mut();

        // ① Confirm input from previous frame
        crate::input::commit_input();
        crate::input::commit_mouse_input();
        if let Some(gm) = &mut inner.gamepad { gm.commit(); }

        // ② Handle pending resize
        if let Some((w, h)) = inner.app_handler.resize_event.take() {
            if w > 0 && h > 0 {
                inner.surface_config.width  = w;
                inner.surface_config.height = h;
                inner.surface.configure(&inner.device, &inner.surface_config);
            }
        }

        // ③ Upload all sprite textures referenced this frame to GPU cache
        {
            let handles: Vec<(u32, Option<u32>)> = inner.draw_queue.iter()
                .filter_map(|cmd| if let DrawCommand::Sprite { handle, mask_handle, .. } = cmd { Some((*handle, *mask_handle)) } else { None })
                .collect();
            for (h, mh) in &handles {
                ensure_sprite(*h, &inner.device, &inner.queue, &mut inner.sprite_cache);
                if let Some(mh) = mh {
                    ensure_sprite(*mh, &inner.device, &inner.queue, &mut inner.sprite_cache);
                }
            }
        }

        // ③ Build vertex data from draw queue
        let mut sprite_verts: Vec<SpriteVertex> = Vec::new();
        let mut color_verts:  Vec<ColorVert>    = Vec::new();

        enum RItem { Polys { base: u32, count: u32 }, Sprite { base: u32, handle: u32, mask_handle: Option<u32>, blend: BlendMode }, Text { base: u32 } }
        let mut items: Vec<RItem> = Vec::new();
        struct TextItem { width: u32, height: u32, rgba: Vec<u8> }
        let mut text_items: Vec<TextItem> = Vec::new();

        for cmd in &inner.draw_queue {
            match cmd {
                DrawCommand::Polys { verts } => {
                    let base  = color_verts.len() as u32;
                    let count = verts.len() as u32;
                    color_verts.extend_from_slice(verts);
                    items.push(RItem::Polys { base, count });
                }
                DrawCommand::Sprite { x, y, handle, mask_handle, mask_ox, mask_oy, params, blend } => {
                    if let Some(gd) = inner.sprite_cache.get(handle) {
                        let base = sprite_verts.len() as u32;
                        let (mox, moy, mw, mh, mon) = if let Some(mh) = mask_handle {
                            if let Some(md) = inner.sprite_cache.get(mh) {
                                (*mask_ox as f32, *mask_oy as f32, md.width as f32, md.height as f32, 1.0f32)
                            } else { (0.0, 0.0, 1.0, 1.0, 0.0) }
                        } else { (0.0, 0.0, 1.0, 1.0, 0.0) };
                        sprite_verts.extend_from_slice(&build_sprite_quad_ex(
                            *x, *y, gd.width, gd.height,
                            inner.screen_width, inner.screen_height,
                            mox, moy, mw, mh, mon, params,
                        ));
                        items.push(RItem::Sprite { base, handle: *handle, mask_handle: *mask_handle, blend: *blend });
                    }
                }
                DrawCommand::Text { x, y, width, height, rgba } => {
                    let base = sprite_verts.len() as u32;
                    sprite_verts.extend_from_slice(&build_sprite_quad_ex(
                        *x, *y, *width, *height,
                        inner.screen_width, inner.screen_height,
                        0.0, 0.0, 1.0, 1.0, 0.0, &DrawSpriteParams::default(),
                    ));
                    text_items.push(TextItem { width: *width, height: *height, rgba: rgba.clone() });
                    items.push(RItem::Text { base });
                }
            }
        }

        // ④ Upload vertex data
        if !sprite_verts.is_empty() {
            inner.queue.write_buffer(&inner.sprite_vbuf, 0, slice_as_bytes(&sprite_verts));
        }

        // Dynamic color vertex buffer — created only when needed
        let color_buf_opt: Option<wgpu::Buffer> = if !color_verts.is_empty() {
            use wgpu::util::DeviceExt;
            Some(inner.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label:    Some("color_vbuf"),
                contents: slice_as_bytes(&color_verts),
                usage:    wgpu::BufferUsages::VERTEX,
            }))
        } else {
            None
        };

        // ⑤ Build bind groups for sprite draws (cached by texture+mask combination)
        let mut sprite_bgs: Vec<Arc<wgpu::BindGroup>> = Vec::new();
        for item in &items {
            if let RItem::Sprite { handle, mask_handle, .. } = item {
                let key = (*handle, *mask_handle);
                if let Some(cached) = inner.sprite_bg_cache.get(&key) {
                    sprite_bgs.push(Arc::clone(cached));
                } else if let Some(sprite_gd) = inner.sprite_cache.get(handle) {
                    let mask_view = mask_handle
                        .and_then(|mh| inner.sprite_cache.get(&mh))
                        .map(|md| &md.view)
                        .unwrap_or(&inner.dummy_view);
                    let bg = Arc::new(inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label:   None,
                        layout:  &inner.sprite_bgl,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sprite_gd.view) },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(mask_view) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&inner.sampler) },
                        ],
                    }));
                    inner.sprite_bg_cache.insert(key, Arc::clone(&bg));
                    sprite_bgs.push(bg);
                }
            }
        }

        // ⑤b Text テクスチャを生成（テクスチャ→ビュー→バインドグループの順で2パス）
        let mut text_temp: Vec<(wgpu::Texture, wgpu::TextureView)> = Vec::new();
        for ti in &text_items {
            let tex = inner.device.create_texture(&wgpu::TextureDescriptor {
                label:           None,
                size:            wgpu::Extent3d { width: ti.width, height: ti.height, depth_or_array_layers: 1 },
                mip_level_count: 1, sample_count: 1,
                dimension:       wgpu::TextureDimension::D2,
                format:          wgpu::TextureFormat::Rgba8UnormSrgb,
                usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats:    &[],
            });
            inner.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &ti.rgba,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ti.width * 4), rows_per_image: Some(ti.height) },
                wgpu::Extent3d { width: ti.width, height: ti.height, depth_or_array_layers: 1 },
            );
            let view = tex.create_view(&Default::default());
            text_temp.push((tex, view));
        }
        let mut text_bgs: Vec<wgpu::BindGroup> = Vec::new();
        for (_, view) in &text_temp {
            text_bgs.push(inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label:   None,
                layout:  &inner.sprite_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&inner.dummy_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&inner.sampler) },
                ],
            }));
        }

        // ⑥ Get swap chain frame
        let frame = match inner.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                inner.surface.configure(&inner.device, &inner.surface_config);
                return !inner.app_handler.should_close;
            }
            Err(e) => { eprintln!("[rustraight] surface error: {e}"); return false; }
        };
        let frame_view = frame.texture.create_view(&Default::default());
        let mut encoder = inner.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        // ⑦ Draw commands → screen_texture
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view:           &inner.screen_texture_view,
                    resolve_target: None,
                    ops:            wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes:         None,
                occlusion_query_set:      None,
            });

            let mut sprite_bg_idx = 0usize;
            let mut text_bg_idx   = 0usize;
            for item in &items {
                match item {
                    RItem::Polys { base, count } => {
                        if let Some(buf) = &color_buf_opt {
                            rpass.set_pipeline(&inner.color_pipeline);
                            rpass.set_vertex_buffer(0, buf.slice(..));
                            rpass.draw(*base..*base + count, 0..1);
                        }
                    }
                    RItem::Sprite { base, blend, .. } => {
                        if sprite_bg_idx < sprite_bgs.len() {
                            let pipeline = match blend {
                                BlendMode::Normal => &inner.sprite_pipeline,
                                BlendMode::Add    => &inner.sprite_pipeline_add,
                                BlendMode::Mul    => &inner.sprite_pipeline_mul,
                            };
                            rpass.set_pipeline(pipeline);
                            rpass.set_bind_group(0, &*sprite_bgs[sprite_bg_idx], &[]);
                            rpass.set_vertex_buffer(0, inner.sprite_vbuf.slice(..));
                            rpass.draw(*base..*base + 6, 0..1);
                            sprite_bg_idx += 1;
                        }
                    }
                    RItem::Text { base } => {
                        if text_bg_idx < text_bgs.len() {
                            rpass.set_pipeline(&inner.sprite_pipeline);
                            rpass.set_bind_group(0, &text_bgs[text_bg_idx], &[]);
                            rpass.set_vertex_buffer(0, inner.sprite_vbuf.slice(..));
                            rpass.draw(*base..*base + 6, 0..1);
                            text_bg_idx += 1;
                        }
                    }
                }
            }
        }

        // ⑧ Blit screen_texture → swap chain
        {
            let clear = if inner.transparent { wgpu::Color { r:0.,g:0.,b:0.,a:0. } } else { wgpu::Color::BLACK };
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view:           &frame_view,
                    resolve_target: None,
                    ops:            wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes:         None,
                occlusion_query_set:      None,
            });
            rpass.set_pipeline(&inner.blit_pipeline);
            rpass.set_bind_group(0, &inner.blit_bind_group, &[]);
            rpass.draw(0..6, 0..1);
        }

        inner.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        // ⑨ Clear draw queue
        inner.draw_queue.clear();

        // ⑩ Process winit events
        let status = inner.event_loop.pump_app_events(Some(Duration::ZERO), &mut inner.app_handler);
        for (key, pressed) in inner.app_handler.key_events.drain(..) {
            crate::input::process_key_event(key, pressed);
        }
        if let Some((px, py)) = inner.app_handler.cursor_moved.take() {
            let vx = (px * inner.screen_width  as f64 / inner.surface_config.width  as f64) as i32;
            let vy = (py * inner.screen_height as f64 / inner.surface_config.height as f64) as i32;
            crate::input::process_mouse_move(vx, vy);
        }
        for (btn, pressed) in inner.app_handler.mouse_btn_events.drain(..) {
            crate::input::process_mouse_button(btn, pressed);
        }

        // ⑪ Update delta time
        crate::time::tick_time();

        // ⑫ Poll gamepad state
        if let Some(gm) = &mut inner.gamepad { gm.poll(); }

        !matches!(status, PumpStatus::Exit(_)) && !inner.app_handler.should_close
    }

    // ── Input ─────────────────────────────────────────────────────────────────

    pub fn is_pressed(&self, key: KeyCode) -> bool      { crate::input::is_pressed(key) }
    pub fn is_just_pressed(&self, key: KeyCode) -> bool { crate::input::is_just_pressed(key) }
    pub fn is_released(&self, key: KeyCode) -> bool     { crate::input::is_released(key) }
    pub fn delta_time(&self) -> f32                     { crate::time::get_delta_time() }
    pub fn elapsed_time(&self) -> f64                   { crate::time::get_elapsed_secs() }

    // ── Mouse ─────────────────────────────────────────────────────────────────

    /// マウス座標を仮想スクリーン座標で返す。
    pub fn mouse_position(&self) -> (i32, i32) { crate::input::mouse_position() }
    pub fn is_mouse_pressed(&self, btn: MouseButton) -> bool      { crate::input::is_mouse_pressed(btn) }
    pub fn is_mouse_just_pressed(&self, btn: MouseButton) -> bool { crate::input::is_mouse_just_pressed(btn) }
    pub fn is_mouse_released(&self, btn: MouseButton) -> bool     { crate::input::is_mouse_released(btn) }

    // ── Gamepad ───────────────────────────────────────────────────────────────

    pub fn is_pad_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.inner_ref().gamepad.as_ref().map(|gm| gm.is_pressed(pad_id, btn)).unwrap_or(false)
    }

    pub fn is_pad_just_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.inner_ref().gamepad.as_ref().map(|gm| gm.is_just_pressed(pad_id, btn)).unwrap_or(false)
    }

    pub fn is_pad_released(&self, pad_id: usize, btn: PadButton) -> bool {
        self.inner_ref().gamepad.as_ref().map(|gm| gm.is_released(pad_id, btn)).unwrap_or(false)
    }

    pub fn pad_axis(&self, pad_id: usize, axis: PadAxis) -> f32 {
        self.inner_ref().gamepad.as_ref().map(|gm| gm.axis(pad_id, axis)).unwrap_or(0.0)
    }

    pub fn is_pad_connected(&self, pad_id: usize) -> bool {
        self.inner_ref().gamepad.as_ref().map(|gm| gm.is_connected(pad_id)).unwrap_or(false)
    }

    pub fn pad_count(&self) -> usize {
        self.inner_ref().gamepad.as_ref().map(|gm| gm.count()).unwrap_or(0)
    }

    // ── テキスト ──────────────────────────────────────────────────────────────

    /// デフォルトフォントのファイルを指定する。
    pub fn font_file(&mut self, path: &str) {
        self.default_font_path = Some(path.to_string());
        if let Some(inner) = &mut self.inner { inner.default_font = None; }
    }

    /// デフォルトフォントのサイズを指定する（ピクセル、デフォルト 16）。
    pub fn font_size(&mut self, size: u32) {
        self.default_font_size = size;
        if let Some(inner) = &mut self.inner { inner.default_font = None; }
    }

    fn ensure_default_font(&mut self) -> u32 {
        if self.inner.is_none() { return 0; }
        if let Some(id) = self.inner.as_ref().unwrap().default_font { return id; }
        let path = self.default_font_path.clone();
        let size = self.default_font_size;
        let id = if let Some(p) = path {
            crate::text::load_font(&p, size)
        } else {
            crate::text::load_default_font(size)
        };
        if id != 0 { self.inner.as_mut().unwrap().default_font = Some(id); }
        id
    }

    /// デフォルトフォントでテキストを描画する。
    pub fn screen_draw_text(&mut self, x: i32, y: i32, text: impl AsRef<str>, color: crate::draw::Color) {
        let font = self.ensure_default_font();
        self.screen_draw_text_ex(x, y, text, color, font);
    }

    /// フォントハンドルを指定してテキストを描画する。
    pub fn screen_draw_text_ex(&mut self, x: i32, y: i32, text: impl AsRef<str>, color: crate::draw::Color, font: u32) {
        if font == 0 { return; }
        if let Some((w, h, rgba)) = crate::text::build_text_bitmap(text.as_ref(), color, font) {
            self.inner_mut().draw_queue.push(DrawCommand::Text { x, y, width: w, height: h, rgba });
        }
    }

    // ── Window position ───────────────────────────────────────────────────────

    pub fn set_position(&self, x: i32, y: i32) {
        self.inner_ref().winit_window.set_outer_position(PhysicalPosition::new(x, y));
    }

    pub fn position(&self) -> (i32, i32) {
        match self.inner_ref().winit_window.outer_position() {
            Ok(p) => (p.x, p.y),
            Err(_) => (0, 0),
        }
    }

    // ── Screen factory ────────────────────────────────────────────────────────

    pub fn create_screen(&mut self, w: u16, h: u16) -> crate::screen::Screen {
        let inner = self.inner_mut();
        let ww = w as u32;
        let hh = h as u32;
        let sprite_id = crate::graphics::register_blank_sprite(ww, hh);
        let texture = inner.device.create_texture(&wgpu::TextureDescriptor {
            label:           None,
            size:            wgpu::Extent3d { width: ww, height: hh, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Rgba8Unorm,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats:    &[],
        });
        let view = texture.create_view(&Default::default());
        inner.sprite_cache.insert(sprite_id, SpriteGpuData {
            _texture:   None,
            view,
            width:      ww,
            height:     hh,
            gpu_native: true,
        });
        crate::screen::Screen::with_gpu(
            w, h, sprite_id,
            inner.device.clone(), inner.queue.clone(), texture,
            Arc::clone(&inner.color_pipeline),
            Arc::clone(&inner.sprite_pipeline),
            Arc::clone(&inner.sprite_pipeline_add),
            Arc::clone(&inner.sprite_pipeline_mul),
            Arc::clone(&inner.sprite_bgl),
        )
    }

    // ── Drawing ───────────────────────────────────────────────────────────────

    pub fn screen_clear(&mut self) {
        self.inner_mut().draw_queue.clear();
    }

    pub fn screen_mask_set(&mut self, x: i32, y: i32, handle: u32) {
        self.inner_mut().mask = Some((x, y, handle));
    }

    pub fn screen_mask_reset(&mut self) {
        self.inner_mut().mask = None;
    }

    pub fn screen_draw_sprite(&mut self, x: i32, y: i32, handle: u32) {
        let inner = self.inner_mut();
        let mask = inner.mask;
        inner.draw_queue.push(DrawCommand::Sprite {
            x, y, handle,
            mask_handle: mask.map(|(_, _, mh)| mh),
            mask_ox:     mask.map(|(mx, _, _)| mx).unwrap_or(0),
            mask_oy:     mask.map(|(_, my, _)| my).unwrap_or(0),
            params:      DrawSpriteParams::default(),
            blend:       BlendMode::Normal,
        });
    }

    pub fn screen_draw_sprite_ex(&mut self, x: i32, y: i32, handle: u32, params: DrawSpriteParams) {
        let inner = self.inner_mut();
        let mask  = inner.mask;
        let blend = inner.blend;
        inner.draw_queue.push(DrawCommand::Sprite {
            x, y, handle,
            mask_handle: mask.map(|(_, _, mh)| mh),
            mask_ox:     mask.map(|(mx, _, _)| mx).unwrap_or(0),
            mask_oy:     mask.map(|(_, my, _)| my).unwrap_or(0),
            params,
            blend,
        });
    }

    pub fn screen_blend_set(&mut self, blend: BlendMode) {
        self.inner_mut().blend = blend;
    }

    fn push_polys(&mut self, verts: Vec<ColorVert>) {
        if !verts.is_empty() {
            self.inner_mut().draw_queue.push(DrawCommand::Polys { verts });
        }
    }

    pub fn screen_draw_fill(&mut self, color: Color) {
        let (sw, sh) = { let i = self.inner_ref(); (i.screen_width, i.screen_height) };
        let v = verts_fill(sw, sh, color);
        self.push_polys(v);
    }

    pub fn screen_draw_pixel(&mut self, x: i32, y: i32, color: Color) {
        let (sw, sh) = { let i = self.inner_ref(); (i.screen_width, i.screen_height) };
        let v = verts_pixel(x, y, sw, sh, color);
        self.push_polys(v);
    }

    pub fn screen_draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: Color) {
        let (sw, sh) = { let i = self.inner_ref(); (i.screen_width, i.screen_height) };
        let v = verts_line(x1, y1, x2, y2, sw, sh, color);
        self.push_polys(v);
    }

    pub fn screen_draw_rectangle(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color, filled: bool) {
        let (sw, sh) = { let i = self.inner_ref(); (i.screen_width, i.screen_height) };
        let v = verts_rectangle(x, y, w, h, sw, sh, color, filled);
        self.push_polys(v);
    }

    pub fn screen_draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color, filled: bool) {
        let (sw, sh) = { let i = self.inner_ref(); (i.screen_width, i.screen_height) };
        let v = verts_circle(cx, cy, radius, sw, sh, color, filled);
        self.push_polys(v);
    }

    pub fn screen_draw_triangle(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, color: Color, filled: bool) {
        let (sw, sh) = { let i = self.inner_ref(); (i.screen_width, i.screen_height) };
        let v = verts_triangle(x1, y1, x2, y2, x3, y3, sw, sh, color, filled);
        self.push_polys(v);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn build_sprite_quad_ex(
    x: i32, y: i32,
    sprite_w: u32, sprite_h: u32,
    screen_w: u32, screen_h: u32,
    mask_ox: f32, mask_oy: f32, mask_w: f32, mask_h: f32, mask_on: f32,
    params: &DrawSpriteParams,
) -> [SpriteVertex; 6] {
    let sw     = screen_w as f32;
    let sh     = screen_h as f32;
    let draw_w = sprite_w as f32 * params.scale_x;
    let draw_h = sprite_h as f32 * params.scale_y;
    let cx     = x as f32 + draw_w * 0.5;
    let cy     = y as f32 + draw_h * 0.5;
    let hw     = draw_w * 0.5;
    let hh     = draw_h * 0.5;

    let rad = params.rotation.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let rot = |lx: f32, ly: f32| -> (f32, f32) { (lx * cos - ly * sin, lx * sin + ly * cos) };

    let (tl_x, tl_y) = rot(-hw, -hh);
    let (tr_x, tr_y) = rot( hw, -hh);
    let (bl_x, bl_y) = rot(-hw,  hh);
    let (br_x, br_y) = rot( hw,  hh);
    let corners = [
        (cx + tl_x, cy + tl_y),
        (cx + tr_x, cy + tr_y),
        (cx + bl_x, cy + bl_y),
        (cx + br_x, cy + br_y),
    ];

    let (u0, u1) = if params.flip_x { (1.0f32, 0.0f32) } else { (0.0f32, 1.0f32) };
    let (v0, v1) = if params.flip_y { (1.0f32, 0.0f32) } else { (0.0f32, 1.0f32) };

    let ndc = |px: f32, py: f32| -> [f32; 2] { [px / sw * 2.0 - 1.0, 1.0 - py / sh * 2.0] };
    let v = |idx: usize, u: f32, tv: f32| SpriteVertex {
        pos:       ndc(corners[idx].0, corners[idx].1),
        uv:        [u, tv],
        screen_xy: [corners[idx].0, corners[idx].1],
        mask_ox, mask_oy, mask_w, mask_h, mask_on,
        alpha:     params.alpha,
    };

    let tl = v(0, u0, v0);
    let tr = v(1, u1, v0);
    let bl = v(2, u0, v1);
    let br = v(3, u1, v1);
    [tl, tr, bl, tr, br, bl]
}
