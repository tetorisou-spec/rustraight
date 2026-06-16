// Blits screen_texture (straight alpha) to swap chain as pre-multiplied alpha.
// Blit for sRGB swap chain: GPU applies gamma automatically; screen_texture has
// premultiplied-linear RGB from ALPHA_BLENDING, so just pass through.
pub(super) const BLIT_SHADER: &str = r#"
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
    return vec4(c.rgb, c.a);
}
"#;

// Blit for Bgra8Unorm swap chain (used when transparent=true): GPU does NOT apply
// gamma, so we encode to sRGB manually.  Alpha is kept as-is (premultiplied-linear
// RGB from ALPHA_BLENDING is already the correct premultiplied value).
pub(super) const BLIT_SHADER_UNORM: &str = r#"
struct Vout { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) vi: u32) -> Vout {
    var p = array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(-1.,1.),vec2(1.,-1.),vec2(1.,1.),vec2(-1.,1.));
    var u = array<vec2<f32>,6>(vec2(0.,1.),vec2(1.,1.),vec2(0.,0.),vec2(1.,1.),vec2(1.,0.),vec2(0.,0.));
    return Vout(vec4(p[vi],0.,1.), u[vi]);
}
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
fn lin_to_srgb(x: f32) -> f32 {
    if x <= 0.0031308 { return x * 12.92; }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> {
    let c = textureSample(t, s, in.uv);
    return vec4(lin_to_srgb(c.r), lin_to_srgb(c.g), lin_to_srgb(c.b), c.a);
}
"#;

// Renders a textured image quad with optional mask modulation and per-vertex alpha.
pub(super) const SPRITE_SHADER: &str = r#"
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
@group(0) @binding(0) var t_image: texture_2d<f32>;
@group(0) @binding(1) var t_mask:   texture_2d<f32>;
@group(0) @binding(2) var s_samp:   sampler;
@vertex fn vs(v: Vin) -> Vout {
    return Vout(vec4(v.pos, 0., 1.), v.uv, v.screen_xy, v.mask_ox, v.mask_oy, v.mask_w, v.mask_h, v.mask_on, v.alpha);
}
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> {
    var c = textureSample(t_image, s_samp, in.uv);
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

// Image shader for overlay (screen_draw overflow): discards fragments inside main window rect.
pub(super) const MASKED_SPRITE_SHADER: &str = r#"
struct Vin {
    @location(0) pos:       vec2<f32>, @location(1) uv:        vec2<f32>,
    @location(2) screen_xy: vec2<f32>, @location(3) mask_ox:   f32,
    @location(4) mask_oy:   f32,       @location(5) mask_w:    f32,
    @location(6) mask_h:    f32,       @location(7) mask_on:   f32,
    @location(8) alpha:     f32,
}
struct Vout {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv:        vec2<f32>, @location(1) screen_xy: vec2<f32>,
    @location(2) mask_ox:   f32,       @location(3) mask_oy:   f32,
    @location(4) mask_w:    f32,       @location(5) mask_h:    f32,
    @location(6) mask_on:   f32,       @location(7) alpha:     f32,
}
@group(0) @binding(0) var t_image: texture_2d<f32>;
@group(0) @binding(1) var t_mask:   texture_2d<f32>;
@group(0) @binding(2) var s_samp:   sampler;
@group(1) @binding(0) var<uniform> main_rect: vec4<f32>; // x,y,w,h in display pixels
@vertex fn vs(v: Vin) -> Vout {
    return Vout(vec4(v.pos,0.,1.), v.uv, v.screen_xy, v.mask_ox, v.mask_oy, v.mask_w, v.mask_h, v.mask_on, v.alpha);
}
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> {
    let p = in.clip.xy;
    if p.x >= main_rect.x && p.x < main_rect.x + main_rect.z
    && p.y >= main_rect.y && p.y < main_rect.y + main_rect.w { discard; }
    var c = textureSample(t_image, s_samp, in.uv);
    if in.mask_on > 0.5 {
        let mx = (in.screen_xy.x - in.mask_ox) / in.mask_w;
        let my = (in.screen_xy.y - in.mask_oy) / in.mask_h;
        if mx >= 0. && mx <= 1. && my >= 0. && my <= 1. {
            c.a *= textureSample(t_mask, s_samp, vec2(mx, my)).a;
        } else { c.a = 0.; }
    }
    c.a *= in.alpha;
    return c;
}
"#;

// Color geometry shader for overlay: discards fragments inside main window rect.
pub(super) const MASKED_COLOR_SHADER: &str = r#"
struct Vin  { @location(0) pos: vec2<f32>, @location(1) color: vec4<f32> }
struct Vout { @builtin(position) clip: vec4<f32>, @location(0) color: vec4<f32> }
@group(0) @binding(0) var<uniform> main_rect: vec4<f32>;
@vertex fn vs(v: Vin) -> Vout { return Vout(vec4(v.pos, 0., 1.), v.color); }
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> {
    let p = in.clip.xy;
    if p.x >= main_rect.x && p.x < main_rect.x + main_rect.z
    && p.y >= main_rect.y && p.y < main_rect.y + main_rect.w { discard; }
    return in.color;
}
"#;

// Renders colored vertex geometry (fill, lines, shapes).
pub(super) const COLOR_SHADER: &str = r#"
struct Vin  { @location(0) pos: vec2<f32>, @location(1) color: vec4<f32> }
struct Vout { @builtin(position) clip: vec4<f32>, @location(0) color: vec4<f32> }
@vertex fn vs(v: Vin) -> Vout { return Vout(vec4(v.pos, 0., 1.), v.color); }
@fragment fn fs(in: Vout) -> @location(0) vec4<f32> { return in.color; }
"#;
