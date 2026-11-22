use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::{
    core::*,
    Win32::Foundation::*,
    Win32::Graphics::Gdi::*,
    Win32::Media::Audio::*,
    Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
    Win32::System::Com::CLSCTX_ALL,
    Win32::System::Threading::*,
    Win32::UI::Shell::*,
    Win32::UI::WindowsAndMessaging::*,
};
use image::ImageEncoder;

use crate::audio;

// exe path > data url, icons never change for a given exe so we only extract once
pub type IconCache = Mutex<HashMap<String, String>>;

// wasapi keeps inactive session entries alive until the app exits, so we drop idle sessions after a short grace window
const IDLE_GRACE_DURATION: Duration = Duration::from_secs(15);

fn recent_active_pids() -> &'static Mutex<HashMap<u32, Instant>> {
    static RECENT: OnceLock<Mutex<HashMap<u32, Instant>>> = OnceLock::new();
    RECENT.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize, Clone)]
pub struct TrackedAppInfo {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub icon: Option<String>,
}

#[cfg(dev)]
static MOCK_CAPTURING_APPS: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();

#[cfg(dev)]
pub fn set_mock_capturing_apps(apps: Option<Vec<String>>) {
    let cell = MOCK_CAPTURING_APPS.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = apps;
}

pub fn get_capturing_sessions(device_id: &str, icon_cache: &IconCache) -> Result<Vec<TrackedAppInfo>> {
    #[cfg(dev)]
    if let Some(cell) = MOCK_CAPTURING_APPS.get() {
        if let Some(mock_names) = cell.lock().unwrap().clone() {
            return Ok(mock_names
                .into_iter()
                .enumerate()
                .map(|(idx, name)| TrackedAppInfo {
                    id: format!("mock-{}", idx + 1),
                    name,
                    active: true,
                    icon: None,
                })
                .collect());
        }
    }

    let our_pid = unsafe { GetCurrentProcessId() };
    let now = Instant::now();
    let mut pid_active: HashMap<u32, bool> = HashMap::new();

    let mut endpoint_ids = Vec::new();
    if let Some(voxis_id) = audio::find_voxis_virtual_mic_id() {
        endpoint_ids.push(voxis_id);
    }
    if !device_id.is_empty() && !endpoint_ids.iter().any(|id| id == device_id) {
        endpoint_ids.push(device_id.to_string());
    }

    for endpoint_id in endpoint_ids {
        let Ok(device) = audio::get_device(&endpoint_id) else { continue };
        let Ok(session_manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) }) else {
            continue;
        };
        let Ok(enumerator) = (unsafe { session_manager.GetSessionEnumerator() }) else { continue };
        let count = unsafe { enumerator.GetCount() }.unwrap_or(0);

        for i in 0..count {
            let Ok(control) = (unsafe { enumerator.GetSession(i) }) else { continue };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };

            let pid = unsafe { control2.GetProcessId() }.unwrap_or(0);
            if pid == 0 || pid == our_pid {
                continue; // pid 0 is the system sounds session, skip that and our own process
            }

            let state = unsafe { control2.GetState() }.unwrap_or(AudioSessionStateInactive);
            let active = state == AudioSessionStateActive;
            pid_active
                .entry(pid)
                .and_modify(|was_active| *was_active |= active)
                .or_insert(active);
        }
    }

    let mut apps: Vec<TrackedAppInfo> = Vec::new();
    let mut recent = recent_active_pids().lock().unwrap();
    recent.retain(|_, last_seen| now.saturating_duration_since(*last_seen) <= IDLE_GRACE_DURATION);

    for (pid, active) in pid_active {
        if active {
            recent.insert(pid, now);
        } else if !recent.contains_key(&pid) {
            continue;
        }

        let Some(exe_path) = process_exe_path(pid) else { continue };
        let name = friendly_process_name(&exe_path);
        let icon = icon_data_url(&exe_path, icon_cache);

        apps.push(TrackedAppInfo {
            id: pid.to_string(),
            name,
            active,
            icon,
        });
    }

    apps.sort_by(|a, b| b.active.cmp(&a.active).then_with(|| a.name.cmp(&b.name)));
    Ok(apps)
}

fn process_exe_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if ok.is_err() {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..size as usize]))
    }
}

fn friendly_process_name(exe_path: &str) -> String {
    std::path::Path::new(exe_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| exe_path.to_string())
}

fn icon_data_url(exe_path: &str, cache: &IconCache) -> Option<String> {
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(exe_path) {
            return Some(cached.clone());
        }
    }

    let data_url = extract_icon_as_data_url(exe_path)?;

    if let Ok(mut guard) = cache.lock() {
        guard.insert(exe_path.to_string(), data_url.clone());
    }

    Some(data_url)
}

fn extract_icon_as_data_url(exe_path: &str) -> Option<String> {
    unsafe {
        let hstr = HSTRING::from(exe_path);
        let mut file_info = SHFILEINFOW::default();
        let flags = SHGFI_ICON | SHGFI_LARGEICON;
        let result = SHGetFileInfoW(
            PCWSTR::from_raw(hstr.as_ptr()),
            FILE_ATTRIBUTE_NORMAL,
            Some(&mut file_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        );
        if result == 0 || file_info.hIcon.is_invalid() {
            return None;
        }

        let png_bytes = icon_to_png(file_info.hIcon);
        let _ = DestroyIcon(file_info.hIcon);

        png_bytes.map(|bytes| {
            use base64::Engine;
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            )
        })
    }
}

// gdi only gives us icons as an hicon, so pull the raw bitmap out and encode it as png by hand
fn icon_to_png(hicon: HICON) -> Option<Vec<u8>> {
    unsafe {
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            return None;
        }

        let mut bitmap = BITMAP::default();
        if GetObjectW(
            icon_info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut _ as *mut _),
        ) == 0
        {
            let _ = DeleteObject(icon_info.hbmColor.into());
            let _ = DeleteObject(icon_info.hbmMask.into());
            return None;
        }

        let width = bitmap.bmWidth;
        let height = bitmap.bmHeight;
        let mut buffer = vec![0u8; (width * height * 4) as usize];

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative height tells gdi to give us rows top down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let hdc = GetDC(None);
        let copied = GetDIBits(
            hdc,
            icon_info.hbmColor,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, hdc);
        let _ = DeleteObject(icon_info.hbmColor.into());
        let _ = DeleteObject(icon_info.hbmMask.into());

        if copied == 0 {
            return None;
        }

        // gdi hands back bgra, everything downstream expects rgba
        for px in buffer.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        let img = image::RgbaImage::from_raw(width as u32, height as u32, buffer)?;
        let mut png_bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png_bytes)
            .write_image(&img, width as u32, height as u32, image::ExtendedColorType::Rgba8)
            .ok()?;
        Some(png_bytes)
    }
}