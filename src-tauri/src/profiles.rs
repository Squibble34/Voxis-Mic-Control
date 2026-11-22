use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app_state, apply_pill_enabled, audio, cue, hotkeys, mic, pill, sessions, tray_icon,
    update_settings_json, vad, TrayAccentState,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoxisProfile {
    pub id: String,
    pub name: String,
    pub snapshot: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRule {
    pub id: String,
    pub app_match: String,
    pub app_label: String,
    pub profile_id: String,
    pub enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesConfig {
    #[serde(default)]
    pub profiles: Vec<VoxisProfile>,
    #[serde(default)]
    pub default_profile_id: Option<String>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default)]
    pub rules: Vec<ProfileRule>,
    #[serde(default = "default_auto_switch_enabled")]
    pub auto_switch_enabled: bool,
}

fn default_auto_switch_enabled() -> bool {
    true
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            default_profile_id: None,
            active_profile_id: None,
            rules: Vec::new(),
            auto_switch_enabled: true,
        }
    }
}

#[derive(Default)]
pub struct ProfilesState {
    config: Mutex<ProfilesConfig>,
    last_matched_rule_profile: Mutex<Option<String>>,
}

impl ProfilesState {
    pub fn set_config(&self, config: ProfilesConfig) {
        *self.config.lock().unwrap() = config;
    }

    pub fn get_config(&self) -> ProfilesConfig {
        self.config.lock().unwrap().clone()
    }
}

pub fn load_saved_profiles(app: &AppHandle) -> ProfilesConfig {
    let Ok(dir) = app.path().app_data_dir() else {
        return ProfilesConfig::default();
    };
    let Ok(contents) = std::fs::read_to_string(dir.join("settings.json")) else {
        return ProfilesConfig::default();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return ProfilesConfig::default();
    };
    json.get("profilesConfig")
        .and_then(|v| serde_json::from_value::<ProfilesConfig>(v.clone()).ok())
        .unwrap_or_default()
}

pub fn normalize_app_key(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    trimmed
        .strip_suffix(".exe")
        .unwrap_or(&trimmed)
        .to_string()
}

pub fn apply_profile_by_id(app: &AppHandle, profile_id: &str) -> bool {
    let state = app.state::<Arc<ProfilesState>>();
    let mut cfg = state.get_config();
    let Some(profile) = cfg.profiles.iter().find(|p| p.id == profile_id).cloned() else {
        return false;
    };

    cfg.active_profile_id = Some(profile.id.clone());
    state.set_config(cfg.clone());

    apply_snapshot_to_runtime(app, &profile.snapshot, Some(&profile.id), Some(&cfg));
    true
}

pub fn apply_snapshot_to_runtime(
    app: &AppHandle,
    snapshot: &serde_json::Value,
    active_profile_id: Option<&str>,
    updated_config: Option<&ProfilesConfig>,
) {
    let feed = app.state::<Arc<mic::MicFeed>>();
    let monitor_state = app.state::<Arc<vad::MonitorState>>();
    let behavior = app.state::<Arc<app_state::WindowBehaviorState>>();
    let tray_accent = app.state::<Arc<TrayAccentState>>();

    if let Some(gain) = snapshot.get("softwareGain").and_then(|v| v.as_f64()) {
        feed.set_gain(gain as f32);
    }

    if let Some(noise) = snapshot.get("noiseSettings") {
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
            if auto_def {
                thread::spawn(|| {
                    let _ = audio::initialize_com();
                    let _ = audio::set_voxis_as_default_mic();
                });
            }
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

    if let Some(sys) = snapshot.get("systemSettings") {
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
            vad::set_enabled(&monitor_state, enabled);
        }
        if let Some(mins) = sys.get("autoMuteMinutes").and_then(|v| v.as_u64()) {
            vad::set_idle_timeout_secs(&monitor_state, mins.saturating_mul(60));
        }
        if let Some(talking) = sys
            .get("talkingWhileMutedPopup")
            .and_then(|v| v.as_bool())
        {
            vad::set_talking_popup_enabled(&monitor_state, talking);
        }
        if let Some(pill_enabled) = sys.get("persistentPillEnabled").and_then(|v| v.as_bool()) {
            apply_pill_enabled(app, pill_enabled, false);
        }
        let _ = app.emit("system-settings-changed", sys.clone());
    }

    if let Some(win_cfg) = snapshot.get("windowsSettings") {
        if let Some(min_to_tray) = win_cfg
            .get("minimizeToTrayOnClose")
            .and_then(|v| v.as_bool())
        {
            behavior.set(min_to_tray);
        }
        if let Some(top) = win_cfg.get("alwaysOnTop").and_then(|v| v.as_bool()) {
            behavior.set_always_on_top(top);
            if let Some(main_win) = app.get_webview_window("main") {
                let _ = main_win.set_always_on_top(top);
            }
        }
    }

    if let Some(app_cfg) = snapshot.get("appearanceSettings") {
        let dark = app_cfg
            .get("theme")
            .and_then(|v| v.as_str())
            .map(|t| t != "light")
            .unwrap_or(true);
        let default_hex = if dark { "#28d6df" } else { "#00919d" };
        let accent = app_cfg
            .get("accentColor")
            .and_then(|v| v.as_str())
            .unwrap_or(default_hex)
            .to_string();
        *tray_accent.0.lock().unwrap() = accent.clone();
        if let Some(tray) = app.tray_by_id("main-tray") {
            let icon = tray_icon::generate_tray_icon(feed.is_muted(), &accent);
            let _ = tray.set_icon(Some(icon));
        }
        if let Some(pill_ctrl) = app.try_state::<Arc<pill::PillController>>() {
            let custom = app_cfg
                .get("accentColor")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            pill_ctrl.set_appearance(dark, custom);
        }
        let _ = app.emit("appearance-settings-changed", app_cfg.clone());
    }

    if let Some(keybinds) = snapshot.get("keybindsSettings") {
        if let Ok(parsed) = serde_json::from_value::<hotkeys::HotkeyConfig>(keybinds.clone()) {
            if let Some(hk) = app.try_state::<Arc<hotkeys::HotkeyManager>>() {
                hk.apply_nonblocking(parsed);
            }
        }
    }

    update_settings_json(app, |root| {
        let Some(obj) = root.as_object_mut() else {
            return;
        };
        for key in [
            "systemSettings",
            "appearanceSettings",
            "keybindsSettings",
            "windowsSettings",
            "noiseSettings",
            "softwareGain",
        ] {
            if let Some(val) = snapshot.get(key) {
                obj.insert(key.to_string(), val.clone());
            }
        }
        if let Some(cfg) = updated_config {
            if let Ok(cfg_val) = serde_json::to_value(cfg) {
                obj.insert("profilesConfig".to_string(), cfg_val);
            }
        }
    });

    let _ = app.emit(
        "profile-applied",
        serde_json::json!({
            "profileId": active_profile_id,
            "snapshot": snapshot,
        }),
    );
}

fn find_matching_rule_profile(
    cfg: &ProfilesConfig,
    capturing: &[sessions::TrackedAppInfo],
) -> Option<String> {
    for rule in &cfg.rules {
        if !rule.enabled {
            continue;
        }
        let target_app = normalize_app_key(&rule.app_match);
        if target_app.is_empty() {
            continue;
        }
        if !cfg.profiles.iter().any(|p| p.id == rule.profile_id) {
            continue;
        }
        let matched = capturing
            .iter()
            .any(|app_info| normalize_app_key(&app_info.name) == target_app);
        if matched {
            return Some(rule.profile_id.clone());
        }
    }
    None
}

pub fn spawn_watcher(
    app: AppHandle,
    monitor_state: Arc<vad::MonitorState>,
    icon_cache: Arc<sessions::IconCache>,
    profiles_state: Arc<ProfilesState>,
) {
    thread::spawn(move || {
        let _ = audio::initialize_com();
        loop {
            thread::sleep(Duration::from_millis(1500));

            let cfg = profiles_state.get_config();
            let has_active_rules =
                cfg.auto_switch_enabled && cfg.rules.iter().any(|r| r.enabled);
            if !has_active_rules {
                *profiles_state.last_matched_rule_profile.lock().unwrap() = None;
                continue;
            }

            let device_id = vad::get_device(&monitor_state);
            let capturing = sessions::get_capturing_sessions(&device_id, &icon_cache)
                .unwrap_or_default();

            let next_rule_profile = find_matching_rule_profile(&cfg, &capturing);
            let prev_rule_profile = {
                let mut guard = profiles_state.last_matched_rule_profile.lock().unwrap();
                let prev = guard.clone();
                if prev != next_rule_profile {
                    *guard = next_rule_profile.clone();
                }
                prev
            };

            if prev_rule_profile == next_rule_profile {
                continue;
            }

            if let Some(target_id) = next_rule_profile {
                let _ = apply_profile_by_id(&app, &target_id);
            } else if prev_rule_profile.is_some() {
                if let Some(ref default_id) = cfg.default_profile_id {
                    let _ = apply_profile_by_id(&app, default_id);
                }
            }
        }
    });
}
