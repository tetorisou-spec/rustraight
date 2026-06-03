use std::collections::HashMap;
use fontdue::{Font, FontSettings};
use crate::draw::Color;

struct FontEntry {
    font: Font,
    size: f32,
}

struct TextState {
    fonts:   HashMap<u32, FontEntry>,
    next_id: u32,
}

impl TextState {
    fn new() -> Self {
        Self { fonts: HashMap::new(), next_id: 1 }
    }
}

thread_local! {
    static TEXT: std::cell::RefCell<TextState> =
        std::cell::RefCell::new(TextState::new());
}

fn insert_font(font: Font, size: f32) -> u32 {
    TEXT.with(|t| {
        let mut t = t.borrow_mut();
        let id = t.next_id;
        t.next_id += 1;
        t.fonts.insert(id, FontEntry { font, size });
        id
    })
}

// ── 公開 API ──────────────────────────────────────────────────────────────────

/// フォントファイル（TTF / OTF / TTC）をロードしてハンドルを返す。失敗時は 0。
pub fn load_font(path: &str, size: u32) -> u32 {
    let bytes = match std::fs::read(path) {
        Ok(b)  => b,
        Err(_) => { crate::log_warn!("フォントファイルを読み込めません: '{path}'"); return 0; }
    };
    let font = match Font::from_bytes(bytes, FontSettings::default()) {
        Ok(f)  => f,
        Err(e) => { crate::log_warn!("フォントのパースに失敗しました: {e}"); return 0; }
    };
    insert_font(font, size as f32)
}

/// テキストを描画したときの幅（ピクセル）を返す。
pub fn get_text_width(text: &str, font_id: u32) -> i32 {
    TEXT.with(|t| {
        let t = t.borrow();
        let Some(entry) = t.fonts.get(&font_id) else { return 0 };
        text.chars()
            .map(|ch| entry.font.metrics(ch, entry.size).advance_width.round() as i32)
            .sum()
    })
}

// ── 内部ヘルパー ──────────────────────────────────────────────────────────────

/// Windows のシステムフォントを探してロードする。
pub(crate) fn load_default_font(size: u32) -> u32 {
    let candidates = [
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\arial.ttf",
    ];
    for path in &candidates {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return insert_font(font, size as f32);
            }
        }
    }
    crate::log_warn!("システムフォントが見つかりません");
    0
}

/// テキスト文字列を RGBA ビットマップに変換して返す。
/// 戻り値: Some((width, height, rgba_bytes)) / None（空文字列など）
pub(crate) fn build_text_bitmap(text: &str, color: Color, font_id: u32) -> Option<(u32, u32, Vec<u8>)> {
    TEXT.with(|t| {
        let t = t.borrow();
        let entry = t.fonts.get(&font_id)?;

        // ── レイアウトパス: 各グリフのメトリクスを収集 ──────────────────────
        struct GlyphInfo {
            metrics: fontdue::Metrics,
            bitmap:  Vec<u8>,
            advance: i32,
        }

        let mut glyphs: Vec<GlyphInfo> = Vec::new();
        let mut total_advance = 0i32;
        let mut max_ascent    = 0i32;
        let mut max_descent   = 0i32;

        for ch in text.chars() {
            let (metrics, bitmap) = entry.font.rasterize(ch, entry.size);
            let advance = metrics.advance_width.round() as i32;
            // baseline より上のピクセル数 / 下のピクセル数
            let ascent  = metrics.height as i32 + metrics.ymin;
            let descent = (-metrics.ymin).max(0);
            max_ascent  = max_ascent.max(ascent);
            max_descent = max_descent.max(descent);
            total_advance += advance;
            glyphs.push(GlyphInfo { metrics, bitmap, advance });
        }

        if total_advance == 0 || glyphs.is_empty() { return None; }

        let total_height = (max_ascent + max_descent).max(1) as u32;
        let total_width  = total_advance as u32;

        // ── 描画パス: RGBA ビットマップを構築 ───────────────────────────────
        let mut rgba = vec![0u8; (total_width * total_height * 4) as usize];

        let mut cursor_x = 0i32;
        for g in &glyphs {
            if g.metrics.width == 0 || g.metrics.height == 0 {
                cursor_x += g.advance;
                continue;
            }
            // グリフ左上の出力座標（baseline 基準で揃える）
            let gx = cursor_x + g.metrics.xmin;
            let gy = max_ascent - (g.metrics.height as i32 + g.metrics.ymin);

            for py in 0..g.metrics.height as i32 {
                for px in 0..g.metrics.width as i32 {
                    let dx = gx + px;
                    let dy = gy + py;
                    if dx < 0 || dy < 0 || dx >= total_width as i32 || dy >= total_height as i32 {
                        continue;
                    }
                    let alpha = g.bitmap[(py * g.metrics.width as i32 + px) as usize];
                    if alpha == 0 { continue; }
                    let dst = (dy as usize * total_width as usize + dx as usize) * 4;
                    rgba[dst]     = color.r;
                    rgba[dst + 1] = color.g;
                    rgba[dst + 2] = color.b;
                    rgba[dst + 3] = alpha;
                }
            }
            cursor_x += g.advance;
        }

        Some((total_width, total_height, rgba))
    })
}
