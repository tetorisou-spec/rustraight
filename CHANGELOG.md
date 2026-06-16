# Changelog

## [0.4.2] - 2026-06-16

### 追加

- **`window_size() -> (u32, u32)`** — ウィンドウのクライアント領域サイズを返す
- **`screen_size() -> (u32, u32)`** — 仮想解像度（スクリーンレンダーターゲット）のサイズを返す
- **`image_size(handle: u32) -> (u32, u32)`** — スプライトまたはサブスクリーンのサイズを返す。`load_image` / `create_screen` 両方のハンドルに対応。無効ハンドルは `(0, 0)`

---

## [0.4.1] - 2026-06-15

### 追加

- **`mouse_wheel() -> i32`** — マウスホイールの回転量を取得（上方向が正、1 ノッチ = 120）。フレームごとにリセットされる
- **`show_cursor(visible: bool)`** — マウスカーソルの表示 / 非表示を切り替える
- **`set_window_size(w: u32, h: u32)`** — ウィンドウのクライアント領域を実行中に指定ピクセルサイズへ変更する
- **`set_screen_size(w: u16, h: u16)`** — 仮想解像度（スクリーンレンダーターゲット）を `init()` 後に動的変更する。変更時はオフスクリーンテクスチャを再生成する

---

## [0.4.0] - 2026-06-09

### 破壊的変更

API を DXLib スタイルのグローバル関数に全面移行。0.3.x との互換性はありません。

- **`Window` 構造体を廃止** — すべての操作がグローバル関数になった
- **初期化**: `Window::default()` + `.init()` → `init(WindowConfig { .. })`
- **メインループ**: `window.advance_frame()` → `advance_frame()`
- **描画**: `window.screen_draw_*` → グローバル `draw_*` + 第1引数 `target: u32`（`0` = メインウィンドウ、それ以外 = サブスクリーン）
- **入力**: `window.is_pressed()` など → グローバル `is_pressed()` など
- **タイム**: `window.delta_time()` → `delta_time()`
- **サブスクリーン**: `Screen` 構造体廃止 → `create_screen(w, h) -> u32` がスプライトハンドルを返す。描画は `draw_image(target, ..)` で統一
- **命名統一**: `load_graph` → `load_image`、`load_div_graph` → `load_div_image`、`free_all_graphs` → `free_all_images`、`screen_draw_sprite` → `draw_image`
- **オーバーレイ有効化**: `window.overlay_enable(true)` → `WindowConfig { overlay_enabled: true, .. }`
- **ブレンド・マスク**: `window.screen_blend_set` → `set_blend`、`window.screen_mask_set` → `set_mask`、`window.screen_mask_reset` → `reset_mask`（target 引数なし）
- **src/screen.rs 削除** — `DrawCommand` に統合

### 追加

- `create_screen(w: u16, h: u16) -> u32` — オフスクリーンレンダーターゲットをスプライトとして作成
- `clear(target: u32)` — 指定ターゲットの描画キューをクリア（`0` = ウィンドウ）
- `src/util.rs` — `slice_as_bytes` / `block_on` を共通ユーティリティとして切り出し
- `src/window/shaders.rs` — WGSL シェーダー定数を分離（window/mod.rs を軽量化）

### 改善

- **テキストキャッシュ** — `draw_text` / `draw_text_ex` の呼び出しごとに GPU テクスチャを生成していた処理を廃止。同一（文字列 + フォント + 色）の組み合わせを `text_cache` に保持し、初出フレームのみ `build_text_bitmap()` + `create_texture()` を実行。240 フレーム未使用のエントリを自動削除（LRU 方式）。テキストが多い場面でのフレームタイムが大幅に改善

---

## [0.3.3] - 2026-06-03

### 追加
- デバッグログマクロを追加 (`src/log.rs`)
  - `log_info!(...)` — 情報ログ（`[情報]` プレフィックス）
  - `log_warn!(...)` — 警告ログ（`[警告]` プレフィックス）
  - `log_error!(...)` — エラーログ（`[エラー]` プレフィックス）
  - デバッグビルドのみ標準エラーに出力。リリースビルドでは出力コードが完全に除去される
  - `use rustraight::prelude::*` で使用可能

### 変更
- ライブラリ内部の `eprintln!` をすべて上記ログマクロに置き換え、メッセージを日本語化
  - 対象: `gamepad.rs` / `graphics.rs` / `sound.rs` / `text.rs` / `window.rs`

### 修正
- コンパイラ警告をすべて解消
  - `unsafe fn` 内での unsafe 呼び出しに `unsafe {}` ブロックを追加 (`window.rs`)
  - 未使用フィールドに `#[allow(dead_code)]` を付与 (`window.rs`)
  - 未使用関数に `#[allow(dead_code)]` を付与 (`window.rs`)
  - 未使用変数の `mut` 除去・`_` プレフィックス付与 (`window.rs`)
  - `JoyState` 構造体に `#[allow(non_snake_case)]` を付与 (`gamepad.rs`)

---

## [0.3.2] - 2026-06-03

### 追加
- `Window::topmost(bool)` — ウィンドウを常に最前面に固定するオプションを追加
- サウンド: OGG Vorbis 形式のロード・再生に対応（lewton クレート使用）

### 変更
- `load_sound()` が `.ogg` 拡張子を自動判別して OGG デコーダを使用するよう変更
- topmost ウィンドウ使用時もオーバーレイが最前面を維持するよう修正

---

## [0.3.1] - 2026-06-03

### 追加
- `GamepadManager` の公開 API を整備:
  - `try_new(hwnd)` — DirectInput マネージャの初期化
  - `commit()` / `poll()` — フレーム状態の更新
  - `is_pressed(pad_id, btn)` / `is_just_pressed(pad_id, btn)` / `is_released(pad_id, btn)`
  - `axis(pad_id, axis)` — スティック・トリガー軸の取得
  - `is_connected(pad_id)` / `count()` — 接続状態の確認

---

## [0.3.0] - 2026-06-03

### 追加
- 入力関数をライブラリ外から直接呼び出せるよう公開 (`pub`):
  - `KeyCode` 列挙型
  - `is_pressed`, `is_just_pressed`, `is_released`
  - `is_mouse_pressed`, `is_mouse_just_pressed`, `is_mouse_released`

### 変更
- **ゲームパッドバックエンドを刷新**: `gilrs` → DirectInput (Windows ネイティブ)
  - 最大 4 パッド同時接続に対応
- **サウンドバックエンドを刷新**: `rodio` → XAudio2 (Windows ネイティブ)
  - Windows Vista 以降のオーディオスタックをネイティブ利用
- **画像ローダーを刷新**: `image` クレート → WIC (Windows Imaging Component)
  - PNG / JPEG / BMP / TIFF / GIF / WebP など WIC 対応フォーマット全般をサポート

---

## [0.2.1] - 2026-06-02

### 変更
- `advance_frame()` の vsync をデフォルト有効 (`true`) に変更
- `overlay_clear()` はフレーム末尾に自動実行されるため、通常は呼び出し不要に変更（ドキュメント更新）

---

## [0.2.0] - 2026-05-30

### 追加
- **オーバーレイウィンドウ** (Windows 専用): スクリーン全体に重なる透過レイヤーウィンドウ
  - `overlay_enable(bool)` — `init()` 前に呼び出してオーバーレイを有効化
  - `overlay_visible(bool)` — 表示 / 非表示の切り替え
  - `overlay_clear()` — 描画キューのクリア
  - `overlay_blend_set(BlendMode)` — ブレンドモード設定
  - `overlay_draw_sprite(x, y, handle)` — スプライト描画
  - `overlay_draw_sprite_ex(x, y, handle, params)` — 拡張パラメータ付きスプライト描画
  - `overlay_draw_text(x, y, text, color)` — テキスト描画
- オーバーレイ内部実装:
  - Win32 ウィンドウを winit を経由せず直接生成
  - 非同期ダブルバッファ readback + GDI バックグラウンドスレッドで `UpdateLayeredWindow` を非同期実行
  - フレームごとの描画コマンドハッシュにより変化がない場合は GPU レンダリングをスキップ

---

## [0.1.2] - 2026-05-24

### 追加
- スプライトのバインドグループキャッシュ (`sprite_bg_cache`) を追加し、同一テクスチャ+マスク組み合わせの再生成を回避

### 変更
- スプライトのテクスチャフォーマットを `Rgba8Unorm` → `Rgba8UnormSrgb` に変更（sRGB 色空間で正確な色再現）

---

## [0.1.1] - 2026-05-24

### 追加
- `Window::elapsed_time() -> f64` — アプリ起動からの経過秒数を取得する API を追加

---

## [0.1.0] - 2026-05-24

### 初回リリース
- ウィンドウの生成・管理 (`Window`)
- スプライト描画: `load_graph`, `load_div_graph`, `free_all_graphs`
- 画面描画 API: `screen_clear`, `screen_draw_sprite`, `screen_draw_sprite_ex`, `screen_draw_text`
- 図形描画: 塗りつぶし矩形・線・円・三角形
- キーボード入力: `is_pressed`, `is_just_pressed`, `is_released`
- マウス入力: ボタン状態・座標取得
- ゲームパッド入力 (gilrs ベース): ボタン・スティック軸
- サウンド (rodio ベース): `load_sound`, `play_sound`, `stop_sound`, `set_volume`
- タイマー: `delta_time`
- フォント設定: `font_file`, `font_size`
