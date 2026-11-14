use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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
const USBIP_INSTALLER_URL: &str =
    "https://github.com/vadimgrn/usbip-win2/releases/download/v0.9.8.1/USBip-0.9.8.1-x64.exe";
static USBIP_INSTALL_ATTEMPTED: AtomicBool = AtomicBool::new(false);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
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

pub fn spawn_usbip_server(feed: Arc<MicFeed>) {
    elevate_process_for_realtime_audio();
    let descriptors = Arc::new(MicDescriptors::new());
    thread::spawn(move || {
        if let Err(err) = serve("127.0.0.1:3240", descriptors, feed) {
            log::error!("usbip server stopped: {err}");
        }
    });
}

fn serve(address: &str, descriptors: Arc<MicDescriptors>, feed: Arc<MicFeed>) -> io::Result<()> {
    let listener = TcpListener::bind(address)?;
    log::info!("usbip server listening on {address}");
    let active_imports = Arc::new(AtomicUsize::new(0));
    spawn_attach_watchdog(Arc::clone(&feed), Arc::clone(&active_imports));

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
        let imports = Arc::clone(&active_imports);
        thread::spawn(move || {
            if let Err(err) = handle_connection(stream, &descriptors, feed, &imports) {
                log::info!("usbip connection ended: {err}");
            }
        });
    }
    Ok(())
}

// driver resets and sleep/wake silently drop the vhci attachment, so keep checking
fn spawn_attach_watchdog(feed: Arc<MicFeed>, active_imports: Arc<AtomicUsize>) {
    thread::spawn(move || {
        loop {
            if active_imports.load(Ordering::Relaxed) == 0 {
                try_attach_usbip(&feed);
            }
            thread::sleep(Duration::from_secs(6));
        }
    });
}

fn ensure_usbip_installed() -> bool {
    if Path::new(USBIP_EXE).exists() {
        return true;
    }
    if USBIP_INSTALL_ATTEMPTED.swap(true, Ordering::Relaxed) {
        return false;
    }

    let installer_path = std::env::temp_dir().join("voxis-usbip-win2-setup.exe");
    let installer_str = match installer_path.to_str() {
        Some(path) => path,
        None => return false,
    };

    log::info!("usbip-win2 not found, downloading installer from {USBIP_INSTALLER_URL}");
    let download_status = std::process::Command::new("curl.exe")
        .args(["-fsSL", "--retry", "2", "-o", installer_str, USBIP_INSTALLER_URL])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    if !matches!(download_status, Ok(status) if status.success()) || !installer_path.exists() {
        log::warn!("failed to download usbip-win2 installer");
        let _ = std::fs::remove_file(&installer_path);
        return false;
    }

    // only install the driver and cli client components so we skip the 20mb pdb debug symbols and desktop shortcut
    let ps_command = format!(
        "Start-Process -FilePath '{}' -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/COMPONENTS=main,client' -Verb RunAs -Wait",
        installer_str.replace('\'', "''")
    );
    let install_status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &ps_command])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    let _ = std::fs::remove_file(&installer_path);

    if !matches!(install_status, Ok(status) if status.success()) {
        log::warn!("usbip-win2 silent installer did not complete");
        return Path::new(USBIP_EXE).exists();
    }

    let installed = Path::new(USBIP_EXE).exists();
    if installed {
        log::info!("usbip-win2 installed silently without requiring a reboot");
    }
    installed
}

fn try_attach_usbip(feed: &MicFeed) {
    if !ensure_usbip_installed() {
        return;
    }
    let port_check = std::process::Command::new(USBIP_EXE)
        .arg("port")
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let already_attached = port_check
        .ok()
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("127.0.0.1") && stdout.contains(MIC_BUS_ID)
        })
        .unwrap_or(false);

    if !already_attached {
        match std::process::Command::new(USBIP_EXE)
            .args(["attach", "-r", "127.0.0.1", "-b", MIC_BUS_ID])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(output) if output.status.success() => {
                log::info!("auto-attached {MIC_BUS_ID} via usbip.exe");
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::warn!("usbip attach exited with {}: {}", output.status, stderr.trim());
            }
            Err(err) => log::warn!("could not run {USBIP_EXE}: {err}"),
        }
    }

    for _ in 0..15 {
        thread::sleep(Duration::from_millis(300));
        if !feed.auto_default_mic() {
            break;
        }
        if crate::audio::set_voxis_as_default_mic().is_ok() {
            break;
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    descriptors: &MicDescriptors,
    feed: Arc<MicFeed>,
    active_imports: &AtomicUsize,
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

            active_imports.fetch_add(1, Ordering::Relaxed);
            let result = serve_urbs(&mut stream, descriptors, feed);
            active_imports.fetch_sub(1, Ordering::Relaxed);
            result
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
