use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
    RegisterClassExW, TranslateMessage, MSG, SM_SHUTTINGDOWN, WM_ENDSESSION, WM_QUERYENDSESSION,
    WNDCLASSEXW, WS_OVERLAPPED,
};

use crate::descriptors::MicDescriptors;
use crate::device::{MicDevice, SetupPacket};
use crate::mic::MicFeed;
use crate::protocol::{
    self, IsoPacket, SubmitRequest, UrbHeader, CMD_SUBMIT, CMD_UNLINK, DIRECTION_IN,
    NO_ISO_PACKETS, OP_REP_DEVLIST, OP_REP_IMPORT, OP_REQ_DEVLIST, OP_REQ_IMPORT, STATUS_OK,
    STATUS_PIPE,
};

const MIC_BUS_ID: &str = "1-1";
const MIC_DEVICE_NUMBER: u32 = 1;
const USBIP_EXE: &str = r"C:\Program Files\USBip\usbip.exe";
pub const USBIP_RELEASE_URL: &str =
    "https://github.com/vadimgrn/usbip-win2/releases/tag/v.0.9.8.1";
const USBIP_INSTALLER_URL: &str =
    "https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe";
static USBIP_INSTALL_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static USBIP_INSTALL_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static USBIP_LISTENING: AtomicBool = AtomicBool::new(false);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static ACTIVE_IMPORTS: AtomicUsize = AtomicUsize::new(0);
static APP_HANDLE_CELL: OnceLock<AppHandle> = OnceLock::new();
static VIRTUAL_MIC_STATUS: OnceLock<Mutex<VirtualMicStatus>> = OnceLock::new();

pub fn mark_shutting_down() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst) || unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

fn is_shutdown_exit_code(code: Option<i32>) -> bool {
    matches!(
        code.map(|c| c as u32),
        Some(0xC000_0142 | 0xC000_013A | 0x4001_0004)
    )
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VirtualMicStatus {
    pub state: String,
    pub usbip_installed: bool,
    pub virtual_mic_found: bool,
    pub error: Option<String>,
}

fn status_store() -> &'static Mutex<VirtualMicStatus> {
    VIRTUAL_MIC_STATUS.get_or_init(|| {
        let installed = Path::new(USBIP_EXE).exists();
        Mutex::new(VirtualMicStatus {
            state: if installed {
                "attaching".to_string()
            } else {
                "usbip-missing".to_string()
            },
            usbip_installed: installed,
            virtual_mic_found: false,
            error: None,
        })
    })
}

pub fn current_virtual_mic_status() -> VirtualMicStatus {
    let mut guard = status_store().lock().unwrap();
    let installed = Path::new(USBIP_EXE).exists();
    guard.usbip_installed = installed;
    if ACTIVE_IMPORTS.load(Ordering::Relaxed) > 0 {
        guard.virtual_mic_found = true;
    }
    if !installed && matches!(guard.state.as_str(), "ready" | "attaching" | "attach-failed") {
        guard.state = "usbip-missing".to_string();
        guard.virtual_mic_found = false;
    }
    guard.clone()
}

pub fn set_virtual_mic_status(state: &str, virtual_mic_found: bool, error: Option<String>) {
    let next = {
        let mut guard = status_store().lock().unwrap();
        let next = VirtualMicStatus {
            state: state.to_string(),
            usbip_installed: Path::new(USBIP_EXE).exists(),
            virtual_mic_found,
            error,
        };
        if *guard == next {
            return;
        }
        *guard = next.clone();
        next
    };
    if let Some(app) = APP_HANDLE_CELL.get() {
        let _ = app.emit("virtual-mic-status-changed", next);
    }
}

#[cfg(dev)]
pub fn dev_override_virtual_mic_status(state: &str, usbip_installed: bool, virtual_mic_found: bool, error: Option<String>) {
    let next = VirtualMicStatus {
        state: state.to_string(),
        usbip_installed,
        virtual_mic_found,
        error,
    };
    *status_store().lock().unwrap() = next.clone();
    if let Some(app) = APP_HANDLE_CELL.get() {
        let _ = app.emit("virtual-mic-status-changed", next);
    }
}

pub fn diagnostics_snapshot() -> (bool, bool, usize) {
    (
        Path::new(USBIP_EXE).exists(),
        USBIP_LISTENING.load(Ordering::Relaxed),
        ACTIVE_IMPORTS.load(Ordering::Relaxed),
    )
}
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SEM_FAILCRITICALERRORS: u32 = 0x0001;
const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
const SHUTDOWN_PRIORITY_EARLY: u32 = 0x0380;
const THREAD_PRIORITY_TIME_CRITICAL: i32 = 15;
const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;
const PROCESS_POWER_THROTTLING: u32 = 4;
const THREAD_POWER_THROTTLING: u32 = 0;
const POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
const POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;
const POWER_THROTTLING_IGNORE_TIMER_RESOLUTION: u32 = 0x4;

#[repr(C)]
struct PowerThrottlingState {
    version: u32,
    control_mask: u32,
    state_mask: u32,
}

#[link(name = "avrt")]
unsafe extern "system" {
    fn AvSetMmThreadCharacteristicsW(task_name: *const u16, task_index: *mut u32) -> *mut core::ffi::c_void;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
    fn GetCurrentThread() -> *mut core::ffi::c_void;
    fn SetPriorityClass(process: *mut core::ffi::c_void, priority_class: u32) -> i32;
    fn SetThreadPriority(thread: *mut core::ffi::c_void, priority: i32) -> i32;
    fn SetProcessInformation(
        process: *mut core::ffi::c_void,
        info_class: u32,
        info: *const core::ffi::c_void,
        info_size: u32,
    ) -> i32;
    fn SetThreadInformation(
        thread: *mut core::ffi::c_void,
        info_class: u32,
        info: *const core::ffi::c_void,
        info_size: u32,
    ) -> i32;
    fn SetErrorMode(u_mode: u32) -> u32;
    fn SetProcessShutdownParameters(dw_level: u32, dw_flags: u32) -> i32;
}

#[link(name = "winmm")]
unsafe extern "system" {
    fn timeBeginPeriod(period_ms: u32) -> u32;
}

// prevents windows 11 from putting the app into ecoqos efficiency mode when hidden in the tray while a game is focused
pub fn elevate_process_for_realtime_audio() {
    let throttling = PowerThrottlingState {
        version: POWER_THROTTLING_CURRENT_VERSION,
        control_mask: POWER_THROTTLING_EXECUTION_SPEED | POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        state_mask: 0,
    };
    unsafe {
        // suppress modal hard error dialogs on child processes like 0xc0000142 during session teardown
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        // receive wm_queryendsession and wm_endsession early so the usbip watchdog pauses before shutdown tears down sockets
        SetProcessShutdownParameters(SHUTDOWN_PRIORITY_EARLY, 0);
        timeBeginPeriod(1);
        SetPriorityClass(GetCurrentProcess(), HIGH_PRIORITY_CLASS);
        SetProcessInformation(
            GetCurrentProcess(),
            PROCESS_POWER_THROTTLING,
            &throttling as *const _ as *const core::ffi::c_void,
            core::mem::size_of::<PowerThrottlingState>() as u32,
        );
    }
}

// matches the real-time scheduling class that wasapi gives the capture callback so game launches dont starve usb replies
pub fn promote_audio_thread() {
    let task_name: [u16; 10] = [
        b'P' as u16, b'r' as u16, b'o' as u16, b' ' as u16, b'A' as u16,
        b'u' as u16, b'd' as u16, b'i' as u16, b'o' as u16, 0,
    ];
    let mut task_index: u32 = 0;
    let throttling = PowerThrottlingState {
        version: POWER_THROTTLING_CURRENT_VERSION,
        control_mask: POWER_THROTTLING_EXECUTION_SPEED | POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        state_mask: 0,
    };
    unsafe {
        AvSetMmThreadCharacteristicsW(task_name.as_ptr(), &mut task_index);
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        SetThreadInformation(
            GetCurrentThread(),
            THREAD_POWER_THROTTLING,
            &throttling as *const _ as *const core::ffi::c_void,
            core::mem::size_of::<PowerThrottlingState>() as u32,
        );
    }
}

pub fn spawn_usbip_server(app: AppHandle, feed: Arc<MicFeed>) {
    let _ = APP_HANDLE_CELL.set(app);
    elevate_process_for_realtime_audio();
    spawn_shutdown_watcher();
    let descriptors = Arc::new(MicDescriptors::new());
    thread::spawn(move || {
        if let Err(err) = serve("127.0.0.1:3240", descriptors, feed) {
            log::error!("usbip server stopped: {err}");
        }
    });
}

// hidden top level window instead of hwnd_message so windows delivers wm_queryendsession and wm_endsession on shutdown
fn spawn_shutdown_watcher() {
    thread::spawn(|| unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class_name = w!("VoxisShutdownWatcherClass");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(shutdown_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wnd_class);

        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("VoxisShutdownWatcher"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        )
        .unwrap_or_default();
        if hwnd.is_invalid() {
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

unsafe extern "system" fn shutdown_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => {
            SHUTTING_DOWN.store(true, Ordering::SeqCst);
            log::info!("windows session ending (WM_QUERYENDSESSION), pausing usbip watchdog");
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                SHUTTING_DOWN.store(true, Ordering::SeqCst);
                if let Some(app) = APP_HANDLE_CELL.get() {
                    app.exit(0);
                }
            } else {
                SHUTTING_DOWN.store(false, Ordering::SeqCst);
                log::info!("windows shutdown cancelled (WM_ENDSESSION), resuming usbip watchdog");
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn trigger_usbip_retry(feed: Arc<MicFeed>, force_reinstall: bool) {
    if USBIP_INSTALL_IN_PROGRESS.load(Ordering::Relaxed) {
        return;
    }
    thread::spawn(move || {
        let _ = crate::audio::initialize_com();
        if force_reinstall || !Path::new(USBIP_EXE).exists() {
            USBIP_INSTALL_ATTEMPTED.store(false, Ordering::Relaxed);
            if !install_usbip(force_reinstall) {
                return;
            }
        }
        try_attach_usbip(&feed);
    });
}

fn serve(address: &str, descriptors: Arc<MicDescriptors>, feed: Arc<MicFeed>) -> io::Result<()> {
    let listener = TcpListener::bind(address)?;
    USBIP_LISTENING.store(true, Ordering::Relaxed);
    log::info!("usbip server listening on {address}");
    spawn_attach_watchdog(Arc::clone(&feed));

    for incoming in listener.incoming() {
        let stream = match incoming {
            Ok(stream) => stream,
            Err(err) => {
                log::warn!("usbip accept failed: {err}");
                continue;
            }
        };
        let descriptors = Arc::clone(&descriptors);
        let feed = Arc::clone(&feed);
        thread::spawn(move || {
            if let Err(err) = handle_connection(stream, &descriptors, feed) {
                log::info!("usbip connection ended: {err}");
            }
        });
    }
    USBIP_LISTENING.store(false, Ordering::Relaxed);
    Ok(())
}

// driver resets and sleep/wake silently drop the vhci attachment, so keep checking
fn spawn_attach_watchdog(feed: Arc<MicFeed>) {
    thread::spawn(move || {
        let _ = crate::audio::initialize_com();
        loop {
            if is_shutting_down() {
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            if ACTIVE_IMPORTS.load(Ordering::Relaxed) > 0 {
                let vmic_ready = crate::audio::find_voxis_virtual_mic_id().is_some();
                set_virtual_mic_status("ready", vmic_ready, None);
            } else if !USBIP_INSTALL_IN_PROGRESS.load(Ordering::Relaxed) {
                try_attach_usbip(&feed);
            }
            thread::sleep(Duration::from_secs(6));
        }
    });
}

fn install_usbip(force: bool) -> bool {
    if is_shutting_down() {
        return false;
    }
    if !force && Path::new(USBIP_EXE).exists() {
        return true;
    }
    if USBIP_INSTALL_IN_PROGRESS.swap(true, Ordering::Relaxed) {
        return false;
    }
    if !force && USBIP_INSTALL_ATTEMPTED.swap(true, Ordering::Relaxed) {
        USBIP_INSTALL_IN_PROGRESS.store(false, Ordering::Relaxed);
        return false;
    }
    if force {
        USBIP_INSTALL_ATTEMPTED.store(true, Ordering::Relaxed);
    }

    set_virtual_mic_status("installing", false, None);

    let installer_path = std::env::temp_dir().join("voxis-usbip-win2-setup.exe");
    let Some(installer_str) = installer_path.to_str() else {
        set_virtual_mic_status(
            "install-failed",
            false,
            Some("Could not resolve Windows temp folder path.".to_string()),
        );
        USBIP_INSTALL_IN_PROGRESS.store(false, Ordering::Relaxed);
        return false;
    };

    log::info!("usbip-win2 not found, downloading installer from {USBIP_INSTALLER_URL}");
    let download_output = std::process::Command::new("curl.exe")
        .args(["-fsSL", "--retry", "2", "-o", installer_str, USBIP_INSTALLER_URL])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let download_ok = match download_output {
        Ok(out) if out.status.success() && installer_path.exists() => true,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let msg = if stderr.is_empty() {
                format!("Failed to download USBip installer ({}).", out.status)
            } else {
                format!("Failed to download USBip installer: {stderr}")
            };
            log::warn!("{msg}");
            set_virtual_mic_status("install-failed", false, Some(msg));
            false
        }
        Err(err) => {
            let msg = format!("Could not start curl.exe to download USBip: {err}");
            log::warn!("{msg}");
            set_virtual_mic_status("install-failed", false, Some(msg));
            false
        }
    };

    if !download_ok || is_shutting_down() {
        let _ = std::fs::remove_file(&installer_path);
        USBIP_INSTALL_IN_PROGRESS.store(false, Ordering::Relaxed);
        return false;
    }

    // only install the driver and cli client components so we skip the 20mb pdb debug symbols and desktop shortcut
    let ps_command = format!(
        "exit (Start-Process -FilePath '{}' -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/COMPONENTS=main,client' -Verb RunAs -Wait -PassThru).ExitCode",
        installer_str.replace('\'', "''")
    );
    let install_output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_command])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let _ = std::fs::remove_file(&installer_path);

    let install_err = match install_output {
        Ok(out) if out.status.success() && Path::new(USBIP_EXE).exists() => None,
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Some(if stderr.is_empty() {
                format!("USBip installer was cancelled or exited with {}.", out.status)
            } else {
                format!("USBip installer failed: {stderr}")
            })
        }
        Ok(_) => Some("USBip installer finished, but C:\\Program Files\\USBip\\usbip.exe was not found.".to_string()),
        Err(err) => Some(format!("Could not launch elevated USBip installer: {err}")),
    };

    USBIP_INSTALL_IN_PROGRESS.store(false, Ordering::Relaxed);

    if let Some(err) = install_err {
        log::warn!("{err}");
        set_virtual_mic_status("install-failed", false, Some(err));
        return false;
    }

    log::info!("usbip-win2 installed silently without requiring a reboot");
    true
}

fn try_attach_usbip(feed: &MicFeed) {
    if is_shutting_down() {
        return;
    }
    if !install_usbip(false) || is_shutting_down() {
        return;
    }

    let current_state = status_store().lock().unwrap().state.clone();
    if current_state != "attach-failed" {
        set_virtual_mic_status("attaching", false, None);
    }

    let port_check = std::process::Command::new(USBIP_EXE)
        .arg("port")
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if is_shutting_down() {
        return;
    }
    if let Ok(ref output) = port_check {
        if is_shutdown_exit_code(output.status.code()) {
            log::info!("usbip port aborted during session teardown ({})", output.status);
            return;
        }
    }
    let already_attached = port_check
        .ok()
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("127.0.0.1") && stdout.contains(MIC_BUS_ID)
        })
        .unwrap_or(false);

    let mut attach_error: Option<String> = None;
    if !already_attached {
        if is_shutting_down() {
            return;
        }
        match std::process::Command::new(USBIP_EXE)
            .args(["attach", "-r", "127.0.0.1", "-b", MIC_BUS_ID])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(output) if output.status.success() => {
                log::info!("auto-attached {MIC_BUS_ID} via usbip.exe");
            }
            Ok(output) if is_shutting_down() || is_shutdown_exit_code(output.status.code()) => {
                log::info!("usbip attach aborted during session teardown ({})", output.status);
                return;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let detail = if !stderr.is_empty() { stderr } else { stdout };
                let msg = if detail.is_empty() {
                    format!("usbip attach exited with {}", output.status)
                } else {
                    detail
                };
                log::warn!("usbip attach failed: {msg}");
                attach_error = Some(msg);
            }
            Err(err) => {
                let msg = format!("Could not run usbip.exe: {err}");
                log::warn!("{msg}");
                attach_error = Some(msg);
            }
        }
    }

    let mut vmic_ready = false;
    for _ in 0..15 {
        if is_shutting_down() {
            return;
        }
        thread::sleep(Duration::from_millis(300));
        if feed.auto_default_mic() {
            let _ = crate::audio::set_voxis_as_default_mic();
        }
        if ACTIVE_IMPORTS.load(Ordering::Relaxed) > 0
            || crate::audio::find_voxis_virtual_mic_id().is_some()
        {
            vmic_ready = true;
            break;
        }
    }

    if vmic_ready {
        set_virtual_mic_status("ready", true, None);
    } else {
        let fallback_err = attach_error.unwrap_or_else(|| {
            "Voxis Microphone did not register in Windows audio endpoints. Try attaching again, reinstalling the driver, or rebooting if you just installed USBip.".to_string()
        });
        set_virtual_mic_status("attach-failed", false, Some(fallback_err));
    }
}

fn handle_connection(
    mut stream: TcpStream,
    descriptors: &MicDescriptors,
    feed: Arc<MicFeed>,
) -> io::Result<()> {
    // small replies must go out immediately, not wait to be batched
    stream.set_nodelay(true)?;
    let header = protocol::read_op_header(&mut stream)?;
    match header.code {
        OP_REQ_DEVLIST => write_device_list(&mut stream, descriptors),
        OP_REQ_IMPORT => {
            let mut requested_raw = [0u8; 32];
            stream.read_exact(&mut requested_raw)?;
            let requested_bus_id = String::from_utf8_lossy(&requested_raw)
                .trim_end_matches('\0')
                .to_string();
            if requested_bus_id != MIC_BUS_ID {
                return protocol::write_op_header(&mut stream, OP_REP_IMPORT, 1);
            }

            let mut reply: Vec<u8> = Vec::new();
            protocol::write_op_header(&mut reply, OP_REP_IMPORT, 0)?;
            protocol::write_usb_device(&mut reply, MIC_BUS_ID, MIC_DEVICE_NUMBER, descriptors)?;
            stream.write_all(&reply)?;
            log::info!("usbip import accepted for {MIC_BUS_ID}");

            ACTIVE_IMPORTS.fetch_add(1, Ordering::Relaxed);
            set_virtual_mic_status("ready", true, None);
            let urb_outcome = serve_urbs(&mut stream, descriptors, feed);
            ACTIVE_IMPORTS.fetch_sub(1, Ordering::Relaxed);
            urb_outcome
        }
        _ => Ok(()),
    }
}

fn write_device_list(stream: &mut TcpStream, descriptors: &MicDescriptors) -> io::Result<()> {
    let mut reply: Vec<u8> = Vec::new();
    protocol::write_op_header(&mut reply, OP_REP_DEVLIST, 0)?;
    reply.extend_from_slice(&1u32.to_be_bytes());
    protocol::write_usb_device(&mut reply, MIC_BUS_ID, MIC_DEVICE_NUMBER, descriptors)?;
    for [class, subclass, protocol_code] in descriptors.interface_summaries() {
        reply.extend_from_slice(&[class, subclass, protocol_code, 0]);
    }
    stream.write_all(&reply)
}

struct UrbScratch {
    request: SubmitRequest,
    reply_packets: Vec<IsoPacket>,
    mic_samples: Vec<u8>,
    wire_frame: Vec<u8>,
}

impl UrbScratch {
    fn new() -> Self {
        UrbScratch {
            request: SubmitRequest {
                header: UrbHeader {
                    command: 0,
                    sequence: 0,
                    direction: 0,
                    endpoint: 0,
                },
                buffer_length: 0,
                start_frame: 0,
                packet_count: NO_ISO_PACKETS,
                setup: [0u8; 8],
                out_payload: Vec::with_capacity(256),
                iso_packets: Vec::with_capacity(64),
            },
            reply_packets: Vec::with_capacity(64),
            mic_samples: Vec::with_capacity(8192),
            wire_frame: Vec::with_capacity(12288),
        }
    }
}

fn serve_urbs(stream: &mut TcpStream, descriptors: &MicDescriptors, feed: Arc<MicFeed>) -> io::Result<()> {
    promote_audio_thread();
    let mut device = MicDevice::new(descriptors);
    let mut capture_clock = CaptureClock { next_deadline: None, feed };
    let mut scratch = UrbScratch::new();

    loop {
        let header = match protocol::read_urb_header(stream) {
            Ok(header) => header,
            Err(err) if is_disconnect(&err) => return Ok(()),
            Err(err) => return Err(err),
        };

        match header.command {
            CMD_SUBMIT => {
                protocol::read_submit_body_into(stream, header, &mut scratch.request)?;
                handle_submit(stream, &mut device, &mut capture_clock, &mut scratch)?;
            }
            CMD_UNLINK => {
                let _target = protocol::read_unlink_body(stream)?;
                protocol::write_ret_unlink(stream, header.sequence, STATUS_OK)?;
            }
            _ => return Ok(()),
        }
    }
}

fn is_disconnect(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
    )
}

fn handle_submit(
    stream: &mut TcpStream,
    device: &mut MicDevice,
    capture_clock: &mut CaptureClock,
    scratch: &mut UrbScratch,
) -> io::Result<()> {
    let request = &scratch.request;
    let endpoint = request.header.endpoint;
    let direction = request.header.direction;

    if endpoint == 0 {
        let setup = SetupPacket::parse(&request.setup);
        return match device.handle_control(&setup, &request.out_payload) {
            Some(payload) if direction == DIRECTION_IN => {
                let sent = &payload[..payload.len().min(request.buffer_length as usize)];
                protocol::write_ret_submit_reusable(
                    stream,
                    request,
                    STATUS_OK,
                    sent.len() as u32,
                    sent,
                    &[],
                    &mut scratch.wire_frame,
                )
            }
            Some(_) => {
                let accepted = request.out_payload.len() as u32;
                protocol::write_ret_submit_reusable(
                    stream,
                    request,
                    STATUS_OK,
                    accepted,
                    &[],
                    &[],
                    &mut scratch.wire_frame,
                )
            }
            None => protocol::write_ret_submit_reusable(
                stream,
                request,
                STATUS_PIPE,
                0,
                &[],
                &[],
                &mut scratch.wire_frame,
            ),
        };
    }

    if endpoint == 1 && direction == DIRECTION_IN && request.is_isochronous() {
        capture_clock.wait_for(request.iso_packets.len());
        scratch.reply_packets.clear();
        let mut total_bytes: usize = 0;
        for packet in &request.iso_packets {
            total_bytes += packet.length as usize;
            scratch.reply_packets.push(IsoPacket {
                offset: packet.offset,
                length: packet.length,
                actual_length: packet.length,
                status: STATUS_OK,
            });
        }
        scratch.mic_samples.clear();
        scratch.mic_samples.resize(total_bytes, 0);
        capture_clock.feed.pull(&mut scratch.mic_samples);
        return protocol::write_ret_submit_reusable(
            stream,
            request,
            STATUS_OK,
            scratch.mic_samples.len() as u32,
            &scratch.mic_samples,
            &scratch.reply_packets,
            &mut scratch.wire_frame,
        );
    }

    scratch.reply_packets.clear();
    for packet in &request.iso_packets {
        scratch.reply_packets.push(IsoPacket {
            offset: packet.offset,
            length: packet.length,
            actual_length: 0,
            status: STATUS_PIPE,
        });
    }
    protocol::write_ret_submit_reusable(
        stream,
        request,
        STATUS_PIPE,
        0,
        &[],
        &scratch.reply_packets,
        &mut scratch.wire_frame,
    )
}

struct CaptureClock {
    next_deadline: Option<Instant>,
    feed: Arc<MicFeed>,
}

impl CaptureClock {
    fn wait_for(&mut self, packet_count: usize) {
        let now = Instant::now();
        // anchoring to the previous deadline stops sleep overshoot from stretching time, an idle gap resyncs to now
        let start = match self.next_deadline {
            Some(previous) if now.saturating_duration_since(previous) < Duration::from_millis(50) => previous,
            _ => now,
        };
        let deadline = start + Duration::from_millis(packet_count.min(250) as u64);
        self.next_deadline = Some(deadline);
        thread::sleep(deadline.saturating_duration_since(now));
    }
}
