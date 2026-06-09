use rustraight::prelude::*;

fn main() {
    init(WindowConfig { title: String::from("rustraight demo"), ..Default::default() });

    let mut dt_holder: Vec<f32> = Vec::new();
    let mut frame_rate = 0.0;

    while advance_frame() {
        let dt = delta_time();
        dt_holder.push(dt);

        if dt_holder.len() == 30 {
            frame_rate = 1.0 / (dt_holder.iter().sum::<f32>() / 30.0);
            dt_holder.clear();
        }

        draw_text(0, 0, 20, format!("delta time: {:.3}", dt), Color::WHITE);
        draw_text(0, 0, 0, format!("frame rate: {:.2}", frame_rate), Color::WHITE);
    }
}
