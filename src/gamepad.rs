const PAD_COUNT:  usize = 4;
const BTN_COUNT:  usize = 16;
const AXIS_COUNT: usize = 6;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PadButton {
    South, East, West, North,
    LBumper, RBumper,
    LTrigger, RTrigger,
    Select, Start,
    LThumb, RThumb,
    DPadUp, DPadDown, DPadLeft, DPadRight,
}

impl PadButton {
    fn index(self) -> usize { self as usize }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PadAxis {
    LeftStickX, LeftStickY,
    RightStickX, RightStickY,
    LeftTrigger, RightTrigger,
}

impl PadAxis {
    fn index(self) -> usize { self as usize }
}

struct PadState {
    connected: bool,
    previous:  [bool; BTN_COUNT],
    current:   [bool; BTN_COUNT],
    axes:      [f32;  AXIS_COUNT],
}

impl PadState {
    fn new() -> Self {
        Self { connected: false, previous: [false; BTN_COUNT], current: [false; BTN_COUNT], axes: [0.0; AXIS_COUNT] }
    }
    fn commit(&mut self) { self.previous = self.current; }
}

pub struct GamepadManager {
    states: [PadState; PAD_COUNT],
}

/// XInput でパッド index 番のボタン/軸状態を読み取る。
/// 接続されていない場合は connected=false を返す。
#[cfg(target_os = "windows")]
fn poll_pad(index: u32) -> (bool, [bool; BTN_COUNT], [f32; AXIS_COUNT]) {
    use windows_sys::Win32::UI::Input::XboxController::*;
    unsafe {
        let mut state = std::mem::zeroed::<XINPUT_STATE>();
        if XInputGetState(index, &mut state) != 0 {
            return (false, [false; BTN_COUNT], [0.0; AXIS_COUNT]);
        }
        let g = state.Gamepad;
        let w = g.wButtons;
        let mut btns = [false; BTN_COUNT];
        btns[PadButton::South     as usize] = w & XINPUT_GAMEPAD_A              != 0;
        btns[PadButton::East      as usize] = w & XINPUT_GAMEPAD_B              != 0;
        btns[PadButton::West      as usize] = w & XINPUT_GAMEPAD_X              != 0;
        btns[PadButton::North     as usize] = w & XINPUT_GAMEPAD_Y              != 0;
        btns[PadButton::LBumper   as usize] = w & XINPUT_GAMEPAD_LEFT_SHOULDER  != 0;
        btns[PadButton::RBumper   as usize] = w & XINPUT_GAMEPAD_RIGHT_SHOULDER != 0;
        btns[PadButton::LTrigger  as usize] = g.bLeftTrigger  > 30;
        btns[PadButton::RTrigger  as usize] = g.bRightTrigger > 30;
        btns[PadButton::Select    as usize] = w & XINPUT_GAMEPAD_BACK           != 0;
        btns[PadButton::Start     as usize] = w & XINPUT_GAMEPAD_START          != 0;
        btns[PadButton::LThumb    as usize] = w & XINPUT_GAMEPAD_LEFT_THUMB     != 0;
        btns[PadButton::RThumb    as usize] = w & XINPUT_GAMEPAD_RIGHT_THUMB    != 0;
        btns[PadButton::DPadUp    as usize] = w & XINPUT_GAMEPAD_DPAD_UP        != 0;
        btns[PadButton::DPadDown  as usize] = w & XINPUT_GAMEPAD_DPAD_DOWN      != 0;
        btns[PadButton::DPadLeft  as usize] = w & XINPUT_GAMEPAD_DPAD_LEFT      != 0;
        btns[PadButton::DPadRight as usize] = w & XINPUT_GAMEPAD_DPAD_RIGHT     != 0;

        let mut axes = [0.0f32; AXIS_COUNT];
        axes[PadAxis::LeftStickX   as usize] = g.sThumbLX     as f32 / 32767.0;
        axes[PadAxis::LeftStickY   as usize] = g.sThumbLY     as f32 / 32767.0;
        axes[PadAxis::RightStickX  as usize] = g.sThumbRX     as f32 / 32767.0;
        axes[PadAxis::RightStickY  as usize] = g.sThumbRY     as f32 / 32767.0;
        axes[PadAxis::LeftTrigger  as usize] = g.bLeftTrigger  as f32 / 255.0;
        axes[PadAxis::RightTrigger as usize] = g.bRightTrigger as f32 / 255.0;

        (true, btns, axes)
    }
}

impl GamepadManager {
    pub fn try_new() -> Option<Self> {
        Some(Self { states: std::array::from_fn(|_| PadState::new()) })
    }

    pub fn commit(&mut self) {
        for s in &mut self.states { s.commit(); }
    }

    pub fn poll(&mut self) {
        #[cfg(target_os = "windows")]
        for i in 0..PAD_COUNT {
            let (connected, btns, axes) = poll_pad(i as u32);
            let s = &mut self.states[i];
            s.connected = connected;
            s.current   = if connected { btns } else { [false; BTN_COUNT] };
            s.axes      = if connected { axes  } else { [0.0;  AXIS_COUNT] };
        }
    }

    pub fn is_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.states.get(pad_id).map(|s| s.connected && s.current[btn.index()]).unwrap_or(false)
    }

    pub fn is_just_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.states.get(pad_id)
            .map(|s| s.connected && s.current[btn.index()] && !s.previous[btn.index()])
            .unwrap_or(false)
    }

    pub fn is_released(&self, pad_id: usize, btn: PadButton) -> bool {
        self.states.get(pad_id)
            .map(|s| s.connected && !s.current[btn.index()] && s.previous[btn.index()])
            .unwrap_or(false)
    }

    pub fn axis(&self, pad_id: usize, axis: PadAxis) -> f32 {
        self.states.get(pad_id)
            .map(|s| if s.connected { s.axes[axis.index()] } else { 0.0 })
            .unwrap_or(0.0)
    }

    pub fn is_connected(&self, pad_id: usize) -> bool {
        self.states.get(pad_id).map(|s| s.connected).unwrap_or(false)
    }

    pub fn count(&self) -> usize {
        self.states.iter().filter(|s| s.connected).count()
    }
}
