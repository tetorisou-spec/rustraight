use std::collections::HashSet;

// ── KeyCode ───────────────────────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    // アルファベット
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    // 数字行
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,
    // テンキー
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4,
    Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadDecimal, NumpadEnter,
    NumLock,
    // ファンクション
    F1,  F2,  F3,  F4,  F5,  F6,
    F7,  F8,  F9,  F10, F11, F12,
    F13, F14, F15, F16, F17, F18,
    F19, F20, F21, F22, F23, F24,
    // カーソル / ナビゲーション
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown, Insert, Delete,
    // 修飾キー
    ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight,
    SuperLeft, SuperRight,
    // 編集
    Backspace, Enter, Tab, Escape, Space, CapsLock,
    // 記号（US 配列）
    Minus, Equal, BracketLeft, BracketRight, Backslash,
    Semicolon, Quote, Backquote, Comma, Period, Slash,
    // その他
    ScrollLock, Pause, PrintScreen,
    Unknown,
}

/// Win32 仮想キーコードを KeyCode にマップする。
/// 修飾キーは wnd_proc 側で L/R 解決済みの VK (0xA0-0xA5) で渡すこと。
/// extended = true のとき NumpadEnter / ControlRight / AltRight を区別する。
pub(crate) fn vk_to_keycode(vk: u16, extended: bool) -> KeyCode {
    match vk {
        0x08 => KeyCode::Backspace,
        0x09 => KeyCode::Tab,
        0x0D => if extended { KeyCode::NumpadEnter } else { KeyCode::Enter },
        0x13 => KeyCode::Pause,
        0x14 => KeyCode::CapsLock,
        0x1B => KeyCode::Escape,
        0x20 => KeyCode::Space,
        0x21 => KeyCode::PageUp,
        0x22 => KeyCode::PageDown,
        0x23 => KeyCode::End,
        0x24 => KeyCode::Home,
        0x25 => KeyCode::ArrowLeft,
        0x26 => KeyCode::ArrowUp,
        0x27 => KeyCode::ArrowRight,
        0x28 => KeyCode::ArrowDown,
        0x2C => KeyCode::PrintScreen,
        0x2D => KeyCode::Insert,
        0x2E => KeyCode::Delete,
        0x30 => KeyCode::Digit0,
        0x31 => KeyCode::Digit1,
        0x32 => KeyCode::Digit2,
        0x33 => KeyCode::Digit3,
        0x34 => KeyCode::Digit4,
        0x35 => KeyCode::Digit5,
        0x36 => KeyCode::Digit6,
        0x37 => KeyCode::Digit7,
        0x38 => KeyCode::Digit8,
        0x39 => KeyCode::Digit9,
        0x41 => KeyCode::KeyA,
        0x42 => KeyCode::KeyB,
        0x43 => KeyCode::KeyC,
        0x44 => KeyCode::KeyD,
        0x45 => KeyCode::KeyE,
        0x46 => KeyCode::KeyF,
        0x47 => KeyCode::KeyG,
        0x48 => KeyCode::KeyH,
        0x49 => KeyCode::KeyI,
        0x4A => KeyCode::KeyJ,
        0x4B => KeyCode::KeyK,
        0x4C => KeyCode::KeyL,
        0x4D => KeyCode::KeyM,
        0x4E => KeyCode::KeyN,
        0x4F => KeyCode::KeyO,
        0x50 => KeyCode::KeyP,
        0x51 => KeyCode::KeyQ,
        0x52 => KeyCode::KeyR,
        0x53 => KeyCode::KeyS,
        0x54 => KeyCode::KeyT,
        0x55 => KeyCode::KeyU,
        0x56 => KeyCode::KeyV,
        0x57 => KeyCode::KeyW,
        0x58 => KeyCode::KeyX,
        0x59 => KeyCode::KeyY,
        0x5A => KeyCode::KeyZ,
        0x5B => KeyCode::SuperLeft,
        0x5C => KeyCode::SuperRight,
        0x60 => KeyCode::Numpad0,
        0x61 => KeyCode::Numpad1,
        0x62 => KeyCode::Numpad2,
        0x63 => KeyCode::Numpad3,
        0x64 => KeyCode::Numpad4,
        0x65 => KeyCode::Numpad5,
        0x66 => KeyCode::Numpad6,
        0x67 => KeyCode::Numpad7,
        0x68 => KeyCode::Numpad8,
        0x69 => KeyCode::Numpad9,
        0x6A => KeyCode::NumpadMultiply,
        0x6B => KeyCode::NumpadAdd,
        0x6D => KeyCode::NumpadSubtract,
        0x6E => KeyCode::NumpadDecimal,
        0x6F => KeyCode::NumpadDivide,
        0x70 => KeyCode::F1,
        0x71 => KeyCode::F2,
        0x72 => KeyCode::F3,
        0x73 => KeyCode::F4,
        0x74 => KeyCode::F5,
        0x75 => KeyCode::F6,
        0x76 => KeyCode::F7,
        0x77 => KeyCode::F8,
        0x78 => KeyCode::F9,
        0x79 => KeyCode::F10,
        0x7A => KeyCode::F11,
        0x7B => KeyCode::F12,
        0x7C => KeyCode::F13,
        0x7D => KeyCode::F14,
        0x7E => KeyCode::F15,
        0x7F => KeyCode::F16,
        0x80 => KeyCode::F17,
        0x81 => KeyCode::F18,
        0x82 => KeyCode::F19,
        0x83 => KeyCode::F20,
        0x84 => KeyCode::F21,
        0x85 => KeyCode::F22,
        0x86 => KeyCode::F23,
        0x87 => KeyCode::F24,
        0x90 => KeyCode::NumLock,
        0x91 => KeyCode::ScrollLock,
        // 修飾キー（wnd_proc で L/R 解決済み）
        0xA0 => KeyCode::ShiftLeft,
        0xA1 => KeyCode::ShiftRight,
        0xA2 => KeyCode::ControlLeft,
        0xA3 => KeyCode::ControlRight,
        0xA4 => KeyCode::AltLeft,
        0xA5 => KeyCode::AltRight,
        // 記号（US 配列）
        0xBA => KeyCode::Semicolon,
        0xBB => KeyCode::Equal,
        0xBC => KeyCode::Comma,
        0xBD => KeyCode::Minus,
        0xBE => KeyCode::Period,
        0xBF => KeyCode::Slash,
        0xC0 => KeyCode::Backquote,
        0xDB => KeyCode::BracketLeft,
        0xDC => KeyCode::Backslash,
        0xDD => KeyCode::BracketRight,
        0xDE => KeyCode::Quote,
        _ => KeyCode::Unknown,
    }
}

// ── Keyboard state ────────────────────────────────────────────────────────────

#[derive(Default)]
struct InputState {
    current:  HashSet<KeyCode>,
    previous: HashSet<KeyCode>,
}

impl InputState {
    fn commit(&mut self) { self.previous.clone_from(&self.current); }
    fn press(&mut self, key: KeyCode)   { self.current.insert(key); }
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

pub fn is_pressed(key: KeyCode) -> bool {
    INPUT.with(|s| s.borrow().current.contains(&key))
}

pub fn is_just_pressed(key: KeyCode) -> bool {
    INPUT.with(|s| { let s = s.borrow(); s.current.contains(&key) && !s.previous.contains(&key) })
}

pub fn is_released(key: KeyCode) -> bool {
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

pub fn is_mouse_pressed(btn: MouseButton) -> bool {
    MOUSE.with(|m| m.borrow().current[btn_idx(btn)])
}

pub fn is_mouse_just_pressed(btn: MouseButton) -> bool {
    MOUSE.with(|m| { let m = m.borrow(); m.current[btn_idx(btn)] && !m.previous[btn_idx(btn)] })
}

pub fn is_mouse_released(btn: MouseButton) -> bool {
    MOUSE.with(|m| { let m = m.borrow(); !m.current[btn_idx(btn)] && m.previous[btn_idx(btn)] })
}
