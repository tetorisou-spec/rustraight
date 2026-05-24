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

    // 4. 1秒間のデルタタイム保存ベクター
    let mut dt_holder: Vec<f32> = Vec::new();
    let mut frame_rate = 0.0;

    // メインループ
    while window.advance_frame() {
        // デルタタイム取得
        let dt = window.delta_time();
        dt_holder.push(dt);

        // nフレームに1回
        if dt_holder.len() == 30 {
            // 平均デルタタイムを取る
            let mut average_dt = 0.0;
            for past_dt in &dt_holder {
                average_dt += past_dt;
            }
            average_dt /= dt_holder.len() as f32;

            // フレームレート計算
            frame_rate = 1.0 / average_dt;

            dt_holder.clear();
        }

        // 描画
        window.screen_clear();
        // フレームレート表示
        window.screen_draw_text(0, 0, format!("fps: {:.2}", frame_rate), Color::WHITE);
    }

    // 解放
    free_all_sounds();
    free_all_graphs();
}
