use std::f32::consts::{FRAC_PI_4, PI};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use tauri::{AppHandle, Listener, Manager};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetMessageW, KillTimer,
    PostMessageW, RegisterClassExW, SetTimer, SetWindowPos, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE, MSG,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, ULW_ALPHA, WM_APP, WM_ERASEBKGND, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const POPUP_SIZE: f64 = 96.0;
const MONITOR_MARGIN: f64 = 16.0;
const CARD_SIZE: f32 = 64.0;
const CARD_RADIUS: f32 = 16.0;
const CARD_BORDER: f32 = 2.0;
const ICON_SIZE: f32 = 28.0;
const ICON_GRID: f32 = 24.0;
const ICON_HALF_STROKE: f32 = 1.0;

const VISIBLE_MS: u32 = 2000;
const FADE_MS: f32 = 200.0;
const FRAME_INTERVAL_MS: u32 = 33;
const MIC_ANIM_MS: f32 = 600.0;
const SLASH_DELAY_MS: f32 = 150.0;
const SLASH_ANIM_MS: f32 = 400.0;
const SLASH_FADE_MS: f32 = 100.0;

const WM_APP_SHOW: u32 = WM_APP + 1;
const WM_APP_WAKE_MAIN: u32 = WM_APP + 2;
const TIMER_ANIM: usize = 1;
const TIMER_HIDE: usize = 2;

#[derive(Clone)]
pub(crate) struct RgbColor {
    pub(crate) r: f32,
    pub(crate) g: f32,
    pub(crate) b: f32,
}

impl RgbColor {
    pub(crate) fn from_oklch(l: f32, c: f32, h_deg: f32) -> Self {
        let h_rad = h_deg.to_radians();
        let a = c * h_rad.cos();
        let b = c * h_rad.sin();

        let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);

        let lin_r = 4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_;
        let lin_g = -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_;
        let lin_b = -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_;

        Self {
            r: linear_to_srgb(lin_r),
            g: linear_to_srgb(lin_g),
            b: linear_to_srgb(lin_b),
        }
    }

    pub(crate) fn from_hex(hex: &str) -> Option<Self> {
        let clean = hex.trim_start_matches('#');
        if clean.len() != 6 {
            return None;
        }
        let val = u32::from_str_radix(clean, 16).ok()?;
        Some(Self {
            r: ((val >> 16) & 0xff) as f32 / 255.0,
            g: ((val >> 8) & 0xff) as f32 / 255.0,
            b: (val & 0xff) as f32 / 255.0,
        })
    }

    pub(crate) fn mix(&self, other: &Self, t: f32) -> Self {
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
        }
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let clamped = c.clamp(0.0, 1.0);
    if clamped <= 0.0031308 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    }
}

#[derive(Clone)]
struct PopupSettings {
    popup_enabled: bool,
    popup_location: String,
    talking_while_muted_popup: bool,
    dark_theme: bool,
    accent_color: Option<String>,
    reduce_motion: bool,
}

impl Default for PopupSettings {
    fn default() -> Self {
        Self {
            popup_enabled: true,
            popup_location: "top-right".to_string(),
            talking_while_muted_popup: true,
            dark_theme: true,
            accent_color: None,
            reduce_motion: false,
        }
    }
}

#[derive(Clone)]
struct PendingTrigger {
    muted: bool,
    warning: bool,
    origin_x: i32,
    origin_y: i32,
    popup_px: i32,
    scale: f32,
    settings: PopupSettings,
}

pub struct PopupController {
    app: AppHandle,
    settings: Mutex<PopupSettings>,
    pending: Arc<Mutex<Option<PendingTrigger>>>,
    hwnd_raw: isize,
}

impl PopupController {
    pub(crate) fn trigger(&self, muted: bool, warning: bool) {
        let settings = self.settings.lock().unwrap().clone();
        if warning {
            if !settings.talking_while_muted_popup {
                return;
            }
        } else if !settings.popup_enabled {
            return;
        }

        let Some((origin_x, origin_y, popup_px, scale)) =
            compute_monitor_rect(&self.app, &settings.popup_location)
        else {
            return;
        };

        *self.pending.lock().unwrap() = Some(PendingTrigger {
            muted,
            warning,
            origin_x,
            origin_y,
            popup_px,
            scale,
            settings,
        });

        let hwnd = HWND(self.hwnd_raw as *mut core::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}

fn compute_monitor_rect(app: &AppHandle, location: &str) -> Option<(i32, i32, i32, f32)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let screen_x = monitor.position().x as f64 / scale;
    let screen_y = monitor.position().y as f64 / scale;
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;

    let logical_x = match location {
        "top-left" | "bottom-left" => screen_x + MONITOR_MARGIN,
        _ => screen_x + screen_w - POPUP_SIZE - MONITOR_MARGIN,
    };
    let logical_y = match location {
        "top-left" | "top-right" => screen_y + MONITOR_MARGIN,
        _ => screen_y + screen_h - POPUP_SIZE - MONITOR_MARGIN,
    };

    let origin_x = (logical_x * scale).round() as i32;
    let origin_y = (logical_y * scale).round() as i32;
    let popup_px = (POPUP_SIZE * scale).round().max(1.0) as i32;
    Some((origin_x, origin_y, popup_px, scale as f32))
}

fn read_saved_popup_settings(app: &AppHandle) -> PopupSettings {
    let mut settings = PopupSettings::default();
    let Ok(dir) = app.path().app_data_dir() else {
        return settings;
    };
    let Ok(contents) = std::fs::read_to_string(dir.join("settings.json")) else {
        return settings;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return settings;
    };

    if let Some(sys) = json.get("systemSettings") {
        if let Some(v) = sys.get("popupEnabled").and_then(|v| v.as_bool()) {
            settings.popup_enabled = v;
        }
        if let Some(v) = sys.get("popupLocation").and_then(|v| v.as_str()) {
            settings.popup_location = v.to_string();
        }
        if let Some(v) = sys.get("talkingWhileMutedPopup").and_then(|v| v.as_bool()) {
            settings.talking_while_muted_popup = v;
        }
    }
    if let Some(app_cfg) = json.get("appearanceSettings") {
        if let Some(v) = app_cfg.get("theme").and_then(|v| v.as_str()) {
            settings.dark_theme = v != "light";
        }
        if let Some(v) = app_cfg.get("accentColor") {
            settings.accent_color = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = app_cfg.get("reduceMotion").and_then(|v| v.as_bool()) {
            settings.reduce_motion = v;
        }
    }
    settings
}

pub fn spawn_popup(app: &AppHandle) -> Arc<PopupController> {
    let initial_settings = read_saved_popup_settings(app);
    let pending = Arc::new(Mutex::new(None::<PendingTrigger>));
    let pending_for_thread = Arc::clone(&pending);
    let app_for_thread = app.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<isize>(1);

    thread::spawn(move || {
        run_popup_loop(app_for_thread, pending_for_thread, ready_tx);
    });

    let hwnd_raw = ready_rx.recv().unwrap_or(0);
    let controller = Arc::new(PopupController {
        app: app.clone(),
        settings: Mutex::new(initial_settings),
        pending,
        hwnd_raw,
    });

    let ctrl = Arc::clone(&controller);
    app.listen("mute-changed", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if let Some(muted) = payload.get("muted").and_then(|v| v.as_bool()) {
                ctrl.trigger(muted, false);
            }
        }
    });

    let ctrl = Arc::clone(&controller);
    app.listen("talking-while-muted", move |_| {
        ctrl.trigger(true, true);
    });

    let ctrl = Arc::clone(&controller);
    app.listen("system-settings-changed", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            let mut guard = ctrl.settings.lock().unwrap();
            if let Some(v) = payload.get("popupEnabled").and_then(|v| v.as_bool()) {
                guard.popup_enabled = v;
            }
            if let Some(v) = payload.get("popupLocation").and_then(|v| v.as_str()) {
                guard.popup_location = v.to_string();
            }
            if let Some(v) = payload.get("talkingWhileMutedPopup").and_then(|v| v.as_bool()) {
                guard.talking_while_muted_popup = v;
            }
        }
    });

    let ctrl = Arc::clone(&controller);
    app.listen("appearance-settings-changed", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            let mut guard = ctrl.settings.lock().unwrap();
            if let Some(v) = payload.get("theme").and_then(|v| v.as_str()) {
                guard.dark_theme = v != "light";
            }
            if let Some(v) = payload.get("accentColor") {
                guard.accent_color = v.as_str().map(|s| s.to_string());
            }
            if let Some(v) = payload.get("reduceMotion").and_then(|v| v.as_bool()) {
                guard.reduce_motion = v;
            }
        }
    });

    controller
}

unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

struct DibSurface {
    size: i32,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: HGDIOBJ,
    bits: *mut u8,
}

impl DibSurface {
    fn ensure(slot: &mut Option<Self>, size: i32) -> Option<&mut Self> {
        let needs_new = slot.as_ref().map_or(true, |s| s.size != size);
        if needs_new {
            *slot = Self::create(size);
        }
        slot.as_mut()
    }

    fn create(size: i32) -> Option<Self> {
        unsafe {
            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            ReleaseDC(None, screen_dc);
            if mem_dc.is_invalid() {
                return None;
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0 as u32,
                    ..Default::default()
                },
                ..Default::default()
            };

            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
            let Ok(bitmap) = bitmap else {
                let _ = DeleteDC(mem_dc);
                return None;
            };
            let old_obj = SelectObject(mem_dc, bitmap.into());
            Some(Self {
                size,
                mem_dc,
                bitmap,
                old_obj,
                bits: bits as *mut u8,
            })
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.size * self.size * 4) as usize;
        unsafe { std::slice::from_raw_parts_mut(self.bits, len) }
    }

    fn push_to_window(&self, hwnd: HWND, origin_x: i32, origin_y: i32) {
        let dst = POINT { x: origin_x, y: origin_y };
        let src = POINT { x: 0, y: 0 };
        let win_size = SIZE { cx: self.size, cy: self.size };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&dst),
                Some(&win_size),
                Some(self.mem_dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for DibSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem_dc, self.old_obj);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.mem_dc);
        }
    }
}

// piggybacks on this always-alive hidden window because "main" can be destroyed while sitting in tray
pub fn wake_existing_instance() -> bool {
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("VoxisNativePopupClass"), None) {
            if !hwnd.is_invalid() {
                let _ = PostMessageW(Some(hwnd), WM_APP_WAKE_MAIN, WPARAM(0), LPARAM(0));
                return true;
            }
        }
        false
    }
}

fn run_popup_loop(
    app: AppHandle,
    pending: Arc<Mutex<Option<PendingTrigger>>>,
    ready_tx: mpsc::SyncSender<isize>,
) {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class_name = w!("VoxisNativePopupClass");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(popup_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wnd_class);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class_name,
            w!(""),
            WS_POPUP,
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

        let _ = ready_tx.send(hwnd.0 as isize);
        if hwnd.is_invalid() {
            return;
        }

        let mut surface: Option<DibSurface> = None;
        let mut active: Option<PendingTrigger> = None;
        let mut show_started = Instant::now();
        let mut hide_started: Option<Instant> = None;
        let mut anim_running = false;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_APP_WAKE_MAIN {
                crate::show_or_create_main_window(&app);
                continue;
            }

            if msg.message == WM_APP_SHOW {
                let next = pending.lock().unwrap().take();
                if let Some(trigger) = next {
                    let reduce = trigger.settings.reduce_motion;
                    let origin_x = trigger.origin_x;
                    let origin_y = trigger.origin_y;
                    let popup_px = trigger.popup_px;
                    let scale = trigger.scale;
                    active = Some(trigger);
                    show_started = Instant::now();
                    hide_started = None;

                    if let Some(surf) = DibSurface::ensure(&mut surface, popup_px) {
                        let init_progress = if reduce { 1.0 } else { 0.0 };
                        let init_ms = if reduce { MIC_ANIM_MS } else { 0.0 };
                        render_popup_frame(
                            surf.pixels_mut(),
                            popup_px,
                            scale,
                            active.as_ref().unwrap(),
                            if reduce { 1.0 } else { 0.9 },
                            init_progress,
                            init_ms,
                        );
                        surf.push_to_window(hwnd, origin_x, origin_y);
                    }

                    // re-assert topmost so fullscreen or borderless windows cannot demote it
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        origin_x,
                        origin_y,
                        popup_px,
                        popup_px,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );

                    SetTimer(Some(hwnd), TIMER_HIDE, VISIBLE_MS, None);
                    if reduce {
                        let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                        anim_running = false;
                    } else {
                        SetTimer(Some(hwnd), TIMER_ANIM, FRAME_INTERVAL_MS, None);
                        anim_running = true;
                    }
                }
                continue;
            }

            if msg.message == WM_TIMER && msg.hwnd == hwnd {
                let timer_id = msg.wParam.0;
                if timer_id == TIMER_HIDE {
                    let _ = KillTimer(Some(hwnd), TIMER_HIDE);
                    if let Some(ref trig) = active {
                        if trig.settings.reduce_motion {
                            let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                            anim_running = false;
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            active = None;
                        } else {
                            hide_started = Some(Instant::now());
                            SetTimer(Some(hwnd), TIMER_ANIM, FRAME_INTERVAL_MS, None);
                            anim_running = true;
                        }
                    }
                    continue;
                }

                if timer_id == TIMER_ANIM {
                    let Some(ref trig) = active else {
                        let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                        anim_running = false;
                        continue;
                    };

                    if let Some(hide_at) = hide_started {
                        let exit_ms = hide_at.elapsed().as_secs_f32() * 1000.0;
                        let t = (exit_ms / FADE_MS).clamp(0.0, 1.0);
                        if t >= 1.0 {
                            let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                            anim_running = false;
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            active = None;
                            hide_started = None;
                            continue;
                        }
                        let eased = ease_out(t);
                        let card_scale = 1.0 - 0.10 * eased;
                        let card_opacity = 1.0 - eased;
                        if let Some(surf) = DibSurface::ensure(&mut surface, trig.popup_px) {
                            render_popup_frame(
                                surf.pixels_mut(),
                                trig.popup_px,
                                trig.scale,
                                trig,
                                card_scale,
                                card_opacity,
                                MIC_ANIM_MS,
                            );
                            surf.push_to_window(hwnd, trig.origin_x, trig.origin_y);
                        }
                    } else {
                        let elapsed_ms = show_started.elapsed().as_secs_f32() * 1000.0;
                        let enter_t = (elapsed_ms / FADE_MS).clamp(0.0, 1.0);
                        let eased = ease_out(enter_t);
                        let card_scale = 0.90 + 0.10 * eased;
                        let card_opacity = eased;

                        let anim_done_ms = if trig.muted {
                            SLASH_DELAY_MS + SLASH_ANIM_MS
                        } else {
                            MIC_ANIM_MS
                        };
                        let done = elapsed_ms >= anim_done_ms && enter_t >= 1.0;

                        if let Some(surf) = DibSurface::ensure(&mut surface, trig.popup_px) {
                            render_popup_frame(
                                surf.pixels_mut(),
                                trig.popup_px,
                                trig.scale,
                                trig,
                                if done { 1.0 } else { card_scale },
                                if done { 1.0 } else { card_opacity },
                                elapsed_ms,
                            );
                            surf.push_to_window(hwnd, trig.origin_x, trig.origin_y);
                        }

                        if done && anim_running {
                            let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                            anim_running = false;
                        }
                    }
                    continue;
                }
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn ease_out(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

fn ease_in_out(t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0);
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}

fn capsule_bounce_y(elapsed_ms: f32) -> f32 {
    let p = (elapsed_ms / MIC_ANIM_MS).clamp(0.0, 1.0);
    if p < 0.25 {
        -3.0 * ease_in_out(p / 0.25)
    } else if p < 0.50 {
        -3.0 * (1.0 - ease_in_out((p - 0.25) / 0.25))
    } else if p < 0.75 {
        -2.0 * ease_in_out((p - 0.50) / 0.25)
    } else {
        -2.0 * (1.0 - ease_in_out((p - 0.75) / 0.25))
    }
}

fn erf_approx(x: f32) -> f32 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
            + 0.254829592)
            * t
            * (-x * x).exp();
    sign * y
}

pub(crate) fn rounded_rect_sdf(px: f32, py: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let inner_x = (half_w - radius).max(0.0);
    let inner_y = (half_h - radius).max(0.0);
    let dx = px.abs() - inner_x;
    let dy = py.abs() - inner_y;
    let out_dist = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let in_dist = dx.max(dy).min(0.0);
    out_dist + in_dist - radius
}

fn rounded_box_sdf(px: f32, py: f32, half_extent: f32, radius: f32) -> f32 {
    rounded_rect_sdf(px, py, half_extent, half_extent, radius)
}

pub(crate) fn dist_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 {
        (((px - ax) * abx + (py - ay) * aby) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn dist_capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, radius: f32) -> f32 {
    (dist_segment(px, py, ax, ay, bx, by) - radius).abs()
}

// start_angle and end_angle run clockwise in screen space (+x right, +y down), wrapping around +-pi if start > end
fn dist_arc(px: f32, py: f32, cx: f32, cy: f32, radius: f32, start: f32, end: f32) -> f32 {
    let dx = px - cx;
    let dy = py - cy;
    let angle = dy.atan2(dx);
    let inside = if start <= end {
        angle >= start && angle <= end
    } else {
        angle >= start || angle <= end
    };
    if inside {
        let r = (dx * dx + dy * dy).sqrt();
        (r - radius).abs()
    } else {
        let x0 = cx + radius * start.cos();
        let y0 = cy + radius * start.sin();
        let x1 = cx + radius * end.cos();
        let y1 = cy + radius * end.sin();
        let d0 = ((px - x0).powi(2) + (py - y0).powi(2)).sqrt();
        let d1 = ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
        d0.min(d1)
    }
}

pub(crate) fn mic_on_sdf(x: f32, y: f32, capsule_dy: f32) -> f32 {
    let d_stem = dist_segment(x, y, 12.0, 19.0, 12.0, 22.0);
    let d_left = dist_segment(x, y, 5.0, 10.0, 5.0, 12.0);
    let d_right = dist_segment(x, y, 19.0, 10.0, 19.0, 12.0);
    let d_arc = dist_arc(x, y, 12.0, 12.0, 7.0, 0.0, PI);
    let d_capsule = dist_capsule(x, y, 12.0, 5.0 + capsule_dy, 12.0, 12.0 + capsule_dy, 3.0);
    d_stem.min(d_left).min(d_right).min(d_arc).min(d_capsule)
}

pub(crate) fn mic_off_static_sdf(x: f32, y: f32) -> f32 {
    let d_stem = dist_segment(x, y, 12.0, 19.0, 12.0, 22.0);
    // lucide mic-off path breakpoints match the 2,2 -> 22,22 slash clearance
    let top_start = (-1.33f32).atan2(-2.68);
    let d_top_arc = dist_arc(x, y, 12.0, 5.0, 3.0, top_start, 0.0);
    let d_top_vert = dist_segment(x, y, 15.0, 5.0, 15.0, 9.34);
    let d_bot_vert = dist_segment(x, y, 9.0, 9.0, 9.0, 12.0);
    let d_bot_arc = dist_arc(x, y, 12.0, 12.0, 3.0, FRAC_PI_4, PI);
    let d_cradle_left_arc = dist_arc(x, y, 12.0, 12.0, 7.0, FRAC_PI_4, PI);
    let d_cradle_left_vert = dist_segment(x, y, 5.0, 12.0, 5.0, 10.0);
    let right_end = 1.23f32.atan2(6.89);
    let d_cradle_right_arc = dist_arc(x, y, 12.0, 12.0, 7.0, 0.0, right_end);
    let d_cradle_right_vert = dist_segment(x, y, 19.0, 12.0, 19.0, 10.0);

    d_stem
        .min(d_top_arc)
        .min(d_top_vert)
        .min(d_bot_vert)
        .min(d_bot_arc)
        .min(d_cradle_left_arc)
        .min(d_cradle_left_vert)
        .min(d_cradle_right_arc)
        .min(d_cradle_right_vert)
}

fn render_popup_frame(
    pixels: &mut [u8],
    size_px: i32,
    dpi_scale: f32,
    trigger: &PendingTrigger,
    card_scale: f32,
    card_opacity: f32,
    elapsed_ms: f32,
) {
    let dark = trigger.settings.dark_theme;
    let card_bg = if dark {
        RgbColor::from_oklch(0.21, 0.006, 250.0)
    } else {
        RgbColor { r: 1.0, g: 1.0, b: 1.0 }
    };

    let primary = trigger
        .settings
        .accent_color
        .as_deref()
        .and_then(RgbColor::from_hex)
        .unwrap_or_else(|| {
            if dark {
                RgbColor::from_oklch(0.8, 0.13, 200.0)
            } else {
                RgbColor::from_oklch(0.6, 0.11, 205.0)
            }
        });
    let destructive = if dark {
        RgbColor::from_oklch(0.704, 0.191, 22.216)
    } else {
        RgbColor::from_oklch(0.577, 0.245, 27.325)
    };
    let warning = if dark {
        RgbColor::from_oklch(0.79, 0.14, 76.0)
    } else {
        RgbColor::from_oklch(0.62, 0.14, 68.0)
    };

    let (accent, border_alpha, glow_blur, glow_alpha) = if trigger.warning {
        (&warning, 0.60, 12.0, 0.50)
    } else if trigger.muted {
        (&destructive, 0.50, 10.0, 0.45)
    } else {
        (&primary, 0.50, 0.0, 0.0)
    };
    let border_rgb = card_bg.mix(accent, border_alpha);

    let center_log = POPUP_SIZE as f32 * 0.5;
    let half_card = (CARD_SIZE * 0.5) * card_scale;
    let corner_r = CARD_RADIUS * card_scale;
    let border_w = CARD_BORDER * card_scale;
    let aa_log = 1.0 / dpi_scale;

    let icon_half = (ICON_SIZE * 0.5) * card_scale;
    let svg_per_log = ICON_GRID / (ICON_SIZE * card_scale);
    let capsule_dy = if !trigger.muted && !trigger.settings.reduce_motion {
        capsule_bounce_y(elapsed_ms)
    } else {
        0.0
    };

    let (slash_progress, slash_alpha) = if trigger.muted {
        if trigger.settings.reduce_motion {
            (1.0, 1.0)
        } else {
            let t = ((elapsed_ms - SLASH_DELAY_MS) / SLASH_ANIM_MS).clamp(0.0, 1.0);
            let a = ((elapsed_ms - SLASH_DELAY_MS) / SLASH_FADE_MS).clamp(0.0, 1.0);
            (ease_in_out(t), a)
        }
    } else {
        (0.0, 0.0)
    };

    let sigma = if glow_blur > 0.0 {
        (glow_blur * 0.5) * card_scale
    } else {
        1.0
    };
    let spread = 1.0 * card_scale;
    let inv_sigma_sqrt2 = 1.0 / (sigma * std::f32::consts::SQRT_2);

    for py in 0..size_px {
        let ly = (py as f32 + 0.5) / dpi_scale - center_log;
        for px in 0..size_px {
            let lx = (px as f32 + 0.5) / dpi_scale - center_log;
            let sd = rounded_box_sdf(lx, ly, half_card, corner_r);

            let card_coverage = ((0.5 - sd / aa_log)).clamp(0.0, 1.0);
            let mut out_r = 0.0f32;
            let mut out_g = 0.0f32;
            let mut out_b = 0.0f32;
            let mut out_a = 0.0f32;

            if glow_blur > 0.0 && sd > -aa_log {
                let d = (sd - spread).max(0.0);
                let g = 0.5 * (1.0 - erf_approx(d * inv_sigma_sqrt2));
                let ga = (g * glow_alpha).clamp(0.0, 1.0);
                out_r = accent.r * ga;
                out_g = accent.g * ga;
                out_b = accent.b * ga;
                out_a = ga;
            }

            if card_coverage > 0.0 {
                let border_t = ((-sd - (border_w - 0.5 * aa_log)) / aa_log).clamp(0.0, 1.0);
                let mut face = border_rgb.mix(&card_bg, border_t);

                if lx.abs() <= icon_half + 6.0 && ly.abs() <= icon_half + 6.0 {
                    let sx = (lx + icon_half) * svg_per_log;
                    let sy = (ly + icon_half) * svg_per_log;
                    let aa_svg = aa_log * svg_per_log;

                    let mut icon_cov = if trigger.muted {
                        let d = mic_off_static_sdf(sx, sy);
                        ((ICON_HALF_STROKE - d) / aa_svg + 0.5).clamp(0.0, 1.0)
                    } else {
                        let d = mic_on_sdf(sx, sy, capsule_dy);
                        ((ICON_HALF_STROKE - d) / aa_svg + 0.5).clamp(0.0, 1.0)
                    };

                    if trigger.muted && slash_alpha > 0.0 && slash_progress > 0.0 {
                        let end_coord = 2.0 + 20.0 * slash_progress;
                        let d_slash = dist_segment(sx, sy, 2.0, 2.0, end_coord, end_coord);
                        let slash_cov =
                            (((ICON_HALF_STROKE - d_slash) / aa_svg + 0.5).clamp(0.0, 1.0))
                                * slash_alpha;
                        icon_cov = icon_cov.max(slash_cov);
                    }

                    if icon_cov > 0.0 {
                        face = face.mix(accent, icon_cov);
                    }
                }

                let inv_card = 1.0 - card_coverage;
                out_r = face.r * card_coverage + out_r * inv_card;
                out_g = face.g * card_coverage + out_g * inv_card;
                out_b = face.b * card_coverage + out_b * inv_card;
                out_a = card_coverage + out_a * inv_card;
            }

            let final_a = (out_a * card_opacity).clamp(0.0, 1.0);
            let idx = ((py * size_px + px) * 4) as usize;
            pixels[idx] = (out_b * card_opacity * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels[idx + 1] = (out_g * card_opacity * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels[idx + 2] = (out_r * card_opacity * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels[idx + 3] = (final_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_premultiplied_frames_without_overflow() {
        for &(muted, warning, dark) in &[
            (false, false, true),
            (true, false, true),
            (true, true, true),
            (false, false, false),
            (true, true, false),
        ] {
            let trigger = PendingTrigger {
                muted,
                warning,
                origin_x: 0,
                origin_y: 0,
                popup_px: 120,
                scale: 1.25,
                settings: PopupSettings {
                    dark_theme: dark,
                    ..PopupSettings::default()
                },
            };
            let mut pixels = vec![0u8; 120 * 120 * 4];
            render_popup_frame(&mut pixels, 120, 1.25, &trigger, 1.0, 1.0, 250.0);

            let mut any_opaque = false;
            for px in pixels.chunks_exact(4) {
                let a = px[3];
                assert!(px[0] <= a && px[1] <= a && px[2] <= a);
                if a == 255 {
                    any_opaque = true;
                }
            }
            assert!(any_opaque);
            // outer corner should stay transparent so glow never clips against window edge
            assert_eq!(pixels[3], 0);
        }
    }
}
