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
    window.decorations(false);
    window.transparent(true);
    window.overlay_enable(true);

    // 2. ウィンドウ作成
    window.init();

    // 3. フォント設定（省略時はシステムフォント・サイズ16を使用）
    // window.font_file("font.ttf");
    window.font_size(16);

    // 4. 1秒間のデルタタイム保存ベクター
    let mut dt_holder: Vec<f32> = Vec::new();
    let mut frame_rate = 0.0;

    // 5. マウス座標とウィンドウ座標
    let mut old_mx = 0;
    let mut old_my = 0;
    let mut wx = 0;
    let mut wy = 0;

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

        // マウスドラッグでウィンドウ位置更新
        if window.is_mouse_just_pressed(MouseButton::Left) {
            // 座標取得
            (wx, wy) = window.position();
            (old_mx, old_my) = window.mouse_position();
        } else if window.is_mouse_pressed(MouseButton::Left) {
            // マウス座標取得
            let (mx, my) = window.mouse_position();
            // ウィンドウ座標更新
            window.set_position(wx + (mx - old_mx), wy + (my - old_my));
            (wx, wy) = window.position();
        }

        // 描画
        window.screen_draw_text(0, 0, format!("fps: {:.2}", frame_rate), Color::WHITE);
        window.screen_draw_text(-10, 20, format!(" dt: {:.3}", dt), Color::WHITE);
    }

    // 解放
    free_all_sounds();
    free_all_graphs();
}
