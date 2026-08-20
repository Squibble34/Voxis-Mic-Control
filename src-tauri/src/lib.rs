mod app_state;
mod audio;
mod meter;
mod sessions;
mod tray_icon;
mod vad;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;
use windows::core::PWSTR;
use windows::Win32::System::RemoteDesktop::{
    WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfo, WTSINFOW,
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

struct TrayAccentState(std::sync::Mutex<String>);

impl Default for TrayAccentState {
    fn default() -> Self {
        Self(std::sync::Mutex::new("#28d6df".to_string()))
    }
}

#[tauri::command]
fn get_audio_devices() -> Vec<audio::AudioDevice> {
    let _ = audio::initialize_com();
    audio::get_devices().unwrap_or_default()
}

#[tauri::command]
fn set_mute_state(device_id: String, muted: bool) -> Result<(), String> {
    let _ = audio::initialize_com();
    audio::set_mute(&device_id, muted).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_volume(device_id: String, level: f32) -> Result<(), String> {
    let _ = audio::initialize_com();
    audio::set_volume(&device_id, level).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_mute_state(device_id: String) -> Result<bool, String> {
    let _ = audio::initialize_com();
    audio::get_mute(&device_id).map_err(|e| e.to_string())
}

// windows gives this back as 0.0-1.0, not a percent
#[tauri::command]
fn get_current_volume(device_id: String) -> Result<f32, String> {
    let _ = audio::initialize_com();
    audio::get_volume_level(&device_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_monitor_device(device_id: String, state: tauri::State<std::sync::Arc<vad::MonitorState>>) {
    vad::set_device(&state, device_id);
}

#[tauri::command]
fn set_auto_mute_enabled(enabled: bool, state: tauri::State<std::sync::Arc<vad::MonitorState>>) {
    vad::set_enabled(&state, enabled);
}

#[tauri::command]
fn set_auto_mute_idle_minutes(minutes: u64, state: tauri::State<std::sync::Arc<vad::MonitorState>>) {
    vad::set_idle_timeout_secs(&state, minutes.saturating_mul(60));
}

#[tauri::command]
fn set_tray_icon(
    app: tauri::AppHandle,
    muted: bool,
    accent_color: String,
    tray_accent: tauri::State<std::sync::Arc<TrayAccentState>>,
) -> Result<(), String> {
    *tray_accent.0.lock().unwrap() = accent_color.clone();
    let icon = tray_icon::generate_tray_icon(muted, &accent_color);
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.set_icon(Some(icon)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn set_minimize_to_tray_on_close(
    enabled: bool,
    state: tauri::State<std::sync::Arc<app_state::WindowBehaviorState>>,
) {
    state.set(enabled);
}

#[tauri::command]
fn get_capturing_sessions(
    device_id: String,
    icon_cache: tauri::State<std::sync::Arc<sessions::IconCache>>,
) -> Result<Vec<sessions::TrackedAppInfo>, String> {
    let _ = audio::initialize_com();
    sessions::get_capturing_sessions(&device_id, &icon_cache).map_err(|e| e.to_string())
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        update
            .download_and_install(|_downloaded, _total| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

fn read_windows_startup_settings(app: &tauri::AppHandle) -> (u64, String) {
    let default_delay = 3u64;
    let default_state = "minimized".to_string();

    let Ok(dir) = app.path().app_data_dir() else {
        return (default_delay, default_state);
    };
    let Ok(contents) = std::fs::read_to_string(dir.join("settings.json")) else {
        return (default_delay, default_state);
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return (default_delay, default_state);
    };

    let windows = json.get("windowsSettings");
    let delay = windows
        .and_then(|w| w.get("startupDelay"))
        .and_then(|v| v.as_u64())
        .unwrap_or(default_delay);
    let state = windows
        .and_then(|w| w.get("startupState"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or(default_state);

    (delay, state)
}

fn read_saved_window_position(app: &tauri::AppHandle) -> Option<(i32, i32)> {
    let dir = app.path().app_data_dir().ok()?;
    let contents = std::fs::read_to_string(dir.join("settings.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;

    let remember = json
        .get("windowsSettings")
        .and_then(|w| w.get("rememberWindowPosition"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !remember {
        return None;
    }

    let pos = json.get("windowPosition")?;
    let x = pos.get("x")?.as_i64()? as i32;
    let y = pos.get("y")?.as_i64()? as i32;
    Some((x, y))
}

fn position_is_visible(monitors: &[tauri::Monitor], x: i32, y: i32) -> bool {
    monitors.iter().any(|m| {
        let pos = m.position();
        let size = m.size();
        x >= pos.x && x < pos.x + size.width as i32 && y >= pos.y && y < pos.y + size.height as i32
    })
}

const AUTOSTART_LOGON_WINDOW_SECS: u64 = 90;

fn is_likely_autostart_launch() -> bool {
    match seconds_since_logon() {
        Some(secs) => secs < AUTOSTART_LOGON_WINDOW_SECS,
        None => false,
    }
}

fn seconds_since_logon() -> Option<u64> {
    unsafe {
        let mut buffer = PWSTR::null();
        let mut bytes_returned = 0u32;
        let ok = WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfo,
            &mut buffer,
            &mut bytes_returned,
        );
        if ok.is_err() || buffer.is_null() {
            return None;
        }

        let logon_ticks = (*(buffer.0 as *const WTSINFOW)).LogonTime as u64;
        WTSFreeMemory(buffer.0 as *mut core::ffi::c_void);

        let now = GetSystemTimeAsFileTime();
        let now_ticks = ((now.dwHighDateTime as u64) << 32) | now.dwLowDateTime as u64;

        Some(now_ticks.saturating_sub(logon_ticks) / 10_000_000)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let monitor_state = std::sync::Arc::new(vad::MonitorState::default());
    let window_behavior_state = std::sync::Arc::new(app_state::WindowBehaviorState::default());
    let icon_cache = std::sync::Arc::new(sessions::IconCache::default());
    let tray_accent_state = std::sync::Arc::new(TrayAccentState::default());

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // passing args here breaks the registered launch path on windows once autostart is enabled, so autostart detection is done separately below with is_likely_autostart_launch()
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(monitor_state.clone())
        .manage(window_behavior_state.clone())
        .manage(icon_cache.clone())
        .manage(tray_accent_state.clone())
        .setup(move |app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            vad::spawn_watcher(app.handle().clone(), monitor_state.clone());
            meter::spawn_watcher(app.handle().clone(), monitor_state.clone());

            let show_item = MenuItem::with_id(app, "show", "Show MicControl", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon::generate_tray_icon(true, "#28d6df"))
                .tooltip("Voxis")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.unminimize();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        let monitor_state = app.state::<std::sync::Arc<vad::MonitorState>>();
                        let device_id = vad::get_device(&monitor_state);
                        if device_id.is_empty() {
                            return;
                        }

                        let _ = audio::initialize_com();
                        let Ok(currently_muted) = audio::get_mute(&device_id) else { return };
                        let new_muted = !currently_muted;
                        if audio::set_mute(&device_id, new_muted).is_err() {
                            return;
                        }

                        let tray_accent = app.state::<std::sync::Arc<TrayAccentState>>();
                        let accent = tray_accent.0.lock().unwrap().clone();
                        let icon = tray_icon::generate_tray_icon(new_muted, &accent);
                        let _ = tray.set_icon(Some(icon));

                        let _ = app.emit("mute-changed", serde_json::json!({ "muted": new_muted }));
                    }
                })
                .build(app)?;

            if let Some(main_win) = app.get_webview_window("main") {
                // restore position before any show/hide decision, otherwise it jumps from tauri's default centered spot first
                if let Some((x, y)) = read_saved_window_position(app.handle()) {
                    if let Ok(monitors) = main_win.available_monitors() {
                        if position_is_visible(&monitors, x, y) {
                            let _ = main_win.set_position(tauri::PhysicalPosition::new(x, y));
                        }
                    }
                }

                let behavior = window_behavior_state.clone();
                let win_for_handler = main_win.clone();
                main_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if behavior.get() {
                            api.prevent_close();
                            let _ = win_for_handler.hide();
                        } else {
                            win_for_handler.app_handle().exit(0);
                        }
                    }
                });
            }

            if is_likely_autostart_launch() {
                if let Some(main_win) = app.get_webview_window("main") {
                    let _ = main_win.hide();
                }
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    let (delay_secs, startup_state) = read_windows_startup_settings(&app_handle);
                    std::thread::sleep(std::time::Duration::from_secs(delay_secs));
                    if let Some(win) = app_handle.get_webview_window("main") {
                        match startup_state.as_str() {
                            "tray" => {}
                            "minimized" => {
                                let _ = win.show();
                                let _ = win.minimize();
                            }
                            _ => {
                                let _ = win.show();
                            }
                        }
                    }
                });
            }

            // check for updates 8s after launch so it doesn't compete with startup
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                use tauri_plugin_updater::UpdaterExt;
                match app_handle.updater() {
                    Ok(updater) => match updater.check().await {
                        Ok(Some(update)) => {
                            let _ = app_handle.emit("update-available", serde_json::json!({
                                "version": update.version,
                                "notes": update.body.clone().unwrap_or_default(),
                            }));
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("[updater] check failed: {e:?}"),
                    },
                    Err(e) => log::warn!("[updater] could not get updater: {e:?}"),
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_audio_devices,
            set_mute_state,
            set_volume,
            get_mute_state,
            get_current_volume,
            set_monitor_device,
            set_auto_mute_enabled,
            set_auto_mute_idle_minutes,
            set_tray_icon,
            set_minimize_to_tray_on_close,
            get_capturing_sessions,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}