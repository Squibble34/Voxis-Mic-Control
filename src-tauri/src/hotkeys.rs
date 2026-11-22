use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VkKeyScanW, VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassExW,
    TranslateMessage, HWND_MESSAGE, MSG, RI_KEY_BREAK, RI_MOUSE_BUTTON_4_DOWN,
    RI_MOUSE_BUTTON_4_UP, RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP, RI_MOUSE_MIDDLE_BUTTON_DOWN,
    RI_MOUSE_MIDDLE_BUTTON_UP, WM_APP, WM_INPUT, WNDCLASSEXW,
};

use crate::{mic, vad};

const WM_APP_RELOAD: u32 = WM_APP + 1;
const WM_APP_PAUSED: u32 = WM_APP + 2;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub hotkeys_enabled: bool,
    pub hotkey_mode: String,
    pub mute_chord: Vec<String>,
    pub open_app_chord: Vec<String>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            hotkeys_enabled: true,
            hotkey_mode: "toggle".to_string(),
            mute_chord: vec!["Ctrl".into(), "Alt".into(), "M".into()],
            open_app_chord: vec!["Ctrl".into(), "Alt".into(), "A".into()],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TriggerInput {
    Key(u16),
    Mouse(u16),
}

#[derive(Clone, PartialEq, Eq)]
struct ParsedChord {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    trigger: TriggerInput,
    strict_modifiers: bool,
}

impl ParsedChord {
    fn has_modifiers(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    fn modifiers_match(&self, held: ModifiersState, is_ptt: bool) -> bool {
        if self.ctrl != held.ctrl && (self.ctrl || (!is_ptt && self.strict_modifiers)) {
            return false;
        }
        if self.alt != held.alt && (self.alt || (!is_ptt && self.strict_modifiers)) {
            return false;
        }
        if self.shift != held.shift && (self.shift || (!is_ptt && self.strict_modifiers)) {
            return false;
        }
        if self.win != held.win && (self.win || (!is_ptt && self.strict_modifiers)) {
            return false;
        }
        true
    }

    fn required_modifiers_held(&self, held: ModifiersState) -> bool {
        (!self.ctrl || held.ctrl)
            && (!self.alt || held.alt)
            && (!self.shift || held.shift)
            && (!self.win || held.win)
    }
}

#[derive(Clone, Copy, Default)]
struct ModifiersState {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

impl ModifiersState {
    fn read_async() -> Self {
        unsafe {
            let down = |vk: u16| (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0;
            Self {
                ctrl: down(VK_CONTROL.0),
                alt: down(VK_MENU.0),
                shift: down(VK_SHIFT.0),
                win: down(VK_LWIN.0) || down(VK_RWIN.0),
            }
        }
    }
}

fn is_typing_vk(vk: u16) -> bool {
    matches!(vk, 0x30..=0x39 | 0x41..=0x5A | 0x20 | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2)
}

fn parse_vk(token: &str) -> Option<u16> {
    if token.len() == 1 {
        let ch = token.chars().next()?;
        let upper = ch.to_ascii_uppercase();
        if upper.is_ascii_alphanumeric() {
            return Some(upper as u16);
        }
        let scan = unsafe { VkKeyScanW(ch as u16) };
        if scan != -1 {
            return Some((scan & 0xff) as u16);
        }
    }

    match token {
        "Space" | " " => Some(0x20),
        "CapsLock" => Some(VK_CAPITAL.0),
        "Tab" => Some(0x09),
        "Enter" => Some(0x0D),
        "Escape" => Some(0x1B),
        "Backspace" => Some(0x08),
        "Insert" => Some(0x2D),
        "Delete" => Some(0x2E),
        "Home" => Some(0x24),
        "End" => Some(0x23),
        "PageUp" => Some(0x21),
        "PageDown" => Some(0x22),
        "ArrowUp" => Some(0x26),
        "ArrowDown" => Some(0x28),
        "ArrowLeft" => Some(0x25),
        "ArrowRight" => Some(0x27),
        "NumLock" => Some(0x90),
        "ScrollLock" => Some(0x91),
        "Pause" => Some(0x13),
        "PrintScreen" => Some(0x2C),
        "ContextMenu" => Some(0x5D),
        other => {
            if let Some(num) = other.strip_prefix('F').and_then(|s| s.parse::<u16>().ok()) {
                if (1..=24).contains(&num) {
                    return Some(0x6F + num);
                }
            }
            None
        }
    }
}

fn parse_chord(tokens: &[String]) -> Option<ParsedChord> {
    if tokens.is_empty() {
        return None;
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut win = false;
    let mut trigger: Option<TriggerInput> = None;

    for token in tokens {
        match token.as_str() {
            "Ctrl" | "Control" => ctrl = true,
            "Alt" => alt = true,
            "Shift" => shift = true,
            "Super" | "Win" | "Meta" => win = true,
            "Mouse3" => trigger = Some(TriggerInput::Mouse(3)),
            "Mouse4" => trigger = Some(TriggerInput::Mouse(4)),
            "Mouse5" => trigger = Some(TriggerInput::Mouse(5)),
            key => {
                let vk = parse_vk(key)?;
                trigger = Some(TriggerInput::Key(vk));
            }
        }
    }

    let trigger = trigger?;
    let strict_modifiers = match trigger {
        TriggerInput::Key(vk) => is_typing_vk(vk),
        TriggerInput::Mouse(_) => false,
    };

    Some(ParsedChord {
        ctrl,
        alt,
        shift,
        win,
        trigger,
        strict_modifiers,
    })
}

struct CompiledHotkeys {
    enabled: bool,
    is_ptt: bool,
    mute: Option<ParsedChord>,
    open_app: Option<ParsedChord>,
    mute_error: bool,
}

impl CompiledHotkeys {
    fn from_config(config: &HotkeyConfig) -> Self {
        let mute = parse_chord(&config.mute_chord);
        let open_app = parse_chord(&config.open_app_chord);
        let mute_error = config.hotkeys_enabled && !config.mute_chord.is_empty() && mute.is_none();
        Self {
            enabled: config.hotkeys_enabled,
            is_ptt: config.hotkey_mode == "push-to-talk",
            mute,
            open_app,
            mute_error,
        }
    }
}

struct SharedState {
    desired: Mutex<(HotkeyConfig, Vec<oneshot::Sender<bool>>)>,
    paused: AtomicBool,
}

pub struct HotkeyManager {
    shared: Arc<SharedState>,
    hwnd_raw: isize,
}

impl HotkeyManager {
    pub fn spawn(app: AppHandle) -> Arc<Self> {
        let shared = Arc::new(SharedState {
            desired: Mutex::new((HotkeyConfig::default(), Vec::new())),
            paused: AtomicBool::new(false),
        });
        let shared_for_thread = Arc::clone(&shared);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<isize>(1);

        thread::spawn(move || {
            run_raw_input_loop(app, shared_for_thread, ready_tx);
        });

        let hwnd_raw = ready_rx.recv().unwrap_or(0);
        Arc::new(Self { shared, hwnd_raw })
    }

    pub fn apply_nonblocking(&self, config: HotkeyConfig) {
        self.shared.desired.lock().unwrap().0 = config;
        self.post_message(WM_APP_RELOAD);
    }

    pub async fn sync(&self, config: HotkeyConfig) -> bool {
        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let mut guard = self.shared.desired.lock().unwrap();
            guard.0 = config;
            guard.1.push(reply_tx);
        }
        self.post_message(WM_APP_RELOAD);
        reply_rx.await.unwrap_or(false)
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
        self.post_message(WM_APP_PAUSED);
    }

    fn post_message(&self, msg: u32) {
        if self.hwnd_raw == 0 {
            return;
        }
        let hwnd = HWND(self.hwnd_raw as *mut core::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
        }
    }
}

struct RuntimeTracker {
    compiled: CompiledHotkeys,
    key_down: [bool; 256],
    mouse_down: [bool; 6],
    ptt_active: bool,
    last_mute_toggle: Option<Instant>,
    last_open_app: Option<Instant>,
}

impl RuntimeTracker {
    fn new(compiled: CompiledHotkeys) -> Self {
        Self {
            compiled,
            key_down: [false; 256],
            mouse_down: [false; 6],
            ptt_active: false,
            last_mute_toggle: None,
            last_open_app: None,
        }
    }

    fn release_ptt_if_needed(&mut self, app: &AppHandle) {
        if self.ptt_active {
            self.ptt_active = false;
            crate::apply_mute_change(app, true);
        }
    }

    fn reload(&mut self, app: &AppHandle, shared: &SharedState) {
        let (config, waiters) = {
            let mut guard = shared.desired.lock().unwrap();
            let waiters = std::mem::take(&mut guard.1);
            (guard.0.clone(), waiters)
        };
        let compiled = CompiledHotkeys::from_config(&config);
        let mute_error = compiled.mute_error;
        if !compiled.enabled || !compiled.is_ptt || compiled.mute != self.compiled.mute {
            self.release_ptt_if_needed(app);
        }
        self.compiled = compiled;
        for waiter in waiters {
            let _ = waiter.send(mute_error);
        }
    }

    fn handle_trigger_event(
        &mut self,
        app: &AppHandle,
        paused: bool,
        input: TriggerInput,
        pressed: bool,
    ) {
        let held = ModifiersState::read_async();

        if paused || !self.compiled.enabled {
            self.release_ptt_if_needed(app);
            return;
        }

        if let Some(ref open_chord) = self.compiled.open_app {
            if pressed && open_chord.trigger == input && open_chord.modifiers_match(held, false) {
                let now = Instant::now();
                let debounced = self
                    .last_open_app
                    .is_some_and(|prev| now.duration_since(prev) < Duration::from_millis(120));
                if !debounced {
                    self.last_open_app = Some(now);
                    crate::show_or_create_main_window(app);
                }
            }
        }

        let Some(ref mute_chord) = self.compiled.mute else {
            self.release_ptt_if_needed(app);
            return;
        };

        let monitor_state = app.state::<Arc<vad::MonitorState>>();
        if vad::get_device(&monitor_state).is_empty() {
            return;
        }

        if self.compiled.is_ptt {
            if pressed && mute_chord.trigger == input && mute_chord.modifiers_match(held, true) {
                if !self.ptt_active {
                    self.ptt_active = true;
                    crate::apply_mute_change(app, false);
                }
            } else if !pressed && self.ptt_active {
                let trigger_released = mute_chord.trigger == input;
                let modifier_released = mute_chord.has_modifiers()
                    && !mute_chord.required_modifiers_held(held);
                if trigger_released || modifier_released {
                    self.ptt_active = false;
                    crate::apply_mute_change(app, true);
                }
            }
            return;
        }

        if pressed && mute_chord.trigger == input && mute_chord.modifiers_match(held, false) {
            let now = Instant::now();
            if let Some(prev) = self.last_mute_toggle {
                if now.duration_since(prev) < Duration::from_millis(50) {
                    return;
                }
            }
            self.last_mute_toggle = Some(now);
            let feed = app.state::<Arc<mic::MicFeed>>();
            crate::apply_mute_change(app, !feed.is_muted());
        }
    }

    fn process_raw_input(&mut self, app: &AppHandle, paused: bool, hraw: HRAWINPUT) {
        let mut size = 0u32;
        unsafe {
            let header_size = size_of::<RAWINPUTHEADER>() as u32;
            if GetRawInputData(hraw, RID_INPUT, None, &mut size, header_size) == u32::MAX
                || size == 0
                || size as usize > size_of::<RAWINPUT>()
            {
                return;
            }
            let mut raw = RAWINPUT::default();
            let copied = GetRawInputData(
                hraw,
                RID_INPUT,
                Some(&mut raw as *mut RAWINPUT as *mut core::ffi::c_void),
                &mut size,
                header_size,
            );
            if copied == u32::MAX {
                return;
            }

            if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
                let kb = raw.data.keyboard;
                let vk = kb.VKey;
                if vk == 0 || vk as usize >= self.key_down.len() {
                    return;
                }
                let pressed = (kb.Flags as u32 & RI_KEY_BREAK) == 0;
                let was_down = self.key_down[vk as usize];
                self.key_down[vk as usize] = pressed;
                if pressed == was_down {
                    return;
                }
                self.handle_trigger_event(app, paused, TriggerInput::Key(vk), pressed);
            } else if raw.header.dwType == RIM_TYPEMOUSE.0 {
                let flags = raw.data.mouse.Anonymous.Anonymous.usButtonFlags as u32;
                if flags == 0 {
                    return;
                }
                let checks = [
                    (3usize, RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_UP),
                    (4usize, RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP),
                    (5usize, RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP),
                ];
                for (button_idx, down_flag, up_flag) in checks {
                    if (flags & down_flag) != 0 {
                        let was_down = self.mouse_down[button_idx];
                        self.mouse_down[button_idx] = true;
                        if !was_down {
                            self.handle_trigger_event(
                                app,
                                paused,
                                TriggerInput::Mouse(button_idx as u16),
                                true,
                            );
                        }
                    }
                    if (flags & up_flag) != 0 {
                        let was_down = self.mouse_down[button_idx];
                        self.mouse_down[button_idx] = false;
                        if was_down {
                            self.handle_trigger_event(
                                app,
                                paused,
                                TriggerInput::Mouse(button_idx as u16),
                                false,
                            );
                        }
                    }
                }
            }
        }
    }
}

unsafe extern "system" fn raw_input_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn run_raw_input_loop(
    app: AppHandle,
    shared: Arc<SharedState>,
    ready_tx: std::sync::mpsc::SyncSender<isize>,
) {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class_name = w!("VoxisRawInputClass");
        let wnd_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(raw_input_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wnd_class);

        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!(""),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance),
            None,
        )
        .unwrap_or_default();

        if !hwnd.is_invalid() {
            let devices = [
                RAWINPUTDEVICE {
                    usUsagePage: 0x01,
                    usUsage: 0x06,
                    dwFlags: RIDEV_INPUTSINK,
                    hwndTarget: hwnd,
                },
                RAWINPUTDEVICE {
                    usUsagePage: 0x01,
                    usUsage: 0x02,
                    dwFlags: RIDEV_INPUTSINK,
                    hwndTarget: hwnd,
                },
            ];
            if let Err(err) =
                RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32)
            {
                log::error!("[hotkeys] failed to register raw input devices: {err}");
            }
        }

        let _ = ready_tx.send(hwnd.0 as isize);
        if hwnd.is_invalid() {
            return;
        }

        let initial_compiled = {
            let guard = shared.desired.lock().unwrap();
            CompiledHotkeys::from_config(&guard.0)
        };
        let mut tracker = RuntimeTracker::new(initial_compiled);
        tracker.reload(&app, &shared);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                WM_APP_RELOAD => tracker.reload(&app, &shared),
                WM_APP_PAUSED => {
                    if shared.paused.load(Ordering::Relaxed) {
                        tracker.release_ptt_if_needed(&app);
                    }
                }
                WM_INPUT => {
                    let paused = shared.paused.load(Ordering::Relaxed);
                    tracker.process_raw_input(&app, paused, HRAWINPUT(msg.lParam.0 as *mut _));
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}
