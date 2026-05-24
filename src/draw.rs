#[derive(Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self { r, g, b, a: 255 } }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Self { r, g, b, a } }

    pub const WHITE:   Color = Color::rgb(255, 255, 255);
    pub const BLACK:   Color = Color::rgb(0,   0,   0  );
    pub const RED:     Color = Color::rgb(255, 0,   0  );
    pub const GREEN:   Color = Color::rgb(0,   255, 0  );
    pub const BLUE:    Color = Color::rgb(0,   0,   255);
    pub const YELLOW:  Color = Color::rgb(255, 255, 0  );
    pub const CYAN:    Color = Color::rgb(0,   255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0,   255);
}

// ── internal helpers ──────────────────────────────────────────────────────────

fn blend(buf: &mut [u8], i: usize, c: Color) {
    if c.a == 0 { return; }
    if c.a == 255 {
        buf[i]     = c.r;
        buf[i + 1] = c.g;
        buf[i + 2] = c.b;
        buf[i + 3] = 255;
    } else {
        let sa = c.a as f32 / 255.0;
        let da = buf[i + 3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        buf[i + 3] = (oa * 255.0) as u8;
        if oa > 0.0 {
            let oi = 1.0 / oa;
            buf[i]     = ((c.r as f32 * sa + buf[i]     as f32 * da * (1.0 - sa)) * oi) as u8;
            buf[i + 1] = ((c.g as f32 * sa + buf[i + 1] as f32 * da * (1.0 - sa)) * oi) as u8;
            buf[i + 2] = ((c.b as f32 * sa + buf[i + 2] as f32 * da * (1.0 - sa)) * oi) as u8;
        }
    }
}

fn put(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, c: Color) {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 { return; }
    blend(buf, (y as usize * w as usize + x as usize) * 4, c);
}

// ── public drawing primitives (operate on raw RGBA buffers) ───────────────────

pub(crate) fn draw_fill(buf: &mut [u8], c: Color) {
    match c.a {
        0 => {}
        255 => {
            for p in buf.chunks_exact_mut(4) {
                p[0] = c.r; p[1] = c.g; p[2] = c.b; p[3] = 255;
            }
        }
        a => {
            let sa = a as f32 / 255.0;
            let inv_sa = 1.0 - sa;
            let cr = c.r as f32 * sa;
            let cg = c.g as f32 * sa;
            let cb = c.b as f32 * sa;
            for p in buf.chunks_exact_mut(4) {
                if p[3] == 0 {
                    p[0] = c.r; p[1] = c.g; p[2] = c.b; p[3] = a;
                } else {
                    let da = p[3] as f32 / 255.0;
                    let oa = sa + da * inv_sa;
                    let oi = 1.0 / oa;
                    p[0] = ((cr + p[0] as f32 * da * inv_sa) * oi) as u8;
                    p[1] = ((cg + p[1] as f32 * da * inv_sa) * oi) as u8;
                    p[2] = ((cb + p[2] as f32 * da * inv_sa) * oi) as u8;
                    p[3] = (oa * 255.0) as u8;
                }
            }
        }
    }
}

pub(crate) fn draw_pixel(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, c: Color) {
    put(buf, w, h, x, y, c);
}

pub(crate) fn draw_line(buf: &mut [u8], w: u32, h: u32, x1: i32, y1: i32, x2: i32, y2: i32, c: Color) {
    let dx = (x2 - x1).abs();
    let dy = (y2 - y1).abs();
    let sx = if x1 < x2 { 1i32 } else { -1 };
    let sy = if y1 < y2 { 1i32 } else { -1 };
    let mut err = dx - dy;
    let (mut x, mut y) = (x1, y1);
    loop {
        put(buf, w, h, x, y, c);
        if x == x2 && y == y2 { break; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 <  dx { err += dx; y += sy; }
    }
}

pub(crate) fn draw_rectangle(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, rw: i32, rh: i32, c: Color, filled: bool) {
    if filled {
        for ry in y..(y + rh) {
            for rx in x..(x + rw) {
                put(buf, w, h, rx, ry, c);
            }
        }
    } else {
        for rx in x..(x + rw) {
            put(buf, w, h, rx, y,          c);
            put(buf, w, h, rx, y + rh - 1, c);
        }
        for ry in y..(y + rh) {
            put(buf, w, h, x,          ry, c);
            put(buf, w, h, x + rw - 1, ry, c);
        }
    }
}

pub(crate) fn draw_triangle(buf: &mut [u8], w: u32, h: u32, x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, c: Color, filled: bool) {
    if !filled {
        draw_line(buf, w, h, x1, y1, x2, y2, c);
        draw_line(buf, w, h, x2, y2, x3, y3, c);
        draw_line(buf, w, h, x3, y3, x1, y1, c);
        return;
    }
    let min_y = y1.min(y2).min(y3);
    let max_y = y1.max(y2).max(y3);
    let edges = [(x1, y1, x2, y2), (x2, y2, x3, y3), (x3, y3, x1, y1)];
    for y in min_y..=max_y {
        let mut x_min = i32::MAX;
        let mut x_max = i32::MIN;
        for (px, py, qx, qy) in edges {
            if py == qy {
                if y == py {
                    x_min = x_min.min(px.min(qx));
                    x_max = x_max.max(px.max(qx));
                }
            } else if y >= py.min(qy) && y <= py.max(qy) {
                let t = (y - py) as f32 / (qy - py) as f32;
                let x = (px as f32 + (qx - px) as f32 * t).round() as i32;
                x_min = x_min.min(x);
                x_max = x_max.max(x);
            }
        }
        for px in x_min..=x_max {
            put(buf, w, h, px, y, c);
        }
    }
}

pub(crate) fn draw_circle(buf: &mut [u8], w: u32, h: u32, cx: i32, cy: i32, radius: i32, c: Color, filled: bool) {
    if radius <= 0 {
        put(buf, w, h, cx, cy, c);
        return;
    }
    if filled {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            let dx = ((r2 - dy * dy) as f64).sqrt() as i32;
            for px in (cx - dx)..=(cx + dx) {
                put(buf, w, h, px, cy + dy, c);
            }
        }
    } else {
        let mut ox = 0i32;
        let mut oy = radius;
        let mut p = 1 - radius;
        while ox <= oy {
            put(buf, w, h, cx + ox, cy + oy, c);
            put(buf, w, h, cx - ox, cy + oy, c);
            put(buf, w, h, cx + ox, cy - oy, c);
            put(buf, w, h, cx - ox, cy - oy, c);
            put(buf, w, h, cx + oy, cy + ox, c);
            put(buf, w, h, cx - oy, cy + ox, c);
            put(buf, w, h, cx + oy, cy - ox, c);
            put(buf, w, h, cx - oy, cy - ox, c);
            ox += 1;
            p += if p < 0 { 2 * ox + 1 } else { oy -= 1; 2 * (ox - oy) + 1 };
        }
    }
}

// ── GPU vertex generation ──────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ColorVert {
    pub pos:   [f32; 2],
    pub color: [f32; 4],
}

pub(crate) fn to_ndc(px: f32, py: f32, sw: u32, sh: u32) -> [f32; 2] {
    [px / sw as f32 * 2.0 - 1.0, 1.0 - py / sh as f32 * 2.0]
}

fn cf(c: Color) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, c.a as f32 / 255.0]
}

fn push_quad(v: &mut Vec<ColorVert>, x0: f32, y0: f32, x1: f32, y1: f32, sw: u32, sh: u32, c: Color) {
    let col = cf(c);
    for (x, y) in [(x0,y0),(x1,y0),(x0,y1),(x1,y0),(x1,y1),(x0,y1)] {
        v.push(ColorVert { pos: to_ndc(x, y, sw, sh), color: col });
    }
}

fn push_line(v: &mut Vec<ColorVert>, x0: f32, y0: f32, x1: f32, y1: f32, sw: u32, sh: u32, c: Color) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx*dx + dy*dy).sqrt();
    if len < 0.5 { push_quad(v, x0, y0, x0+1.0, y0+1.0, sw, sh, c); return; }
    let (nx, ny) = (-dy / len, dx / len);
    let col = cf(c);
    for (x, y) in [(x0+nx,y0+ny),(x1+nx,y1+ny),(x0-nx,y0-ny),(x1+nx,y1+ny),(x1-nx,y1-ny),(x0-nx,y0-ny)] {
        v.push(ColorVert { pos: to_ndc(x, y, sw, sh), color: col });
    }
}

pub(crate) fn verts_fill(sw: u32, sh: u32, c: Color) -> Vec<ColorVert> {
    let mut v = Vec::with_capacity(6);
    push_quad(&mut v, 0.0, 0.0, sw as f32, sh as f32, sw, sh, c);
    v
}

pub(crate) fn verts_pixel(x: i32, y: i32, sw: u32, sh: u32, c: Color) -> Vec<ColorVert> {
    let mut v = Vec::with_capacity(6);
    push_quad(&mut v, x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0, sw, sh, c);
    v
}

pub(crate) fn verts_line(x1: i32, y1: i32, x2: i32, y2: i32, sw: u32, sh: u32, c: Color) -> Vec<ColorVert> {
    let mut v = Vec::with_capacity(6);
    push_line(&mut v, x1 as f32, y1 as f32, x2 as f32, y2 as f32, sw, sh, c);
    v
}

pub(crate) fn verts_rectangle(x: i32, y: i32, w: i32, h: i32, sw: u32, sh: u32, c: Color, filled: bool) -> Vec<ColorVert> {
    let mut v = Vec::new();
    if filled {
        push_quad(&mut v, x as f32, y as f32, (x+w) as f32, (y+h) as f32, sw, sh, c);
    } else {
        push_quad(&mut v, x as f32,       y as f32,       (x+w) as f32,   y as f32 + 1.0,    sw, sh, c);
        push_quad(&mut v, x as f32,       (y+h-1) as f32, (x+w) as f32,   (y+h) as f32,       sw, sh, c);
        push_quad(&mut v, x as f32,       (y+1) as f32,   x as f32 + 1.0, (y+h-1) as f32,     sw, sh, c);
        push_quad(&mut v, (x+w-1) as f32, (y+1) as f32,   (x+w) as f32,   (y+h-1) as f32,     sw, sh, c);
    }
    v
}

pub(crate) fn verts_triangle(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, sw: u32, sh: u32, c: Color, filled: bool) -> Vec<ColorVert> {
    let col = cf(c);
    let mut v = Vec::new();
    if filled {
        for (x, y) in [(x1,y1),(x2,y2),(x3,y3)] {
            v.push(ColorVert { pos: to_ndc(x as f32, y as f32, sw, sh), color: col });
        }
    } else {
        push_line(&mut v, x1 as f32, y1 as f32, x2 as f32, y2 as f32, sw, sh, c);
        push_line(&mut v, x2 as f32, y2 as f32, x3 as f32, y3 as f32, sw, sh, c);
        push_line(&mut v, x3 as f32, y3 as f32, x1 as f32, y1 as f32, sw, sh, c);
    }
    v
}

pub(crate) fn verts_circle(cx: i32, cy: i32, radius: i32, sw: u32, sh: u32, c: Color, filled: bool) -> Vec<ColorVert> {
    let col = cf(c);
    let (cx, cy, r) = (cx as f32, cy as f32, radius.max(1) as f32);
    let n = (radius * 4).clamp(16, 256) as usize;
    let mut v = Vec::with_capacity(if filled { n * 3 } else { n * 6 });
    for i in 0..n {
        let a0 = i       as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let (p0x, p0y) = (cx + r * a0.cos(), cy + r * a0.sin());
        let (p1x, p1y) = (cx + r * a1.cos(), cy + r * a1.sin());
        if filled {
            v.push(ColorVert { pos: to_ndc(cx,  cy,  sw, sh), color: col });
            v.push(ColorVert { pos: to_ndc(p0x, p0y, sw, sh), color: col });
            v.push(ColorVert { pos: to_ndc(p1x, p1y, sw, sh), color: col });
        } else {
            push_line(&mut v, p0x, p0y, p1x, p1y, sw, sh, c);
        }
    }
    v
}
