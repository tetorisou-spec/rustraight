use std::collections::HashMap;
use std::sync::Arc;
use std::num::NonZeroIsize;

use raw_window_handle::{Win32WindowHandle, WindowsDisplayHandle, RawWindowHandle, RawDisplayHandle};

use crate::draw::{Color, ColorVert, verts_circle, verts_fill, verts_line, verts_pixel, verts_rectangle, verts_triangle};
use crate::gamepad::{GamepadManager, PadAxis, PadButton};
use crate::graphics::{BlendMode, DrawSpriteParams};
use crate::input::{KeyCode, MouseButton};

// ── Win32 イベントバッファ ─────────────────────────────────────────────────────

#[derive(Default)]
struct Win32Events {
    should_close:     bool,
    key_events:       Vec<(u16, bool, bool)>, // (resolved_vk, extended, pressed)
    resize_event:     Option<(u32, u32)>,
    cursor_moved:     Option<(i32, i32)>,
    mouse_btn_events: Vec<(u8, bool)>,
}

impl Win32Events {
    fn take_frame(&mut self) -> Self {
        Self {
            should_close:     self.should_close, // 一度立ったら保持
            key_events:       std::mem::take(&mut self.key_events),
            resize_event:     self.resize_event.take(),
            cursor_moved:     self.cursor_moved.take(),
            mouse_btn_events: std::mem::take(&mut self.mouse_btn_events),
        }
    }
}

thread_local! {
    static WIN32_EVENTS: std::cell::RefCell<Win32Events> =
        std::cell::RefCell::new(Win32Events::default());
}

/// メインウィンドウ用 Win32 ウィンドウプロシージャ。
/// WM_CLOSE を捕捉して should_close を立てる。実際の DestroyWindow は WindowInner::Drop が行う。
#[cfg(target_os = "windows")]
unsafe extern "system" fn main_wnd_proc(
    hwnd:   windows_sys::Win32::Foundation::HWND,
    msg:    u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    match msg {
        WM_CLOSE => {
            WIN32_EVENTS.with(|e| e.borrow_mut().should_close = true);
            0 // DefWindowProcW を呼ばない → DestroyWindow は WindowInner::Drop に委ねる
        }
        WM_SIZE => {
            let w = (lparam & 0xFFFF) as u32;
            let h = ((lparam >> 16) & 0xFFFF) as u32;
            if w > 0 && h > 0 {
                WIN32_EVENTS.with(|e| e.borrow_mut().resize_event = Some((w, h)));
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let repeat = (lparam >> 30) & 1 == 1;
            if !repeat {
                let raw_vk = wparam as u16;
                let scan   = ((lparam >> 16) & 0xFF) as u16;
                let ext    = ((lparam >> 24) & 1) != 0;
                // Shift の L/R 解決: スキャンコード 0x2A=Left 0x36=Right
                let vk = match raw_vk {
                    0x10 => if scan == 0x2A { 0xA0u16 } else { 0xA1u16 },
                    0x11 => if ext { 0xA3u16 } else { 0xA2u16 },
                    0x12 => if ext { 0xA5u16 } else { 0xA4u16 },
                    v    => v,
                };
                WIN32_EVENTS.with(|e| e.borrow_mut().key_events.push((vk, ext, true)));
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let raw_vk = wparam as u16;
            let scan   = ((lparam >> 16) & 0xFF) as u16;
            let ext    = ((lparam >> 24) & 1) != 0;
            let vk = match raw_vk {
                0x10 => if scan == 0x2A { 0xA0u16 } else { 0xA1u16 },
                0x11 => if ext { 0xA3u16 } else { 0xA2u16 },
                0x12 => if ext { 0xA5u16 } else { 0xA4u16 },
                v    => v,
            };
            WIN32_EVENTS.with(|e| e.borrow_mut().key_events.push((vk, ext, false)));
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as i32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
            WIN32_EVENTS.with(|e| e.borrow_mut().cursor_moved = Some((x, y)));
            0
        }
        WM_LBUTTONDOWN => { WIN32_EVENTS.with(|e| e.borrow_mut().mouse_btn_events.push((0, true)));  0 }
        WM_LBUTTONUP   => { WIN32_EVENTS.with(|e| e.borrow_mut().mouse_btn_events.push((0, false))); 0 }
        WM_RBUTTONDOWN => { WIN32_EVENTS.with(|e| e.borrow_mut().mouse_btn_events.push((1, true)));  0 }
        WM_RBUTTONUP   => { WIN32_EVENTS.with(|e| e.borrow_mut().mouse_btn_events.push((1, false))); 0 }
        WM_MBUTTONDOWN => { WIN32_EVENTS.with(|e| e.borrow_mut().mouse_btn_events.push((2, true)));  0 }
        WM_MBUTTONUP   => { WIN32_EVENTS.with(|e| e.borrow_mut().mouse_btn_events.push((2, false))); 0 }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

/// メインウィンドウを Win32 で直接生成する。
#[cfg(target_os = "windows")]
fn create_main_hwnd(
    title: &str, w: u32, h: u32,
    resizable: bool, decorations: bool, transparent: bool, topmost: bool,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    unsafe {
        // DPI 対応: スケーリングなし (物理ピクセル = 論理ピクセル)
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance  = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = "RustraightMain\0".encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize:        std::mem::size_of::<WNDCLASSEXW>() as u32,
            style:         CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc:   Some(main_wnd_proc),
            cbClsExtra:    0, cbWndExtra: 0,
            hInstance:     hinstance,
            hIcon: 0, hCursor: LoadCursorW(0, IDC_ARROW as *const u16),
            hbrBackground: 0,
            lpszMenuName:  std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm:       0,
        };
        RegisterClassExW(&wc); // 既登録でも続行

        // WS_EX_NOREDIRECTIONBITMAP: DXGI per-pixel alpha に必須
        // 透過の場合は WS_EX_LAYERED も一時付与して DWM に alpha compositing 対象として登録させる
        // (winit の動作を再現: LAYERED で作成 → LAYERED 除去 + NOREDIRECTIONBITMAP 維持)
        let topmost_flag: u32 = if topmost { WS_EX_TOPMOST } else { 0 };
        let ex_style_final: u32 = (if transparent { 0x0020_0000 } else { 0 }) | topmost_flag; // WS_EX_NOREDIRECTIONBITMAP
        let ex_style_create: u32 = if transparent { ex_style_final | WS_EX_LAYERED } else { ex_style_final };
        let style: u32 = if decorations {
            if resizable { WS_OVERLAPPEDWINDOW } else { WS_OVERLAPPEDWINDOW & !WS_THICKFRAME }
        } else {
            WS_POPUP
        };

        // クライアント領域が w×h になるよう外枠サイズを調整
        let mut rect = windows_sys::Win32::Foundation::RECT {
            left: 0, top: 0, right: w as i32, bottom: h as i32,
        };
        AdjustWindowRectEx(&mut rect, style, 0, ex_style_final);
        let aw = rect.right  - rect.left;
        let ah = rect.bottom - rect.top;

        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = CreateWindowExW(
            ex_style_create, class_name.as_ptr(), title_w.as_ptr(),
            style, CW_USEDEFAULT, CW_USEDEFAULT, aw, ah,
            0, 0, hinstance, std::ptr::null(),
        );
        assert!(hwnd != 0, "CreateWindowExW failed for main window");
        ShowWindow(hwnd, SW_SHOWDEFAULT);

        // WS_EX_LAYERED はここでは除去しない。
        // wgpu が caps を返すとき WS_EX_LAYERED を検出して PreMultiplied をサポートに含める。
        // 実際のパッチ (LAYERED 除去 → NOREDIRECTIONBITMAP 維持) は
        // surface.get_capabilities() の直後に Window::init() 内で行う。

        hwnd
    }
}

/// HWND を所有し Drop 時に DestroyWindow を呼ぶ RAII ラッパー。
/// WindowInner の最後のフィールドに置き、wgpu Surface より後に破棄させる。
struct HwndOwner(isize);
impl Drop for HwndOwner {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0); }
        }
    }
}

// ── Shaders ───────────────────────────────────────────────────────────────────

// Blits screen_texture (straight alpha) to swap chain as pre-multiplied alpha.
// Blit for sRGB swap chain: GPU applies gamma automatically; screen_texture has
// premultiplied-linear RGB from ALPHA_BLENDING, so just pass through.
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
    return vec4(c.rgb, c.a);
}
"#;

// Blit for Bgra8Unorm swap chain (used when transparent=true): GPU does NOT apply
// gamma, so we encode to sRGB manually.  Alpha is kept as-is (premultiplied-linear
// RGB from ALPHA_BLENDING is already the correct premultiplied value).
const BLIT_SHADER_UNORM: &str = r#"
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

// Sprite shader for overlay (screen_draw overflow): discards fragments inside main window rect.
const MASKED_SPRITE_SHADER: &str = r#"
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
@group(0) @binding(0) var t_sprite: texture_2d<f32>;
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
    var c = textureSample(t_sprite, s_samp, in.uv);
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
const MASKED_COLOR_SHADER: &str = r#"
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

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |p| RawWaker::new(p, &VTABLE),
        |_| {}, |_| {}, |_| {},
    );
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) { return v; }
        std::hint::spin_loop();
    }
}

// ── Overlay (Windows-only) ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
struct OverlayInner {
    hwnd:                  isize,
    // GPU render target (Rgba8Unorm straight alpha)
    overlay_texture:       wgpu::Texture,
    overlay_view:          wgpu::TextureView,
    // 非同期ダブルバッファ readback (同期待ちなし)
    staging_bufs:          [wgpu::Buffer; 2],
    bytes_per_row:         u32,
    staging_idx:           usize,
    staging_ready:         [std::sync::Arc<std::sync::atomic::AtomicBool>; 2],
    staging_pending:       [bool; 2],  // map_async 発行済みでまだ unmap していない
    // Masked pipelines: screen_draw overflow (Bgra8Unorm, discards main-window rect)
    masked_sprite_pip:     wgpu::RenderPipeline,
    masked_sprite_pip_add: wgpu::RenderPipeline,
    masked_sprite_pip_mul: wgpu::RenderPipeline,
    masked_color_pip:      wgpu::RenderPipeline,
    // Unmasked pipelines: overlay_draw_* (Bgra8Unorm)
    unmasked_sprite_pip:     wgpu::RenderPipeline,
    unmasked_sprite_pip_add: wgpu::RenderPipeline,
    unmasked_sprite_pip_mul: wgpu::RenderPipeline,
    unmasked_color_pip:      wgpu::RenderPipeline,
    sprite_bgl:            Arc<wgpu::BindGroupLayout>,
    #[allow(dead_code)]
    rect_bgl:              wgpu::BindGroupLayout,
    main_rect_buf:         wgpu::Buffer,
    rect_bg_sprite:        wgpu::BindGroup,
    rect_bg_color:         wgpu::BindGroup,
    sprite_vbuf:           wgpu::Buffer,
    #[allow(dead_code)]
    draw_queue:            Vec<DrawCommand>,
    #[allow(dead_code)]
    blend:                 BlendMode,
    bg_cache:              HashMap<(u32, Option<u32>), Arc<wgpu::BindGroup>>,
    display_w:             u32,
    display_h:             u32,
    visible:               bool,
    // 案2: 前フレームのオーバーレイ描画ハッシュ (同一なら GPU レンダリングをスキップ)
    prev_overlay_hash:     u64,
    // 案5: GDI スレッドへのチャネル (UpdateLayeredWindow をバックグラウンド実行)
    gdi_tx:      std::sync::mpsc::SyncSender<Option<Vec<u8>>>,
    gdi_rx:      std::sync::mpsc::Receiver<Vec<u8>>,
    gdi_thread:  Option<std::thread::JoinHandle<()>>,
    reuse_buf:   Option<Vec<u8>>,
}

#[cfg(target_os = "windows")]
impl Drop for OverlayInner {
    fn drop(&mut self) {
        // GDI スレッドに終了を通知してから join → スレッドが hwnd を使い終えるまで待つ
        let _ = self.gdi_tx.send(None);
        if let Some(h) = self.gdi_thread.take() { h.join().ok(); }
        // スレッド終了後に安全に DestroyWindow
        use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;
        unsafe { DestroyWindow(self.hwnd); }
    }
}

/// Win32 ウィンドウプロシージャ — WM_NCHITTEST を HTTRANSPARENT で返すことでクリックスルーを実現。
#[cfg(target_os = "windows")]
unsafe extern "system" fn overlay_wnd_proc(
    hwnd:   windows_sys::Win32::Foundation::HWND,
    msg:    u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    match msg {
        WM_NCHITTEST => HTTRANSPARENT as isize,
        WM_DESTROY   => 0,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Win32 オーバーレイウィンドウを直接生成し、DWM 透過とクリックスルーを設定する。
/// winit を経由しないため DWM 設定への干渉が一切ない。
#[cfg(target_os = "windows")]
fn create_overlay_hwnd(dw: u32, dh: u32, visible: bool) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = "RustraightOverlay\0".encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize:        std::mem::size_of::<WNDCLASSEXW>() as u32,
            style:         0,
            lpfnWndProc:   Some(overlay_wnd_proc),
            cbClsExtra:    0, cbWndExtra: 0,
            hInstance:     hinstance,
            hIcon: 0, hCursor: 0,
            hbrBackground: 0,  // ブラシなし → 起動時の白・黒フラッシュを防ぐ
            lpszMenuName:  std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm:       0,
        };
        RegisterClassExW(&wc); // 既登録でも無視

        // WS_EX_LAYERED:     UpdateLayeredWindow による per-pixel alpha に必須
        // WS_EX_TRANSPARENT: クリックスルー (WM_NCHITTEST → HTTRANSPARENT でも保証)
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class_name.as_ptr(), std::ptr::null(),
            WS_POPUP,
            0, 0, dw as i32, dh as i32,
            0, 0, hinstance, std::ptr::null(),
        );
        assert!(hwnd != 0, "CreateWindowExW failed for overlay");

        if visible {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        hwnd
    }
}

/// 案2: オーバーレイ描画コマンドの FNV-1a ハッシュ。同一なら GPU レンダリングをスキップ。
#[cfg(target_os = "windows")]
fn compute_overlay_hash(draw_queue: &[DrawCommand], overlay_queue: &[DrawCommand], sw: i32, sh: i32) -> u64 {
    const P: u64 = 1099511628211;
    let mut h: u64 = 14695981039346656037;
    let mut feed = |v: u64| { h ^= v; h = h.wrapping_mul(P); };
    for cmd in draw_queue {
        match cmd {
            DrawCommand::Sprite { x, y, handle, .. } if *x < 0 || *y < 0 || *x >= sw || *y >= sh
                => { feed(*x as u64); feed(*y as u64); feed(*handle as u64); }
            DrawCommand::Text { x, y, rgba, .. } if *x < 0 || *y < 0 || *x >= sw || *y >= sh
                => { feed(*x as u64); feed(*y as u64); for c in rgba.chunks(64) { feed(c[0] as u64); } }
            _ => {}
        }
    }
    for cmd in overlay_queue {
        match cmd {
            DrawCommand::Sprite { x, y, handle, .. } => { feed(*x as u64); feed(*y as u64); feed(*handle as u64); }
            DrawCommand::Text   { x, y, rgba, .. }   => { feed(*x as u64); feed(*y as u64); for c in rgba.chunks(64) { feed(c[0] as u64); } }
            DrawCommand::Polys  { verts }             => { feed(verts.len() as u64); }
        }
    }
    h
}

/// 案1: overlay_texture が Bgra8Unorm のため staging データは既に BGRA 順。
/// 単純な行コピーのみ (BGRA スワップ不要) → debug ビルドでも高速。
#[cfg(target_os = "windows")]
#[allow(dead_code)]
unsafe fn update_layered_window(
    hwnd: isize, gdi_dc_mem: isize, gdi_pv_bits: usize,
    staged_bgra: &[u8], display_w: u32, display_h: u32, bytes_per_row: usize,
) {
    let row = display_w as usize * 4;
    let dst = unsafe { std::slice::from_raw_parts_mut(gdi_pv_bits as *mut u8, display_w as usize * display_h as usize * 4) };
    for y in 0..display_h as usize {
        dst[y*row .. (y+1)*row].copy_from_slice(&staged_bgra[y*bytes_per_row .. y*bytes_per_row + row]);
    }
    unsafe { gdi_present(hwnd, gdi_dc_mem, display_w, display_h); }
}

/// UpdateLayeredWindow の GDI 呼び出し部分を分離 (GDI スレッドから直接呼ぶ用)。
#[cfg(target_os = "windows")]
unsafe fn gdi_present(hwnd: isize, gdi_dc_mem: isize, display_w: u32, display_h: u32) {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC, AC_SRC_ALPHA, BLENDFUNCTION};
    use windows_sys::Win32::UI::WindowsAndMessaging::UpdateLayeredWindow;
    use windows_sys::Win32::Foundation::{POINT, SIZE};
    let blend  = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
    let pt_src = POINT { x: 0, y: 0 };
    let pt_dst = POINT { x: 0, y: 0 };
    let sz     = SIZE  { cx: display_w as i32, cy: display_h as i32 };
    unsafe {
        let hdc_screen = GetDC(0);
        UpdateLayeredWindow(hwnd, hdc_screen, &pt_dst, &sz, gdi_dc_mem, &pt_src, 0, &blend, 2);
        ReleaseDC(0, hdc_screen);
    }
}

#[cfg(target_os = "windows")]
fn build_overlay(
    device:     &wgpu::Device,
    sprite_bgl: &Arc<wgpu::BindGroupLayout>,
    visible:    bool,
) -> OverlayInner {
    let (dw, dh) = unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN};
        (GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32, GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32)
    };

    let hwnd = create_overlay_hwnd(dw, dh, visible);

    // 案1: Bgra8Unorm → GPU が R↔B スワップを担当、staging は直接 GDI に渡せる BGRA データになる
    let overlay_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("overlay_tex"), size: wgpu::Extent3d { width: dw, height: dh, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let overlay_view = overlay_texture.create_view(&Default::default());

    // 非同期ダブルバッファ staging (poll(Poll) + AtomicBool で同期待ちなし)
    let bytes_per_row = (dw * 4 + wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1)
        & !(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT - 1);
    let mk_staging = || device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ov_staging"), size: (bytes_per_row * dh) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
    });
    let staging_bufs  = [mk_staging(), mk_staging()];
    let staging_ready = [
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    ];

    // 案5: GDI リソースをスレッドに移管して UpdateLayeredWindow をバックグラウンド実行
    // GDI DIB セクション作成
    let (gdi_dc_mem, gdi_bitmap, gdi_pv_bits) = unsafe {
        use windows_sys::Win32::Graphics::Gdi::*;
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: dw as i32, biHeight: -(dh as i32),
                biPlanes: 1, biBitCount: 32, biCompression: BI_RGB,
                biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0,
                biClrUsed: 0, biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };
        let hdc_screen = GetDC(0);
        let dc_mem     = CreateCompatibleDC(hdc_screen);
        let mut pv: *mut ::core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(dc_mem, &bmi, DIB_RGB_COLORS, &mut pv, 0, 0);
        SelectObject(dc_mem, bmp);
        ReleaseDC(0, hdc_screen);
        // 初期化: 全透明
        std::ptr::write_bytes(pv as *mut u8, 0u8, (dw * dh * 4) as usize);
        gdi_present(hwnd, dc_mem, dw, dh);
        (dc_mem, bmp, pv as usize)
    };

    // GDI スレッド起動: Vec<u8>(BGRA) を受け取り GDI DIB にコピー → UpdateLayeredWindow
    let (gdi_tx, gdi_thread_rx) = std::sync::mpsc::sync_channel::<Option<Vec<u8>>>(1);
    let (gdi_recycle_tx, gdi_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
    // 再利用バッファを1つ事前確保
    let _ = gdi_recycle_tx.send(vec![0u8; (dw * dh * 4) as usize]);
    let thread_hwnd = hwnd;
    let gdi_thread = std::thread::spawn(move || {
        // GDI リソースをスレッドが所有し、終了時に Drop でクリーンアップ
        struct GdiOwned { dc_mem: isize, bitmap: isize }
        unsafe impl Send for GdiOwned {}
        impl Drop for GdiOwned {
            fn drop(&mut self) {
                use windows_sys::Win32::Graphics::Gdi::*;
                unsafe { SelectObject(self.dc_mem, 0); DeleteObject(self.bitmap); DeleteDC(self.dc_mem); }
            }
        }
        let _gdi = GdiOwned { dc_mem: gdi_dc_mem, bitmap: gdi_bitmap };
        while let Ok(Some(buf)) = gdi_thread_rx.recv() {
            unsafe {
                let row = dw as usize * 4;
                let dst = std::slice::from_raw_parts_mut(gdi_pv_bits as *mut u8, dw as usize * dh as usize * 4);
                for y in 0..dh as usize { dst[y*row..(y+1)*row].copy_from_slice(&buf[y*row..(y+1)*row]); }
                gdi_present(thread_hwnd, gdi_dc_mem, dw, dh);
            }
            let _ = gdi_recycle_tx.send(buf);
        }
    });

    // ── main_rect BGL + buffer ────────────────────────────────────────────────
    let rect_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rect_bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });
    let main_rect_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("main_rect"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
    });
    let rect_bg_sprite = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &rect_bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: main_rect_buf.as_entire_binding() }],
    });
    let rect_bg_color = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None, layout: &rect_bgl,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: main_rect_buf.as_entire_binding() }],
    });

    // ── Masked sprite pipelines ───────────────────────────────────────────────
    let masked_sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("msk_spr"), source: wgpu::ShaderSource::Wgsl(MASKED_SPRITE_SHADER.into()) });
    let masked_sprite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None, bind_group_layouts: &[sprite_bgl, &rect_bgl], push_constant_ranges: &[],
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
    let sprite_vbl = wgpu::VertexBufferLayout { array_stride: 48, step_mode: wgpu::VertexStepMode::Vertex, attributes: &sprite_attrs };
    let mk_masked_spr = |blend: wgpu::BlendState| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None, layout: Some(&masked_sprite_layout),
        vertex:   wgpu::VertexState { module: &masked_sprite_shader, entry_point: Some("vs"), buffers: &[sprite_vbl.clone()], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &masked_sprite_shader, entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Bgra8Unorm, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default() }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: None, multisample: wgpu::MultisampleState::default(), multiview: None, cache: None,
    });
    let masked_sprite_pip     = mk_masked_spr(wgpu::BlendState::ALPHA_BLENDING);
    let masked_sprite_pip_add = mk_masked_spr(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add } });
    let masked_sprite_pip_mul = mk_masked_spr(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add } });

    // ── Masked color pipeline ─────────────────────────────────────────────────
    let masked_color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("msk_col"), source: wgpu::ShaderSource::Wgsl(MASKED_COLOR_SHADER.into()) });
    let masked_color_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None, bind_group_layouts: &[&rect_bgl], push_constant_ranges: &[],
    });
    let color_vbl = wgpu::VertexBufferLayout {
        array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute { shader_location: 0, offset:  0, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { shader_location: 1, offset:  8, format: wgpu::VertexFormat::Float32x4 },
        ],
    };
    let masked_color_pip = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None, layout: Some(&masked_color_layout),
        vertex:   wgpu::VertexState { module: &masked_color_shader, entry_point: Some("vs"), buffers: &[color_vbl], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &masked_color_shader, entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Bgra8Unorm, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default() }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: None, multisample: wgpu::MultisampleState::default(), multiview: None, cache: None,
    });

    // ── Unmasked sprite/color pipelines (Bgra8Unorm, overlay_draw_* 用) ──────────
    let unmasked_sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ov_spr"), source: wgpu::ShaderSource::Wgsl(SPRITE_SHADER.into()),
    });
    let unmasked_sprite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None, bind_group_layouts: &[sprite_bgl], push_constant_ranges: &[],
    });
    let sprite_vbl2 = wgpu::VertexBufferLayout { array_stride: 48, step_mode: wgpu::VertexStepMode::Vertex, attributes: &sprite_attrs };
    let mk_unmasked_spr = |blend: wgpu::BlendState| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None, layout: Some(&unmasked_sprite_layout),
        vertex: wgpu::VertexState { module: &unmasked_sprite_shader, entry_point: Some("vs"), buffers: &[sprite_vbl2.clone()], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &unmasked_sprite_shader, entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Bgra8Unorm, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default() }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: None, multisample: wgpu::MultisampleState::default(), multiview: None, cache: None,
    });
    let unmasked_sprite_pip     = mk_unmasked_spr(wgpu::BlendState::ALPHA_BLENDING);
    let unmasked_sprite_pip_add = mk_unmasked_spr(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add } });
    let unmasked_sprite_pip_mul = mk_unmasked_spr(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add } });
    let unmasked_color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ov_col"), source: wgpu::ShaderSource::Wgsl(COLOR_SHADER.into()),
    });
    let unmasked_color_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None, bind_group_layouts: &[], push_constant_ranges: &[],
    });
    let color_vbl2 = wgpu::VertexBufferLayout { array_stride: 24, step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute { shader_location: 0, offset:  0, format: wgpu::VertexFormat::Float32x2 },
            wgpu::VertexAttribute { shader_location: 1, offset:  8, format: wgpu::VertexFormat::Float32x4 },
        ],
    };
    let unmasked_color_pip = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None, layout: Some(&unmasked_color_layout),
        vertex: wgpu::VertexState { module: &unmasked_color_shader, entry_point: Some("vs"), buffers: &[color_vbl2], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &unmasked_color_shader, entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Bgra8Unorm, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            compilation_options: Default::default() }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: None, multisample: wgpu::MultisampleState::default(), multiview: None, cache: None,
    });

    let sprite_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ov_sprite_vbuf"), size: 1024 * 6 * 48,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
    });

    OverlayInner {
        hwnd,
        overlay_texture, overlay_view,
        staging_bufs, bytes_per_row, staging_idx: 0, staging_ready, staging_pending: [false; 2],
        prev_overlay_hash: u64::MAX,
        gdi_tx, gdi_rx, gdi_thread: Some(gdi_thread), reuse_buf: None,
        masked_sprite_pip, masked_sprite_pip_add, masked_sprite_pip_mul, masked_color_pip,
        unmasked_sprite_pip, unmasked_sprite_pip_add, unmasked_sprite_pip_mul, unmasked_color_pip,
        sprite_bgl: Arc::clone(sprite_bgl),
        rect_bgl, main_rect_buf, rect_bg_sprite, rect_bg_color,
        sprite_vbuf,
        draw_queue: Vec::new(),
        blend: BlendMode::Normal,
        bg_cache: HashMap::new(),
        display_w: dw, display_h: dh,
        visible,
    }
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

// ── Runtime state ─────────────────────────────────────────────────────────────

struct WindowInner {
    device:              wgpu::Device,
    queue:               wgpu::Queue,
    surface:             wgpu::Surface<'static>,
    surface_config:      wgpu::SurfaceConfiguration,
    screen_width:        u32,
    screen_height:       u32,
    // Screen render target (RENDER_ATTACHMENT + TEXTURE_BINDING)
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    pending_resize:      Option<(u32, u32)>,
    // Overlay
    #[cfg(target_os = "windows")]
    overlay:             Option<Box<OverlayInner>>,
    overlay_draw_queue:  Vec<DrawCommand>,
    overlay_blend:       BlendMode,
    // LAST フィールド: Surface より後に drop されるよう末尾に置く
    hwnd:                HwndOwner,
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
    topmost:           bool,
    default_font_path: Option<String>,
    default_font_size: u32,
    overlay_enabled:   bool,
    overlay_visible:   bool,
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
            topmost:           false,
            default_font_path: None,
            default_font_size: 16,
            overlay_enabled:   false,
            overlay_visible:   true,
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
    pub fn topmost(&mut self, v: bool)           { self.topmost = v; }

    pub fn init(&mut self) {
        // Win32 でメインウィンドウを直接生成
        #[cfg(target_os = "windows")]
        let hwnd = create_main_hwnd(
            &self.title,
            self.win_width  as u32,
            self.win_height as u32,
            self.resizable,
            self.decorations,
            self.transparent,
            self.topmost,
        );
        #[cfg(not(target_os = "windows"))]
        let hwnd = 0isize; // stub

        // wgpu サーフェスを raw HWND から作成
        let hinstance = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null())
        };

        // DX12 は PreMultiplied alpha (透過ウィンドウ) に必須
        #[cfg(target_os = "windows")]
        let backends = wgpu::Backends::DX12;
        #[cfg(not(target_os = "windows"))]
        let backends = wgpu::Backends::all();

        // 透過ウィンドウ: wgpu 27 では DxgiFromVisual (DirectComposition) 経由でのみ
        // PreMultiplied alpha mode が使える。Dx12SwapchainKind は wgpu 27 から公開されて
        // いないため、from_env_or_default() が読む環境変数で指定する。
        #[cfg(target_os = "windows")]
        if self.transparent {
            // SAFETY: wgpu インスタンス生成前・シングルスレッドのみ
            unsafe { std::env::set_var("WGPU_DX12_PRESENTATION_SYSTEM", "visual"); }
        }

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions::from_env_or_default(),
                ..Default::default()
            },
            ..Default::default()
        });

        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
                raw_window_handle:  RawWindowHandle::Win32({
                    let mut h = Win32WindowHandle::new(
                        NonZeroIsize::new(hwnd as isize).expect("valid hwnd")
                    );
                    h.hinstance = NonZeroIsize::new(hinstance as isize);
                    h
                }),
            })
        }.expect("failed to create surface");

        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference:   wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
        })).expect("no suitable GPU adapter");

        let (device, queue) = block_on(adapter.request_device(
            &wgpu::DeviceDescriptor::default(),
        )).expect("failed to create wgpu device");

        let caps = surface.get_capabilities(&adapter);

        // 透過ウィンドウ: ここで初めて WS_EX_LAYERED を除去し WS_EX_NOREDIRECTIONBITMAP に切り替える。
        // wgpu は WS_EX_LAYERED を検出して PreMultiplied を caps に含める → caps 取得後にパッチ。
        // surface.configure() 時に DXGI は WS_EX_NOREDIRECTIONBITMAP を見て
        // DXGI_ALPHA_MODE_PREMULTIPLIED のスワップチェーンを作成する。
        #[cfg(target_os = "windows")]
        if self.transparent {
            unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                let cur_ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
                SetWindowLongW(hwnd, GWL_EXSTYLE, cur_ex & !(WS_EX_LAYERED as i32));
                if self.decorations {
                    use windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
                    use windows_sys::Win32::UI::Controls::MARGINS;
                    let m = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
                    DwmExtendFrameIntoClientArea(hwnd, &m);
                }
            }
        }

        // For transparent windows, DX12 PreMultiplied alpha mode requires Bgra8Unorm
        // (the sRGB variant does not reliably support DXGI_ALPHA_MODE_PREMULTIPLIED).
        let fmt = if self.transparent {
            caps.formats.iter()
                .find(|&&f| f == wgpu::TextureFormat::Bgra8Unorm)
                .copied()
                .unwrap_or(caps.formats[0])
        } else {
            caps.formats[0]
        };
        let present = if self.vsync_enabled { wgpu::PresentMode::Fifo } else { wgpu::PresentMode::Immediate };
        let alpha = if self.transparent {
            let selected = caps.alpha_modes.iter().find(|&&m| m == wgpu::CompositeAlphaMode::PreMultiplied)
                .or_else(|| caps.alpha_modes.iter().find(|&&m| m == wgpu::CompositeAlphaMode::PostMultiplied))
                .copied().unwrap_or(caps.alpha_modes[0]);
            crate::log_info!("透過: フォーマット一覧 = {:?}", caps.formats);
            crate::log_info!("透過: アルファモード一覧 = {:?}、選択 = {:?}", caps.alpha_modes, selected);
            if selected == wgpu::CompositeAlphaMode::Opaque {
                crate::log_warn!("PreMultiplied/PostMultiplied が使用できません、透過は機能しません");
            }
            selected
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
        let blit_shader_src = if self.transparent { BLIT_SHADER_UNORM } else { BLIT_SHADER };
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"), source: wgpu::ShaderSource::Wgsl(blit_shader_src.into()),
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

        // ── Overlay (Windows-only) ────────────────────────────────────────────
        #[cfg(target_os = "windows")]
        let overlay = if self.overlay_enabled {
            Some(Box::new(build_overlay(&device, &sprite_bgl, self.overlay_visible)))
        } else {
            None
        };

        self.inner = Some(Box::new(WindowInner {
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
            mask:            None,
            blend:           BlendMode::Normal,
            transparent:     self.transparent,
            gamepad:         GamepadManager::try_new(hwnd),
            default_font:    None,
            pending_resize:  None,
            #[cfg(target_os = "windows")]
            overlay,
            overlay_draw_queue: Vec::new(),
            overlay_blend:      BlendMode::Normal,
            hwnd: HwndOwner(hwnd),
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

        // ② Handle pending resize (前フレーム ⑩ で積まれたもの)
        if let Some((w, h)) = inner.pending_resize.take() {
            if w > 0 && h > 0 {
                inner.surface_config.width  = w;
                inner.surface_config.height = h;
                inner.surface.configure(&inner.device, &inner.surface_config);
            }
        }

        // ③ Upload all sprite textures referenced this frame to GPU cache
        {
            let mut handles: Vec<(u32, Option<u32>)> = inner.draw_queue.iter()
                .filter_map(|cmd| if let DrawCommand::Sprite { handle, mask_handle, .. } = cmd { Some((*handle, *mask_handle)) } else { None })
                .collect();
            // Also ensure sprites used in overlay_draw_* queue
            #[cfg(target_os = "windows")]
            handles.extend(inner.overlay_draw_queue.iter()
                .filter_map(|cmd| if let DrawCommand::Sprite { handle, mask_handle, .. } = cmd { Some((*handle, *mask_handle)) } else { None }));
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
                return !WIN32_EVENTS.with(|e| e.borrow().should_close);
            }
            Err(e) => { crate::log_error!("サーフェスエラー: {e}"); return false; }
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
                    depth_slice:    None,
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
                    depth_slice:    None,
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

        // ⑨ Overlay render (Windows-only)
        #[cfg(target_os = "windows")]
        if let Some(ref mut ov) = inner.overlay {
            // Only render overlay if there are overlay_draw_* commands
            // OR if any screen_draw command has out-of-bounds coordinates.
            let sw = inner.screen_width as i32;
            let sh = inner.screen_height as i32;
            let has_screen_overflow = inner.draw_queue.iter().any(|cmd| match cmd {
                DrawCommand::Sprite { x, y, .. } => *x < 0 || *y < 0 || *x >= sw || *y >= sh,
                DrawCommand::Text   { x, y, .. } => *x < 0 || *y < 0 || *x >= sw || *y >= sh,
                DrawCommand::Polys  { .. }       => false,
            });
            let has_overlay_cmds = !inner.overlay_draw_queue.is_empty();
            if !ov.visible || (!has_screen_overflow && !has_overlay_cmds) {
                inner.overlay_draw_queue.clear();
            } else { 'overlay: {

            // 案2: ハッシュが前フレームと同一なら GPU レンダリングをスキップ（窓はそのまま正しい表示）
            let cur_hash = compute_overlay_hash(&inner.draw_queue, &inner.overlay_draw_queue, sw, sh);
            if cur_hash == ov.prev_overlay_hash { break 'overlay; }
            ov.prev_overlay_hash = cur_hash;

            // Update main_rect uniform (display pixel position + size of main window)
            // GetWindowInfo でクライアント領域のスクリーン座標を取得
            let main_pos = {
                use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowInfo, WINDOWINFO};
                let mut info: WINDOWINFO = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<WINDOWINFO>() as u32;
                unsafe { GetWindowInfo(inner.hwnd.0, &mut info); }
                windows_sys::Win32::Foundation::POINT {
                    x: info.rcClient.left,
                    y: info.rcClient.top,
                }
            };
            let main_rect: [f32; 4] = [
                main_pos.x as f32, main_pos.y as f32,
                inner.surface_config.width as f32, inner.surface_config.height as f32,
            ];
            inner.queue.write_buffer(&ov.main_rect_buf, 0, slice_as_bytes(&main_rect));

            // Upload screen_draw sprites to overlay bg_cache (shared key space)
            {
                let handles: Vec<(u32, Option<u32>)> = inner.draw_queue.iter()
                    .filter_map(|cmd| if let DrawCommand::Sprite { handle, mask_handle, .. } = cmd { Some((*handle, *mask_handle)) } else { None })
                    .collect();
                for (h, mh) in &handles {
                    let key = (*h, *mh);
                    if !ov.bg_cache.contains_key(&key) {
                        if let Some(sprite_gd) = inner.sprite_cache.get(h) {
                            let mask_view = mh.and_then(|m| inner.sprite_cache.get(&m)).map(|md| &md.view).unwrap_or(&inner.dummy_view);
                            let bg = Arc::new(inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: None, layout: &ov.sprite_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sprite_gd.view) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(mask_view) },
                                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&inner.sampler) },
                                ],
                            }));
                            ov.bg_cache.insert(key, bg);
                        }
                    }
                }
                // overlay_draw_* sprites (inner.overlay_draw_queue is the correct source)
                let ov_handles: Vec<(u32, Option<u32>)> = inner.overlay_draw_queue.iter()
                    .filter_map(|cmd| if let DrawCommand::Sprite { handle, mask_handle, .. } = cmd { Some((*handle, *mask_handle)) } else { None })
                    .collect();
                for (h, mh) in &ov_handles {
                    let key = (*h, *mh);
                    if !ov.bg_cache.contains_key(&key) {
                        if let Some(sprite_gd) = inner.sprite_cache.get(h) {
                            let mask_view = mh.and_then(|m| inner.sprite_cache.get(&m)).map(|md| &md.view).unwrap_or(&inner.dummy_view);
                            let bg = Arc::new(inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                label: None, layout: &ov.sprite_bgl,
                                entries: &[
                                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sprite_gd.view) },
                                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(mask_view) },
                                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&inner.sampler) },
                                ],
                            }));
                            ov.bg_cache.insert(key, bg);
                        }
                    }
                }
            }

            // Build overlay vertex data
            // screen_draw_* (masked) + overlay_draw_* (unmasked) in one vertex buffer
            let (dw, dh) = (ov.display_w, ov.display_h);
            // Scale factors: virtual-screen pixels → window display pixels
            let scale_x = inner.surface_config.width  as f32 / inner.screen_width  as f32;
            let scale_y = inner.surface_config.height as f32 / inner.screen_height as f32;
            let mut ov_sprite_verts: Vec<SpriteVertex> = Vec::new();
            let mut ov_color_verts:  Vec<crate::draw::ColorVert> = Vec::new();
            enum OvItem { MaskedSprite  { base: u32, handle: u32, mask_handle: Option<u32>, blend: BlendMode },
                          MaskedColor   { base: u32, count: u32 },
                          MaskedText    { base: u32, text_idx: usize },
                          UnmaskedSprite{ base: u32, handle: u32, mask_handle: Option<u32>, blend: BlendMode },
                          UnmaskedColor { base: u32, count: u32 },
                          UnmaskedText  { base: u32, text_idx: usize } }
            let mut ov_items: Vec<OvItem> = Vec::new();
            // Text items for overlay (both masked screen_draw overflow and unmasked overlay_draw)
            let mut ov_text_items_all: Vec<(u32, u32, Vec<u8>)> = Vec::new();

            // screen_draw commands → masked
            for cmd in &inner.draw_queue {
                match cmd {
                    DrawCommand::Sprite { x, y, handle, mask_handle, mask_ox, mask_oy, params, blend } => {
                        if let Some(gd) = inner.sprite_cache.get(handle) {
                            let base = ov_sprite_verts.len() as u32;
                            let (mox, moy, mw, mh, mon) = if let Some(mhv) = mask_handle {
                                if let Some(md) = inner.sprite_cache.get(mhv) { (*mask_ox as f32, *mask_oy as f32, md.width as f32, md.height as f32, 1.0f32) } else { (0.,0.,1.,1.,0.) }
                            } else { (0.,0.,1.,1.,0.) };
                            ov_sprite_verts.extend_from_slice(&build_sprite_quad_overlay(*x, *y, gd.width, gd.height, dw, dh, main_pos.x, main_pos.y, scale_x, scale_y, mox, moy, mw, mh, mon, params));
                            ov_items.push(OvItem::MaskedSprite { base, handle: *handle, mask_handle: *mask_handle, blend: *blend });
                        }
                    }
                    DrawCommand::Polys { verts } => {
                        let base = ov_color_verts.len() as u32;
                        let count = verts.len() as u32;
                        // Re-map vertices to display coords
                        let remapped: Vec<crate::draw::ColorVert> = verts.iter().map(|v| {
                            let sx = (v.pos[0] * 0.5 + 0.5) * inner.surface_config.width  as f32 + main_pos.x as f32;
                            let sy = (1.0 - (v.pos[1] * 0.5 + 0.5)) * inner.surface_config.height as f32 + main_pos.y as f32;
                            let nx = sx / dw as f32 * 2.0 - 1.0;
                            let ny = 1.0 - sy / dh as f32 * 2.0;
                            crate::draw::ColorVert { pos: [nx, ny], color: v.color }
                        }).collect();
                        ov_color_verts.extend_from_slice(&remapped);
                        ov_items.push(OvItem::MaskedColor { base, count });
                    }
                    DrawCommand::Text { x, y, width, height, rgba } => {
                        if *x < 0 || *y < 0 || *x >= sw || *y >= sh {
                            let base = ov_sprite_verts.len() as u32;
                            let text_idx = ov_text_items_all.len();
                            ov_sprite_verts.extend_from_slice(&build_sprite_quad_overlay(*x, *y, *width, *height, dw, dh, main_pos.x, main_pos.y, scale_x, scale_y, 0., 0., 1., 1., 0., &DrawSpriteParams::default()));
                            ov_text_items_all.push((*width, *height, rgba.clone()));
                            ov_items.push(OvItem::MaskedText { base, text_idx });
                        }
                    }
                }
            }
            // overlay_draw_* commands → unmasked (use inner.overlay_draw_queue, NOT ov.draw_queue)
            let ov_draw_queue_snapshot: Vec<DrawCommand> = inner.overlay_draw_queue.clone();
            for cmd in &ov_draw_queue_snapshot {
                match cmd {
                    DrawCommand::Sprite { x, y, handle, mask_handle, mask_ox, mask_oy, params, blend } => {
                        if let Some(gd) = inner.sprite_cache.get(handle) {
                            let base = ov_sprite_verts.len() as u32;
                            let (mox, moy, mw, mh, mon) = if let Some(mhv) = mask_handle {
                                if let Some(md) = inner.sprite_cache.get(mhv) { (*mask_ox as f32, *mask_oy as f32, md.width as f32, md.height as f32, 1.0f32) } else { (0.,0.,1.,1.,0.) }
                            } else { (0.,0.,1.,1.,0.) };
                            ov_sprite_verts.extend_from_slice(&build_sprite_quad_overlay(*x, *y, gd.width, gd.height, dw, dh, main_pos.x, main_pos.y, scale_x, scale_y, mox, moy, mw, mh, mon, params));
                            ov_items.push(OvItem::UnmaskedSprite { base, handle: *handle, mask_handle: *mask_handle, blend: *blend });
                        }
                    }
                    DrawCommand::Polys { verts } => {
                        let base = ov_color_verts.len() as u32;
                        let count = verts.len() as u32;
                        let remapped: Vec<crate::draw::ColorVert> = verts.iter().map(|v| {
                            let sx = (v.pos[0] * 0.5 + 0.5) * inner.surface_config.width  as f32 + main_pos.x as f32;
                            let sy = (1.0 - (v.pos[1] * 0.5 + 0.5)) * inner.surface_config.height as f32 + main_pos.y as f32;
                            let nx = sx / dw as f32 * 2.0 - 1.0;
                            let ny = 1.0 - sy / dh as f32 * 2.0;
                            crate::draw::ColorVert { pos: [nx, ny], color: v.color }
                        }).collect();
                        ov_color_verts.extend_from_slice(&remapped);
                        ov_items.push(OvItem::UnmaskedColor { base, count });
                    }
                    DrawCommand::Text { x, y, width, height, rgba } => {
                        let base = ov_sprite_verts.len() as u32;
                        let text_idx = ov_text_items_all.len();
                        ov_sprite_verts.extend_from_slice(&build_sprite_quad_overlay(*x, *y, *width, *height, dw, dh, main_pos.x, main_pos.y, scale_x, scale_y, 0., 0., 1., 1., 0., &DrawSpriteParams::default()));
                        ov_text_items_all.push((*width, *height, rgba.clone()));
                        ov_items.push(OvItem::UnmaskedText { base, text_idx });
                    }
                }
            }

            // Upload vertex data
            if !ov_sprite_verts.is_empty() {
                inner.queue.write_buffer(&ov.sprite_vbuf, 0, slice_as_bytes(&ov_sprite_verts));
            }
            let ov_color_buf: Option<wgpu::Buffer> = if !ov_color_verts.is_empty() {
                use wgpu::util::DeviceExt;
                Some(inner.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ov_color_vbuf"), contents: slice_as_bytes(&ov_color_verts), usage: wgpu::BufferUsages::VERTEX,
                }))
            } else { None };

            // Build GPU textures for all overlay text items (masked screen_draw + unmasked overlay_draw)
            let mut ov_text_temp: Vec<(wgpu::Texture, wgpu::TextureView)> = Vec::new();
            for (w, h, rgba) in &ov_text_items_all {
                let tex = inner.device.create_texture(&wgpu::TextureDescriptor {
                    label: None, size: wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                inner.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(*h) },
                    wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
                );
                let view = tex.create_view(&Default::default());
                ov_text_temp.push((tex, view));
            }
            let mut ov_text_bgs: Vec<wgpu::BindGroup> = Vec::new();
            for (_, view) in &ov_text_temp {
                ov_text_bgs.push(inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None, layout: &ov.sprite_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&inner.dummy_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&inner.sampler) },
                    ],
                }));
            }

            // Overlay render pass → overlay_texture
            let mut ov_enc = inner.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut rpass = ov_enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &ov.overlay_view, resolve_target: None, depth_slice: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r:0.,g:0.,b:0.,a:0. }), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None,
                });
                let _unmasked_text_idx = 0usize;
                for item in &ov_items {
                    match item {
                        OvItem::MaskedSprite { base, handle, mask_handle, blend } => {
                            let key = (*handle, *mask_handle);
                            if let Some(bg) = ov.bg_cache.get(&key) {
                                let pip = match blend { BlendMode::Normal => &ov.masked_sprite_pip, BlendMode::Add => &ov.masked_sprite_pip_add, BlendMode::Mul => &ov.masked_sprite_pip_mul };
                                rpass.set_pipeline(pip);
                                rpass.set_bind_group(0, &**bg, &[]);
                                rpass.set_bind_group(1, &ov.rect_bg_sprite, &[]);
                                rpass.set_vertex_buffer(0, ov.sprite_vbuf.slice(..));
                                rpass.draw(*base..*base + 6, 0..1);
                            }
                        }
                        OvItem::MaskedColor { base, count } => {
                            if let Some(buf) = &ov_color_buf {
                                rpass.set_pipeline(&ov.masked_color_pip);
                                rpass.set_bind_group(0, &ov.rect_bg_color, &[]);
                                rpass.set_vertex_buffer(0, buf.slice(..));
                                rpass.draw(*base..*base + count, 0..1);
                            }
                        }
                        OvItem::MaskedText { base, text_idx } => {
                            if let Some(bg) = ov_text_bgs.get(*text_idx) {
                                rpass.set_pipeline(&ov.masked_sprite_pip);
                                rpass.set_bind_group(0, bg, &[]);
                                rpass.set_bind_group(1, &ov.rect_bg_sprite, &[]);
                                rpass.set_vertex_buffer(0, ov.sprite_vbuf.slice(..));
                                rpass.draw(*base..*base + 6, 0..1);
                            }
                        }
                        OvItem::UnmaskedSprite { base, handle, mask_handle, blend } => {
                            if let Some(bg) = ov.bg_cache.get(&(*handle, *mask_handle)) {
                                let pip = match blend {
                                    BlendMode::Normal => &ov.unmasked_sprite_pip,
                                    BlendMode::Add    => &ov.unmasked_sprite_pip_add,
                                    BlendMode::Mul    => &ov.unmasked_sprite_pip_mul,
                                };
                                rpass.set_pipeline(pip);
                                rpass.set_bind_group(0, bg.as_ref(), &[]);
                                rpass.set_vertex_buffer(0, ov.sprite_vbuf.slice(..));
                                rpass.draw(*base..*base + 6, 0..1);
                            }
                        }
                        OvItem::UnmaskedColor { base, count } => {
                            if let Some(buf) = &ov_color_buf {
                                rpass.set_pipeline(&ov.unmasked_color_pip);
                                rpass.set_vertex_buffer(0, buf.slice(..));
                                rpass.draw(*base..*base + count, 0..1);
                            }
                        }
                        OvItem::UnmaskedText { base, text_idx } => {
                            if let Some(bg) = ov_text_bgs.get(*text_idx) {
                                rpass.set_pipeline(&ov.unmasked_sprite_pip);
                                rpass.set_bind_group(0, bg, &[]);
                                rpass.set_vertex_buffer(0, ov.sprite_vbuf.slice(..));
                                rpass.draw(*base..*base + 6, 0..1);
                            }
                        }
                    }
                }
            }
            let cur  = ov.staging_idx;
            let prev = 1 - cur;
            use std::sync::atomic::Ordering;

            // staging[cur] が前々フレームの map_async から未解放なら解放する
            // (高FPSでは通常発生しないが、念のため安全処理)
            if ov.staging_pending[cur] {
                let _ = inner.device.poll(wgpu::PollType::Poll);
                if !ov.staging_ready[cur].load(Ordering::Acquire) {
                    let _ = inner.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }); // 稀: GPU が2フレーム以上遅延
                }
                ov.staging_bufs[cur].unmap();
                ov.staging_ready[cur].store(false, Ordering::Release);
                ov.staging_pending[cur] = false;
            }

            // 今フレームの overlay_texture → staging[cur] へコピーして submit
            ov_enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &ov.overlay_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo  { buffer: &ov.staging_bufs[cur], layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ov.bytes_per_row), rows_per_image: Some(ov.display_h) } },
                wgpu::Extent3d { width: ov.display_w, height: ov.display_h, depth_or_array_layers: 1 },
            );
            inner.queue.submit(std::iter::once(ov_enc.finish()));

            // map_async は submit の後に呼ぶ (wgpu のルール)
            {
                let ready = std::sync::Arc::clone(&ov.staging_ready[cur]);
                ov.staging_bufs[cur].slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    if r.is_ok() { ready.store(true, Ordering::Release); }
                });
                ov.staging_pending[cur] = true;
            }

            let _ = inner.device.poll(wgpu::PollType::Poll); // 非同期: ブロックしない

            // 前フレームの staging[prev] が ready なら案5: GDI スレッドへ送信（main スレッドはブロックしない）
            if ov.staging_pending[prev] && ov.staging_ready[prev].load(Ordering::Acquire) {
                let row = ov.display_w as usize * 4;
                let view = ov.staging_bufs[prev].slice(..).get_mapped_range();
                // 再利用バッファを優先取得（チャネル返却 > reuse_buf > 新規確保）
                let mut buf = ov.gdi_rx.try_recv().ok()
                    .or_else(|| ov.reuse_buf.take())
                    .unwrap_or_else(|| vec![0u8; ov.display_w as usize * ov.display_h as usize * 4]);
                // staging → buf へコピー（system memcpy、debug でも高速）
                let bpr = ov.bytes_per_row as usize;
                for y in 0..ov.display_h as usize {
                    buf[y*row..(y+1)*row].copy_from_slice(&view[y*bpr..y*bpr+row]);
                }
                drop(view);
                ov.staging_bufs[prev].unmap();
                ov.staging_ready[prev].store(false, Ordering::Release);
                ov.staging_pending[prev] = false;
                // GDI スレッドへ送信。チャネル満杯なら buf を保存（フレームドロップ、表示は前フレームのまま）
                match ov.gdi_tx.try_send(Some(buf)) {
                    Err(std::sync::mpsc::TrySendError::Full(v))
                    | Err(std::sync::mpsc::TrySendError::Disconnected(v)) => { ov.reuse_buf = v; }
                    Ok(()) => {}
                }
            }

            ov.staging_idx = prev; // 次フレームでバッファを入れ替え

            } } // 'overlay ブロック終了 + else (has draws to render) 終了
            inner.overlay_draw_queue.clear();
        } // if let Some(ov)

        // ⑨ Clear draw queue
        inner.draw_queue.clear();

        // ⑩ Process Win32 messages (main window + overlay)
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // メインウィンドウ
            unsafe {
                while PeekMessageW(&mut msg, inner.hwnd.0, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            // オーバーレイウィンドウ
            #[cfg(target_os = "windows")]
            if let Some(ov) = &inner.overlay {
                unsafe {
                    while PeekMessageW(&mut msg, ov.hwnd, 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    // メインウィンドウが topmost の場合も含め、常にオーバーレイを最前面に維持する
                    if ov.visible {
                        SetWindowPos(ov.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                }
            }
        }

        // WIN32_EVENTS を消費してフレームイベントを処理
        let events = WIN32_EVENTS.with(|e| e.borrow_mut().take_frame());

        for (vk, ext, pressed) in events.key_events {
            let key = crate::input::vk_to_keycode(vk, ext);
            crate::input::process_key_event(key, pressed);
        }
        if let Some((x, y)) = events.cursor_moved {
            let vx = (x * inner.screen_width  as i32) / inner.surface_config.width  as i32;
            let vy = (y * inner.screen_height as i32) / inner.surface_config.height as i32;
            crate::input::process_mouse_move(vx, vy);
        }
        for (btn, pressed) in events.mouse_btn_events {
            let btn = match btn { 0 => MouseButton::Left, 1 => MouseButton::Right, _ => MouseButton::Middle };
            crate::input::process_mouse_button(btn, pressed);
        }
        // リサイズは次フレームの ② で適用
        if let Some(sz) = events.resize_event {
            inner.pending_resize = Some(sz);
        }

        // ⑪ Update delta time
        crate::time::tick_time();

        // ⑫ Poll gamepad state
        if let Some(gm) = &mut inner.gamepad { gm.poll(); }

        !events.should_close
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
        let hwnd = self.inner_ref().hwnd.0;
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOSIZE, SWP_NOZORDER};
            SetWindowPos(hwnd, 0, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
        }
    }

    pub fn position(&self) -> (i32, i32) {
        let hwnd = self.inner_ref().hwnd.0;
        unsafe {
            use windows_sys::Win32::Foundation::RECT;
            use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut r);
            (r.left, r.top)
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

    // ── Overlay API ───────────────────────────────────────────────────────────

    /// オーバーレイを有効化する（init() の前に呼ぶ）。
    pub fn overlay_enable(&mut self, enabled: bool) { self.overlay_enabled = enabled; }

    /// オーバーレイの表示・非表示を切り替える。
    pub fn overlay_visible(&mut self, visible: bool) {
        self.overlay_visible = visible;
        #[cfg(target_os = "windows")]
        if let Some(inner) = &mut self.inner {
            if let Some(ov) = &mut inner.overlay {
                ov.visible = visible;
                use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE, SW_HIDE};
                unsafe { ShowWindow(ov.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }); }
            }
        }
    }

    /// オーバーレイの描画ブレンドモードを設定する。
    pub fn overlay_blend_set(&mut self, blend: BlendMode) {
        self.inner_mut().overlay_blend = blend;
    }

    /// オーバーレイにスプライトを描画する（マスクなし・メインウィンドウ上にも描ける）。
    pub fn overlay_draw_sprite(&mut self, x: i32, y: i32, handle: u32) {
        let blend = self.inner_ref().overlay_blend;
        self.inner_mut().overlay_draw_queue.push(DrawCommand::Sprite {
            x, y, handle, mask_handle: None, mask_ox: 0, mask_oy: 0,
            params: DrawSpriteParams::default(), blend,
        });
    }

    /// オーバーレイにスプライトを描画する（拡張パラメータ付き）。
    pub fn overlay_draw_sprite_ex(&mut self, x: i32, y: i32, handle: u32, params: DrawSpriteParams) {
        let blend = self.inner_ref().overlay_blend;
        self.inner_mut().overlay_draw_queue.push(DrawCommand::Sprite {
            x, y, handle, mask_handle: None, mask_ox: 0, mask_oy: 0,
            params, blend,
        });
    }

    /// オーバーレイにテキストを描画する。
    pub fn overlay_draw_text(&mut self, x: i32, y: i32, text: impl AsRef<str>, color: Color) {
        let font = self.ensure_default_font();
        if font == 0 { return; }
        if let Some((w, h, rgba)) = crate::text::build_text_bitmap(text.as_ref(), color, font) {
            self.inner_mut().overlay_draw_queue.push(DrawCommand::Text { x, y, width: w, height: h, rgba });
        }
    }

    /// オーバーレイの描画キューをクリアする。
    pub fn overlay_clear(&mut self) {
        self.inner_mut().overlay_draw_queue.clear();
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

/// Build a sprite quad for overlay rendering.
/// x/y are virtual-screen pixel coords; win_scale_* converts them to display pixels.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_sprite_quad_overlay(
    x: i32, y: i32,
    sprite_w: u32, sprite_h: u32,
    display_w: u32, display_h: u32,
    main_x: i32, main_y: i32,
    win_scale_x: f32, win_scale_y: f32,
    mask_ox: f32, mask_oy: f32, mask_w: f32, mask_h: f32, mask_on: f32,
    params: &DrawSpriteParams,
) -> [SpriteVertex; 6] {
    let dw     = display_w as f32;
    let dh     = display_h as f32;
    // Scale sprite size and position from virtual-screen pixels to display pixels
    let draw_w = sprite_w as f32 * params.scale_x * win_scale_x;
    let draw_h = sprite_h as f32 * params.scale_y * win_scale_y;
    let cx = x as f32 * win_scale_x + draw_w * 0.5 + main_x as f32;
    let cy = y as f32 * win_scale_y + draw_h * 0.5 + main_y as f32;
    let hw = draw_w * 0.5;
    let hh = draw_h * 0.5;

    // Scale mask coords from virtual-screen space to display space
    let (dmox, dmoy, dmw, dmh) = if mask_on > 0.5 {
        (mask_ox * win_scale_x + main_x as f32,
         mask_oy * win_scale_y + main_y as f32,
         mask_w  * win_scale_x,
         mask_h  * win_scale_y)
    } else {
        (mask_ox, mask_oy, mask_w, mask_h)
    };

    let rad = params.rotation.to_radians();
    let cos = rad.cos();
    let sin = rad.sin();
    let rot = |lx: f32, ly: f32| -> (f32, f32) { (lx * cos - ly * sin, lx * sin + ly * cos) };
    let (tl_x, tl_y) = rot(-hw, -hh);
    let (tr_x, tr_y) = rot( hw, -hh);
    let (bl_x, bl_y) = rot(-hw,  hh);
    let (br_x, br_y) = rot( hw,  hh);
    let corners = [(cx+tl_x, cy+tl_y), (cx+tr_x, cy+tr_y), (cx+bl_x, cy+bl_y), (cx+br_x, cy+br_y)];

    let (u0, u1) = if params.flip_x { (1.0f32, 0.0f32) } else { (0.0f32, 1.0f32) };
    let (v0, v1) = if params.flip_y { (1.0f32, 0.0f32) } else { (0.0f32, 1.0f32) };
    let ndc = |px: f32, py: f32| -> [f32; 2] { [px / dw * 2.0 - 1.0, 1.0 - py / dh * 2.0] };
    let sv = |idx: usize, u: f32, tv: f32| SpriteVertex {
        pos: ndc(corners[idx].0, corners[idx].1),
        uv: [u, tv],
        screen_xy: [corners[idx].0, corners[idx].1],
        mask_ox: dmox, mask_oy: dmoy, mask_w: dmw, mask_h: dmh, mask_on,
        alpha: params.alpha,
    };
    let tl = sv(0, u0, v0); let tr = sv(1, u1, v0);
    let bl = sv(2, u0, v1); let br = sv(3, u1, v1);
    [tl, tr, bl, tr, br, bl]
}
