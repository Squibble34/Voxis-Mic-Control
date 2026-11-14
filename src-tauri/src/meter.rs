use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::mic::MicFeed;
use crate::vad::{self, MonitorState};

const POLL_INTERVAL_MS: u64 = 40;

pub fn spawn_watcher(app: AppHandle, monitor_state: Arc<MonitorState>, feed: Arc<MicFeed>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

        let device_id = vad::get_device(&monitor_state);
        if device_id.is_empty() {
            continue;
        }

        let level = feed.take_peak_level();
        let percent = (level.clamp(0.0, 1.0) * 100.0).round() as u8;
        let _ = app.emit("input-level-changed", percent);
    });
}