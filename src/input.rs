use std::collections::HashSet;
pub use winit::keyboard::KeyCode;

// ── Keyboard ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct InputState {
    current:  HashSet<KeyCode>,
    previous: HashSet<KeyCode>,
}

impl InputState {
    fn commit(&mut self) { self.previous.clone_from(&self.current); }
    fn press(&mut self, key: KeyCode) { self.current.insert(key); }
    fn release(&mut self, key: KeyCode) { self.current.remove(&key); }
}

thread_local! {
    static INPUT: std::cell::RefCell<InputState> = std::cell::RefCell::new(InputState::default());
}

pub(crate) fn process_key_event(key: KeyCode, pressed: bool) {
    INPUT.with(|s| { let mut s = s.borrow_mut(); if pressed { s.press(key); } else { s.release(key); } });
}

pub(crate) fn commit_input() {
    INPUT.with(|s| s.borrow_mut().commit());
}

pub(crate) fn is_pressed(key: KeyCode) -> bool {
    INPUT.with(|s| s.borrow().current.contains(&key))
}

pub(crate) fn is_just_pressed(key: KeyCode) -> bool {
    INPUT.with(|s| { let s = s.borrow(); s.current.contains(&key) && !s.previous.contains(&key) })
}

pub(crate) fn is_released(key: KeyCode) -> bool {
    INPUT.with(|s| { let s = s.borrow(); !s.current.contains(&key) && s.previous.contains(&key) })
}

// ── Mouse ─────────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum MouseButton { Left, Right, Middle }

struct MouseState {
    x:        i32,
    y:        i32,
    current:  [bool; 3],
    previous: [bool; 3],
}

impl Default for MouseState {
    fn default() -> Self { Self { x: 0, y: 0, current: [false; 3], previous: [false; 3] } }
}

fn btn_idx(b: MouseButton) -> usize {
    match b { MouseButton::Left => 0, MouseButton::Right => 1, MouseButton::Middle => 2 }
}

thread_local! {
    static MOUSE: std::cell::RefCell<MouseState> = std::cell::RefCell::new(MouseState::default());
}

pub(crate) fn commit_mouse_input() {
    MOUSE.with(|m| { let mut m = m.borrow_mut(); m.previous = m.current; });
}

pub(crate) fn process_mouse_move(x: i32, y: i32) {
    MOUSE.with(|m| { let mut m = m.borrow_mut(); m.x = x; m.y = y; });
}

pub(crate) fn process_mouse_button(btn: MouseButton, pressed: bool) {
    MOUSE.with(|m| { m.borrow_mut().current[btn_idx(btn)] = pressed; });
}

pub(crate) fn mouse_position() -> (i32, i32) {
    MOUSE.with(|m| { let m = m.borrow(); (m.x, m.y) })
}

pub(crate) fn is_mouse_pressed(btn: MouseButton) -> bool {
    MOUSE.with(|m| m.borrow().current[btn_idx(btn)])
}

pub(crate) fn is_mouse_just_pressed(btn: MouseButton) -> bool {
    MOUSE.with(|m| { let m = m.borrow(); m.current[btn_idx(btn)] && !m.previous[btn_idx(btn)] })
}

pub(crate) fn is_mouse_released(btn: MouseButton) -> bool {
    MOUSE.with(|m| { let m = m.borrow(); !m.current[btn_idx(btn)] && m.previous[btn_idx(btn)] })
}
