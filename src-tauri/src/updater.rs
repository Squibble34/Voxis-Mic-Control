use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{PendingUpdateState, UpdateInfo, UserShownWindow};

const UPDATE_STATE_FILE: &str = "update-state.json";
const POST_UPDATE_TTL_SECS: u64 = 600;
const AUTOSTART_RETRY_DELAYS_SECS: &[u64] = &[4, 6, 10, 14];
const MANUAL_RETRY_DELAYS_SECS: &[u64] = &[0, 4];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledUpdateInfo {
    pub version: String,
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostUpdateLaunch {
    pub mode: String,
    pub created_at_epoch_secs: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePersistedState {
    pub scheduled: Option<ScheduledUpdateInfo>,
    pub post_update_launch: Option<PostUpdateLaunch>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledUpdateProgress {
    pub version: String,
    pub phase: String,
}

pub struct UpdaterRuntimeState {
    pub launch_intent_mode: std::sync::Mutex<String>,
    pub scheduled_cancel: AtomicBool,
    pub active_progress: std::sync::Mutex<Option<ScheduledUpdateProgress>>,
}

impl Default for UpdaterRuntimeState {
    fn default() -> Self {
        Self {
            launch_intent_mode: std::sync::Mutex::new("visible".to_string()),
            scheduled_cancel: AtomicBool::new(false),
            active_progress: std::sync::Mutex::new(None),
        }
    }
}

fn epoch_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(UPDATE_STATE_FILE))
}

pub fn read_persisted_state(app: &AppHandle) -> UpdatePersistedState {
    let Some(path) = state_path(app) else {
        return UpdatePersistedState::default();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return UpdatePersistedState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn write_persisted_state(app: &AppHandle, state: &UpdatePersistedState) {
    let Some(path) = state_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

pub fn take_post_update_mode(app: &AppHandle) -> Option<String> {
    let mut persisted = read_persisted_state(app);
    let post = persisted.post_update_launch.take()?;
    write_persisted_state(app, &persisted);

    let age = epoch_secs_now().saturating_sub(post.created_at_epoch_secs);
    if age > POST_UPDATE_TTL_SECS {
        return None;
    }
    Some(post.mode)
}

pub fn set_launch_intent_mode(app: &AppHandle, mode: &str) {
    if let Some(state) = app.try_state::<std::sync::Arc<UpdaterRuntimeState>>() {
        *state.launch_intent_mode.lock().unwrap() = mode.to_string();
    }
}

pub fn schedule_for_reopen(app: &AppHandle, version: String, notes: String) {
    let scheduled_info = ScheduledUpdateInfo { version, notes };
    let mut persisted = read_persisted_state(app);
    persisted.scheduled = Some(scheduled_info.clone());
    write_persisted_state(app, &persisted);

    if let Some(state) = app.try_state::<std::sync::Arc<UpdaterRuntimeState>>() {
        state.scheduled_cancel.store(false, Ordering::Relaxed);
    }
    let _ = app.emit("scheduled-update-changed", Some(scheduled_info));
}

pub fn cancel_scheduled(app: &AppHandle) {
    if let Some(state) = app.try_state::<std::sync::Arc<UpdaterRuntimeState>>() {
        state.scheduled_cancel.store(true, Ordering::Relaxed);
        *state.active_progress.lock().unwrap() = None;
    }
    let mut persisted = read_persisted_state(app);
    persisted.scheduled = None;
    write_persisted_state(app, &persisted);

    let _ = app.emit("scheduled-update-progress", Option::<ScheduledUpdateProgress>::None);
    let _ = app.emit("scheduled-update-changed", Option::<ScheduledUpdateInfo>::None);
}

pub fn set_progress(app: &AppHandle, progress: Option<ScheduledUpdateProgress>) {
    if let Some(state) = app.try_state::<std::sync::Arc<UpdaterRuntimeState>>() {
        *state.active_progress.lock().unwrap() = progress.clone();
    }
    let _ = app.emit("scheduled-update-progress", progress);
}

pub fn resolve_relaunch_mode(app: &AppHandle) -> String {
    let user_shown = app
        .try_state::<std::sync::Arc<UserShownWindow>>()
        .map(|s| s.0.load(Ordering::Relaxed))
        .unwrap_or(false);
    if user_shown {
        return "visible".to_string();
    }

    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(false);
        let minimized = win.is_minimized().unwrap_or(false);
        if visible && !minimized {
            return "visible".to_string();
        }
        if visible && minimized {
            return "minimized".to_string();
        }
    }

    app.try_state::<std::sync::Arc<UpdaterRuntimeState>>()
        .map(|s| s.launch_intent_mode.lock().unwrap().clone())
        .unwrap_or_else(|| "visible".to_string())
}

pub fn mark_installing_and_save_relaunch_mode(app: &AppHandle) -> String {
    let mode = resolve_relaunch_mode(app);
    let mut persisted = read_persisted_state(app);
    persisted.scheduled = None;
    persisted.post_update_launch = Some(PostUpdateLaunch {
        mode: mode.clone(),
        created_at_epoch_secs: epoch_secs_now(),
    });
    write_persisted_state(app, &persisted);
    mode
}

pub fn clear_post_update_mode(app: &AppHandle) {
    let mut persisted = read_persisted_state(app);
    if persisted.post_update_launch.is_some() {
        persisted.post_update_launch = None;
        write_persisted_state(app, &persisted);
    }
}

pub fn spawn_scheduled_update_runner(app: AppHandle, scheduled: ScheduledUpdateInfo, is_autostart: bool) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<std::sync::Arc<UpdaterRuntimeState>>();
        state.scheduled_cancel.store(false, Ordering::Relaxed);

        let retry_delays = if is_autostart {
            AUTOSTART_RETRY_DELAYS_SECS
        } else {
            MANUAL_RETRY_DELAYS_SECS
        };

        let mut downloaded_update: Option<(tauri_plugin_updater::Update, Vec<u8>)> = None;

        for (idx, delay_secs) in retry_delays.iter().enumerate() {
            if *delay_secs > 0 {
                set_progress(
                    &app,
                    Some(ScheduledUpdateProgress {
                        version: scheduled.version.clone(),
                        phase: if idx == 0 {
                            "preparing".to_string()
                        } else {
                            "waiting-network".to_string()
                        },
                    }),
                );
                // poll cancellation in short steps so cancel during boot retry reacts immediately
                for _ in 0..(*delay_secs * 5) {
                    if state.scheduled_cancel.load(Ordering::Relaxed) {
                        set_progress(&app, None);
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }

            if state.scheduled_cancel.load(Ordering::Relaxed) {
                set_progress(&app, None);
                return;
            }

            set_progress(
                &app,
                Some(ScheduledUpdateProgress {
                    version: scheduled.version.clone(),
                    phase: "downloading".to_string(),
                }),
            );

            use tauri_plugin_updater::UpdaterExt;
            let Ok(updater) = app.updater() else {
                continue;
            };
            match updater.check().await {
                Ok(Some(update)) => match update.download(|_, _| {}, || {}).await {
                    Ok(bytes) => {
                        downloaded_update = Some((update, bytes));
                        break;
                    }
                    Err(err) => log::warn!("[updater] scheduled download attempt {} failed: {err:?}", idx + 1),
                },
                Ok(None) => {
                    let mut persisted = read_persisted_state(&app);
                    persisted.scheduled = None;
                    write_persisted_state(&app, &persisted);
                    set_progress(&app, None);
                    let _ = app.emit("scheduled-update-changed", Option::<ScheduledUpdateInfo>::None);
                    return;
                }
                Err(err) => log::warn!("[updater] scheduled check attempt {} failed: {err:?}", idx + 1),
            }
        }

        if state.scheduled_cancel.load(Ordering::Relaxed) {
            set_progress(&app, None);
            return;
        }

        if let Some((update, bytes)) = downloaded_update {
            set_progress(
                &app,
                Some(ScheduledUpdateProgress {
                    version: scheduled.version.clone(),
                    phase: "installing".to_string(),
                }),
            );
            mark_installing_and_save_relaunch_mode(&app);
            if let Err(err) = update.install(bytes) {
                log::warn!("[updater] scheduled install failed: {err:?}");
                clear_post_update_mode(&app);
            } else {
                return;
            }
        }

        restore_banner_after_scheduled_failure(&app, scheduled);
    });
}

pub fn restore_banner_after_scheduled_failure(app: &AppHandle, scheduled: ScheduledUpdateInfo) {
    let mut persisted = read_persisted_state(app);
    persisted.scheduled = None;
    write_persisted_state(app, &persisted);

    set_progress(app, None);
    let _ = app.emit("scheduled-update-changed", Option::<ScheduledUpdateInfo>::None);

    let update_info = UpdateInfo {
        version: scheduled.version,
        notes: scheduled.notes,
    };
    let pending = app.state::<std::sync::Arc<PendingUpdateState>>();
    *pending.0.lock().unwrap() = Some(update_info.clone());
    let _ = app.emit("update-available", &update_info);
}
