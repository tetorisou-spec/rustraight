use std::collections::HashMap;
use gilrs::{Axis as GAxis, Button as GButton, GamepadId, Gilrs};

const PAD_BUTTON_COUNT: usize = 16;
const PAD_AXIS_COUNT:   usize = 6;

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

    fn to_gilrs(self) -> GButton {
        match self {
            PadButton::South     => GButton::South,
            PadButton::East      => GButton::East,
            PadButton::West      => GButton::West,
            PadButton::North     => GButton::North,
            PadButton::LBumper   => GButton::LeftTrigger,
            PadButton::RBumper   => GButton::RightTrigger,
            PadButton::LTrigger  => GButton::LeftTrigger2,
            PadButton::RTrigger  => GButton::RightTrigger2,
            PadButton::Select    => GButton::Select,
            PadButton::Start     => GButton::Start,
            PadButton::LThumb    => GButton::LeftThumb,
            PadButton::RThumb    => GButton::RightThumb,
            PadButton::DPadUp    => GButton::DPadUp,
            PadButton::DPadDown  => GButton::DPadDown,
            PadButton::DPadLeft  => GButton::DPadLeft,
            PadButton::DPadRight => GButton::DPadRight,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PadAxis {
    LeftStickX, LeftStickY,
    RightStickX, RightStickY,
    LeftTrigger, RightTrigger,
}

impl PadAxis {
    fn index(self) -> usize { self as usize }

    fn to_gilrs(self) -> GAxis {
        match self {
            PadAxis::LeftStickX   => GAxis::LeftStickX,
            PadAxis::LeftStickY   => GAxis::LeftStickY,
            PadAxis::RightStickX  => GAxis::RightStickX,
            PadAxis::RightStickY  => GAxis::RightStickY,
            PadAxis::LeftTrigger  => GAxis::LeftZ,
            PadAxis::RightTrigger => GAxis::RightZ,
        }
    }
}

struct PadState {
    connected: bool,
    previous:  [bool; PAD_BUTTON_COUNT],
    current:   [bool; PAD_BUTTON_COUNT],
    axes:      [f32;  PAD_AXIS_COUNT],
}

impl PadState {
    fn new(connected: bool) -> Self {
        Self {
            connected,
            previous: [false; PAD_BUTTON_COUNT],
            current:  [false; PAD_BUTTON_COUNT],
            axes:     [0.0;   PAD_AXIS_COUNT],
        }
    }

    fn commit(&mut self) {
        self.previous = self.current;
    }
}

pub struct GamepadManager {
    gilrs:      Gilrs,
    pad_order:  Vec<GamepadId>,
    pad_states: HashMap<GamepadId, PadState>,
}

impl GamepadManager {
    pub fn try_new() -> Option<Self> {
        let gilrs = Gilrs::new().ok()?;
        let mut pad_order  = Vec::new();
        let mut pad_states = HashMap::new();
        for (id, _) in gilrs.gamepads() {
            pad_order.push(id);
            pad_states.insert(id, PadState::new(true));
        }
        Some(Self { gilrs, pad_order, pad_states })
    }

    // Call at the start of advance_frame — snapshot current into previous.
    pub fn commit(&mut self) {
        for state in self.pad_states.values_mut() {
            state.commit();
        }
    }

    // Call at the end of advance_frame — drain events then read current state.
    pub fn poll(&mut self) {
        while let Some(event) = self.gilrs.next_event() {
            let id = event.id;
            match event.event {
                gilrs::EventType::Connected => {
                    if !self.pad_order.contains(&id) {
                        self.pad_order.push(id);
                    }
                    self.pad_states
                        .entry(id)
                        .or_insert_with(|| PadState::new(false))
                        .connected = true;
                }
                gilrs::EventType::Disconnected => {
                    if let Some(state) = self.pad_states.get_mut(&id) {
                        state.connected = false;
                        state.current   = [false; PAD_BUTTON_COUNT];
                        state.axes      = [0.0;   PAD_AXIS_COUNT];
                    }
                }
                _ => {}
            }
        }

        const ALL_BUTTONS: [PadButton; PAD_BUTTON_COUNT] = [
            PadButton::South, PadButton::East, PadButton::West, PadButton::North,
            PadButton::LBumper, PadButton::RBumper, PadButton::LTrigger, PadButton::RTrigger,
            PadButton::Select, PadButton::Start, PadButton::LThumb, PadButton::RThumb,
            PadButton::DPadUp, PadButton::DPadDown, PadButton::DPadLeft, PadButton::DPadRight,
        ];
        const ALL_AXES: [PadAxis; PAD_AXIS_COUNT] = [
            PadAxis::LeftStickX, PadAxis::LeftStickY,
            PadAxis::RightStickX, PadAxis::RightStickY,
            PadAxis::LeftTrigger, PadAxis::RightTrigger,
        ];

        let connected_ids: Vec<GamepadId> = self.pad_states.iter()
            .filter(|(_, s)| s.connected)
            .map(|(&id, _)| id)
            .collect();

        for id in connected_ids {
            let (current, axes) = {
                let pad = self.gilrs.gamepad(id);
                let mut current = [false; PAD_BUTTON_COUNT];
                let mut axes    = [0.0f32; PAD_AXIS_COUNT];
                for &btn in &ALL_BUTTONS {
                    current[btn.index()] = pad.is_pressed(btn.to_gilrs());
                }
                for &axis in &ALL_AXES {
                    axes[axis.index()] = pad.value(axis.to_gilrs());
                }
                (current, axes)
            };
            if let Some(state) = self.pad_states.get_mut(&id) {
                state.current = current;
                state.axes    = axes;
            }
        }
    }

    fn state(&self, pad_id: usize) -> Option<&PadState> {
        self.pad_order.get(pad_id).and_then(|id| self.pad_states.get(id))
    }

    pub fn is_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.state(pad_id).map(|s| s.connected && s.current[btn.index()]).unwrap_or(false)
    }

    pub fn is_just_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.state(pad_id)
            .map(|s| s.connected && s.current[btn.index()] && !s.previous[btn.index()])
            .unwrap_or(false)
    }

    pub fn is_released(&self, pad_id: usize, btn: PadButton) -> bool {
        self.state(pad_id)
            .map(|s| s.connected && !s.current[btn.index()] && s.previous[btn.index()])
            .unwrap_or(false)
    }

    pub fn axis(&self, pad_id: usize, axis: PadAxis) -> f32 {
        self.state(pad_id)
            .map(|s| if s.connected { s.axes[axis.index()] } else { 0.0 })
            .unwrap_or(0.0)
    }

    pub fn is_connected(&self, pad_id: usize) -> bool {
        self.state(pad_id).map(|s| s.connected).unwrap_or(false)
    }

    pub fn count(&self) -> usize {
        self.pad_states.values().filter(|s| s.connected).count()
    }
}
