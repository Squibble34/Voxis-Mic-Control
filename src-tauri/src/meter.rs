use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::audio;
use crate::vad::{self, MonitorState};

const POLL_INTERVAL_MS: u64 = 40; // 25Hz

pub fn spawn_watcher(app: AppHandle, monitor_state: Arc<MonitorState>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

        let device_id = vad::get_device(&monitor_state);
        if device_id.is_empty() {
            continue;
        }

        match audio::get_peak_level(&device_id) {
            Ok(level) => {
                let percent = (level.clamp(0.0, 1.0) * 100.0).round() as u8;
                let _ = app.emit("input-level-changed", percent);
            }
            Err(e) => log::error!("[meter] get_peak_level failed: {e:?}"),
        }
    });
}