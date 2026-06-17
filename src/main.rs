use rustraight::prelude::*;

fn main() {
    init(WindowConfig {
        title: String::from("rustraight demo"),
        transparent: true,
        overlay_enabled: true,
        ..Default::default()
    });

    let mut old_mx: i32 = 0;
    let mut old_my: i32 = 0;
    let mut y = 0;

    while advance_frame() {
        draw_rectangle(MAIN_SCREEN, -50, y, 100, 100, Color::RED, true);
        if is_mouse_just_pressed(MouseButton::Left) {
            (old_mx, old_my) = mouse_position();
        }
        else if is_mouse_pressed(MouseButton::Left) {
            let (mx, my) = mouse_position();
            let (wx, wy) = window_position();
            set_window_position(wx + (mx - old_mx), wy + (my - old_my));
        }
        y += 3;
    }
}
