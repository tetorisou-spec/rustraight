use std::collections::HashMap;
use std::sync::Arc;

use crate::draw::{
    draw_circle, draw_fill, draw_line, draw_pixel, draw_rectangle, draw_triangle, Color,
    ColorVert, verts_circle, verts_fill, verts_line, verts_pixel, verts_rectangle, verts_triangle,
};
use crate::graphics::{
    blit_sprite, blit_sprite_masked, register_blank_sprite, update_sprite,
    BlendMode, DrawSpriteParams,
};
use crate::window::{SpriteVertex, build_sprite_quad_ex};

fn slice_as_bytes<T>(data: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data)) }
}

// ── Local GPU sprite cache ────────────────────────────────────────────────────

struct ScreenSpriteData {
    _texture: wgpu::Texture,
    view:     wgpu::TextureView,
    width:    u32,
    height:   u32,
}

fn ensure_screen_sprite(
    handle: u32,
    device: &wgpu::Device,
    queue:  &wgpu::Queue,
    cache:  &mut HashMap<u32, ScreenSpriteData>,
) {
    if cache.contains_key(&handle) { return; }
    crate::graphics::with_sprite(handle, |w, h, rgba| {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label:           None,
            size:            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count:    1,
            dimension:       wgpu::TextureDimension::D2,
            format:          wgpu::TextureFormat::Rgba8Unorm,
            usage:           wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats:    &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex, mip_level: 0,
                origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&Default::default());
        cache.insert(handle, ScreenSpriteData { _texture: tex, view, width: w, height: h });
    });
}

// ── Draw command queue ────────────────────────────────────────────────────────

#[derive(Clone)]
enum ScreenCmd {
    Polys(Vec<ColorVert>),
    Sprite {
        x: i32, y: i32, handle: u32,
        mask_handle: Option<u32>, mask_ox: i32, mask_oy: i32,
        params: DrawSpriteParams, blend: BlendMode,
    },
}

// ── GPU state ─────────────────────────────────────────────────────────────────

struct ScreenGpu {
    device:              wgpu::Device,
    queue:               wgpu::Queue,
    _texture:            wgpu::Texture,
    render_view:         wgpu::TextureView,
    color_pipeline:      Arc<wgpu::RenderPipeline>,
    sprite_pipeline:     Arc<wgpu::RenderPipeline>,
    sprite_pipeline_add: Arc<wgpu::RenderPipeline>,
    sprite_pipeline_mul: Arc<wgpu::RenderPipeline>,
    sprite_bgl:          Arc<wgpu::BindGroupLayout>,
    sampler:             wgpu::Sampler,
    _dummy_texture:      wgpu::Texture,
    dummy_view:          wgpu::TextureView,
    cmds:                Vec<ScreenCmd>,
    sprite_cache:        HashMap<u32, ScreenSpriteData>,
    cleared:             bool,
    blend:               BlendMode,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub struct Screen {
    width:     u32,
    height:    u32,
    buffer:    Vec<u8>,
    sprite_id: u32,
    mask:      Option<(i32, i32, u32)>,
    gpu:       Option<Box<ScreenGpu>>,
}

impl Screen {
    pub fn new(width: u16, height: u16) -> Self {
        let w = width as u32;
        let h = height as u32;
        Self {
            width:     w,
            height:    h,
            buffer:    vec![0u8; w as usize * h as usize * 4],
            sprite_id: register_blank_sprite(w, h),
            mask:      None,
            gpu:       None,
        }
    }

    pub(crate) fn with_gpu(
        width: u16, height: u16, sprite_id: u32,
        device: wgpu::Device, queue: wgpu::Queue,
        texture: wgpu::Texture,
        color_pipeline:      Arc<wgpu::RenderPipeline>,
        sprite_pipeline:     Arc<wgpu::RenderPipeline>,
        sprite_pipeline_add: Arc<wgpu::RenderPipeline>,
        sprite_pipeline_mul: Arc<wgpu::RenderPipeline>,
        sprite_bgl:          Arc<wgpu::BindGroupLayout>,
    ) -> Self {
        let w = width as u32;
        let h = height as u32;
        let render_view = texture.create_view(&Default::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter:     wgpu::FilterMode::Nearest,
            min_filter:     wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let dummy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label:           Some("screen_dummy"),
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

        Self {
            width: w, height: h,
            buffer:    vec![0u8; w as usize * h as usize * 4],
            sprite_id,
            mask:  None,
            gpu:   Some(Box::new(ScreenGpu {
                device, queue, _texture: texture, render_view,
                color_pipeline, sprite_pipeline, sprite_pipeline_add, sprite_pipeline_mul,
                sprite_bgl, sampler, _dummy_texture: dummy_texture, dummy_view,
                cmds:         Vec::new(),
                sprite_cache: HashMap::new(),
                cleared:      true,
                blend:        BlendMode::Normal,
            })),
        }
    }

    pub fn clear(&mut self) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.clear();
            gpu.cleared = true;
        } else {
            self.buffer.fill(0);
        }
    }

    // ── Color primitives ──────────────────────────────────────────────────────

    pub fn draw_fill(&mut self, color: Color) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.push(ScreenCmd::Polys(verts_fill(self.width, self.height, color)));
        } else {
            draw_fill(&mut self.buffer, color);
        }
    }

    pub fn draw_pixel(&mut self, x: i32, y: i32, color: Color) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.push(ScreenCmd::Polys(verts_pixel(x, y, self.width, self.height, color)));
        } else {
            draw_pixel(&mut self.buffer, self.width, self.height, x, y, color);
        }
    }

    pub fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: Color) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.push(ScreenCmd::Polys(verts_line(x1, y1, x2, y2, self.width, self.height, color)));
        } else {
            draw_line(&mut self.buffer, self.width, self.height, x1, y1, x2, y2, color);
        }
    }

    pub fn draw_rectangle(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color, filled: bool) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.push(ScreenCmd::Polys(verts_rectangle(x, y, w, h, self.width, self.height, color, filled)));
        } else {
            draw_rectangle(&mut self.buffer, self.width, self.height, x, y, w, h, color, filled);
        }
    }

    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color, filled: bool) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.push(ScreenCmd::Polys(verts_circle(cx, cy, radius, self.width, self.height, color, filled)));
        } else {
            draw_circle(&mut self.buffer, self.width, self.height, cx, cy, radius, color, filled);
        }
    }

    pub fn draw_triangle(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, color: Color, filled: bool) {
        if let Some(gpu) = &mut self.gpu {
            gpu.cmds.push(ScreenCmd::Polys(verts_triangle(x1, y1, x2, y2, x3, y3, self.width, self.height, color, filled)));
        } else {
            draw_triangle(&mut self.buffer, self.width, self.height, x1, y1, x2, y2, x3, y3, color, filled);
        }
    }

    // ── Sprite drawing ────────────────────────────────────────────────────────

    pub fn mask_set(&mut self, x: i32, y: i32, handle: u32) {
        self.mask = Some((x, y, handle));
    }

    pub fn mask_reset(&mut self) {
        self.mask = None;
    }

    pub fn draw_sprite(&mut self, x: i32, y: i32, handle: u32) {
        let mask = self.mask;
        if let Some(gpu) = &mut self.gpu {
            let blend = gpu.blend;
            gpu.cmds.push(ScreenCmd::Sprite {
                x, y, handle,
                mask_handle: mask.map(|(_, _, mh)| mh),
                mask_ox:     mask.map(|(mx, _, _)| mx).unwrap_or(0),
                mask_oy:     mask.map(|(_, my, _)| my).unwrap_or(0),
                params:      DrawSpriteParams::default(),
                blend,
            });
        } else if let Some((mx, my, mh)) = mask {
            blit_sprite_masked(&mut self.buffer, self.width, self.height, x, y, handle, mx, my, mh);
        } else {
            blit_sprite(&mut self.buffer, self.width, self.height, x, y, handle);
        }
    }

    pub fn draw_sprite_ex(&mut self, x: i32, y: i32, handle: u32, params: DrawSpriteParams) {
        let mask = self.mask;
        if let Some(gpu) = &mut self.gpu {
            let blend = gpu.blend;
            gpu.cmds.push(ScreenCmd::Sprite {
                x, y, handle,
                mask_handle: mask.map(|(_, _, mh)| mh),
                mask_ox:     mask.map(|(mx, _, _)| mx).unwrap_or(0),
                mask_oy:     mask.map(|(_, my, _)| my).unwrap_or(0),
                params,
                blend,
            });
        } else {
            // CPU fallback: ignore rotation/scale/flip, apply alpha via blit
            if let Some((mx, my, mh)) = mask {
                blit_sprite_masked(&mut self.buffer, self.width, self.height, x, y, handle, mx, my, mh);
            } else {
                blit_sprite(&mut self.buffer, self.width, self.height, x, y, handle);
            }
        }
    }

    pub fn blend_set(&mut self, blend: BlendMode) {
        if let Some(gpu) = &mut self.gpu { gpu.blend = blend; }
    }

    // ── Flush / sprite handle ─────────────────────────────────────────────────

    pub fn sprite(&mut self) -> u32 {
        let sw = self.width;
        let sh = self.height;
        if let Some(gpu) = &mut self.gpu {
            let has_work = gpu.cleared || !gpu.cmds.is_empty();
            if has_work {
                // Drain command queue before any other borrows
                let cmds = std::mem::take(&mut gpu.cmds);

                // Upload any new CPU sprites to Screen's local GPU cache
                for cmd in &cmds {
                    if let ScreenCmd::Sprite { handle, mask_handle, .. } = cmd {
                        ensure_screen_sprite(*handle, &gpu.device, &gpu.queue, &mut gpu.sprite_cache);
                        if let Some(mh) = mask_handle {
                            ensure_screen_sprite(*mh, &gpu.device, &gpu.queue, &mut gpu.sprite_cache);
                        }
                    }
                }

                // Build vertex data maintaining draw order
                let mut color_verts:  Vec<ColorVert>   = Vec::new();
                let mut sprite_verts: Vec<SpriteVertex> = Vec::new();

                enum RItem {
                    Polys  { base: u32, count: u32 },
                    Sprite { base: u32, handle: u32, mask_handle: Option<u32>, blend: BlendMode },
                }
                let mut items: Vec<RItem> = Vec::new();

                for cmd in &cmds {
                    match cmd {
                        ScreenCmd::Polys(verts) => {
                            let base  = color_verts.len() as u32;
                            let count = verts.len() as u32;
                            color_verts.extend_from_slice(verts);
                            items.push(RItem::Polys { base, count });
                        }
                        ScreenCmd::Sprite { x, y, handle, mask_handle, mask_ox, mask_oy, params, blend } => {
                            if let Some(sd) = gpu.sprite_cache.get(handle) {
                                let base = sprite_verts.len() as u32;
                                let (mox, moy, mw, mh, mon) = if let Some(mh) = mask_handle {
                                    if let Some(md) = gpu.sprite_cache.get(mh) {
                                        (*mask_ox as f32, *mask_oy as f32, md.width as f32, md.height as f32, 1.0f32)
                                    } else { (0., 0., 1., 1., 0.) }
                                } else { (0., 0., 1., 1., 0.) };
                                sprite_verts.extend_from_slice(&build_sprite_quad_ex(
                                    *x, *y, sd.width, sd.height, sw, sh,
                                    mox, moy, mw, mh, mon, params,
                                ));
                                items.push(RItem::Sprite { base, handle: *handle, mask_handle: *mask_handle, blend: *blend });
                            }
                        }
                    }
                }

                // Create vertex buffers
                let color_buf = if !color_verts.is_empty() {
                    use wgpu::util::DeviceExt;
                    Some(gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label:    Some("screen_color_vbuf"),
                        contents: slice_as_bytes(&color_verts),
                        usage:    wgpu::BufferUsages::VERTEX,
                    }))
                } else { None };

                let sprite_buf = if !sprite_verts.is_empty() {
                    use wgpu::util::DeviceExt;
                    Some(gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label:    Some("screen_sprite_vbuf"),
                        contents: slice_as_bytes(&sprite_verts),
                        usage:    wgpu::BufferUsages::VERTEX,
                    }))
                } else { None };

                // Build sprite bind groups
                let mut sprite_bgs: Vec<wgpu::BindGroup> = Vec::new();
                for item in &items {
                    if let RItem::Sprite { handle, mask_handle, .. } = item {
                        if let Some(sd) = gpu.sprite_cache.get(handle) {
                            let mask_view = mask_handle
                                .and_then(|mh| gpu.sprite_cache.get(&mh))
                                .map(|md| &md.view)
                                .unwrap_or(&gpu.dummy_view);
                            sprite_bgs.push(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label:   None,
                                layout:  &gpu.sprite_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sd.view) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(mask_view) },
                                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&gpu.sampler) },
                                ],
                            }));
                        }
                    }
                }

                // Render pass
                let load = if gpu.cleared {
                    wgpu::LoadOp::Clear(wgpu::Color { r: 0., g: 0., b: 0., a: 0. })
                } else {
                    wgpu::LoadOp::Load
                };
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("screen_draw"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view:           &gpu.render_view,
                            resolve_target: None,
                            ops:            wgpu::Operations { load, store: wgpu::StoreOp::Store },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes:         None,
                        occlusion_query_set:      None,
                    });

                    let mut sprite_bg_idx = 0usize;
                    for item in &items {
                        match item {
                            RItem::Polys { base, count } => {
                                if let Some(buf) = &color_buf {
                                    rpass.set_pipeline(&gpu.color_pipeline);
                                    rpass.set_vertex_buffer(0, buf.slice(..));
                                    rpass.draw(*base..*base + count, 0..1);
                                }
                            }
                            RItem::Sprite { base, blend, .. } => {
                                if sprite_bg_idx < sprite_bgs.len() {
                                    if let Some(buf) = &sprite_buf {
                                        let pipeline = match blend {
                                            BlendMode::Normal => &gpu.sprite_pipeline,
                                            BlendMode::Add    => &gpu.sprite_pipeline_add,
                                            BlendMode::Mul    => &gpu.sprite_pipeline_mul,
                                        };
                                        rpass.set_pipeline(pipeline);
                                        rpass.set_bind_group(0, &sprite_bgs[sprite_bg_idx], &[]);
                                        rpass.set_vertex_buffer(0, buf.slice(..));
                                        rpass.draw(*base..*base + 6, 0..1);
                                    }
                                    sprite_bg_idx += 1;
                                }
                            }
                        }
                    }
                }
                gpu.queue.submit(std::iter::once(encoder.finish()));
                gpu.cleared = false;
            }
        } else {
            update_sprite(self.sprite_id, &self.buffer);
        }
        self.sprite_id
    }
}
