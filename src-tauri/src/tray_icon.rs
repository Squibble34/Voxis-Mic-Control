use tauri::image::Image;

const ICON_SIZE: u32 = 64;
const SUPERSAMPLE: i32 = 4; // 4x4 samples per pixel for anti aliased edges
const MUTED_RGB: (u8, u8, u8) = (239, 68, 68);
const DEFAULT_RGB: (u8, u8, u8) = (79, 140, 255);

// matches lucides own 24x24 viewbox so these numbers line up with the same
// mic glyph used everywhere else in the app instead of a separately drawn shape
const GRID: f32 = 24.0;
const STROKE_WIDTH: f32 = 2.0;

fn parse_hex_rgb(hex: &str) -> (u8, u8, u8) {
    let clean = hex.trim_start_matches('#');
    match u32::from_str_radix(clean, 16) {
        Ok(value) if clean.len() == 6 => (
            ((value >> 16) & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            (value & 0xff) as u8,
        ),
        _ => DEFAULT_RGB,
    }
}

fn dist_to_segment(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (abx, aby) = (bx - ax, by - ay);
    let (apx, apy) = (x - ax, y - ay);
    let ab_len2 = abx * abx + aby * aby;
    let t = if ab_len2 > 0.0 {
        ((apx * abx + apy * aby) / ab_len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (ax + t * abx, ay + t * aby);
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()
}

fn on_line(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32, half_width: f32) -> bool {
    dist_to_segment(x, y, ax, ay, bx, by) <= half_width
}

fn on_capsule_outline(
    x: f32,
    y: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    radius: f32,
    half_width: f32,
) -> bool {
    (dist_to_segment(x, y, ax, ay, bx, by) - radius).abs() <= half_width
}

fn on_cradle_arc(x: f32, y: f32, cx: f32, cy: f32, radius: f32, half_width: f32) -> bool {
    let dy = y - cy;
    if dy < -half_width {
        return false;
    }
    let dist = ((x - cx).powi(2) + dy.powi(2)).sqrt();
    (dist - radius).abs() <= half_width
}

// draws the same mic glyph as the lucide Mic icon elsewhere in the app,
// just rebuilt from raw shapes since a tray icon needs actual pixels not svg
fn on_mic_glyph(x: f32, y: f32) -> bool {
    let half = STROKE_WIDTH / 2.0;

    // capsule head, vertical stadium from (12,5.5) to (12,12.2), radius 3
    let in_head = on_capsule_outline(x, y, 12.0, 5.5, 12.0, 12.2, 3.0, half);

    // cradle arc, lower half of a circle centered (12,12), radius 7
    let in_arc = on_cradle_arc(x, y, 12.0, 12.0, 7.0, half);

    // short verticals connecting the head to the arcs open ends
    let in_left_link = on_line(x, y, 5.0, 10.0, 5.0, 12.0, half);
    let in_right_link = on_line(x, y, 19.0, 10.0, 19.0, 12.0, half);

    // stem below the arc
    let in_stem = on_line(x, y, 12.0, 19.0, 12.0, 22.0, half);

    in_head || in_arc || in_left_link || in_right_link || in_stem
}

// same corner to corner strike lucide draws for MicOff
fn on_strike(x: f32, y: f32) -> bool {
    on_line(x, y, 2.0, 2.0, 22.0, 22.0, STROKE_WIDTH / 2.0 + 0.2)
}

pub fn generate_tray_icon(muted: bool, accent_hex: &str) -> Image<'static> {
    let (r, g, b) = if muted {
        MUTED_RGB
    } else {
        parse_hex_rgb(accent_hex)
    };

    let size = ICON_SIZE as i32;
    let mut pixels = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let scale = ICON_SIZE as f32 / GRID;

    for py in 0..size {
        for px in 0..size {
            let mut coverage = 0u32;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let x = (px as f32 + (sx as f32 + 0.5) / SUPERSAMPLE as f32) / scale;
                    let y = (py as f32 + (sy as f32 + 0.5) / SUPERSAMPLE as f32) / scale;
                    let inside = on_mic_glyph(x, y) || (muted && on_strike(x, y));
                    if inside {
                        coverage += 1;
                    }
                }
            }
            if coverage > 0 {
                let alpha = (coverage * 255 / (SUPERSAMPLE * SUPERSAMPLE) as u32) as u8;
                let idx = ((py * size + px) * 4) as usize;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = alpha;
            }
        }
    }

    Image::new_owned(pixels, ICON_SIZE, ICON_SIZE)
}