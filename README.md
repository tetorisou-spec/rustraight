# rustraight

[![Crates.io](https://img.shields.io/crates/v/rustraight)](https://crates.io/crates/rustraight)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[DXLib](https://dxlib.xsrv.jp/) にインスパイアされた、Rust 向けシンプル 2D ゲームライブラリです。

[wgpu](https://wgpu.rs/) と Win32 API を基盤とし、GPU の低レイヤな知識なしに 2D ゲームを作れる入門者向け API を提供します。

> **Windows 専用ライブラリです。** グラフィックス・入力・サウンドにそれぞれ DX12 / Win32 / XAudio2 を直接使用しているため、Windows 10 以降が動作要件となります。

## 特徴

- **ウィンドウ & 仮想スクリーン** — ウィンドウサイズに依存しない独立した仮想解像度でゲームロジックを記述
- **背景透過ウィンドウ** — DX12 DirectComposition + DWM による per-pixel alpha 透過
- **スプライト描画** — PNG / JPEG / BMP など WIC 対応フォーマットの読み込みとスケール / 回転 / アルファ / 反転付き描画
- **スプライトシート** — `load_div_graph` でスプライトシートを個別スプライトに分割
- **図形描画** — ピクセル・線・矩形・円・三角形（塗りつぶし・アウトライン）
- **ブレンドモード** — 通常 / 加算 / 乗算
- **サブスクリーン** — オフスクリーンレンダーターゲットをスプライトとして利用
- **キーボード & マウス入力** — 押している / 押した瞬間 / 離した瞬間を全キー・ボタンで判定
- **ゲームパッド入力** — ボタンとアナログスティック (DirectInput / XInput)
- **サウンド** — WAV（PCM 8/16-bit）・OGG Vorbis の読み込み・再生（XAudio2）
- **テキスト描画** — システムフォントまたはカスタム TTF/OTF ファイルによる文字描画
- **デルタタイム & 経過時間** — フレームレート非依存の移動処理を標準サポート
- **オーバーレイウィンドウ** — メインウィンドウの外側（全画面）に描画できる透過オーバーレイ
- **デバッグログ** — デバッグビルド時のみコンソールに出力されるログマクロ

## インストール

`Cargo.toml` に以下を追加してください：

```toml
[dependencies]
rustraight = "0.3"
```

## クイックスタート

```rust
use rustraight::prelude::*;

fn main() {
    // 1. ウィンドウ設定
    let mut window = Window::default();
    window.title("マイゲーム");
    window.size(800, 600);
    window.screen_size(320, 240); // 仮想解像度
    window.vsync(true);
    window.init();

    // 2. アセット読み込み
    let player = load_graph("player.png");
    let bgm    = load_sound("bgm.wav");
    play_sound(bgm, true); // ループ再生

    let mut x = 0i32;
    let mut y = 0i32;
    let speed = 100.0f32;

    // 3. メインループ
    while window.advance_frame() {
        let dt = window.delta_time();

        if window.is_pressed(KeyCode::ArrowLeft)  { x -= (speed * dt) as i32; }
        if window.is_pressed(KeyCode::ArrowRight) { x += (speed * dt) as i32; }
        if window.is_pressed(KeyCode::ArrowUp)    { y -= (speed * dt) as i32; }
        if window.is_pressed(KeyCode::ArrowDown)  { y += (speed * dt) as i32; }

        window.screen_draw_sprite(x, y, player);
        window.screen_draw_text(0, 0, format!("pos: ({x}, {y})"), Color::WHITE);
    }

    // 4. リソース解放
    free_all_sounds();
    free_all_graphs();
}
```

## API リファレンス

### ウィンドウ

```rust
let mut window = Window::default();
window.title("タイトル");
window.size(800, 600);           // ウィンドウサイズ (ピクセル)
window.screen_size(320, 240);    // 仮想スクリーン解像度
window.resizable(true);
window.vsync(true);
window.decorations(true);        // タイトルバー等の装飾
window.transparent(false);       // ウィンドウ背景の透過 (DX12 DirectComposition)
window.topmost(false);           // 常に最前面に表示
window.init();                   // ウィンドウを開く

window.advance_frame() -> bool   // ウィンドウが閉じられると false を返す
window.delta_time()   -> f32     // 前フレームからの秒数
window.elapsed_time() -> f64     // ウィンドウ作成からの秒数
window.set_position(x, y)        // ウィンドウ位置を設定
window.position()    -> (i32, i32)
```

### グラフィックス

```rust
// 画像の読み込み (WIC 経由: PNG / JPEG / BMP / TIFF / GIF / WebP 等)
let spr:   u32      = load_graph("image.png");
let sheet: [u32; N] = load_div_graph("sheet.png", N, tile_w, tile_h);
free_all_graphs();

// スプライト描画
window.screen_draw_sprite(x, y, handle);
window.screen_draw_sprite_ex(x, y, handle, DrawSpriteParams {
    scale_x:  2.0,
    scale_y:  2.0,
    rotation: 45.0, // 度数法
    alpha:    0.5,  // 0.0 〜 1.0
    flip_x:   false,
    flip_y:   false,
    ..Default::default()
});

// 図形描画
window.screen_draw_fill(Color::BLACK);                         // 全塗りつぶし
window.screen_draw_pixel(x, y, Color::WHITE);                  // ピクセル
window.screen_draw_line(x1, y1, x2, y2, Color::WHITE);        // 線
window.screen_draw_rectangle(x, y, w, h, Color::RED, true);   // 矩形 (true=塗りつぶし)
window.screen_draw_circle(cx, cy, radius, Color::BLUE, false); // 円 (false=アウトライン)
window.screen_draw_triangle(x1,y1, x2,y2, x3,y3, Color::GREEN, true);

// ブレンドモード
window.screen_blend_set(BlendMode::Add);    // 加算
window.screen_blend_set(BlendMode::Mul);    // 乗算
window.screen_blend_set(BlendMode::Normal); // 通常に戻す

// マスク
window.screen_mask_set(x, y, mask_handle); // スプライトをマスクとして設定
window.screen_mask_reset();
```

### サブスクリーン

オフスクリーンのレンダーターゲットをスプライトとして利用できます。

```rust
let mut screen = window.create_screen(64, 64);
screen.clear();
screen.draw_sprite(0, 0, spr);
screen.draw_rectangle(0, 0, 64, 64, Color::RED, false);
window.screen_draw_sprite(100, 100, screen.handle());
```

### 入力

```rust
// キーボード
window.is_pressed(KeyCode::Space)       // 押している間 true
window.is_just_pressed(KeyCode::Enter)  // 押した瞬間だけ true
window.is_released(KeyCode::Escape)     // 離した瞬間だけ true

// マウス
window.mouse_position()                          // (x, y) 仮想スクリーン座標
window.is_mouse_pressed(MouseButton::Left)       // 押している間
window.is_mouse_just_pressed(MouseButton::Right) // 押した瞬間
window.is_mouse_released(MouseButton::Middle)    // 離した瞬間

// ゲームパッド (XInput)
window.is_pad_pressed(0, PadButton::South)        // pad_id=0
window.is_pad_just_pressed(0, PadButton::East)
window.is_pad_released(0, PadButton::West)
window.pad_axis(0, PadAxis::LeftStickX)           // -1.0 〜 1.0
window.is_pad_connected(0)
window.pad_count() -> usize
```

### サウンド

XAudio2 による WAV 再生です。

```rust
let se  = load_sound("jump.wav");
let bgm = load_sound("bgm.wav");

play_sound(se,  false); // 1 回再生
play_sound(bgm, true);  // ループ再生
stop_sound(bgm);
set_volume(bgm, 0.5);   // 音量 0.0 〜 1.0
free_all_sounds();
```

対応フォーマット: **WAV（PCM 8-bit / 16-bit）**、**OGG Vorbis**

### テキスト

```rust
window.font_size(16);           // サイズ 16 でシステムフォントを使用
window.font_file("font.ttf");   // カスタムフォントファイルを指定

window.screen_draw_text(x, y, "こんにちは", Color::WHITE);

// フォントハンドルを直接指定
let font = load_font("font.ttf", 24);
window.screen_draw_text_ex(x, y, "Hello!", Color::YELLOW, font);
let w = get_text_width("Hello!", font);
```

### オーバーレイウィンドウ

メインウィンドウの外側を含む全画面に透過描画できるオーバーレイです。OBS 等のウィンドウキャプチャと組み合わせて、ゲーム画面とデスクトップ上の演出を同時に実現できます。

```rust
// init() の前に有効化する
window.overlay_enable(true);
window.init();

// メインループ内
while window.advance_frame() {
    // メインウィンドウへの通常描画
    window.screen_draw_sprite(x, y, sprite);

    // オーバーレイへの描画 (座標はメインウィンドウ基準、範囲外も描ける)
    window.overlay_draw_sprite(-50, 100, sprite);    // ウィンドウ枠をはみ出して描画
    window.overlay_draw_text(-30, 0, "FPS: 300", Color::WHITE);
    window.overlay_blend_set(BlendMode::Add);
    window.overlay_draw_sprite_ex(x, y, glow, DrawSpriteParams {
        alpha: 0.5, ..Default::default()
    });
    window.overlay_blend_set(BlendMode::Normal);
}
```

`screen_draw_*` でメインウィンドウ外座標に描画すると自動的にオーバーレイへフォールバックします。

| メソッド | 説明 |
|---|---|
| `overlay_enable(bool)` | オーバーレイを有効化 (`init()` 前に呼ぶ) |
| `overlay_visible(bool)` | オーバーレイウィンドウの表示/非表示 |
| `overlay_clear()` | オーバーレイの描画キューをクリア |
| `overlay_draw_sprite(x, y, handle)` | スプライトをオーバーレイに描画 |
| `overlay_draw_sprite_ex(x, y, handle, params)` | 拡張パラメータ付き描画 |
| `overlay_draw_text(x, y, text, color)` | テキストをオーバーレイに描画 |
| `overlay_blend_set(BlendMode)` | オーバーレイのブレンドモードを設定 |

### デバッグログ

デバッグビルド (`cargo run` / `cargo build`) のみ標準エラーに出力されるログマクロです。リリースビルド (`cargo build --release`) では出力コードが完全に除去されます。

```rust
log_info!("スプライトをロードしました: '{}'", path);  // [情報] スプライトをロードしました: 'player.png'
log_warn!("フォントが見つかりません: '{}'", path);     // [警告] フォントが見つかりません: 'font.ttf'
log_error!("初期化に失敗しました: {}", reason);        // [エラー] 初期化に失敗しました: ...
```

ライブラリ内部のイベント（ファイル読み込みエラー・デバイス接続など）もこのマクロで出力されます。

| マクロ | 用途 |
|---|---|
| `log_info!(...)` | 通常の情報（読み込み完了・デバイス認識など） |
| `log_warn!(...)` | 警告（ファイルが見つからない・デコード失敗など） |
| `log_error!(...)` | エラー（致命的な初期化失敗など） |

## 内部実装

| 機能 | 実装 |
|---|---|
| グラフィックス | wgpu 27 (DX12) + DirectComposition |
| 画像読み込み | WIC (Windows Imaging Component) |
| サウンド | XAudio2 |
| キーボード / マウス | Win32 メッセージ (WM_KEYDOWN 等) |
| ゲームパッド | XInput |
| テキスト描画 | fontdue |

## プラットフォーム対応

| OS | 対応状況 |
|---|---|
| Windows 10 / 11 | ✅ |
| macOS / Linux | ❌ (Win32 API を直接使用) |

## ライセンス

MIT — [LICENSE](LICENSE) を参照
