use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use tauri::{AppHandle, Listener, Manager};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetDC,
    GetGlyphOutlineW, GetTextMetricsW, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, FIXED, FW_SEMIBOLD, GDI_ERROR,
    GGO_GRAY8_BITMAP, GLYPHMETRICS, HBITMAP, HDC, HGDIOBJ, MAT2, OUT_TT_PRECIS, TEXTMETRICW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassExW,
    SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW,
    HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    ULW_ALPHA, WM_APP, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::popup::{
    dist_segment, mic_off_static_sdf, mic_on_sdf, rounded_rect_sdf, RgbColor,
};

const PILL_TOP_MARGIN: f64 = 12.0;
const PILL_SURFACE_PAD: f32 = 4.0;
const PILL_CARD_H: f32 = 28.0;
const PILL_PAD_X: f32 = 11.0;
const PILL_ICON_SIZE: f32 = 13.0;
const PILL_ICON_GAP: f32 = 6.0;
const PILL_BORDER: f32 = 1.5;
const PILL_FONT_LOGICAL: f32 = 11.0;
const ICON_GRID: f32 = 24.0;
const ICON_HALF_STROKE: f32 = 1.15;

const WM_APP_PILL_UPDATE: u32 = WM_APP + 1;

#[derive(Clone)]
struct PillState {
    enabled: bool,
    muted: bool,
    dark_theme: bool,
    accent_color: Option<String>,
}

impl Default for PillState {
    fn default() -> Self {
        Self {
            enabled: false,
            muted: false,
            dark_theme: true,
            accent_color: None,
        }
    }
}

pub struct PillController {
    state: Arc<Mutex<PillState>>,
    hwnd_raw: isize,
}

impl PillController {
    pub fn is_enabled(&self) -> bool {
        self.state.lock().unwrap().enabled
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.state.lock().unwrap().enabled = enabled;
        self.post_update();
    }

    pub fn set_muted(&self, muted: bool) {
        self.state.lock().unwrap().muted = muted;
        self.post_update();
    }

    pub fn set_appearance(&self, dark_theme: bool, accent_color: Option<String>) {
        let mut guard = self.state.lock().unwrap();
        guard.dark_theme = dark_theme;
        guard.accent_color = accent_color;
        drop(guard);
        self.post_update();
    }

    fn post_update(&self) {
        if self.hwnd_raw == 0 {
            return;
        }
        let hwnd = HWND(self.hwnd_raw as *mut core::ffi::c_void);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_PILL_UPDATE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn read_saved_pill_settings(app: &AppHandle) -> (bool, bool, Option<String>) {
    let mut enabled = false;
    let mut dark_theme = true;
    let mut accent_color = None;

    let Ok(dir) = app.path().app_data_dir() else {
        return (enabled, dark_theme, accent_color);
    };
    let Ok(contents) = std::fs::read_to_string(dir.join("settings.json")) else {
        return (enabled, dark_theme, accent_color);
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return (enabled, dark_theme, accent_color);
    };

    if let Some(sys) = json.get("systemSettings") {
        if let Some(v) = sys.get("persistentPillEnabled").and_then(|v| v.as_bool()) {
            enabled = v;
        }
    }
    if let Some(app_cfg) = json.get("appearanceSettings") {
        if let Some(v) = app_cfg.get("theme").and_then(|v| v.as_str()) {
            dark_theme = v != "light";
        }
        if let Some(v) = app_cfg.get("accentColor").and_then(|v| v.as_str()) {
            accent_color = Some(v.to_string());
        }
    }
    (enabled, dark_theme, accent_color)
}

pub fn spawn_pill(app: &AppHandle, initial_muted: bool) -> Arc<PillController> {
    let (enabled, dark_theme, accent_color) = read_saved_pill_settings(app);
    let state = Arc::new(Mutex::new(PillState {
        enabled,
        muted: initial_muted,
        dark_theme,
        accent_color,
    }));

    let state_for_thread = Arc::clone(&state);
    let app_for_thread = app.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<isize>(1);

    thread::spawn(move || {
        run_pill_loop(app_for_thread, state_for_thread, ready_tx);
    });

    let hwnd_raw = ready_rx.recv().unwrap_or(0);
    let controller = Arc::new(PillController { state, hwnd_raw });

    let ctrl = Arc::clone(&controller);
    app.listen("mute-changed", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if let Some(muted) = payload.get("muted").and_then(|v| v.as_bool()) {
                ctrl.set_muted(muted);
            }
        }
    });

    let ctrl = Arc::clone(&controller);
    app.listen("system-settings-changed", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            if let Some(v) = payload
                .get("persistentPillEnabled")
                .and_then(|v| v.as_bool())
            {
                ctrl.set_enabled(v);
            }
        }
    });

    let ctrl = Arc::clone(&controller);
    app.listen("appearance-settings-changed", move |event| {
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            let mut guard = ctrl.state.lock().unwrap();
            if let Some(v) = payload.get("theme").and_then(|v| v.as_str()) {
                guard.dark_theme = v != "light";
            }
            if let Some(v) = payload.get("accentColor") {
                guard.accent_color = v.as_str().map(|s| s.to_string());
            }
            drop(guard);
            ctrl.post_update();
        }
    });

    if enabled {
        controller.post_update();
    }

    controller
}

unsafe extern "system" fn pill_wnd_proc(
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

struct PillSurface {
    width: i32,
    height: i32,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: HGDIOBJ,
    bits: *mut u8,
}

impl PillSurface {
    fn ensure(slot: &mut Option<Self>, width: i32, height: i32) -> Option<&mut Self> {
        let needs_new = slot
            .as_ref()
            .map_or(true, |s| s.width != width || s.height != height);
        if needs_new {
            *slot = Self::create(width, height);
        }
        slot.as_mut()
    }

    fn create(width: i32, height: i32) -> Option<Self> {
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
                    biWidth: width,
                    biHeight: -height,
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
                width,
                height,
                mem_dc,
                bitmap,
                old_obj,
                bits: bits as *mut u8,
            })
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.width * self.height * 4) as usize;
        unsafe { std::slice::from_raw_parts_mut(self.bits, len) }
    }

    fn push_to_window(&self, hwnd: HWND, origin_x: i32, origin_y: i32) {
        let dst = POINT {
            x: origin_x,
            y: origin_y,
        };
        let src = POINT { x: 0, y: 0 };
        let win_size = SIZE {
            cx: self.width,
            cy: self.height,
        };
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

impl Drop for PillSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem_dc, self.old_obj);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.mem_dc);
        }
    }
}

struct RasterizedText {
    width: i32,
    height: i32,
    alpha: Vec<f32>,
}

fn rasterize_label(hdc: HDC, text: &str, dpi_scale: f32) -> RasterizedText {
    let font_px = (PILL_FONT_LOGICAL * dpi_scale).round().max(10.0) as i32;
    let mut glyphs = Vec::new();
    let mut pen_x = 0i32;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    let mut ascent = font_px;

    unsafe {
        let font = CreateFontW(
            -font_px,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(hdc, font.into());

        let mut tm = TEXTMETRICW::default();
        if GetTextMetricsW(hdc, &mut tm).as_bool() {
            ascent = tm.tmAscent;
        }

        let mat = MAT2 {
            eM11: FIXED { fract: 0, value: 1 },
            eM12: FIXED { fract: 0, value: 0 },
            eM21: FIXED { fract: 0, value: 0 },
            eM22: FIXED { fract: 0, value: 1 },
        };

        for ch in text.chars() {
            let mut gm = GLYPHMETRICS::default();
            let size = GetGlyphOutlineW(
                hdc,
                ch as u32,
                GGO_GRAY8_BITMAP,
                &mut gm,
                0,
                None,
                &mat,
            );
            if size == (GDI_ERROR as u32)
                || size == 0
                || gm.gmBlackBoxX == 0
                || gm.gmBlackBoxY == 0
            {
                pen_x += gm.gmCellIncX as i32;
                continue;
            }

            let mut buf = vec![0u8; size as usize];
            let glyph_bytes = GetGlyphOutlineW(
                hdc,
                ch as u32,
                GGO_GRAY8_BITMAP,
                &mut gm,
                size,
                Some(buf.as_mut_ptr() as *mut _),
                &mat,
            );
            if glyph_bytes != (GDI_ERROR as u32) {
                let gx = pen_x + gm.gmptGlyphOrigin.x;
                let gy = ascent - gm.gmptGlyphOrigin.y;
                min_y = min_y.min(gy);
                max_y = max_y.max(gy + gm.gmBlackBoxY as i32);
                glyphs.push((gx, gy, gm.gmBlackBoxX as usize, gm.gmBlackBoxY as usize, buf));
            }
            pen_x += gm.gmCellIncX as i32;
        }

        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
    }

    if glyphs.is_empty() || min_y >= max_y {
        return RasterizedText {
            width: pen_x.max(1),
            height: 1,
            alpha: vec![0.0; pen_x.max(1) as usize],
        };
    }

    let width = pen_x.max(1);
    let height = (max_y - min_y).max(1);
    let mut alpha = vec![0.0f32; (width * height) as usize];

    for (gx, gy, bw, bh, buf) in glyphs {
        let stride = (bw + 3) & !3;
        for row in 0..bh {
            let dst_y = gy - min_y + row as i32;
            if dst_y < 0 || dst_y >= height {
                continue;
            }
            for col in 0..bw {
                let dst_x = gx + col as i32;
                if dst_x < 0 || dst_x >= width {
                    continue;
                }
                let raw = buf[row * stride + col].min(64) as f32 / 64.0;
                let idx = (dst_y * width + dst_x) as usize;
                alpha[idx] = alpha[idx].max(raw);
            }
        }
    }

    RasterizedText {
        width,
        height,
        alpha,
    }
}

fn run_pill_loop(app: AppHandle, state: Arc<Mutex<PillState>>, ready_tx: mpsc::SyncSender<isize>) {
    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class_name = w!("VoxisNativePillClass");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(pill_wnd_proc),
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

        let mut surface: Option<PillSurface> = None;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_APP_PILL_UPDATE {
                let snap = state.lock().unwrap().clone();
                if !snap.enabled {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    continue;
                }

                let Ok(Some(monitor)) = app.primary_monitor() else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    continue;
                };
                let scale = monitor.scale_factor() as f32;
                let label = if snap.muted { "MUTED" } else { "UNMUTED" };

                // rasterize_label needs an hdc to measure text width before allocating the real surface
                if surface.is_none() {
                    let _ = PillSurface::ensure(&mut surface, 16, 16);
                }
                let Some(ref surf_ref) = surface else {
                    continue;
                };
                let text_bmp = rasterize_label(surf_ref.mem_dc, label, scale);
                let text_log_w = text_bmp.width as f32 / scale;

                let card_log_w =
                    PILL_PAD_X * 2.0 + PILL_ICON_SIZE + PILL_ICON_GAP + text_log_w;
                let win_log_w = card_log_w + PILL_SURFACE_PAD * 2.0;
                let win_log_h = PILL_CARD_H + PILL_SURFACE_PAD * 2.0;

                let win_px_w = (win_log_w * scale).round().max(1.0) as i32;
                let win_px_h = (win_log_h * scale).round().max(1.0) as i32;

                let screen_x = monitor.position().x as f64 / scale as f64;
                let screen_y = monitor.position().y as f64 / scale as f64;
                let screen_w = monitor.size().width as f64 / scale as f64;

                let logical_x = screen_x + (screen_w - win_log_w as f64) * 0.5;
                let logical_y = screen_y + PILL_TOP_MARGIN - PILL_SURFACE_PAD as f64;
                let origin_x = (logical_x * scale as f64).round() as i32;
                let origin_y = (logical_y * scale as f64).round() as i32;

                if let Some(surf) = PillSurface::ensure(&mut surface, win_px_w, win_px_h) {
                    render_pill_frame(
                        surf.pixels_mut(),
                        win_px_w,
                        win_px_h,
                        scale,
                        card_log_w,
                        &snap,
                        &text_bmp,
                    );
                    surf.push_to_window(hwnd, origin_x, origin_y);
                }

                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    origin_x,
                    origin_y,
                    win_px_w,
                    win_px_h,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                continue;
            }

            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

fn render_pill_frame(
    pixels: &mut [u8],
    width_px: i32,
    height_px: i32,
    dpi_scale: f32,
    card_log_w: f32,
    state: &PillState,
    text_bmp: &RasterizedText,
) {
    let dark = state.dark_theme;
    let card_bg = if dark {
        RgbColor::from_oklch(0.21, 0.006, 250.0)
    } else {
        RgbColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }
    };
    let primary = state
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
    let fg_text = if state.muted {
        destructive.clone()
    } else if dark {
        RgbColor {
            r: 0.94,
            g: 0.95,
            b: 0.96,
        }
    } else {
        RgbColor {
            r: 0.11,
            g: 0.12,
            b: 0.14,
        }
    };
    let icon_color = if state.muted { &destructive } else { &primary };
    let border_rgb = if state.muted {
        card_bg.mix(&destructive, if dark { 0.55 } else { 0.45 })
    } else {
        card_bg.mix(&primary, if dark { 0.38 } else { 0.30 })
    };

    let center_log_x = (width_px as f32 / dpi_scale) * 0.5;
    let center_log_y = (height_px as f32 / dpi_scale) * 0.5;
    let half_w = card_log_w * 0.5;
    let half_h = PILL_CARD_H * 0.5;
    let corner_r = half_h;
    let aa_log = 1.0 / dpi_scale;

    let icon_center_x = -half_w + PILL_PAD_X + PILL_ICON_SIZE * 0.5;
    let icon_half = PILL_ICON_SIZE * 0.5;
    let svg_per_log = ICON_GRID / PILL_ICON_SIZE;
    let aa_svg = aa_log * svg_per_log;

    let text_start_x =
        ((center_log_x - half_w + PILL_PAD_X + PILL_ICON_SIZE + PILL_ICON_GAP) * dpi_scale).round()
            as i32;
    let text_start_y = (height_px - text_bmp.height) / 2;

    for py in 0..height_px {
        let ly = (py as f32 + 0.5) / dpi_scale - center_log_y;
        for px in 0..width_px {
            let lx = (px as f32 + 0.5) / dpi_scale - center_log_x;
            let sd = rounded_rect_sdf(lx, ly, half_w, half_h, corner_r);

            let card_cov = (0.5 - sd / aa_log).clamp(0.0, 1.0);
            let mut out_r = 0.0f32;
            let mut out_g = 0.0f32;
            let mut out_b = 0.0f32;
            let mut out_a = 0.0f32;

            // shadow keeps a light mode pill readable over white windows
            if sd > -aa_log && sd < 3.5 {
                let shadow_a = ((1.0 - sd.max(0.0) / 3.5).powi(2) * 0.28).clamp(0.0, 0.28);
                out_a = shadow_a;
            }

            if card_cov > 0.0 {
                let border_t =
                    ((-sd - (PILL_BORDER - 0.5 * aa_log)) / aa_log).clamp(0.0, 1.0);
                let mut face = border_rgb.mix(&card_bg, border_t);

                if (lx - icon_center_x).abs() <= icon_half + 1.5 && ly.abs() <= icon_half + 1.5 {
                    let sx = (lx - icon_center_x + icon_half) * svg_per_log;
                    let sy = (ly + icon_half) * svg_per_log;

                    let mut icon_cov = if state.muted {
                        let d_mic = mic_off_static_sdf(sx, sy);
                        let d_slash = dist_segment(sx, sy, 2.0, 2.0, 22.0, 22.0);
                        let d = d_mic.min(d_slash);
                        ((ICON_HALF_STROKE - d) / aa_svg + 0.5).clamp(0.0, 1.0)
                    } else {
                        let d = mic_on_sdf(sx, sy, 0.0);
                        ((ICON_HALF_STROKE - d) / aa_svg + 0.5).clamp(0.0, 1.0)
                    };
                    icon_cov = icon_cov.clamp(0.0, 1.0);
                    if icon_cov > 0.0 {
                        face = face.mix(icon_color, icon_cov);
                    }
                }

                let tx = px - text_start_x;
                let ty = py - text_start_y;
                if tx >= 0 && tx < text_bmp.width && ty >= 0 && ty < text_bmp.height {
                    let text_a = text_bmp.alpha[(ty * text_bmp.width + tx) as usize];
                    if text_a > 0.0 {
                        face = face.mix(&fg_text, text_a);
                    }
                }

                let inv_card = 1.0 - card_cov;
                out_r = face.r * card_cov + out_r * inv_card;
                out_g = face.g * card_cov + out_g * inv_card;
                out_b = face.b * card_cov + out_b * inv_card;
                out_a = card_cov + out_a * inv_card;
            }

            let idx = ((py * width_px + px) * 4) as usize;
            pixels[idx] = (out_b * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels[idx + 1] = (out_g * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels[idx + 2] = (out_r * 255.0).round().clamp(0.0, 255.0) as u8;
            pixels[idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}
