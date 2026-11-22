use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::{
    apply_mute_change, apply_pill_enabled, mic, pill, popup, profiles, server, sessions,
    show_or_create_main_window, updater, vad, PendingUpdateState, UpdateInfo, DEV_FAKE_UPDATE,
    DEV_FAKE_UPDATE_FAIL,
};

const DEV_ADDR: &str = "127.0.0.1:47329";

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(listener) = TcpListener::bind(DEV_ADDR) else {
            eprintln!("[dev] could not bind control socket on {DEV_ADDR}");
            return;
        };
        println!("[dev] control socket ready on {DEV_ADDR} (run 'npm run devctl' in another terminal)");

        for stream in listener.incoming().flatten() {
            let app_handle = app.clone();
            std::thread::spawn(move || handle_connection(&app_handle, stream));
        }
    });
}

fn handle_connection(app: &AppHandle, mut stream: TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }

    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; content_length.min(16 * 1024)];
    if !body.is_empty() && reader.read_exact(&mut body).is_err() {
        return;
    }

    let cmd = String::from_utf8_lossy(&body).trim().to_string();
    let out = if cmd.is_empty() {
        help_text()
    } else {
        execute_command(app, &cmd)
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        out.len(),
        out
    );
    let _ = stream.write_all(response.as_bytes());
}

fn help_text() -> String {
    [
        "Available dev commands:",
        "  help | ?                             Show this command list",
        "  update trigger [version] [notes]     Show the update-available banner/state (default 9.9.9, 'none' for empty notes)",
        "  update clear                         Clear simulated pending update",
        "  update fake <on|off|toggle>          Bypass GitHub check so 'Check for updates' finds 9.9.9",
        "  update fail <on|off|toggle>          Simulate install_update error when fake update is on",
        "  update schedule [version]            Schedule update for next reopen (persisted)",
        "  update cancel-schedule               Cancel scheduled update",
        "  update run-scheduled [fail]          Simulate scheduled update flow (or failure fallback)",
        "  update relaunch <visible|minimized|tray>  Save post-update mode for next dev restart",
        "  status <state>                       Set app status banner/bar:",
        "                                       ready | reconnecting | disconnected | permission-denied",
        "                                       no-devices | in-use | volume-unsupported",
        "  popup <mute|unmute|talking>          Trigger native Win32 mute/unmute/warning popup",
        "  talking <on|off|flash>               Toggle talking-while-muted state or flash popup",
        "  mute-error [message]                 Show red mute error banner in main window",
        "  hotkey-error <on|off>                Toggle hotkey conflict warning banner",
        "  vmic <state>                         Set virtual mic / USBip banner state:",
        "                                       ready | installing | missing | install-failed | attach-failed",
        "  mute | unmute | toggle               Change mic mute state (updates tray, pill, popup)",
        "  pill <on|off|toggle>                 Toggle persistent mute pill overlay",
        "  level <0-100>                        Send fake input level to VU meter",
        "  apps mock <a,b,...> | apps clear     Mock active capturing apps (tests UI + profile switch)",
        "  profile list | profile apply <q>     List saved profiles or apply one by name/id",
        "  window <show|tray>                   Open main window or close it to tray",
        "  diag                                 Print live audio/virtual-mic/dev state summary",
        "  quit                                 Exit the app",
    ]
    .join("\n")
}

fn execute_command(app: &AppHandle, input: &str) -> String {
    let mut parts = input.split_whitespace();
    let Some(cmd) = parts.next() else {
        return String::new();
    };
    let rest: Vec<&str> = parts.collect();

    match cmd.to_ascii_lowercase().as_str() {
        "ping" => "pong".to_string(),
        "help" | "?" => help_text(),
        "update" => handle_update(app, &rest),
        "status" => handle_status(app, &rest),
        "popup" => handle_popup(app, &rest),
        "talking" => handle_talking(app, &rest),
        "mute-error" => {
            let msg = if rest.is_empty() {
                "Couldn't change mute state".to_string()
            } else {
                rest.join(" ")
            };
            let _ = app.emit("dev-mute-error", msg);
            "emitted dev-mute-error".to_string()
        }
        "hotkey-error" => {
            let on = parse_toggle_arg(rest.first().copied(), true);
            let _ = app.emit("dev-hotkey-error", on);
            format!("hotkey-error -> {on}")
        }
        "vmic" => handle_vmic(&rest),
        "mute" => {
            apply_mute_change(app, true);
            "muted".to_string()
        }
        "unmute" => {
            apply_mute_change(app, false);
            "unmuted".to_string()
        }
        "toggle" => {
            let feed = app.state::<Arc<mic::MicFeed>>();
            let next = !feed.is_muted();
            apply_mute_change(app, next);
            format!("muted -> {next}")
        }
        "pill" => handle_pill(app, &rest),
        "level" => {
            let lvl = rest
                .first()
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(65.0)
                .clamp(0.0, 100.0);
            let _ = app.emit("input-level-changed", lvl);
            format!("input level -> {lvl}")
        }
        "apps" => handle_apps(&rest),
        "profile" => handle_profile(app, &rest),
        "window" => handle_window(app, &rest),
        "diag" => format_diag(app),
        "quit" | "exit" => {
            let handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                handle.exit(0);
            });
            "exiting voxis".to_string()
        }
        other => format!("unknown command '{other}' (type 'help')"),
    }
}

fn parse_toggle_arg(arg: Option<&str>, current: bool) -> bool {
    match arg.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("on" | "true" | "1") => true,
        Some("off" | "false" | "0") => false,
        _ => !current,
    }
}

fn handle_update(app: &AppHandle, args: &[&str]) -> String {
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("trigger") => {
            let version = args.get(1).copied().unwrap_or("9.9.9").to_string();
            let notes = if args.len() <= 2 {
                "Bug fixes and performance improvements.".to_string()
            } else if args[2].eq_ignore_ascii_case("none") {
                String::new()
            } else {
                args[2..].join(" ")
            };
            let update_info = UpdateInfo {
                version: version.clone(),
                notes,
            };
            let pending = app.state::<Arc<PendingUpdateState>>();
            *pending.0.lock().unwrap() = Some(update_info.clone());
            let _ = app.emit("update-available", &update_info);
            format!("triggered update popup/banner for v{version}")
        }
        Some("clear") => {
            let pending = app.state::<Arc<PendingUpdateState>>();
            *pending.0.lock().unwrap() = None;
            let _ = app.emit("dev-update-cleared", ());
            "cleared pending update".to_string()
        }
        Some("fake") => {
            let cur = DEV_FAKE_UPDATE.load(Ordering::Relaxed);
            let next = parse_toggle_arg(args.get(1).copied(), cur);
            DEV_FAKE_UPDATE.store(next, Ordering::Relaxed);
            format!("update fake bypass -> {next}")
        }
        Some("fail") => {
            let cur = DEV_FAKE_UPDATE_FAIL.load(Ordering::Relaxed);
            let next = parse_toggle_arg(args.get(1).copied(), cur);
            DEV_FAKE_UPDATE_FAIL.store(next, Ordering::Relaxed);
            format!("update fail simulation -> {next}")
        }
        Some("schedule") => {
            let version = args.get(1).copied().unwrap_or("9.9.9").to_string();
            let notes = "Bug fixes and performance improvements.".to_string();
            updater::schedule_for_reopen(app, version.clone(), notes);
            format!("scheduled update v{version} for next reopen")
        }
        Some("cancel-schedule") => {
            updater::cancel_scheduled(app);
            "cancelled scheduled update".to_string()
        }
        Some("run-scheduled") => {
            let should_fail = args.get(1).is_some_and(|s| s.eq_ignore_ascii_case("fail"));
            let scheduled = updater::read_persisted_state(app).scheduled.unwrap_or(
                updater::ScheduledUpdateInfo {
                    version: "9.9.9".to_string(),
                    notes: "Bug fixes and performance improvements.".to_string(),
                },
            );
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<Arc<updater::UpdaterRuntimeState>>();
                state.scheduled_cancel.store(false, Ordering::Relaxed);

                for phase in ["waiting-network", "downloading", "installing"] {
                    if phase == "installing" && should_fail {
                        updater::restore_banner_after_scheduled_failure(&app_handle, scheduled);
                        return;
                    }
                    updater::set_progress(
                        &app_handle,
                        Some(updater::ScheduledUpdateProgress {
                            version: scheduled.version.clone(),
                            phase: phase.to_string(),
                        }),
                    );
                    for _ in 0..8 {
                        if state.scheduled_cancel.load(Ordering::Relaxed) {
                            updater::set_progress(&app_handle, None);
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
                updater::cancel_scheduled(&app_handle);
            });
            if should_fail {
                "running simulated scheduled update (will fail and restore banner)".to_string()
            } else {
                "running simulated scheduled update".to_string()
            }
        }
        Some("relaunch") => {
            let Some(mode) = args.get(1).copied() else {
                return "usage: update relaunch <visible|minimized|tray>".to_string();
            };
            if !matches!(mode, "visible" | "minimized" | "tray") {
                return format!("invalid relaunch mode '{mode}'");
            }
            let mut persisted = updater::read_persisted_state(app);
            persisted.post_update_launch = Some(updater::PostUpdateLaunch {
                mode: mode.to_string(),
                created_at_epoch_secs: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
            updater::write_persisted_state(app, &persisted);
            format!("saved post-update launch mode '{mode}' for next start")
        }
        _ => "usage: update <trigger [ver]|clear|fake|fail|schedule|cancel-schedule|run-scheduled|relaunch>".to_string(),
    }
}

fn handle_vmic(args: &[&str]) -> String {
    match args.first().copied() {
        Some("ready") => {
            server::dev_override_virtual_mic_status("ready", true, true, None);
            "vmic -> ready".to_string()
        }
        Some("installing") => {
            server::dev_override_virtual_mic_status("installing", false, false, None);
            "vmic -> installing".to_string()
        }
        Some("missing") => {
            server::dev_override_virtual_mic_status("usbip-missing", false, false, None);
            "vmic -> usbip-missing".to_string()
        }
        Some("install-failed") => {
            server::dev_override_virtual_mic_status(
                "install-failed",
                false,
                false,
                Some("Failed to download USBip installer: curl: (22) The requested URL returned error: 404".to_string()),
            );
            "vmic -> install-failed".to_string()
        }
        Some("attach-failed") => {
            server::dev_override_virtual_mic_status(
                "attach-failed",
                true,
                false,
                Some("Voxis Microphone did not register in Windows audio endpoints.".to_string()),
            );
            "vmic -> attach-failed".to_string()
        }
        _ => "usage: vmic <ready|installing|missing|install-failed|attach-failed>".to_string(),
    }
}

fn handle_status(app: &AppHandle, args: &[&str]) -> String {
    let Some(status) = args.first().copied() else {
        return "usage: status <ready|reconnecting|disconnected|permission-denied|no-devices|in-use|volume-unsupported>".to_string();
    };
    let valid = matches!(
        status,
        "ready"
            | "reconnecting"
            | "disconnected"
            | "permission-denied"
            | "no-devices"
            | "in-use"
            | "volume-unsupported"
    );
    if !valid {
        return format!("invalid status '{status}'");
    }
    let _ = app.emit("dev-status", status);
    format!("status -> {status}")
}

fn handle_popup(app: &AppHandle, args: &[&str]) -> String {
    let Some(ctrl) = app.try_state::<Arc<popup::PopupController>>() else {
        return "popup controller not initialized".to_string();
    };
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("mute") => {
            ctrl.trigger(true, false);
            "triggered native mute popup".to_string()
        }
        Some("unmute") => {
            ctrl.trigger(false, false);
            "triggered native unmute popup".to_string()
        }
        Some("talking" | "warn" | "warning") => {
            ctrl.trigger(true, true);
            "triggered native talking-while-muted warning popup".to_string()
        }
        _ => "usage: popup <mute|unmute|talking>".to_string(),
    }
}

fn handle_talking(app: &AppHandle, args: &[&str]) -> String {
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("on") => {
            let feed = app.state::<Arc<mic::MicFeed>>();
            if !feed.is_muted() {
                apply_mute_change(app, true);
            }
            let _ = app.emit("talking-while-muted-state", true);
            let _ = app.emit("talking-while-muted", ());
            "talking-while-muted -> on".to_string()
        }
        Some("off") => {
            let _ = app.emit("talking-while-muted-state", false);
            "talking-while-muted -> off".to_string()
        }
        Some("flash") => {
            let _ = app.emit("talking-while-muted", ());
            "flashed talking-while-muted popup".to_string()
        }
        _ => "usage: talking <on|off|flash>".to_string(),
    }
}

fn handle_pill(app: &AppHandle, args: &[&str]) -> String {
    let cur = app
        .try_state::<Arc<pill::PillController>>()
        .map(|p| p.is_enabled())
        .unwrap_or(false);
    let next = parse_toggle_arg(args.first().copied(), cur);
    apply_pill_enabled(app, next, true);
    format!("persistent pill -> {next}")
}

fn handle_apps(args: &[&str]) -> String {
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("mock") => {
            let joined = args[1..].join(" ");
            let list: Vec<String> = joined
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if list.is_empty() {
                return "usage: apps mock <discord,obs64,...>".to_string();
            }
            sessions::set_mock_capturing_apps(Some(list.clone()));
            format!("mocking capturing apps: {:?}", list)
        }
        Some("clear") => {
            sessions::set_mock_capturing_apps(None);
            "cleared mock capturing apps (using real Windows sessions)".to_string()
        }
        _ => "usage: apps <mock app1,app2 | clear>".to_string(),
    }
}

fn handle_profile(app: &AppHandle, args: &[&str]) -> String {
    let Some(state) = app.try_state::<Arc<profiles::ProfilesState>>() else {
        return "profiles state unavailable".to_string();
    };
    let cfg = state.get_config();
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("list") => {
            if cfg.profiles.is_empty() {
                return "no saved profiles".to_string();
            }
            let mut lines = vec![format!("auto-switch enabled: {}", cfg.auto_switch_enabled)];
            for p in &cfg.profiles {
                let mut tags = Vec::new();
                if cfg.active_profile_id.as_deref() == Some(p.id.as_str()) {
                    tags.push("active");
                }
                if cfg.default_profile_id.as_deref() == Some(p.id.as_str()) {
                    tags.push("default");
                }
                lines.push(format!("  {} ({}) {:?}", p.name, p.id, tags));
            }
            lines.join("\n")
        }
        Some("apply") => {
            let query = args[1..].join(" ").to_ascii_lowercase();
            if query.is_empty() {
                return "usage: profile apply <name or id>".to_string();
            }
            let found = cfg
                .profiles
                .iter()
                .find(|p| p.id == query || p.name.to_ascii_lowercase() == query);
            match found {
                Some(p) => {
                    profiles::apply_profile_by_id(app, &p.id);
                    format!("applied profile '{}' ({})", p.name, p.id)
                }
                None => format!("profile not found: '{query}'"),
            }
        }
        _ => "usage: profile <list|apply <name|id>>".to_string(),
    }
}

fn handle_window(app: &AppHandle, args: &[&str]) -> String {
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("show" | "open") => {
            show_or_create_main_window(app);
            "showed main window".to_string()
        }
        Some("tray" | "hide" | "close") => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(win) = handle.get_webview_window("main") {
                    let _ = win.close();
                }
            });
            "closed main window to tray".to_string()
        }
        _ => "usage: window <show|tray>".to_string(),
    }
}

fn format_diag(app: &AppHandle) -> String {
    let feed = app.state::<Arc<mic::MicFeed>>();
    let monitor_state = app.state::<Arc<vad::MonitorState>>();
    let (sample_rate, underruns, overflows, df_fallback, engine) = feed.diagnostics_snapshot();
    format!(
        "device='{}' rate={}Hz engine={} df_fallback={} muted={} sidetone={} underruns={} overflows={} fake_update={}",
        vad::get_device(&monitor_state),
        sample_rate,
        engine,
        df_fallback,
        feed.is_muted(),
        feed.sidetone_enabled(),
        underruns,
        overflows,
        DEV_FAKE_UPDATE.load(Ordering::Relaxed),
    )
}
