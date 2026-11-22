use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::mic::MicFeed;
use crate::vad::{self, MonitorState};

const POLL_INTERVAL_MS: u64 = 40;
// checking window state talks to the main thread so only do it every ~240 ms
const WINDOW_CHECK_EVERY_TICKS: u32 = 6;
const SPEECH_LEVEL_PERCENT: u8 = 10;
const SPEECH_PROB_MIN: f32 = 0.50;
const SPEECH_TICKS_TO_WARN: u32 = 22;
const SPEECH_GAP_RESET: Duration = Duration::from_millis(1400);
const WARNING_COOLDOWN: Duration = Duration::from_secs(8);

#[derive(Default)]
struct MutedSpeechTracker {
    loud_ticks: u32,
    last_loud_at: Option<Instant>,
    warning_shown: bool,
    last_popup_at: Option<Instant>,
}

impl MutedSpeechTracker {
    fn clear(&mut self, app: &AppHandle) {
        self.loud_ticks = 0;
        self.last_loud_at = None;
        if self.warning_shown {
            self.warning_shown = false;
            let _ = app.emit("talking-while-muted-state", false);
        }
    }

    fn observe(
        &mut self,
        app: &AppHandle,
        monitor_state: &MonitorState,
        muted: bool,
        percent: u8,
        speech_prob: f32,
    ) {
        if !muted {
            self.clear(app);
            return;
        }

        let now = Instant::now();
        if percent >= SPEECH_LEVEL_PERCENT && speech_prob >= SPEECH_PROB_MIN {
            self.loud_ticks += 1;
            self.last_loud_at = Some(now);
            if self.loud_ticks < SPEECH_TICKS_TO_WARN {
                return;
            }
            if !self.warning_shown {
                self.warning_shown = true;
                let _ = app.emit("talking-while-muted-state", true);
            }
            let cooled_down = self
                .last_popup_at
                .map_or(true, |at| now.duration_since(at) > WARNING_COOLDOWN);
            if cooled_down && vad::talking_popup_enabled(monitor_state) {
                self.last_popup_at = Some(now);
                let _ = app.emit("talking-while-muted", ());
            }
        } else {
            self.loud_ticks = self.loud_ticks.saturating_sub(1);
            if self.last_loud_at.is_some_and(|at| now.duration_since(at) > SPEECH_GAP_RESET) {
                self.clear(app);
            }
        }
    }
}

fn main_window_showing(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false)
}

pub fn spawn_watcher(app: AppHandle, monitor_state: Arc<MonitorState>, feed: Arc<MicFeed>) {
    std::thread::spawn(move || {
        let mut tracker = MutedSpeechTracker::default();
        let mut window_showing = main_window_showing(&app);
        let mut ticks_since_window_check = 0u32;

        loop {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

            let device_id = vad::get_device(&monitor_state);
            if device_id.is_empty() {
                continue;
            }

            let level = feed.take_peak_level();
            let speech_prob = feed.take_speech_prob();
            let percent = (level.clamp(0.0, 1.0) * 100.0).round() as u8;

            ticks_since_window_check += 1;
            if ticks_since_window_check >= WINDOW_CHECK_EVERY_TICKS {
                ticks_since_window_check = 0;
                window_showing = main_window_showing(&app);
            }
            if window_showing {
                let _ = app.emit("input-level-changed", percent);
            }

            tracker.observe(&app, &monitor_state, feed.is_muted(), percent, speech_prob);
        }
    });
}
