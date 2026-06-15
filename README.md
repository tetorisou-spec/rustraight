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
- **スプライトシート** — `load_div_image` でスプライトシートを個別スプライトに分割
- **図形描画** — ピクセル・線・矩形・円・三角形（塗りつぶし・アウトライン）
- **ブレンドモード** — 通常 / 加算 / 乗算
- **サブスクリーン** — オフスクリーンレンダーターゲットをスプライトハンドルとして利用
- **キーボード & マウス入力** — 押している / 押した瞬間 / 離した瞬間を全キー・ボタンで判定
- **ゲームパッド入力** — ボタンとアナログスティック (DirectInput / XInput)
- **サウンド** — WAV（PCM 8/16-bit）・OGG Vorbis の読み込み・再生（XAudio2）
- **テキスト描画** — システムフォントまたはカスタム TTF/OTF ファイルによる文字描画（テクスチャキャッシュ済み）
- **デルタタイム & 経過時間** — フレームレート非依存の移動処理を標準サポート
- **オーバーレイウィンドウ** — メインウィンドウの外側（全画面）に描画できる透過オーバーレイ
- **デバッグログ** — デバッグビルド時のみコンソールに出力されるログマクロ

## インストール

`Cargo.toml` に以下を追加してください：

```toml
[dependencies]
rustraight = "0.4"
```

## クイックスタート

```rust
use rustraight::prelude::*;

fn main() {
    // 1. 初期化
    init(WindowConfig {
        title:  String::from("マイゲーム"),
        width:  800,
        height: 600,
        ..Default::default()
    });

    // 2. アセット読み込み
    let player = load_image("player.png");
    let bgm    = load_sound("bgm.wav");
    play_sound(bgm, true); // ループ再生

    let mut x = 0i32;
    let mut y = 0i32;

    // 3. メインループ
    while advance_frame() {
        let dt = delta_time();
        let speed = 100.0f32;

        if is_pressed(KeyCode::ArrowLeft)  { x -= (speed * dt) as i32; }
        if is_pressed(KeyCode::ArrowRight) { x += (speed * dt) as i32; }
        if is_pressed(KeyCode::ArrowUp)    { y -= (speed * dt) as i32; }
        if is_pressed(KeyCode::ArrowDown)  { y += (speed * dt) as i32; }

        draw_image(0, x, y, player);
        draw_text(0, x, y + 20, format!("pos: ({x}, {y})"), Color::WHITE);
    }

    // 4. リソース解放
    free_all_sounds();
    free_all_images();
}
```

## API リファレンス

### 初期化

```rust
init(WindowConfig {
    title:         String::from("タイトル"),
    width:         800,      // ウィンドウ幅 (ピクセル)
    height:        600,      // ウィンドウ高さ
    screen_width:  320,      // 仮想スクリーン幅（省略時 = width）
    screen_height: 240,      // 仮想スクリーン高さ（省略時 = height）
    resizable:     true,
    vsync:         true,
    decorations:   true,     // タイトルバー等の装飾
    transparent:   false,    // ウィンドウ背景の透過 (DX12 DirectComposition)
    topmost:       false,    // 常に最前面に表示
    font_path:     None,     // デフォルトフォントファイルパス
    font_size:     16,       // デフォルトフォントサイズ
    overlay_enabled: false,  // オーバーレイウィンドウを有効化
    overlay_visible: true,
    ..Default::default()     // 未指定フィールドはデフォルト値
});

advance_frame() -> bool  // ウィンドウが閉じられると false を返す
delta_time()    -> f32   // 前フレームからの秒数
elapsed_time()  -> f64   // アプリ起動からの経過秒数
set_window_position(x, y)
window_position() -> (i32, i32)
set_window_size(w, h)    // ウィンドウのクライアント領域サイズを変更
set_screen_size(w, h)    // 仮想解像度（スクリーンレンダーターゲット）を変更
show_cursor(visible)     // マウスカーソルの表示 / 非表示
```

### グラフィックス

`target` には `0`（メインウィンドウ）またはサブスクリーンのハンドルを渡します。

```rust
// 画像の読み込み (WIC 経由: PNG / JPEG / BMP / TIFF / GIF / WebP 等)
let spr: u32   = load_image("image.png");
let sheet: [u32; 4] = load_div_image("sheet.png", 4, 32, 32);
free_all_images();

// スプライト描画
draw_image(target, x, y, handle);
draw_image_ex(target, x, y, handle, DrawSpriteParams {
    scale_x:  2.0,
    scale_y:  2.0,
    rotation: 45.0, // 度数法
    alpha:    0.5,  // 0.0 〜 1.0
    flip_x:   false,
    flip_y:   false,
    ..Default::default()
});

// 図形描画
draw_fill(target, Color::BLACK);                              // 全塗りつぶし
draw_pixel(target, x, y, Color::WHITE);                       // ピクセル
draw_line(target, x1, y1, x2, y2, Color::WHITE);              // 線
draw_rectangle(target, x, y, w, h, Color::RED, true);         // 矩形 (true=塗りつぶし)
draw_circle(target, cx, cy, radius, Color::BLUE, false);      // 円 (false=アウトライン)
draw_triangle(target, x1,y1, x2,y2, x3,y3, Color::GREEN, true);

// 描画キューのクリア (画面をリセット)
clear(target);

// ブレンドモード（次の draw_* コマンドに適用）
set_blend(BlendMode::Add);    // 加算
set_blend(BlendMode::Mul);    // 乗算
set_blend(BlendMode::Normal); // 通常に戻す

// マスク（次の draw_image / draw_image_ex に適用）
set_mask(mx, my, mask_handle); // mask_handle のアルファ値を形状マスクとして使用
reset_mask();
```

### サブスクリーン

オフスクリーンのレンダーターゲットをスプライトハンドルとして利用できます。

```rust
// サブスクリーンを作成（スプライトハンドルが返る）
let screen = create_screen(128, 128);

// サブスクリーンに描画
clear(screen);
draw_rectangle(screen, 0, 0, 128, 128, Color::BLUE, true);
draw_image(screen, 0, 0, player);

// メインウィンドウにサブスクリーンを貼る
draw_image(0, 200, 100, screen);
```

### 入力

```rust
// キーボード
is_pressed(KeyCode::Space)       // 押している間 true
is_just_pressed(KeyCode::Enter)  // 押した瞬間だけ true
is_released(KeyCode::Escape)     // 離した瞬間だけ true

// マウス
mouse_position()                          // (x, y) 仮想スクリーン座標
is_mouse_pressed(MouseButton::Left)       // 押している間
is_mouse_just_pressed(MouseButton::Right) // 押した瞬間
is_mouse_released(MouseButton::Middle)    // 離した瞬間
mouse_wheel()                             // ホイール回転量（上=正、1ノッチ=120）

// ゲームパッド
is_pad_pressed(0, PadButton::South)       // pad_id=0
is_pad_just_pressed(0, PadButton::East)
is_pad_released(0, PadButton::West)
pad_axis(0, PadAxis::LeftStickX)          // -1.0 〜 1.0
is_pad_connected(0)
pad_count() -> usize
```

### サウンド

```rust
let se  = load_sound("jump.wav");
let bgm = load_sound("bgm.ogg");

play_sound(se,  false); // 1 回再生
play_sound(bgm, true);  // ループ再生
stop_sound(bgm);
set_volume(bgm, 0.5);   // 音量 0.0 〜 1.0
free_all_sounds();
```

対応フォーマット: **WAV（PCM 8-bit / 16-bit）**、**OGG Vorbis**

### テキスト

```rust
set_font_size(24);            // デフォルトフォントサイズを変更
set_font_file("font.ttf");    // カスタムフォントファイルを指定

draw_text(target, x, y, "こんにちは", Color::WHITE);

// フォントハンドルを直接指定
let font = load_font("font.ttf", 24);
draw_text_ex(target, x, y, "Hello!", Color::YELLOW, font);
let w = get_text_width("Hello!", font);
```

同一（文字列 + フォント + 色）の組み合わせは GPU テクスチャがキャッシュされます。240 フレーム未使用のエントリは自動削除されます。

### オーバーレイウィンドウ

メインウィンドウの外側を含む全画面に透過描画できるオーバーレイです。

```rust
// WindowConfig で有効化する
init(WindowConfig {
    overlay_enabled: true,
    overlay_visible: true,
    ..Default::default()
});

while advance_frame() {
    // メインウィンドウへの通常描画
    draw_image(0, x, y, player);

    // オーバーレイへの描画（座標はメインウィンドウ基準、範囲外も描ける）
    overlay_draw_image(-50, 100, player);
    overlay_draw_text(-30, 0, "FPS: 300", Color::WHITE);
    overlay_blend_set(BlendMode::Add);
    overlay_draw_image_ex(x, y, glow, DrawSpriteParams { alpha: 0.5, ..Default::default() });
    overlay_blend_set(BlendMode::Normal);
}
```

`draw_*` でメインウィンドウ外座標に描画すると自動的にオーバーレイへフォールバックします。

| 関数 | 説明 |
|---|---|
| `overlay_visible(bool)` | オーバーレイウィンドウの表示/非表示 |
| `overlay_draw_image(x, y, handle)` | スプライトをオーバーレイに描画 |
| `overlay_draw_image_ex(x, y, handle, params)` | 拡張パラメータ付き描画 |
| `overlay_draw_text(x, y, text, color)` | テキストをオーバーレイに描画 |
| `overlay_blend_set(BlendMode)` | オーバーレイのブレンドモードを設定 |
| `overlay_clear()` | オーバーレイの描画キューをクリア |

### デバッグログ

デバッグビルド (`cargo run` / `cargo build`) のみ標準エラーに出力されるログマクロです。リリースビルド (`cargo build --release`) では出力コードが完全に除去されます。

```rust
log_info!("スプライトをロードしました: '{}'", path);  // [情報] スプライトをロードしました: 'player.png'
log_warn!("フォントが見つかりません: '{}'", path);     // [警告] フォントが見つかりません: 'font.ttf'
log_error!("初期化に失敗しました: {}", reason);        // [エラー] 初期化に失敗しました: ...
```

## 内部実装

| 機能 | 実装 |
|---|---|
| グラフィックス | wgpu 27 (DX12) + DirectComposition |
| 画像読み込み | WIC (Windows Imaging Component) |
| サウンド | XAudio2 |
| キーボード / マウス | Win32 メッセージ (WM_KEYDOWN 等) |
| ゲームパッド | XInput |
| テキスト描画 | fontdue + テクスチャキャッシュ |

## プラットフォーム対応

| OS | 対応状況 |
|---|---|
| Windows 10 / 11 | ✅ |
| macOS / Linux | ❌ (Win32 API を直接使用) |

## ライセンス

MIT — [LICENSE](LICENSE) を参照
