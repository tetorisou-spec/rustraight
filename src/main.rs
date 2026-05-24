use rustraight::prelude::*;

const WINDOW_WIDTH: u16 = 800;
const WINDOW_HEIGHT: u16 = 600;
const SCREEN_WIDTH: u16 = 320;
const SCREEN_HEIGHT: u16 = 240;

fn main() {
    // 1. ウィンドウ設定
    let mut window = Window::default();
    window.title("ゲーム");
    window.size(WINDOW_WIDTH, WINDOW_HEIGHT);
    window.screen_size(SCREEN_WIDTH, SCREEN_HEIGHT);
    window.resizable(true);
    window.vsync(true);
    window.decorations(true);
    window.transparent(false);

    // 2. ウィンドウ作成
    window.init();

    // 3. フォント設定（省略時はシステムフォント・サイズ16を使用）
    // window.font_file("font.ttf");
    window.font_size(16);

    // メインループ
    while window.advance_frame() {
        // 描画
        window.screen_clear();

        // フレームレート表示
        window.screen_draw_text(0, 0, format!("経過秒数: {:.3}", window.elapsed_time()), Color::WHITE);
    }

    // 解放
    free_all_sounds();
    free_all_graphs();
}
