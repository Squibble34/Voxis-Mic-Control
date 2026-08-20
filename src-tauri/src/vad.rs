use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use crate::audio;

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300; // 5 minutes

pub struct MonitorState {
    device_id: Mutex<String>,
    enabled: AtomicBool,
    idle_timeout_secs: AtomicU64,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            device_id: Mutex::new(String::new()),
            enabled: AtomicBool::new(true),
            idle_timeout_secs: AtomicU64::new(DEFAULT_IDLE_TIMEOUT_SECS),
        }
    }
}

pub fn set_device(state: &MonitorState, device_id: String) {
    if let Ok(mut guard) = state.device_id.lock() {
        *guard = device_id;
    }
}

// lets other watchers (the meter poller) follow the same device without
// keeping their own separate copy of which device is selected
pub fn get_device(state: &MonitorState) -> String {
    state.device_id.lock().map(|g| g.clone()).unwrap_or_default()
}

pub fn set_enabled(state: &MonitorState, enabled: bool) {
    state.enabled.store(enabled, Ordering::Relaxed);
}

pub fn set_idle_timeout_secs(state: &MonitorState, secs: u64) {
    state.idle_timeout_secs.store(secs, Ordering::Relaxed);
}

fn current_device(state: &MonitorState) -> String {
    state.device_id.lock().map(|g| g.clone()).unwrap_or_default()
}

fn idle_seconds() -> Option<u64> {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    let ok = unsafe { GetLastInputInfo(&mut info) };
    if ok.as_bool() {
        let now = unsafe { GetTickCount() };
        Some(now.wrapping_sub(info.dwTime) as u64 / 1000)
    } else {
        None
    }
}

pub fn spawn_watcher(app: AppHandle, state: Arc<MonitorState>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));

        if !state.enabled.load(Ordering::Relaxed) {
            continue;
        }

        let device_id = current_device(&state);
        if device_id.is_empty() {
            continue;
        }

        let Some(idle) = idle_seconds() else { continue };
        let timeout = state.idle_timeout_secs.load(Ordering::Relaxed);
        if idle < timeout {
            continue;
        }

        match audio::get_mute(&device_id) {
            Ok(true) => continue,
            Ok(false) => {
                log::info!("[idle] {idle}s idle, auto-muting");
                if let Err(e) = audio::set_mute(&device_id, true) {
                    log::error!("[idle] auto-mute failed: {e:?}");
                    continue;
                }
                if let Err(e) = app.emit("auto-muted", ()) {
                    log::error!("[idle] emit auto-muted failed: {e:?}");
                }
            }
            Err(e) => log::error!("[idle] get_mute failed: {e:?}"),
        }
    });
}