use std::time::Instant;

struct TimeState {
    last_update: Instant,
    delta_time: f32,
}

impl TimeState {
    fn new() -> Self {
        Self {
            last_update: Instant::now(),
            delta_time: 0.0,
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.delta_time = now.duration_since(self.last_update).as_secs_f32();
        self.last_update = now;
    }
}

thread_local! {
    static TIME: std::cell::RefCell<TimeState> = std::cell::RefCell::new(TimeState::new());
}

pub(crate) fn tick_time() {
    TIME.with(|t| t.borrow_mut().tick());
}

pub(crate) fn get_delta_time() -> f32 {
    TIME.with(|t| t.borrow().delta_time)
}
