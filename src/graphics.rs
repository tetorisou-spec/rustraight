use std::cell::RefCell;
use std::collections::HashMap;

pub struct SpriteData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct SpriteStore {
    sprites: HashMap<u32, SpriteData>,
    next_id: u32,
}

impl Default for SpriteStore {
    fn default() -> Self {
        Self { sprites: HashMap::new(), next_id: 1 }
    }
}

impl SpriteStore {
    fn insert(&mut self, data: SpriteData) -> u32 {
        let id = self.next_id;
        self.sprites.insert(id, data);
        self.next_id += 1;
        id
    }

    fn get(&self, id: u32) -> Option<&SpriteData> {
        self.sprites.get(&id)
    }

    fn clear(&mut self) {
        self.sprites.clear();
    }
}

thread_local! {
    static STORE: RefCell<SpriteStore> = RefCell::new(SpriteStore::default());
}

fn missing_sprite() -> SpriteData {
    const W: u32 = 8;
    const H: u32 = 8;
    let mut rgba = vec![0u8; (W * H * 4) as usize];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[0] = 0xFF;
        pixel[1] = 0x00;
        pixel[2] = 0xFF;
        pixel[3] = 0xFF;
    }
    SpriteData { width: W, height: H, rgba }
}

/// WIC (Windows Imaging Component) で画像ファイルを RGBA8 にデコードする。
/// PNG/JPEG/BMP/TIFF/GIF/WebP など WIC が対応する全フォーマットを読める。
#[cfg(target_os = "windows")]
fn load_image_wic(path: &str) -> Option<SpriteData> {
    use windows::{
        core::PCWSTR,
        Win32::Foundation::GENERIC_ACCESS_RIGHTS,
        Win32::Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA,
            IWICBitmapFrameDecode, IWICFormatConverter, IWICImagingFactory, IWICPalette,
            WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut,
            WICDecodeMetadataCacheOnDemand,
        },
        Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
    };
    unsafe {
        // すでに別モードで初期化済みでも WIC オブジェクトはアパートメント非依存なので続行する
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;

        let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(path_w.as_ptr()),
                None,
                GENERIC_ACCESS_RIGHTS(0x8000_0000), // GENERIC_READ
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;

        let frame: IWICBitmapFrameDecode = decoder.GetFrame(0).ok()?;
        let mut width = 0u32;
        let mut height = 0u32;
        frame.GetSize(&mut width, &mut height).ok()?;

        // RGBA8 フォーマットコンバータを作成
        let converter: IWICFormatConverter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None::<&IWICPalette>,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;

        let stride = width * 4;
        let mut rgba = vec![0u8; (stride * height) as usize];
        converter
            .CopyPixels(std::ptr::null(), stride, &mut rgba)
            .ok()?;

        Some(SpriteData { width, height, rgba })
    }
}

fn decode_image_file(path: &str) -> SpriteData {
    #[cfg(target_os = "windows")]
    if let Some(s) = load_image_wic(path) {
        return s;
    }
    crate::log_warn!("画像の読み込みに失敗しました: '{path}'、代替スプライトを使用します");
    missing_sprite()
}

pub fn load_image(path: &str) -> u32 {
    let data = decode_image_file(path);
    STORE.with(|s| s.borrow_mut().insert(data))
}

pub fn load_div_image(path: &str, count: usize, tile_w: u32, tile_h: u32) -> Vec<u32> {
    let img = decode_image_file(path);
    let cols = img.width / tile_w;
    let rows = img.height / tile_h;
    let total = (cols * rows) as usize;

    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        if i < total {
            let col = (i as u32) % cols;
            let row = (i as u32) / cols;
            let x0 = col * tile_w;
            let y0 = row * tile_h;
            let mut tile_rgba = vec![0u8; (tile_w * tile_h * 4) as usize];
            for ty in 0..tile_h {
                for tx in 0..tile_w {
                    let src = ((y0 + ty) * img.width + (x0 + tx)) as usize * 4;
                    let dst = (ty * tile_w + tx) as usize * 4;
                    tile_rgba[dst..dst + 4].copy_from_slice(&img.rgba[src..src + 4]);
                }
            }
            ids.push(STORE.with(|s| {
                s.borrow_mut().insert(SpriteData { width: tile_w, height: tile_h, rgba: tile_rgba })
            }));
        } else {
            ids.push(STORE.with(|s| s.borrow_mut().insert(missing_sprite())));
        }
    }
    ids
}

pub fn free_all_images() {
    STORE.with(|s| s.borrow_mut().clear());
}

#[allow(dead_code)]
pub(crate) fn get_sprite(id: u32) -> Option<SpriteData> {
    STORE.with(|s| {
        s.borrow().get(id).map(|d| SpriteData {
            width: d.width,
            height: d.height,
            rgba: d.rgba.clone(),
        })
    })
}

pub(crate) fn register_blank_sprite(width: u32, height: u32) -> u32 {
    STORE.with(|s| {
        s.borrow_mut().insert(SpriteData {
            width,
            height,
            rgba: vec![0u8; width as usize * height as usize * 4],
        })
    })
}

pub(crate) fn with_sprite<F: FnOnce(u32, u32, &[u8])>(id: u32, f: F) {
    STORE.with(|s| {
        if let Some(sprite) = s.borrow().get(id) {
            f(sprite.width, sprite.height, &sprite.rgba);
        }
    });
}

pub(crate) fn update_sprite(id: u32, rgba: &[u8]) {
    STORE.with(|s| {
        if let Some(sprite) = s.borrow_mut().sprites.get_mut(&id) {
            sprite.rgba.copy_from_slice(rgba);
        }
    });
}

pub(crate) fn blit_sprite_masked(
    dst: &mut [u8], dst_w: u32, dst_h: u32,
    x: i32, y: i32, handle: u32,
    mask_ox: i32, mask_oy: i32, mask_handle: u32,
) {
    STORE.with(|s| {
        let s = s.borrow();
        let Some(sprite) = s.get(handle) else { return };
        let Some(mask)   = s.get(mask_handle) else { return };
        let bw = dst_w as i32;
        let bh = dst_h as i32;
        for sy in 0..sprite.height as i32 {
            for sx in 0..sprite.width as i32 {
                let px = x + sx;
                let py = y + sy;
                if px < 0 || py < 0 || px >= bw || py >= bh { continue; }

                // マスクのサンプル座標
                let mx = px - mask_ox;
                let my = py - mask_oy;
                let mask_alpha = if mx >= 0 && my >= 0
                    && mx < mask.width as i32 && my < mask.height as i32
                {
                    mask.rgba[(my as usize * mask.width as usize + mx as usize) * 4 + 3]
                } else {
                    0
                };
                if mask_alpha == 0 { continue; }

                let src = (sy as usize * sprite.width as usize + sx as usize) * 4;
                let di  = (py as usize * dst_w  as usize + px  as usize) * 4;
                let effective_alpha = (sprite.rgba[src + 3] as u32 * mask_alpha as u32 / 255) as u8;
                if effective_alpha == 0 { continue; }
                if effective_alpha == 255 {
                    dst[di..di + 4].copy_from_slice(&sprite.rgba[src..src + 4]);
                } else {
                    let sa = effective_alpha as f32 / 255.0;
                    let da = dst[di + 3] as f32 / 255.0;
                    let oa = sa + da * (1.0 - sa);
                    dst[di + 3] = (oa * 255.0) as u8;
                    if oa > 0.0 {
                        let oi = 1.0 / oa;
                        dst[di]     = ((sprite.rgba[src]     as f32 * sa + dst[di]     as f32 * da * (1.0 - sa)) * oi) as u8;
                        dst[di + 1] = ((sprite.rgba[src + 1] as f32 * sa + dst[di + 1] as f32 * da * (1.0 - sa)) * oi) as u8;
                        dst[di + 2] = ((sprite.rgba[src + 2] as f32 * sa + dst[di + 2] as f32 * da * (1.0 - sa)) * oi) as u8;
                    }
                }
            }
        }
    });
}

pub(crate) fn blit_sprite(dst: &mut [u8], dst_w: u32, dst_h: u32, x: i32, y: i32, handle: u32) {
    STORE.with(|s| {
        let s = s.borrow();
        let Some(sprite) = s.get(handle) else { return };
        let bw = dst_w as i32;
        let bh = dst_h as i32;
        for sy in 0..sprite.height as i32 {
            for sx in 0..sprite.width as i32 {
                let px = x + sx;
                let py = y + sy;
                if px < 0 || py < 0 || px >= bw || py >= bh { continue; }
                let src = (sy as usize * sprite.width as usize + sx as usize) * 4;
                let di = (py as usize * dst_w as usize + px as usize) * 4;
                let a = sprite.rgba[src + 3];
                if a == 0 { continue; }
                if a == 255 {
                    dst[di..di + 4].copy_from_slice(&sprite.rgba[src..src + 4]);
                } else {
                    let sa = a as f32 / 255.0;
                    let da = dst[di + 3] as f32 / 255.0;
                    let oa = sa + da * (1.0 - sa);
                    dst[di + 3] = (oa * 255.0) as u8;
                    if oa > 0.0 {
                        let oi = 1.0 / oa;
                        dst[di]     = ((sprite.rgba[src]     as f32 * sa + dst[di]     as f32 * da * (1.0 - sa)) * oi) as u8;
                        dst[di + 1] = ((sprite.rgba[src + 1] as f32 * sa + dst[di + 1] as f32 * da * (1.0 - sa)) * oi) as u8;
                        dst[di + 2] = ((sprite.rgba[src + 2] as f32 * sa + dst[di + 2] as f32 * da * (1.0 - sa)) * oi) as u8;
                    }
                }
            }
        }
    });
}

// -----------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum BlendMode { #[default] Normal, Add, Mul }

#[derive(Copy, Clone)]
pub struct DrawSpriteParams {
    pub scale_x:  f32,
    pub scale_y:  f32,
    pub rotation: f32,   // 度数法
    pub alpha:    f32,
    pub flip_x:   bool,
    pub flip_y:   bool,
}

impl Default for DrawSpriteParams {
    fn default() -> Self {
        Self {
            scale_x:  1.0,
            scale_y:  1.0,
            rotation: 0.0,
            alpha:    1.0,
            flip_x:   false,
            flip_y:   false,
        }
    }
}
