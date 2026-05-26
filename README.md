# rustraight

[![Crates.io](https://img.shields.io/crates/v/rustraight)](https://crates.io/crates/rustraight)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A simple 2D game library for Rust, inspired by [DXLib](https://dxlib.xsrv.jp/).

rustraight is built on [wgpu](https://wgpu.rs/) and [winit](https://github.com/rust-windowing/winit), providing a beginner-friendly API for making 2D games without dealing with low-level GPU details.

## Features

- **Window & virtual screen** — Create a window with an independent virtual resolution that scales to any window size
- **Sprite rendering** — Load PNG images and draw them with scale, rotation, alpha, and flip
- **Sprite sheets** — Split a sprite sheet into individual sprites with `load_div_graph`
- **Shape drawing** — Pixels, lines, rectangles, circles, triangles (filled or outline)
- **Blend modes** — Normal, Additive, Multiply
- **Sub-screen** — Off-screen render targets you can draw onto and use as sprites
- **Keyboard & mouse input** — Pressed / just-pressed / released state for every key and button
- **Gamepad input** — Buttons and analog sticks via [gilrs](https://gitlab.com/gilrs-project/gilrs)
- **Sound** — Load and play WAV, OGG, MP3, and FLAC files via [rodio](https://github.com/RustAudio/rodio)
- **Text rendering** — Draw text using system fonts or a custom TTF/OTF file via [fontdue](https://github.com/mooman219/fontdue)
- **Delta time & elapsed time** — Frame-rate independent movement out of the box

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
rustraight = "0.1"
```

## Quick Start

```rust
use rustraight::prelude::*;

fn main() {
    // 1. Configure window
    let mut window = Window::default();
    window.title("My Game");
    window.size(800, 600);
    window.screen_size(320, 240); // virtual resolution
    window.vsync(true);
    window.init();

    // 2. Load assets
    let player = load_graph("player.png");
    let bgm    = load_sound("bgm.ogg");
    play_sound(bgm, true); // loop

    let mut x = 0i32;
    let mut y = 0i32;
    let speed = 100.0f32;

    // 3. Main loop
    while window.advance_frame() {
        let dt = window.delta_time();

        if window.is_pressed(KeyCode::ArrowLeft)  { x -= (speed * dt) as i32; }
        if window.is_pressed(KeyCode::ArrowRight) { x += (speed * dt) as i32; }
        if window.is_pressed(KeyCode::ArrowUp)    { y -= (speed * dt) as i32; }
        if window.is_pressed(KeyCode::ArrowDown)  { y += (speed * dt) as i32; }

        window.screen_clear();
        window.screen_draw_sprite(x, y, player);
        window.screen_draw_text(0, 0, format!("pos: ({x}, {y})"), Color::WHITE);
    }

    // 4. Free resources
    free_all_sounds();
    free_all_graphs();
}
```

## API Overview

### Window

```rust
let mut window = Window::default();
window.title("title");
window.size(800, 600);           // window size in pixels
window.screen_size(320, 240);    // virtual screen resolution
window.resizable(true);
window.vsync(true);
window.init();                   // open the window

window.advance_frame() -> bool   // returns false when the window is closed
window.delta_time()   -> f32     // seconds since last frame
window.elapsed_time() -> f64     // seconds since window creation
```

### Graphics

```rust
// Load
let spr:  u32     = load_graph("image.png");
let srps: [u32; N] = load_div_graph("sheet.png", N, tile_w, tile_h);
free_all_graphs();

// Draw
window.screen_clear();
window.screen_draw_sprite(x, y, handle);
window.screen_draw_sprite_ex(x, y, handle, DrawSpriteParams {
    scale_x: 2.0, scale_y: 2.0,
    rotation: 45.0, // degrees
    alpha: 0.5,
    flip_x: false, flip_y: false,
    ..Default::default()
});

// Shapes
window.screen_draw_fill(Color::BLACK);
window.screen_draw_rectangle(x, y, w, h, Color::RED, true);
window.screen_draw_circle(cx, cy, radius, Color::BLUE, false);
window.screen_draw_line(x1, y1, x2, y2, Color::WHITE);
window.screen_draw_triangle(x1,y1, x2,y2, x3,y3, Color::GREEN, true);

// Blend mode
window.screen_blend_set(BlendMode::Add);
window.screen_draw_sprite(x, y, glow);
window.screen_blend_set(BlendMode::Normal);

// Sub-screen
let mut screen = window.create_screen(64, 64);
screen.clear();
screen.draw_sprite(0, 0, spr);
window.screen_draw_sprite(100, 100, screen.handle());
```

### Input

```rust
// Keyboard
window.is_pressed(KeyCode::Space)       // held down
window.is_just_pressed(KeyCode::Enter)  // pressed this frame
window.is_released(KeyCode::Escape)     // released this frame

// Mouse
window.mouse_position()                 // (x, y) in virtual screen coords
window.is_mouse_pressed(MouseButton::Left)
window.is_mouse_just_pressed(MouseButton::Right)

// Gamepad
window.is_pad_pressed(0, PadButton::South)
window.pad_axis(0, PadAxis::LeftStickX) // -1.0 .. 1.0
window.is_pad_connected(0)
```

### Sound

```rust
let se  = load_sound("jump.wav");
let bgm = load_sound("bgm.ogg");

play_sound(se,  false); // play once
play_sound(bgm, true);  // loop
stop_sound(bgm);
set_volume(bgm, 0.5);   // 0.0 .. 1.0
free_all_sounds();
```

### Text

```rust
window.font_size(16);             // use system font at size 16
window.font_file("myfont.ttf");   // or load a custom font file

window.screen_draw_text(x, y, "Hello!", Color::WHITE);

// With explicit font handle
let font = load_font("myfont.ttf", 24);
window.screen_draw_text_ex(x, y, "Hello!", Color::YELLOW, font);
let w = get_text_width("Hello!", font);
```

## Platform Support

rustraight has been tested on Windows. It should work on macOS and Linux (both are supported by wgpu and winit) but has not been verified.

## License

MIT — see [LICENSE](LICENSE)
