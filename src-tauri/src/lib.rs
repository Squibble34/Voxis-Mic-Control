mod app_state;
mod audio;
mod descriptors;
mod device;
mod meter;
mod mic;
mod protocol;
mod server;
mod sessions;
mod tray_icon;
mod vad;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;
use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::RemoteDesktop::{
    WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfo, WTSINFOW,
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(
        lp_mutex_attributes: *const core::ffi::c_void,
        b_initial_owner: i32,
        lp_name: *const u16,
    ) -> *mut core::ffi::c_void;
}

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
fn set_mute_state(
    _device_id: String,
    muted: bool,
    feed: tauri::State<std::sync::Arc<mic::MicFeed>>,
) -> Result<(), String> {
    feed.set_muted(muted);
    Ok(())
}

#[tauri::command]
fn set_volume(
    _device_id: String,
    level: f32,
    feed: tauri::State<std::sync::Arc<mic::MicFeed>>,
) -> Result<(), String> {
    feed.set_gain(level);
    Ok(())
}

#[tauri::command]
fn get_mute_state(
    _device_id: String,
    feed: tauri::State<std::sync::Arc<mic::MicFeed>>,
) -> Result<bool, String> {
    Ok(feed.is_muted())
}

#[tauri::command]
fn get_current_volume(
    _device_id: String,
    feed: tauri::State<std::sync::Arc<mic::MicFeed>>,
) -> Result<f32, String> {
    Ok(feed.gain())
}

#[tauri::command]
fn set_monitor_device(
    device_id: String,
    state: tauri::State<std::sync::Arc<vad::MonitorState>>,
    feed: tauri::State<std::sync::Arc<mic::MicFeed>>,
) {
    let _ = audio::initialize_com();
    // previous versions muted the physical endpoint directly in windows, unmute it so cpal receives live audio
    if audio::get_mute(&device_id) == Ok(true) {
        let _ = audio::set_mute(&device_id, false);
    }
    vad::set_device(&state, device_id.clone());
    feed.set_target_device(device_id);
}

#[tauri::command]
fn set_noise_engine(engine: String, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_engine(mic::NoiseEngine::from_str(&engine));
}

#[tauri::command]
fn set_noise_atten_lim(lim_db: f32, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_atten_lim_db(lim_db);
}

#[tauri::command]
fn set_noise_post_filter(beta: f32, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_post_filter_beta(beta);
}

#[tauri::command]
fn set_auto_default_mic(enabled: bool, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_auto_default_mic(enabled);
    if enabled {
        let _ = audio::set_voxis_as_default_mic();
    }
}

#[tauri::command]
fn set_high_pass_enabled(enabled: bool, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_high_pass_enabled(enabled);
}

#[tauri::command]
fn set_voice_tone(tone: String, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_voice_tone(mic::VoiceTone::from_str(&tone));
}

#[tauri::command]
fn set_noise_gate_enabled(enabled: bool, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_noise_gate_enabled(enabled);
}

#[tauri::command]
fn set_noise_gate_threshold(threshold: f32, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_noise_gate_threshold(threshold);
}

#[tauri::command]
fn set_compressor_enabled(enabled: bool, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_compressor_enabled(enabled);
}

#[tauri::command]
fn set_funny_mic_enabled(enabled: bool, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_funny_mic_enabled(enabled);
}

#[tauri::command]
fn set_sidetone_enabled(enabled: bool, feed: tauri::State<std::sync::Arc<mic::MicFeed>>) {
    feed.set_sidetone_enabled(enabled);
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

// re-asserts hwnd_topmost every time the popup shows so fullscreen apps or the taskbar cant demote its z-order
#[tauri::command]
fn show_popup_topmost(app: tauri::AppHandle) -> Result<(), String> {
    let Some(win) = app.get_webview_window("popup") else {
        return Err("popup window not found".to_string());
    };
    let _ = win.show();
    let _ = win.set_always_on_top(true);
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
    Ok(())
}

#[tauri::command]
fn get_capturing_sessions(
    device_id: String,
    icon_cache: tauri::State<std::sync::Arc<sessions::IconCache>>,
) -> Result<Vec<sessions::TrackedAppInfo>, String> {
    let _ = audio::initialize_com();
    sessions::get_capturing_sessions(&device_id, &icon_cache).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct UpdateInfo {
    version: String,
    notes: String,
}

#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body.unwrap_or_default(),
    }))
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update found".to_string())?;
    update
        .download_and_install(|_downloaded, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
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

fn apply_saved_noise_settings(app: &tauri::AppHandle, feed: &mic::MicFeed) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let Ok(contents) = std::fs::read_to_string(dir.join("settings.json")) else { return };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else { return };

    if let Some(last_id) = json.get("lastDeviceId").and_then(|v| v.as_str()) {
        feed.set_target_device(last_id.to_string());
    }
    if let Some(saved_gain) = json.get("softwareGain").and_then(|v| v.as_f64()) {
        feed.set_gain(saved_gain as f32);
    }
    let Some(noise) = json.get("noiseSettings") else { return };
    if let Some(engine) = noise.get("engine").and_then(|v| v.as_str()) {
        feed.set_engine(mic::NoiseEngine::from_str(engine));
    }
    if let Some(lim) = noise.get("attenLimDb").and_then(|v| v.as_f64()) {
        feed.set_atten_lim_db(lim as f32);
    }
    if let Some(beta) = noise.get("postFilterBeta").and_then(|v| v.as_f64()) {
        feed.set_post_filter_beta(beta as f32);
    }
    if let Some(auto_def) = noise.get("autoSetDefaultMic").and_then(|v| v.as_bool()) {
        feed.set_auto_default_mic(auto_def);
    }
    if let Some(hp) = noise.get("highPassEnabled").and_then(|v| v.as_bool()) {
        feed.set_high_pass_enabled(hp);
    }
    if let Some(tone) = noise.get("voiceTone").and_then(|v| v.as_str()) {
        feed.set_voice_tone(mic::VoiceTone::from_str(tone));
    }
    if let Some(gate_en) = noise.get("noiseGateEnabled").and_then(|v| v.as_bool()) {
        feed.set_noise_gate_enabled(gate_en);
    }
    if let Some(gate_th) = noise.get("noiseGateThreshold").and_then(|v| v.as_f64()) {
        feed.set_noise_gate_threshold((gate_th / 100.0) as f32);
    }
    if let Some(comp) = noise.get("compressorEnabled").and_then(|v| v.as_bool()) {
        feed.set_compressor_enabled(comp);
    }
}

fn ensure_single_instance() -> bool {
    unsafe {
        let _ = CreateMutexW(
            std::ptr::null(),
            1,
            w!("Local\\VoxisMicControlSingleInstance").as_ptr(),
        );
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(hwnd) = FindWindowW(None, w!("Voxis Mic Control")) {
                if !hwnd.is_invalid() {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            return false;
        }
        true
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !ensure_single_instance() {
        return;
    }

    let monitor_state = std::sync::Arc::new(vad::MonitorState::default());
    let window_behavior_state = std::sync::Arc::new(app_state::WindowBehaviorState::default());
    let icon_cache = std::sync::Arc::new(sessions::IconCache::default());
    let tray_accent_state = std::sync::Arc::new(TrayAccentState::default());
    let mic_feed = std::sync::Arc::new(mic::MicFeed::new());
    let user_shown_window = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // passing args here breaks the registered launch path on windows once autostart is enabled, so autostart detection is done separately below with is_likely_autostart_launch()
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let _ = webview.eval(
                    "if (!window.__voxisNoContextMenu) { window.__voxisNoContextMenu = true; window.addEventListener('contextmenu', function (e) { e.preventDefault(); }, true); }",
                );
            }
        })
        .manage(monitor_state.clone())
        .manage(window_behavior_state.clone())
        .manage(icon_cache.clone())
        .manage(tray_accent_state.clone())
        .manage(mic_feed.clone())
        .setup(move |app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .level_for("df", log::LevelFilter::Warn)
                    .level_for("tract_linalg", log::LevelFilter::Warn)
                    .build(),
            )?;
            apply_saved_noise_settings(app.handle(), &mic_feed);
            mic::spawn_capture_worker(mic_feed.clone());
            mic::spawn_sidetone_worker(mic_feed.clone());
            server::spawn_usbip_server(mic_feed.clone());
            vad::spawn_watcher(app.handle().clone(), monitor_state.clone(), mic_feed.clone());
            meter::spawn_watcher(app.handle().clone(), monitor_state.clone(), mic_feed.clone());

            let show_item = MenuItem::with_id(app, "show", "Show MicControl", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let user_shown_for_menu = user_shown_window.clone();
            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon::generate_tray_icon(true, "#28d6df"))
                .tooltip("Voxis")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => {
                        user_shown_for_menu.store(true, std::sync::atomic::Ordering::Relaxed);
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

                        let feed = app.state::<std::sync::Arc<mic::MicFeed>>();
                        let new_muted = !feed.is_muted();
                        feed.set_muted(new_muted);

                        let tray_accent = app.state::<std::sync::Arc<TrayAccentState>>();
                        let accent = tray_accent.0.lock().unwrap().clone();
                        let icon = tray_icon::generate_tray_icon(new_muted, &accent);
                        let _ = tray.set_icon(Some(icon));

                        let _ = app.emit("mute-changed", serde_json::json!({ "muted": new_muted }));
                    }
                })
                .build(app)?;

            if let Some(main_win) = app.get_webview_window("main") {
                // restore position before any show/hide decision, otherwise it jumps from tauris default centered spot first
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
                let user_shown_for_autostart = user_shown_window.clone();
                std::thread::spawn(move || {
                    let (delay_secs, startup_state) = read_windows_startup_settings(&app_handle);
                    std::thread::sleep(std::time::Duration::from_secs(delay_secs));
                    if user_shown_for_autostart.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    if let Some(win) = app_handle.get_webview_window("main") {
                        if win.is_visible().unwrap_or(false) {
                            return;
                        }
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

            // check for updates 8s after launch so it doesnt compete with startup
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
            set_noise_engine,
            set_noise_atten_lim,
            set_noise_post_filter,
            set_auto_default_mic,
            set_high_pass_enabled,
            set_voice_tone,
            set_noise_gate_enabled,
            set_noise_gate_threshold,
            set_compressor_enabled,
            set_funny_mic_enabled,
            set_sidetone_enabled,
            set_auto_mute_enabled,
            set_auto_mute_idle_minutes,
            set_tray_icon,
            set_minimize_to_tray_on_close,
            show_popup_topmost,
            get_capturing_sessions,
            check_for_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}