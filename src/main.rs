use rustraight::prelude::*;

fn main() {
    // ウィンドウ初期化
    init(WindowConfig {
        title: String::from("rustraight demo"),
        screen_width: 320,
        screen_height: 240,
        ..Default::default()
    });

    let screen = create_screen(320, 240);
    draw_fill(screen, Color::RED);

    while advance_frame() {
        draw_image(MAIN_SCREEN,0,0,screen);
    }

    free_all_images();
}
