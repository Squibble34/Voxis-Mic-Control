mod app_state;
mod audio;
mod cue;
mod descriptors;
mod device;
#[cfg(dev)]
mod dev_console;
mod hotkeys;
mod meter;
mod mic;
mod pill;
mod popup;
mod profiles;
mod protocol;
mod server;
mod sessions;
mod tray_icon;
mod updater;
mod vad;

use std::sync::atomic::Ordering;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
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

#[derive(Default)]
pub(crate) struct PendingUpdateState(std::sync::Mutex<Option<UpdateInfo>>);

#[cfg(dev)]
pub(crate) static DEV_FAKE_UPDATE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(dev)]
pub(crate) static DEV_FAKE_UPDATE_FAIL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
struct UserShownWindow(std::sync::atomic::AtomicBool);

struct TrayMenuState {
    muted_item: CheckMenuItem<tauri::Wry>,
    pill_item: CheckMenuItem<tauri::Wry>,
    device_submenu: Submenu<tauri::Wry>,
    device_items: std::sync::Mutex<Vec<(String, CheckMenuItem<tauri::Wry>)>>,
}

static SETTINGS_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn update_settings_json(app: &tauri::AppHandle, updater: impl FnOnce(&mut serde_json::Value)) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let _guard = SETTINGS_FILE_LOCK.lock().unwrap();
    let path = dir.join("settings.json");
    let mut json = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    updater(&mut json);
    if let Ok(serialized) = serde_json::to_string_pretty(&json) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(path, serialized);
    }
}

pub fn sync_tray_devices(app: &tauri::AppHandle, devices: Vec<audio::AudioDevice>) {
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(menu_state) = app_handle.try_state::<std::sync::Arc<TrayMenuState>>() else {
            return;
        };
        let monitor_state = app_handle.state::<std::sync::Arc<vad::MonitorState>>();
        let active_id = vad::get_device(&monitor_state);
        if let Ok(existing) = menu_state.device_submenu.items() {
            for item in &existing {
                let _ = menu_state.device_submenu.remove(item);
            }
        }
        let mut next_items = Vec::new();
        if devices.is_empty() {
            if let Ok(empty_item) = MenuItem::with_id(
                &app_handle,
                "dev-none",
                "No input devices",
                false,
                None::<&str>,
            ) {
                let _ = menu_state.device_submenu.append(&empty_item);
            }
        } else {
            for dev in devices {
                if let Ok(item) = CheckMenuItem::with_id(
                    &app_handle,
                    format!("dev:{}", dev.id),
                    &dev.name,
                    true,
                    dev.id == active_id,
                    None::<&str>,
                ) {
                    let _ = menu_state.device_submenu.append(&item);
                    next_items.push((dev.id, item));
                }
            }
        }
        *menu_state.device_items.lock().unwrap() = next_items;
    });
}

pub fn update_tray_active_device(app: &tauri::AppHandle, active_id: &str) {
    let app_handle = app.clone();
    let active = active_id.to_string();
    let _ = app.run_on_main_thread(move || {
        let Some(menu_state) = app_handle.try_state::<std::sync::Arc<TrayMenuState>>() else {
            return;
        };
        let items = menu_state.device_items.lock().unwrap();
        for (id, item) in items.iter() {
            let _ = item.set_checked(*id == active);
        }
    });
}

pub fn apply_pill_enabled(app: &tauri::AppHandle, enabled: bool, from_tray: bool) {
    if let Some(pill_ctrl) = app.try_state::<std::sync::Arc<pill::PillController>>() {
        pill_ctrl.set_enabled(enabled);
    }
    let app_handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(menu_state) = app_handle.try_state::<std::sync::Arc<TrayMenuState>>() {
            let _ = menu_state.pill_item.set_checked(enabled);
        }
    });
    if from_tray {
        update_settings_json(app, |json| {
            let obj = json.as_object_mut();
            if let Some(root) = obj {
                let sys = root
                    .entry("systemSettings")
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(sys_obj) = sys.as_object_mut() {
                    sys_obj.insert("persistentPillEnabled".to_string(), serde_json::json!(enabled));
                }
            }
        });
        let _ = app.emit(
            "persistent-pill-changed",
            serde_json::json!({ "enabled": enabled }),
        );
    }
}

pub fn select_input_device(app: &tauri::AppHandle, device_id: &str, from_tray: bool) {
    let _ = audio::initialize_com();
    if audio::get_mute(device_id) == Ok(true) {
        let _ = audio::set_mute(device_id, false);
    }
    let state = app.state::<std::sync::Arc<vad::MonitorState>>();
    let feed = app.state::<std::sync::Arc<mic::MicFeed>>();
    vad::set_device(&state, device_id.to_string());
    feed.set_target_device(device_id.to_string());
    update_tray_active_device(app, device_id);
    if from_tray {
        let dev_owned = device_id.to_string();
        update_settings_json(app, |json| {
            if let Some(root) = json.as_object_mut() {
                root.insert("lastDeviceId".to_string(), serde_json::json!(dev_owned));
            }
        });
        let _ = app.emit(
            "device-changed",
            serde_json::json!({ "deviceId": device_id }),
        );
    }
}

pub fn apply_mute_change(app: &tauri::AppHandle, muted: bool) {
    let feed = app.state::<std::sync::Arc<mic::MicFeed>>();
    feed.set_muted(muted);

    let tray_accent = app.state::<std::sync::Arc<TrayAccentState>>();
    let accent = tray_accent.0.lock().unwrap().clone();
    if let Some(tray) = app.tray_by_id("main-tray") {
        let icon = tray_icon::generate_tray_icon(muted, &accent);
        let _ = tray.set_icon(Some(icon));
    }

    let app_for_menu = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(menu_state) = app_for_menu.try_state::<std::sync::Arc<TrayMenuState>>() {
            let _ = menu_state.muted_item.set_checked(muted);
        }
    });

    cue::play_if_enabled(muted);
    let _ = app.emit("mute-changed", serde_json::json!({ "muted": muted }));
}

#[tauri::command]
fn get_audio_devices(app: tauri::AppHandle) -> Vec<audio::AudioDevice> {
    let _ = audio::initialize_com();
    let devices = audio::get_devices().unwrap_or_default();
    sync_tray_devices(&app, devices.clone());
    devices
}

#[tauri::command]
fn set_mute_state(
    app: tauri::AppHandle,
    _device_id: String,
    muted: bool,
) -> Result<(), String> {
    apply_mute_change(&app, muted);
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
fn set_monitor_device(app: tauri::AppHandle, device_id: String) {
    select_input_device(&app, &device_id, false);
}

#[tauri::command]
fn set_persistent_pill_enabled(app: tauri::AppHandle, enabled: bool) {
    apply_pill_enabled(&app, enabled, false);
}

#[tauri::command]
fn sync_profiles_config(
    config: profiles::ProfilesConfig,
    state: tauri::State<std::sync::Arc<profiles::ProfilesState>>,
) {
    state.set_config(config);
}

#[tauri::command]
fn apply_profile(app: tauri::AppHandle, profile_id: String) -> bool {
    profiles::apply_profile_by_id(&app, &profile_id)
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
fn set_talking_popup_enabled(enabled: bool, state: tauri::State<std::sync::Arc<vad::MonitorState>>) {
    vad::set_talking_popup_enabled(&state, enabled);
}

#[tauri::command]
fn set_sound_settings(enabled: bool, volume: f32) {
    cue::set_settings(enabled, volume);
}

#[tauri::command]
fn set_tray_icon(
    app: tauri::AppHandle,
    accent_color: String,
    feed: tauri::State<std::sync::Arc<mic::MicFeed>>,
    tray_accent: tauri::State<std::sync::Arc<TrayAccentState>>,
) -> Result<(), String> {
    *tray_accent.0.lock().unwrap() = accent_color.clone();
    let icon = tray_icon::generate_tray_icon(feed.is_muted(), &accent_color);
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
fn set_always_on_top(
    app: tauri::AppHandle,
    enabled: bool,
    state: tauri::State<std::sync::Arc<app_state::WindowBehaviorState>>,
) {
    state.set_always_on_top(enabled);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_always_on_top(enabled);
    }
}

#[tauri::command]
async fn sync_hotkey_settings(
    config: hotkeys::HotkeyConfig,
    manager: tauri::State<'_, std::sync::Arc<hotkeys::HotkeyManager>>,
) -> Result<bool, String> {
    Ok(manager.sync(config).await)
}

#[tauri::command]
fn set_hotkeys_paused(
    paused: bool,
    manager: tauri::State<std::sync::Arc<hotkeys::HotkeyManager>>,
) {
    manager.set_paused(paused);
}

#[tauri::command]
fn get_pending_update(
    state: tauri::State<std::sync::Arc<PendingUpdateState>>,
) -> Option<UpdateInfo> {
    state.0.lock().unwrap().clone()
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsReport {
    app_version: String,
    virtual_mic_found: bool,
    virtual_mic_id: Option<String>,
    usbip_installed: bool,
    usbip_listening: bool,
    usbip_active_connections: usize,
    input_device_id: String,
    input_device_name: Option<String>,
    input_sample_rate_hz: u32,
    noise_engine: String,
    deepfilter_fallback_active: bool,
    buffer_underruns: u64,
    buffer_overflow_drops: u64,
    muted: bool,
    sidetone_enabled: bool,
}

#[tauri::command]
async fn get_diagnostics(
    app: tauri::AppHandle,
    feed: tauri::State<'_, std::sync::Arc<mic::MicFeed>>,
) -> Result<DiagnosticsReport, String> {
    let app_version = app.package_info().version.to_string();
    let input_device_id = feed.target_device();
    let (
        input_sample_rate_hz,
        buffer_underruns,
        buffer_overflow_drops,
        deepfilter_fallback_active,
        noise_engine,
    ) = feed.diagnostics_snapshot();
    let muted = feed.is_muted();
    let sidetone_enabled = feed.sidetone_enabled();
    let (usbip_installed, usbip_listening, usbip_active_connections) =
        server::diagnostics_snapshot();

    let id_for_lookup = input_device_id.clone();
    let (virtual_mic_id, input_device_name) = tauri::async_runtime::spawn_blocking(move || {
        let _ = audio::initialize_com();
        let vmic = audio::find_voxis_virtual_mic_id();
        let devs = audio::get_devices().unwrap_or_default();
        let name = devs
            .into_iter()
            .find(|d| d.id == id_for_lookup)
            .map(|d| d.name);
        (vmic, name)
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(DiagnosticsReport {
        app_version,
        virtual_mic_found: virtual_mic_id.is_some(),
        virtual_mic_id,
        usbip_installed,
        usbip_listening,
        usbip_active_connections,
        input_device_id,
        input_device_name,
        input_sample_rate_hz,
        noise_engine: noise_engine.to_string(),
        deepfilter_fallback_active,
        buffer_underruns,
        buffer_overflow_drops,
        muted,
        sidetone_enabled,
    })
}

#[tauri::command]
fn open_logs_folder(app: tauri::AppHandle) -> Result<(), String> {
    let log_dir = app
        .path()
        .app_log_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer.exe")
        .arg(&log_dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_virtual_mic_status() -> server::VirtualMicStatus {
    server::current_virtual_mic_status()
}

#[tauri::command]
fn retry_usbip_setup(
    feed: tauri::State<'_, std::sync::Arc<mic::MicFeed>>,
    force_reinstall: bool,
) {
    server::trigger_usbip_retry(feed.inner().clone(), force_reinstall);
}

#[tauri::command]
fn open_usbip_release_page() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd.exe")
        .args(["/C", "start", "", server::USBIP_RELEASE_URL])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
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

#[derive(Clone, serde::Serialize)]
pub(crate) struct UpdateInfo {
    pub(crate) version: String,
    pub(crate) notes: String,
}

#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    #[cfg(dev)]
    if DEV_FAKE_UPDATE.load(Ordering::Relaxed) {
        let update_info = UpdateInfo {
            version: "9.9.9".to_string(),
            notes: "Bug fixes and performance improvements.".to_string(),
        };
        let pending = app.state::<std::sync::Arc<PendingUpdateState>>();
        *pending.0.lock().unwrap() = Some(update_info.clone());
        return Ok(Some(update_info));
    }

    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body.unwrap_or_default(),
    }))
}

#[tauri::command]
fn get_scheduled_update(app: tauri::AppHandle) -> Option<updater::ScheduledUpdateInfo> {
    updater::read_persisted_state(&app).scheduled
}

#[tauri::command]
fn get_scheduled_update_progress(
    state: tauri::State<std::sync::Arc<updater::UpdaterRuntimeState>>,
) -> Option<updater::ScheduledUpdateProgress> {
    state.active_progress.lock().unwrap().clone()
}

#[tauri::command]
fn schedule_update_for_reopen(app: tauri::AppHandle, version: String, notes: String) {
    updater::schedule_for_reopen(&app, version, notes);
}

#[tauri::command]
fn cancel_scheduled_update(app: tauri::AppHandle) {
    updater::cancel_scheduled(&app);
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(dev)]
    if DEV_FAKE_UPDATE.load(Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        if DEV_FAKE_UPDATE_FAIL.load(Ordering::Relaxed) {
            return Err("error sending request for url (https://github.com/Squibble34/Voxis-Mic-Control/releases/latest/download/latest.json): connection closed".to_string());
        }
        updater::cancel_scheduled(&app);
        let pending = app.state::<std::sync::Arc<PendingUpdateState>>();
        *pending.0.lock().unwrap() = None;
        let _ = app.emit("dev-update-cleared", ());
        return Ok(());
    }

    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update found".to_string())?;
    let bytes = update
        .download(|_downloaded, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    updater::mark_installing_and_save_relaunch_mode(&app);
    if let Err(err) = update.install(bytes) {
        updater::clear_post_update_mode(&app);
        return Err(err.to_string());
    }
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

fn create_main_window(
    app: &tauri::AppHandle,
    visible: bool,
    minimized: bool,
) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(existing) = app.get_webview_window("main") {
        return Ok(existing);
    }

    let window_cfg = app
        .config()
        .app
        .windows
        .first()
        .ok_or_else(|| tauri::Error::AssetNotFound("main window config".into()))?;

    let main_win = tauri::WebviewWindowBuilder::from_config(app, window_cfg)?
        .visible(false)
        .build()?;

    // restore position before showing so it never flashes at tauris default centered spot
    if let Some((x, y)) = read_saved_window_position(app) {
        if let Ok(monitors) = main_win.available_monitors() {
            if position_is_visible(&monitors, x, y) {
                let _ = main_win.set_position(tauri::PhysicalPosition::new(x, y));
            }
        }
    }

    let behavior = app.state::<std::sync::Arc<app_state::WindowBehaviorState>>();
    let _ = main_win.set_always_on_top(behavior.always_on_top());

    let behavior_for_event = behavior.inner().clone();
    let win_for_handler = main_win.clone();
    main_win.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            if !behavior_for_event.get() {
                api.prevent_close();
                win_for_handler.app_handle().exit(0);
            }
        }
        tauri::WindowEvent::Destroyed => {
            let app_handle = win_for_handler.app_handle();
            let feed = app_handle.state::<std::sync::Arc<mic::MicFeed>>();
            feed.set_sidetone_enabled(false);
            let hotkeys = app_handle.state::<std::sync::Arc<hotkeys::HotkeyManager>>();
            hotkeys.set_paused(false);
        }
        _ => {}
    });

    if visible {
        let _ = main_win.show();
        if minimized {
            let _ = main_win.minimize();
        } else {
            let _ = main_win.unminimize();
            let _ = main_win.set_focus();
        }
    }

    Ok(main_win)
}

pub fn show_or_create_main_window(app: &tauri::AppHandle) {
    let user_shown = app.state::<std::sync::Arc<UserShownWindow>>();
    user_shown.0.store(true, Ordering::Relaxed);

    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = handle.get_webview_window("main") {
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
        } else if let Err(err) = create_main_window(&handle, true, false) {
            log::error!("[window] failed to recreate main window: {err:?}");
        }
    });
}

fn apply_saved_settings(
    app: &tauri::AppHandle,
    feed: &mic::MicFeed,
    monitor_state: &vad::MonitorState,
    behavior: &app_state::WindowBehaviorState,
    tray_accent: &TrayAccentState,
) -> (String, hotkeys::HotkeyConfig, Vec<audio::AudioDevice>) {
    let json = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join("settings.json")).ok())
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok());

    let preferred_id = json
        .as_ref()
        .and_then(|j| j.get("lastDeviceId"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let auto_default_mic = json
        .as_ref()
        .and_then(|j| j.get("noiseSettings"))
        .and_then(|n| n.get("autoSetDefaultMic"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // run com calls off the main thread so tao oleinitialize keeps sta for later window creation
    let (target_device, devices) = std::thread::spawn(move || {
        let _ = audio::initialize_com();
        let devices = audio::get_devices().unwrap_or_default();
        let chosen = preferred_id
            .filter(|id| devices.iter().any(|d| &d.id == id))
            .or_else(|| devices.first().map(|d| d.id.clone()));
        if let Some(ref target) = chosen {
            if audio::get_mute(target) == Ok(true) {
                let _ = audio::set_mute(target, false);
            }
        }
        if auto_default_mic {
            let _ = audio::set_voxis_as_default_mic();
        }
        (chosen, devices)
    })
    .join()
    .ok()
    .unwrap_or((None, Vec::new()));

    if let Some(target) = target_device {
        vad::set_device(monitor_state, target.clone());
        feed.set_target_device(target);
    }

    let mut initial_accent = "#28d6df".to_string();
    let mut hotkey_config = hotkeys::HotkeyConfig::default();

    let Some(json) = json else {
        *tray_accent.0.lock().unwrap() = initial_accent.clone();
        return (initial_accent, hotkey_config, devices);
    };

    if let Some(saved_gain) = json.get("softwareGain").and_then(|v| v.as_f64()) {
        feed.set_gain(saved_gain as f32);
    }
    if let Some(noise) = json.get("noiseSettings") {
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

    if let Some(sys) = json.get("systemSettings") {
        let sound_enabled = sys
            .get("soundEnabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let sound_volume = sys
            .get("soundVolume")
            .and_then(|v| v.as_f64())
            .unwrap_or(70.0) as f32;
        cue::set_settings(sound_enabled, sound_volume);

        if let Some(enabled) = sys.get("autoMuteEnabled").and_then(|v| v.as_bool()) {
            vad::set_enabled(monitor_state, enabled);
        }
        if let Some(mins) = sys.get("autoMuteMinutes").and_then(|v| v.as_u64()) {
            vad::set_idle_timeout_secs(monitor_state, mins.saturating_mul(60));
        }
        if let Some(talking) = sys.get("talkingWhileMutedPopup").and_then(|v| v.as_bool()) {
            vad::set_talking_popup_enabled(monitor_state, talking);
        }
    }

    if let Some(win) = json.get("windowsSettings") {
        if let Some(min_to_tray) = win.get("minimizeToTrayOnClose").and_then(|v| v.as_bool()) {
            behavior.set(min_to_tray);
        }
        if let Some(top) = win.get("alwaysOnTop").and_then(|v| v.as_bool()) {
            behavior.set_always_on_top(top);
        }
    }

    if let Some(app_cfg) = json.get("appearanceSettings") {
        let dark = app_cfg
            .get("theme")
            .and_then(|v| v.as_str())
            .map(|t| t != "light")
            .unwrap_or(true);
        let default_hex = if dark { "#28d6df" } else { "#00919d" };
        initial_accent = app_cfg
            .get("accentColor")
            .and_then(|v| v.as_str())
            .unwrap_or(default_hex)
            .to_string();
    }
    *tray_accent.0.lock().unwrap() = initial_accent.clone();

    if let Some(keybinds) = json.get("keybindsSettings") {
        if let Ok(parsed) = serde_json::from_value::<hotkeys::HotkeyConfig>(keybinds.clone()) {
            hotkey_config = parsed;
        }
    }

    (initial_accent, hotkey_config, devices)
}

fn ensure_single_instance() -> bool {
    #[cfg(dev)]
    let mutex_name = w!("Local\\VoxisMicControlSingleInstanceDev");
    #[cfg(not(dev))]
    let mutex_name = w!("Local\\VoxisMicControlSingleInstance");

    unsafe {
        let _ = CreateMutexW(std::ptr::null(), 1, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            #[cfg(dev)]
            eprintln!("[dev] another dev instance already holds Local\\VoxisMicControlSingleInstanceDev");
            let _ = popup::wake_existing_instance();
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
    let pending_update_state = std::sync::Arc::new(PendingUpdateState::default());
    let updater_runtime_state = std::sync::Arc::new(updater::UpdaterRuntimeState::default());
    let user_shown_window = std::sync::Arc::new(UserShownWindow::default());
    let mic_feed = std::sync::Arc::new(mic::MicFeed::new());
    let profiles_state = std::sync::Arc::new(profiles::ProfilesState::default());

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
        .manage(pending_update_state.clone())
        .manage(updater_runtime_state.clone())
        .manage(user_shown_window.clone())
        .manage(mic_feed.clone())
        .manage(profiles_state.clone())
        .setup(move |app| {
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .level_for("df", log::LevelFilter::Warn)
                    .level_for("tract_linalg", log::LevelFilter::Warn)
                    .build(),
            )?;
            let (initial_accent, initial_hotkeys, initial_devices) = apply_saved_settings(
                app.handle(),
                &mic_feed,
                &monitor_state,
                &window_behavior_state,
                &tray_accent_state,
            );
            mic::spawn_capture_worker(mic_feed.clone());
            mic::spawn_sidetone_worker(mic_feed.clone());
            server::spawn_usbip_server(app.handle().clone(), mic_feed.clone());
            vad::spawn_watcher(app.handle().clone(), monitor_state.clone(), mic_feed.clone());
            meter::spawn_watcher(app.handle().clone(), monitor_state.clone(), mic_feed.clone());
            audio::spawn_device_watcher(app.handle().clone());
            let popup_controller = popup::spawn_popup(app.handle());
            app.manage(popup_controller);
            let pill_controller = pill::spawn_pill(app.handle(), mic_feed.is_muted());
            let pill_enabled = pill_controller.is_enabled();
            app.manage(pill_controller);
            profiles_state.set_config(profiles::load_saved_profiles(app.handle()));
            profiles::spawn_watcher(
                app.handle().clone(),
                monitor_state.clone(),
                icon_cache.clone(),
                profiles_state.clone(),
            );
            let hotkey_manager = hotkeys::HotkeyManager::spawn(app.handle().clone());
            hotkey_manager.apply_nonblocking(initial_hotkeys);
            app.manage(hotkey_manager);
            #[cfg(dev)]
            dev_console::spawn(app.handle().clone());

            let show_item = MenuItem::with_id(app, "show", "Show MicControl", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let muted_item =
                CheckMenuItem::with_id(app, "tray-muted", "Muted", true, mic_feed.is_muted(), None::<&str>)?;
            let pill_item = CheckMenuItem::with_id(
                app,
                "tray-pill",
                "Show Persistent Pill",
                true,
                pill_enabled,
                None::<&str>,
            )?;
            let device_submenu = Submenu::with_id(app, "tray-devices", "Input Device", true)?;
            let active_dev_id = vad::get_device(&monitor_state);
            let mut device_items = Vec::new();
            if initial_devices.is_empty() {
                let empty_item =
                    MenuItem::with_id(app, "dev-none", "No input devices", false, None::<&str>)?;
                device_submenu.append(&empty_item)?;
            } else {
                for dev in &initial_devices {
                    let item = CheckMenuItem::with_id(
                        app,
                        format!("dev:{}", dev.id),
                        &dev.name,
                        true,
                        dev.id == active_dev_id,
                        None::<&str>,
                    )?;
                    device_submenu.append(&item)?;
                    device_items.push((dev.id.clone(), item));
                }
            }
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(
                app,
                &[
                    &show_item,
                    &sep1,
                    &muted_item,
                    &pill_item,
                    &device_submenu,
                    &sep2,
                    &quit_item,
                ],
            )?;

            app.manage(std::sync::Arc::new(TrayMenuState {
                muted_item,
                pill_item,
                device_submenu,
                device_items: std::sync::Mutex::new(device_items),
            }));

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon::generate_tray_icon(mic_feed.is_muted(), &initial_accent))
                .tooltip("Voxis")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let id = event.id().as_ref();
                    if id == "show" {
                        show_or_create_main_window(app);
                    } else if id == "quit" {
                        app.exit(0);
                    } else if id == "tray-muted" {
                        let monitor_state = app.state::<std::sync::Arc<vad::MonitorState>>();
                        if vad::get_device(&monitor_state).is_empty() {
                            if let Some(ms) = app.try_state::<std::sync::Arc<TrayMenuState>>() {
                                let _ = ms.muted_item.set_checked(false);
                            }
                            return;
                        }
                        let feed = app.state::<std::sync::Arc<mic::MicFeed>>();
                        let new_muted = !feed.is_muted();
                        apply_mute_change(app, new_muted);
                    } else if id == "tray-pill" {
                        let next = app
                            .try_state::<std::sync::Arc<pill::PillController>>()
                            .map(|p| !p.is_enabled())
                            .unwrap_or(false);
                        apply_pill_enabled(app, next, true);
                    } else if let Some(dev_id) = id.strip_prefix("dev:") {
                        let app_clone = app.clone();
                        let target_id = dev_id.to_string();
                        std::thread::spawn(move || {
                            select_input_device(&app_clone, &target_id, true);
                        });
                    }
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
                        if vad::get_device(&monitor_state).is_empty() {
                            return;
                        }
                        let feed = app.state::<std::sync::Arc<mic::MicFeed>>();
                        let new_muted = !feed.is_muted();
                        apply_mute_change(app, new_muted);
                    }
                })
                .build(app)?;

            let just_updated_mode = updater::take_post_update_mode(app.handle());
            let mut started_scheduled_update = false;

            if let Some(restored_mode) = just_updated_mode {
                updater::set_launch_intent_mode(app.handle(), &restored_mode);
                match restored_mode.as_str() {
                    "tray" => {}
                    "minimized" => {
                        create_main_window(app.handle(), true, true)?;
                    }
                    _ => {
                        create_main_window(app.handle(), true, false)?;
                    }
                }
            } else {
                let autostart = is_likely_autostart_launch();
                if autostart {
                    let app_handle = app.handle().clone();
                    let (delay_secs, startup_state) = read_windows_startup_settings(&app_handle);
                    updater::set_launch_intent_mode(&app_handle, &startup_state);
                    std::thread::spawn(move || {
                        if startup_state == "tray" {
                            return;
                        }
                        if delay_secs > 0 {
                            std::thread::sleep(std::time::Duration::from_secs(delay_secs));
                        }
                        let user_shown = app_handle.state::<std::sync::Arc<UserShownWindow>>();
                        if user_shown.0.load(Ordering::Relaxed) {
                            return;
                        }
                        let minimized = startup_state == "minimized";
                        let handle_for_main = app_handle.clone();
                        let _ = app_handle.run_on_main_thread(move || {
                            if handle_for_main.get_webview_window("main").is_none() {
                                let _ = create_main_window(&handle_for_main, true, minimized);
                            }
                        });
                    });
                } else {
                    updater::set_launch_intent_mode(app.handle(), "visible");
                    create_main_window(app.handle(), true, false)?;
                }

                if let Some(scheduled) = updater::read_persisted_state(app.handle()).scheduled {
                    started_scheduled_update = true;
                    updater::spawn_scheduled_update_runner(app.handle().clone(), scheduled, autostart);
                }
            }

            if !started_scheduled_update {
                // check for updates 8s after launch so it doesnt compete with startup
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    use tauri_plugin_updater::UpdaterExt;
                    match app_handle.updater() {
                        Ok(updater) => match updater.check().await {
                            Ok(Some(update)) => {
                                let update_info = UpdateInfo {
                                    version: update.version,
                                    notes: update.body.clone().unwrap_or_default(),
                                };
                                let pending = app_handle.state::<std::sync::Arc<PendingUpdateState>>();
                                *pending.0.lock().unwrap() = Some(update_info.clone());
                                let _ = app_handle.emit("update-available", &update_info);
                            }
                            Ok(None) => {}
                            Err(e) => log::warn!("[updater] check failed: {e:?}"),
                        },
                        Err(e) => log::warn!("[updater] could not get updater: {e:?}"),
                    }
                });
            }

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
            set_talking_popup_enabled,
            set_persistent_pill_enabled,
            sync_profiles_config,
            apply_profile,
            set_sound_settings,
            set_tray_icon,
            set_minimize_to_tray_on_close,
            set_always_on_top,
            sync_hotkey_settings,
            set_hotkeys_paused,
            get_pending_update,
            get_scheduled_update,
            get_scheduled_update_progress,
            schedule_update_for_reopen,
            cancel_scheduled_update,
            get_diagnostics,
            get_virtual_mic_status,
            retry_usbip_setup,
            open_usbip_release_page,
            open_logs_folder,
            get_capturing_sessions,
            check_for_update,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                // code is only set on explicit app.exit(), not when closing the last window to tray
                if code.is_none() && !server::is_shutting_down() {
                    api.prevent_exit();
                } else {
                    server::mark_shutting_down();
                }
            }
            tauri::RunEvent::Exit => {
                server::mark_shutting_down();
            }
            _ => {}
        });
}