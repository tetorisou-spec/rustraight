const MAX_XINPUT: usize = 4;
const MAX_DINPUT: usize = 4;
const BTN_COUNT:  usize = 16;
const AXIS_COUNT: usize = 6;
/// DirectInput デバイスの再列挙インターバル（フレーム数）
const DINPUT_REENUM_FRAMES: u32 = 300;

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

// ── 内部ステート ────────────────────────────────────────────────────────────────

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

// ── 仮想パッド参照 (内部用) ─────────────────────────────────────────────────────

/// virtual_pads[外部pad_id] がどのバックエンドのどのスロットを指すか。
#[derive(Copy, Clone)]
enum VPad {
    XInput(usize),
    #[cfg(target_os = "windows")]
    DInput(usize),
}

// ── DirectInput コンテキスト ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
struct DInputDev {
    guid:   windows::core::GUID,
    name:   String,
    device: windows::Win32::Devices::HumanInterfaceDevice::IDirectInputDevice8W,
    state:  PadState,
}

#[cfg(target_os = "windows")]
unsafe impl Send for DInputDev {}

#[cfg(target_os = "windows")]
struct DInputMgr {
    dinput:  windows::Win32::Devices::HumanInterfaceDevice::IDirectInput8W,
    hwnd:    isize,
    devices: Vec<DInputDev>,
    counter: u32,
}

#[cfg(target_os = "windows")]
unsafe impl Send for DInputMgr {}

// ── GamepadManager ──────────────────────────────────────────────────────────────

pub struct GamepadManager {
    xinput_states: [PadState; MAX_XINPUT],
    #[cfg(target_os = "windows")]
    dinput_mgr:    Option<DInputMgr>,
    /// pad_id 0-3 の割り当てテーブル。
    /// 全スロットが切断されるとリセットされ、次の接続から 0 番に詰め直される。
    slots: [Option<VPad>; 4],
}

impl GamepadManager {
    pub fn try_new(hwnd: isize) -> Option<Self> {
        #[cfg(target_os = "windows")]
        let dinput_mgr = unsafe { init_dinput_mgr(hwnd) };

        Some(Self {
            xinput_states: std::array::from_fn(|_| PadState::new()),
            #[cfg(target_os = "windows")]
            dinput_mgr,
            slots: [None; 4],
        })
    }

    pub fn commit(&mut self) {
        for s in &mut self.xinput_states { s.commit(); }
        #[cfg(target_os = "windows")]
        if let Some(mgr) = &mut self.dinput_mgr {
            for dev in &mut mgr.devices { dev.state.commit(); }
        }
    }

    pub fn poll(&mut self) {
        // ── XInput スロット 0-3 ────────────────────────────────────────────
        #[cfg(target_os = "windows")]
        for i in 0..MAX_XINPUT {
            let (conn, btns, axes) = poll_xinput(i as u32);
            let s = &mut self.xinput_states[i];
            s.connected = conn;
            s.current   = if conn { btns } else { [false; BTN_COUNT] };
            s.axes      = if conn { axes  } else { [0.0;  AXIS_COUNT] };
        }

        // ── DirectInput デバイス ───────────────────────────────────────────
        #[cfg(target_os = "windows")]
        if let Some(mgr) = &mut self.dinput_mgr {
            // 定期的に新デバイスを列挙
            mgr.counter += 1;
            if mgr.counter >= DINPUT_REENUM_FRAMES {
                mgr.counter = 0;
                unsafe { reenum_dinput(mgr); }
            }
            // 各デバイスをポール
            for dev in &mut mgr.devices {
                unsafe { poll_dinput_dev(dev); }
            }
        }

        // ── 新規接続デバイスに pad_id を割り当て（既存割り当ては変更しない）
        self.assign_new_pads();
    }

    /// 割り当て済みスロットが全て切断状態かを返す（未割り当て None は無視）。
    fn all_assigned_disconnected(&self) -> bool {
        let mut has_assigned = false;
        for slot in &self.slots {
            let connected = match slot {
                None => continue,
                Some(VPad::XInput(i)) => self.xinput_states[*i].connected,
                #[cfg(target_os = "windows")]
                Some(VPad::DInput(i)) => self.dinput_mgr.as_ref()
                    .map(|m| m.devices[*i].state.connected)
                    .unwrap_or(false),
            };
            has_assigned = true;
            if connected { return false; }
        }
        has_assigned // 1 台以上が割り当て済み かつ 全て切断
    }

    /// 未割り当ての接続済みデバイスを空きスロットに前詰めする。
    /// 割り当て済みスロットが全て切断になった場合はリセットして再割り当て。
    fn assign_new_pads(&mut self) {
        if self.all_assigned_disconnected() {
            self.slots = [None; 4];
        }

        // XInput (スロット番号順)
        'xi: for i in 0..MAX_XINPUT {
            if !self.xinput_states[i].connected { continue; }
            // 既に割り当て済みならスキップ
            if self.slots.iter().any(|s| matches!(s, Some(VPad::XInput(j)) if *j == i)) { continue; }
            // 空きスロットに割り当て
            for slot in &mut self.slots {
                if slot.is_none() { *slot = Some(VPad::XInput(i)); continue 'xi; }
            }
        }

        // DirectInput (デバイスインデックス順)
        #[cfg(target_os = "windows")]
        {
            let n = self.dinput_mgr.as_ref().map(|m| m.devices.len()).unwrap_or(0);
            'd: for i in 0..n {
                let conn = self.dinput_mgr.as_ref()
                    .map(|m| m.devices[i].state.connected)
                    .unwrap_or(false);
                if !conn { continue; }
                if self.slots.iter().any(|s| matches!(s, Some(VPad::DInput(j)) if *j == i)) { continue; }
                for slot in &mut self.slots {
                    if slot.is_none() { *slot = Some(VPad::DInput(i)); continue 'd; }
                }
            }
        }
    }

    fn get_state(&self, pad_id: usize) -> Option<&PadState> {
        if pad_id >= 4 { return None; }
        let s = match self.slots[pad_id].as_ref()? {
            VPad::XInput(i) => &self.xinput_states[*i],
            #[cfg(target_os = "windows")]
            VPad::DInput(i) => &self.dinput_mgr.as_ref()?.devices[*i].state,
        };
        if s.connected { Some(s) } else { None }
    }

    pub fn is_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.get_state(pad_id).map(|s| s.current[btn.index()]).unwrap_or(false)
    }

    pub fn is_just_pressed(&self, pad_id: usize, btn: PadButton) -> bool {
        self.get_state(pad_id)
            .map(|s| s.current[btn.index()] && !s.previous[btn.index()])
            .unwrap_or(false)
    }

    pub fn is_released(&self, pad_id: usize, btn: PadButton) -> bool {
        self.get_state(pad_id)
            .map(|s| !s.current[btn.index()] && s.previous[btn.index()])
            .unwrap_or(false)
    }

    pub fn axis(&self, pad_id: usize, axis: PadAxis) -> f32 {
        self.get_state(pad_id).map(|s| s.axes[axis.index()]).unwrap_or(0.0)
    }

    pub fn is_connected(&self, pad_id: usize) -> bool {
        if pad_id >= 4 { return false; }
        match self.slots[pad_id].as_ref() {
            None => false,
            Some(VPad::XInput(i)) => self.xinput_states[*i].connected,
            #[cfg(target_os = "windows")]
            Some(VPad::DInput(i)) => self.dinput_mgr.as_ref()
                .map(|m| m.devices[*i].state.connected)
                .unwrap_or(false),
        }
    }

    /// 現在接続中のコントローラー数（スロット 0-3 の中で接続中のもの）。
    pub fn count(&self) -> usize {
        (0..4).filter(|&id| self.is_connected(id)).count()
    }
}

// ── XInput ポール ───────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn poll_xinput(index: u32) -> (bool, [bool; BTN_COUNT], [f32; AXIS_COUNT]) {
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

// ── DirectInput 定数・型 ────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
const DIDFT_AXIS_OPT: u32   = 0x80FFFF03;
#[cfg(target_os = "windows")]
const DIDFT_POV_OPT: u32    = 0x80FFFF10;
#[cfg(target_os = "windows")]
const DIDFT_BUTTON_OPT: u32 = 0x80FFFF0C;
#[cfg(target_os = "windows")]
const DIPROP_RANGE_PTR: *const windows::core::GUID = 4 as *const windows::core::GUID;

/// DirectInput から読み取るジョイスティック状態 (DIJOYSTATE 相当、80 バイト)。
#[repr(C)]
#[cfg(target_os = "windows")]
struct JoyState {
    lX:         i32,
    lY:         i32,
    lZ:         i32,
    lRx:        i32,
    lRy:        i32,
    lRz:        i32,
    rglSlider:  [i32; 2],
    rgdwPOV:    [u32; 4],
    rgbButtons: [u8; 32],
}

// ── DirectInput データフォーマット構築 ─────────────────────────────────────────

#[cfg(target_os = "windows")]
fn build_joystick_odfs() -> Vec<windows::Win32::Devices::HumanInterfaceDevice::DIOBJECTDATAFORMAT> {
    use windows::Win32::Devices::HumanInterfaceDevice::DIOBJECTDATAFORMAT;
    let null = core::ptr::null::<windows::core::GUID>();
    let mut v = Vec::with_capacity(44);
    for off in [0u32, 4, 8, 12, 16, 20] {
        v.push(DIOBJECTDATAFORMAT { pguid: null, dwOfs: off, dwType: DIDFT_AXIS_OPT, dwFlags: 0 });
    }
    for off in [24u32, 28] {
        v.push(DIOBJECTDATAFORMAT { pguid: null, dwOfs: off, dwType: DIDFT_AXIS_OPT, dwFlags: 0 });
    }
    for off in [32u32, 36, 40, 44] {
        v.push(DIOBJECTDATAFORMAT { pguid: null, dwOfs: off, dwType: DIDFT_POV_OPT, dwFlags: 0 });
    }
    for i in 0u32..32 {
        v.push(DIOBJECTDATAFORMAT { pguid: null, dwOfs: 48 + i, dwType: DIDFT_BUTTON_OPT, dwFlags: 0 });
    }
    v
}

// ── DirectInput 初期化 ──────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
unsafe fn init_dinput_mgr(hwnd: isize) -> Option<DInputMgr> {
    use windows::{core::Interface, Win32::Devices::HumanInterfaceDevice::*, Win32::Foundation::*};

    let hinstance = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null())
    };
    let mut punk = std::ptr::null_mut::<core::ffi::c_void>();
    let hr = unsafe {
        DirectInput8Create(
            HINSTANCE(hinstance as *mut _),
            0x0800,
            &IDirectInput8W::IID,
            &mut punk,
            None,
        )
    };
    if hr.is_err() || punk.is_null() {
        eprintln!("[rustraight] DirectInput8Create failed: {hr:?}");
        return None;
    }
    let dinput: IDirectInput8W = unsafe { std::mem::transmute(punk) };
    let mut mgr = DInputMgr { dinput, hwnd, devices: Vec::new(), counter: 0 };
    unsafe { reenum_dinput(&mut mgr); }
    Some(mgr)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_devices_cb(
    inst: *mut windows::Win32::Devices::HumanInterfaceDevice::DIDEVICEINSTANCEW,
    pv:   *mut core::ffi::c_void,
) -> windows::Win32::Foundation::BOOL {
    unsafe {
        let vec = &mut *(pv as *mut Vec<windows::Win32::Devices::HumanInterfaceDevice::DIDEVICEINSTANCEW>);
        vec.push(*inst);
    }
    windows::Win32::Foundation::BOOL(1)
}

/// 新しく接続されたデバイスを mgr.devices に追加する。既知の GUID はスキップ。
#[cfg(target_os = "windows")]
unsafe fn reenum_dinput(mgr: &mut DInputMgr) {
    use windows::Win32::Devices::HumanInterfaceDevice::*;
    use windows::Win32::Foundation::HWND;

    let mut instances: Vec<DIDEVICEINSTANCEW> = Vec::new();
    let _ = unsafe {
        mgr.dinput.EnumDevices(
            4, // DI8DEVCLASS_GAMECTRL
            Some(enum_devices_cb),
            &mut instances as *mut Vec<DIDEVICEINSTANCEW> as *mut _,
            1, // DIEDFL_ATTACHEDONLY
        )
    };

    for inst in &instances {
        if mgr.devices.iter().any(|d| d.guid == inst.guidInstance) {
            continue;
        }
        if mgr.devices.len() >= MAX_DINPUT { break; }
        let hwnd_w = HWND(mgr.hwnd as *mut _);
        match unsafe { create_dinput_dev(&mgr.dinput, inst, hwnd_w) } {
            Ok(dev) => {
                eprintln!("[rustraight] DirectInput device connected: {}", dev.name);
                mgr.devices.push(dev);
            }
            Err(e) => eprintln!("[rustraight] DInput create failed: {e:?}"),
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn create_dinput_dev(
    dinput: &windows::Win32::Devices::HumanInterfaceDevice::IDirectInput8W,
    inst:   &windows::Win32::Devices::HumanInterfaceDevice::DIDEVICEINSTANCEW,
    hwnd:   windows::Win32::Foundation::HWND,
) -> windows::core::Result<DInputDev> {
    use windows::Win32::Devices::HumanInterfaceDevice::*;

    let mut opt: Option<IDirectInputDevice8W> = None;
    unsafe {
        dinput.CreateDevice(&inst.guidInstance, &mut opt, None::<&windows::core::IUnknown>)?;
    }
    let device = opt.ok_or_else(|| windows::core::Error::from_win32())?;

    let odfs = build_joystick_odfs();
    let mut fmt = DIDATAFORMAT {
        dwSize:    std::mem::size_of::<DIDATAFORMAT>() as u32,
        dwObjSize: std::mem::size_of::<DIOBJECTDATAFORMAT>() as u32,
        dwFlags:   1, // DIDF_ABSAXIS
        dwDataSize: std::mem::size_of::<JoyState>() as u32,
        dwNumObjs: odfs.len() as u32,
        rgodf:     odfs.as_ptr() as *mut _,
    };
    unsafe {
        device.SetDataFormat(&mut fmt as *mut _)?;
        device.SetCooperativeLevel(hwnd, DISCL_BACKGROUND | DISCL_NONEXCLUSIVE)?;
        let mut range = DIPROPRANGE {
            diph: DIPROPHEADER {
                dwSize:       std::mem::size_of::<DIPROPRANGE>() as u32,
                dwHeaderSize: std::mem::size_of::<DIPROPHEADER>() as u32,
                dwObj: 0,
                dwHow: 0, // DIPH_DEVICE
            },
            lMin: -32767,
            lMax:  32767,
        };
        let _ = device.SetProperty(DIPROP_RANGE_PTR, &mut range.diph as *mut _);
        device.Acquire()?;
    }

    let name = String::from_utf16_lossy(
        inst.tszInstanceName.split(|&c| c == 0).next().unwrap_or(&[])
    );
    Ok(DInputDev { guid: inst.guidInstance, name, device, state: PadState::new() })
}

// ── DirectInput ポール ──────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
unsafe fn poll_dinput_dev(dev: &mut DInputDev) {
    unsafe {
        let _ = dev.device.Poll();
        let mut js: JoyState = std::mem::zeroed();
        let result = dev.device.GetDeviceState(
            std::mem::size_of::<JoyState>() as u32,
            &mut js as *mut _ as *mut _,
        );
        match result {
            Ok(_) => {
                dev.state.connected = true;
                apply_joystate(&mut dev.state, &js);
            }
            Err(_) => {
                // 切断時は再アクワイアを試みる (再接続後に自動復帰)
                let _ = dev.device.Acquire();
                dev.state.connected = false;
                dev.state.current   = [false; BTN_COUNT];
                dev.state.axes      = [0.0;  AXIS_COUNT];
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn apply_joystate(state: &mut PadState, js: &JoyState) {
    let b = &js.rgbButtons;
    state.current[PadButton::South    as usize] = b[0]  & 0x80 != 0;
    state.current[PadButton::East     as usize] = b[1]  & 0x80 != 0;
    state.current[PadButton::West     as usize] = b[2]  & 0x80 != 0;
    state.current[PadButton::North    as usize] = b[3]  & 0x80 != 0;
    state.current[PadButton::LBumper  as usize] = b[4]  & 0x80 != 0;
    state.current[PadButton::RBumper  as usize] = b[5]  & 0x80 != 0;
    state.current[PadButton::LTrigger as usize] = b[6]  & 0x80 != 0;
    state.current[PadButton::RTrigger as usize] = b[7]  & 0x80 != 0;
    state.current[PadButton::Select   as usize] = b[8]  & 0x80 != 0;
    state.current[PadButton::Start    as usize] = b[9]  & 0x80 != 0;
    state.current[PadButton::LThumb   as usize] = b[10] & 0x80 != 0;
    state.current[PadButton::RThumb   as usize] = b[11] & 0x80 != 0;

    // POV ハット → D-pad (1/100 度単位)
    let pov = js.rgdwPOV[0];
    let valid = pov != 0xFFFF_FFFF;
    state.current[PadButton::DPadUp    as usize] = valid && (pov <= 4500 || pov >= 31500);
    state.current[PadButton::DPadRight as usize] = valid && (4500..=13500).contains(&pov);
    state.current[PadButton::DPadDown  as usize] = valid && (13500..=22500).contains(&pov);
    state.current[PadButton::DPadLeft  as usize] = valid && (22500..=31500).contains(&pov);

    let norm = |v: i32| (v as f32 / 32767.0).clamp(-1.0, 1.0);
    state.axes[PadAxis::LeftStickX   as usize] = norm(js.lX);
    state.axes[PadAxis::LeftStickY   as usize] = norm(js.lY);
    state.axes[PadAxis::RightStickX  as usize] = norm(js.lRx);
    state.axes[PadAxis::RightStickY  as usize] = norm(js.lRy);
    state.axes[PadAxis::LeftTrigger  as usize] = norm(js.lZ);
    state.axes[PadAxis::RightTrigger as usize] = norm(js.lRz);
}
