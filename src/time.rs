use std::time::Instant;

struct TimeState {
    start:        Instant,
    last_update:  Instant,
    delta_time:   f32,
    elapsed_secs: f64,
}

impl TimeState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            start:        now,
            last_update:  now,
            delta_time:   0.0,
            elapsed_secs: 0.0,
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.delta_time   = now.duration_since(self.last_update).as_secs_f32();
        self.elapsed_secs = now.duration_since(self.start).as_secs_f64();
        self.last_update  = now;
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

pub(crate) fn get_elapsed_secs() -> f64 {
    TIME.with(|t| t.borrow().elapsed_secs)
}
