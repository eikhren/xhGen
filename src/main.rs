// Crosshair generator with a native GUI preview.
// The GUI exposes every setting with a live preview and can still batch
// render SVGs from the CSV color pairs used by the original CLI.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use eframe::egui::{self, Color32, IconData, Pos2, Rgba, Stroke, color_picker, pos2, vec2};
use eframe::icon_data;
use serde::{Deserialize, Serialize};
use svg::Document;
use svg::node::element::Circle;
use svg::node::element::Group;
use svg::node::element::Path as SvgPath;
use svg::node::element::path::Data;

const USER_BASE_SUFFIX: &str = ".local/lib/xhGen";
const USER_CSV_DIR_SUFFIX: &str = "csv-library";
const USER_OUTPUT_DIR_SUFFIX: &str = "xhGenerated";
const USER_PROFILE_DIR_SUFFIX: &str = "profiles";
const SYSTEM_CSV_DIR: &str = "/usr/local/lib/xhGen/csv-library";
const DEFAULT_CSV_FILENAME: &str = "unique_crosshair_color_pairs.csv";
const MIN_CANVAS_SIZE: u32 = 64;
const MAX_CANVAS_SIZE: u32 = 8192;
const MAX_RING_OUTER_RADIUS: f64 = 4192.0;

#[derive(Clone, Serialize, Deserialize)]
struct CrosshairConfig {
    size: u32,
    ring_outer_radius: f64,
    ring_thickness: f64,
    rim_color: (u8, u8, u8, f32), // RGBA with opacity
    arm_color: (u8, u8, u8, f32), // RGBA with opacity
    gap_from_ring: f64,
    center_gap_radius: f64,
    spoke_base_width: f64,
    spoke_tip_width: f64,
    angles: Vec<f64>,
    blur_radius: f32,
    glow_radius: f32,
}

impl Default for CrosshairConfig {
    fn default() -> Self {
        Self {
            size: 256,
            ring_outer_radius: 118.0,
            ring_thickness: 20.0,
            rim_color: (255, 255, 255, 1.0),
            arm_color: (0, 0, 0, 1.0),
            gap_from_ring: 10.0,
            center_gap_radius: 2.0,
            spoke_base_width: 12.0,
            spoke_tip_width: 1.5,
            angles: vec![45.0, 135.0, 225.0, 315.0],
            blur_radius: 1.0,
            glow_radius: 2.0,
        }
    }
}

fn canvas_border_radius(size: u32) -> f64 {
    size as f64 / 2.0
}

fn ring_draw_radius(config: &CrosshairConfig) -> f64 {
    (config.ring_outer_radius - config.ring_thickness / 2.0).max(0.0)
}

fn ring_inner_radius(config: &CrosshairConfig) -> f64 {
    (config.ring_outer_radius - config.ring_thickness).max(0.0)
}

fn spoke_base_radius(config: &CrosshairConfig) -> f64 {
    (ring_inner_radius(config) - config.gap_from_ring).max(0.0)
}

fn spoke_tip_radius(config: &CrosshairConfig) -> f64 {
    config.center_gap_radius.max(0.0)
}

// ------------------------------------------------------------
// SVG GENERATION
// ------------------------------------------------------------

fn bezier_spoke(
    cx: f64,
    cy: f64,
    angle_deg: f64,
    tip_r: f64,
    base_r: f64,
    base_width: f64,
    tip_width: f64,
) -> SvgPath {
    let th = angle_deg.to_radians();
    let ux = th.cos();
    let uy = th.sin();

    let px = -uy;
    let py = ux;

    let (tip_x, tip_y) = (cx + tip_r * ux, cy + tip_r * uy);
    let (base_x, base_y) = (cx + base_r * ux, cy + base_r * uy);

    let base_half = base_width / 2.0;
    let tip_half = tip_width / 2.0;

    let (bl_x, bl_y) = (base_x - px * base_half, base_y - py * base_half);
    let (br_x, br_y) = (base_x + px * base_half, base_y + py * base_half);

    let data = if tip_width <= 0.01 {
        // Razor tip with gradual taper: wide shoulder -> slimmer mid -> narrow pinch -> point.
        let dist = (base_r - tip_r).abs();

        let shoulder_r = base_r - dist * 0.25;
        let mid_r = base_r - dist * 0.6;
        let pinch_r = base_r - dist * 0.9;

        let shoulder_half = base_half * 0.9;
        let mid_half = base_half * 0.6;
        let pinch_half = base_half * 0.18;

        let (shoulder_x, shoulder_y) = (cx + shoulder_r * ux, cy + shoulder_r * uy);
        let (mid_x, mid_y) = (cx + mid_r * ux, cy + mid_r * uy);
        let (pinch_x, pinch_y) = (cx + pinch_r * ux, cy + pinch_r * uy);

        let (sr_x, sr_y) = (
            shoulder_x + px * shoulder_half,
            shoulder_y + py * shoulder_half,
        );
        let (sl_x, sl_y) = (
            shoulder_x - px * shoulder_half,
            shoulder_y - py * shoulder_half,
        );

        let (mr_x, mr_y) = (mid_x + px * mid_half, mid_y + py * mid_half);
        let (ml_x, ml_y) = (mid_x - px * mid_half, mid_y - py * mid_half);

        let (pr_x, pr_y) = (pinch_x + px * pinch_half, pinch_y + py * pinch_half);
        let (pl_x, pl_y) = (pinch_x - px * pinch_half, pinch_y - py * pinch_half);

        Data::new()
            .move_to((bl_x, bl_y))
            .line_to((br_x, br_y))
            .line_to((sr_x, sr_y))
            .quadratic_curve_to(((sr_x + mr_x) / 2.0, (sr_y + mr_y) / 2.0, mr_x, mr_y))
            .quadratic_curve_to(((mr_x + pr_x) / 2.0, (mr_y + pr_y) / 2.0, pr_x, pr_y))
            .quadratic_curve_to(((pr_x + tip_x) / 2.0, (pr_y + tip_y) / 2.0, tip_x, tip_y))
            .quadratic_curve_to(((pl_x + tip_x) / 2.0, (pl_y + tip_y) / 2.0, pl_x, pl_y))
            .quadratic_curve_to(((ml_x + pl_x) / 2.0, (ml_y + pl_y) / 2.0, ml_x, ml_y))
            .quadratic_curve_to(((sl_x + ml_x) / 2.0, (sl_y + ml_y) / 2.0, sl_x, sl_y))
            .line_to((bl_x, bl_y))
            .close()
    } else {
        let (tl_x, tl_y) = (tip_x - px * tip_half, tip_y - py * tip_half);
        let (tr_x, tr_y) = (tip_x + px * tip_half, tip_y + py * tip_half);

        // Slightly curve the sides toward the tip to keep the beveled look while adding width at the center.
        let ctrl_br_x = (br_x + tr_x) / 2.0;
        let ctrl_br_y = (br_y + tr_y) / 2.0;
        let ctrl_bl_x = (bl_x + tl_x) / 2.0;
        let ctrl_bl_y = (bl_y + tl_y) / 2.0;

        Data::new()
            .move_to((bl_x, bl_y))
            .line_to((br_x, br_y))
            .quadratic_curve_to((ctrl_br_x, ctrl_br_y, tr_x, tr_y))
            .line_to((tl_x, tl_y))
            .quadratic_curve_to((ctrl_bl_x, ctrl_bl_y, bl_x, bl_y))
            .close()
    };

    SvgPath::new().set("d", data)
}

fn generate_svg(config: &CrosshairConfig) -> Document {
    let cx = config.size as f64 / 2.0;
    let cy = cx;

    let base_r = spoke_base_radius(config);
    let tip_r = spoke_tip_radius(config);

    let rim_color = format!(
        "rgba({},{},{},{})",
        config.rim_color.0, config.rim_color.1, config.rim_color.2, config.rim_color.3
    );

    let arm_color = format!(
        "rgba({},{},{},{})",
        config.arm_color.0, config.arm_color.1, config.arm_color.2, config.arm_color.3
    );

    let mut arms = Group::new();

    for angle in &config.angles {
        let path = bezier_spoke(
            cx,
            cy,
            *angle,
            tip_r,
            base_r,
            config.spoke_base_width,
            config.spoke_tip_width,
        )
        .set("fill", arm_color.as_str());

        arms = arms.add(path);
    }

    let ring = Circle::new()
        .set("cx", cx)
        .set("cy", cy)
        .set("r", ring_draw_radius(config))
        .set("stroke-width", config.ring_thickness)
        .set("stroke", rim_color.as_str())
        .set("fill", "none");

    Document::new()
        .set("width", config.size)
        .set("height", config.size)
        .set("viewBox", format!("0 0 {} {}", config.size, config.size))
        .add(arms)
        .add(ring)
}

// ------------------------------------------------------------
// COLOR HELPERS
// ------------------------------------------------------------

fn clamp_alpha(alpha: f32) -> f32 {
    alpha.clamp(0.0, 1.0)
}

fn open_path_in_file_manager(path: &Path) -> Result<(), String> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };

    let status = Command::new(cmd)
        .arg(path)
        .status()
        .map_err(|e| format!("Launch failed: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("File manager exited with status {}", status))
    }
}

fn tuple_to_rgba(color: (u8, u8, u8, f32)) -> Rgba {
    Rgba::from_rgba_unmultiplied(
        color.0 as f32 / 255.0,
        color.1 as f32 / 255.0,
        color.2 as f32 / 255.0,
        clamp_alpha(color.3),
    )
}

fn rgba_to_tuple(rgba: Rgba) -> (u8, u8, u8, f32) {
    let [r, g, b, a] = rgba.to_rgba_unmultiplied();
    (
        (r * 255.0).round().clamp(0.0, 255.0) as u8,
        (g * 255.0).round().clamp(0.0, 255.0) as u8,
        (b * 255.0).round().clamp(0.0, 255.0) as u8,
        clamp_alpha(a),
    )
}

fn tuple_to_color32(color: (u8, u8, u8, f32)) -> Color32 {
    let alpha = (color.3 * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(color.0, color.1, color.2, alpha)
}

fn app_icon() -> IconData {
    icon_data::from_png_bytes(include_bytes!("../resources/icon/icon_full.png"))
        .expect("bundled icon must load")
}

// ------------------------------------------------------------
// USER PATH HELPERS & SEEDING
// ------------------------------------------------------------

fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = env::var("HOME") {
        return Some(PathBuf::from(home));
    }
    if let Ok(profile) = env::var("USERPROFILE") {
        return Some(PathBuf::from(profile));
    }
    None
}

fn user_base_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(USER_BASE_SUFFIX)
}

fn user_csv_dir() -> PathBuf {
    user_base_dir().join(USER_CSV_DIR_SUFFIX)
}

fn default_csv_path() -> PathBuf {
    user_csv_dir().join(DEFAULT_CSV_FILENAME)
}

fn user_output_dir() -> PathBuf {
    user_base_dir().join(USER_OUTPUT_DIR_SUFFIX)
}

fn profiles_dir() -> PathBuf {
    user_base_dir().join(USER_PROFILE_DIR_SUFFIX)
}

fn ensure_profiles_dir() -> io::Result<PathBuf> {
    let dir = profiles_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn seed_user_csvs() -> Result<(), String> {
    let user_dir = user_csv_dir();
    fs::create_dir_all(&user_dir).map_err(|e| format!("Create dir failed: {}", e))?;

    let mut seeded = false;

    // Copy from system install if present.
    let system_dir = Path::new(SYSTEM_CSV_DIR);
    if system_dir.exists() {
        for entry in fs::read_dir(system_dir).map_err(|e| format!("Read dir failed: {}", e))? {
            let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
            if entry.path().extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            let dest = user_dir.join(entry.file_name());
            if !dest.exists() {
                fs::copy(entry.path(), &dest).map_err(|e| format!("Copy failed: {}", e))?;
                seeded = true;
            }
        }
    }

    // Copy from repo resources as a fallback.
    let repo_dir = Path::new("resources/csv-defaults");
    if repo_dir.exists() {
        for entry in fs::read_dir(repo_dir).map_err(|e| format!("Read dir failed: {}", e))? {
            let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
            if entry.path().extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            let dest = user_dir.join(entry.file_name());
            if !dest.exists() {
                fs::copy(entry.path(), &dest).map_err(|e| format!("Copy failed: {}", e))?;
                seeded = true;
            }
        }
    }

    if seeded || default_csv_path().exists() {
        Ok(())
    } else {
        Err("Could not locate default CSV files to seed the user directory.".to_string())
    }
}

fn default_csv_path_string() -> String {
    default_csv_path().to_string_lossy().to_string()
}

fn default_output_dir_string() -> String {
    user_output_dir().to_string_lossy().to_string()
}

fn sanitize_profile_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cleaned: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn profile_path(name: &str) -> Result<PathBuf, String> {
    let safe = sanitize_profile_name(name).ok_or("Enter a profile name.")?;
    let dir = ensure_profiles_dir().map_err(|e| format!("Profile dir error: {}", e))?;
    Ok(dir.join(format!("{}.json", safe)))
}

fn list_profiles() -> Result<Vec<String>, String> {
    let dir = ensure_profiles_dir().map_err(|e| format!("Profile dir error: {}", e))?;
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("Read dir failed: {}", e))? {
        let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
        let path = entry.path();
        if path.is_file() {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn save_profile_to_disk(config: &CrosshairConfig, name: &str) -> Result<PathBuf, String> {
    let path = profile_path(name)?;
    let file = fs::File::create(&path).map_err(|e| format!("Save failed: {}", e))?;
    serde_json::to_writer_pretty(file, config).map_err(|e| format!("Write failed: {}", e))?;
    Ok(path)
}

fn load_profile_from_disk(name: &str) -> Result<CrosshairConfig, String> {
    let path = profile_path(name)?;
    let data = fs::read_to_string(&path).map_err(|e| format!("Read failed: {}", e))?;
    serde_json::from_str(&data).map_err(|e| format!("Parse failed: {}", e))
}

// ------------------------------------------------------------
// PREVIEW GEOMETRY
// ------------------------------------------------------------

fn add_quad_samples(points: &mut Vec<(f64, f64)>, ctrl: (f64, f64), end: (f64, f64), steps: usize) {
    let start = match points.last().copied() {
        Some(p) => p,
        None => return,
    };

    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let omt = 1.0 - t;
        let x = omt * omt * start.0 + 2.0 * omt * t * ctrl.0 + t * t * end.0;
        let y = omt * omt * start.1 + 2.0 * omt * t * ctrl.1 + t * t * end.1;
        points.push((x, y));
    }
}

fn spoke_outline_points(
    cx: f64,
    cy: f64,
    angle_deg: f64,
    tip_r: f64,
    base_r: f64,
    base_width: f64,
    tip_width: f64,
) -> Vec<(f64, f64)> {
    let th = angle_deg.to_radians();
    let ux = th.cos();
    let uy = th.sin();

    let px = -uy;
    let py = ux;

    let tip_x = cx + tip_r * ux;
    let tip_y = cy + tip_r * uy;
    let base_x = cx + base_r * ux;
    let base_y = cy + base_r * uy;

    let base_half = base_width / 2.0;
    let tip_half = tip_width / 2.0;

    let bl_x = base_x - px * base_half;
    let bl_y = base_y - py * base_half;
    let br_x = base_x + px * base_half;
    let br_y = base_y + py * base_half;

    if tip_width <= 0.01 {
        let dist = (base_r - tip_r).abs();
        let shoulder_r = base_r - dist * 0.25;
        let mid_r = base_r - dist * 0.6;
        let pinch_r = base_r - dist * 0.9;

        let shoulder_half = base_half * 0.9;
        let mid_half = base_half * 0.6;
        let pinch_half = base_half * 0.18;

        let shoulder_x = cx + shoulder_r * ux;
        let shoulder_y = cy + shoulder_r * uy;
        let mid_x = cx + mid_r * ux;
        let mid_y = cy + mid_r * uy;
        let pinch_x = cx + pinch_r * ux;
        let pinch_y = cy + pinch_r * uy;

        let sr_x = shoulder_x + px * shoulder_half;
        let sr_y = shoulder_y + py * shoulder_half;
        let sl_x = shoulder_x - px * shoulder_half;
        let sl_y = shoulder_y - py * shoulder_half;

        let mr_x = mid_x + px * mid_half;
        let mr_y = mid_y + py * mid_half;
        let ml_x = mid_x - px * mid_half;
        let ml_y = mid_y - py * mid_half;

        let pr_x = pinch_x + px * pinch_half;
        let pr_y = pinch_y + py * pinch_half;
        let pl_x = pinch_x - px * pinch_half;
        let pl_y = pinch_y - py * pinch_half;

        let mut pts = vec![(bl_x, bl_y), (br_x, br_y), (sr_x, sr_y)];
        add_quad_samples(
            &mut pts,
            ((sr_x + mr_x) / 2.0, (sr_y + mr_y) / 2.0),
            (mr_x, mr_y),
            6,
        );
        add_quad_samples(
            &mut pts,
            ((mr_x + pr_x) / 2.0, (mr_y + pr_y) / 2.0),
            (pr_x, pr_y),
            6,
        );
        add_quad_samples(
            &mut pts,
            ((pr_x + tip_x) / 2.0, (pr_y + tip_y) / 2.0),
            (tip_x, tip_y),
            6,
        );
        add_quad_samples(
            &mut pts,
            ((pl_x + tip_x) / 2.0, (pl_y + tip_y) / 2.0),
            (pl_x, pl_y),
            6,
        );
        add_quad_samples(
            &mut pts,
            ((ml_x + pl_x) / 2.0, (ml_y + pl_y) / 2.0),
            (ml_x, ml_y),
            6,
        );
        add_quad_samples(
            &mut pts,
            ((sl_x + ml_x) / 2.0, (sl_y + ml_y) / 2.0),
            (sl_x, sl_y),
            6,
        );
        pts.push((bl_x, bl_y));
        return pts;
    }

    let tl_x = tip_x - px * tip_half;
    let tl_y = tip_y - py * tip_half;
    let tr_x = tip_x + px * tip_half;
    let tr_y = tip_y + py * tip_half;

    let ctrl_br_x = (br_x + tr_x) / 2.0;
    let ctrl_br_y = (br_y + tr_y) / 2.0;
    let ctrl_bl_x = (bl_x + tl_x) / 2.0;
    let ctrl_bl_y = (bl_y + tl_y) / 2.0;

    let mut pts = vec![(bl_x, bl_y), (br_x, br_y)];
    add_quad_samples(&mut pts, (ctrl_br_x, ctrl_br_y), (tr_x, tr_y), 6);
    pts.push((tl_x, tl_y));
    add_quad_samples(&mut pts, (ctrl_bl_x, ctrl_bl_y), (bl_x, bl_y), 6);
    pts
}

fn draw_crosshair_preview(ui: &mut egui::Ui, config: &CrosshairConfig) {
    let available = ui.available_size();
    if available.x <= 0.0 || available.y <= 0.0 {
        return;
    }

    let side = available.x.min(available.y).max(140.0);
    let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();

    let scale = side / config.size as f32;
    let half = config.size as f32 / 2.0;

    let rim_color = tuple_to_color32(config.rim_color);
    let arm_color = tuple_to_color32(config.arm_color);

    let base_r = spoke_base_radius(config);
    let tip_r = spoke_tip_radius(config);

    painter.rect_filled(rect, 8.0, ui.visuals().faint_bg_color);
    for angle in &config.angles {
        let points = spoke_outline_points(
            config.size as f64 / 2.0,
            config.size as f64 / 2.0,
            *angle,
            tip_r,
            base_r,
            config.spoke_base_width,
            config.spoke_tip_width,
        );

        if points.len() < 3 {
            continue;
        }

        let screen_points: Vec<Pos2> = points
            .into_iter()
            .map(|(x, y)| {
                pos2(
                    center.x + ((x as f32 - half) * scale),
                    center.y + ((y as f32 - half) * scale),
                )
            })
            .collect();

        painter.add(egui::Shape::convex_polygon(
            screen_points,
            arm_color,
            Stroke::NONE,
        ));
    }

    painter.circle_stroke(
        center,
        (ring_draw_radius(config) as f32 * scale).max(0.5),
        Stroke {
            width: (config.ring_thickness as f32 * scale).max(1.0),
            color: rim_color,
        },
    );

    painter.circle_filled(
        center,
        (config.center_gap_radius as f32 * scale).max(0.0),
        ui.visuals().extreme_bg_color,
    );
}

// ------------------------------------------------------------
// CSV + COLOR PARSING
// ------------------------------------------------------------

struct ColorSpec {
    rgb: (u8, u8, u8),
    hex: String,
}

fn normalize_hex(raw: &str) -> Result<String, io::Error> {
    let trimmed = raw.trim().trim_start_matches('#');
    if trimmed.len() != 6 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid hex color: {}", raw),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

fn hex_to_rgb(hex: &str) -> Result<(u8, u8, u8), io::Error> {
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid red in {}: {}", hex, e),
        )
    })?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid green in {}: {}", hex, e),
        )
    })?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid blue in {}: {}", hex, e),
        )
    })?;
    Ok((r, g, b))
}

fn parse_color_spec(raw: &str) -> Result<ColorSpec, io::Error> {
    let hex = normalize_hex(raw)?;
    let rgb = hex_to_rgb(&hex)?;
    Ok(ColorSpec { rgb, hex })
}

fn load_color_pairs(path: &str) -> Result<Vec<(ColorSpec, ColorSpec)>, Box<dyn std::error::Error>> {
    if Path::new(path) == default_csv_path() {
        let _ = seed_user_csvs();
    }

    let csv = fs::read_to_string(path)?;
    let mut pairs = Vec::new();

    for (idx, line) in csv.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }

        let (outer, inner) = line.split_once(',').ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid row {}: {}", idx + 1, line),
            )
        })?;

        pairs.push((parse_color_spec(outer)?, parse_color_spec(inner)?));
    }

    Ok(pairs)
}

fn generate_batch_svgs(
    config: &CrosshairConfig,
    csv_path: &str,
    out_dir: &Path,
    verbose: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let pairs = load_color_pairs(csv_path)?;
    fs::create_dir_all(out_dir)?;

    let mut cfg = config.clone();
    for (idx, (rim, arms)) in pairs.iter().enumerate() {
        cfg.rim_color = (rim.rgb.0, rim.rgb.1, rim.rgb.2, 1.0);
        cfg.arm_color = (arms.rgb.0, arms.rgb.1, arms.rgb.2, 1.0);

        let filename = format!("xhMan_256px-rim-{}_arms-{}.svg", rim.hex, arms.hex);
        let path = out_dir.join(&filename);

        let doc = generate_svg(&cfg);
        svg::save(&path, &doc)?;

        if verbose {
            println!("{:>3}/{} -> {}", idx + 1, pairs.len(), path.display());
        }
    }

    Ok(pairs.len())
}

// ------------------------------------------------------------
// GUI
// ------------------------------------------------------------

struct CrosshairApp {
    config: CrosshairConfig,
    output_path: String,
    csv_path: String,
    batch_dir: String,
    profile_name: String,
    selected_profile: Option<String>,
    available_profiles: Vec<String>,
    status: Option<String>,
    chain_canvas_and_radius: bool,
}

impl CrosshairApp {
    fn new() -> Self {
        let default_file = user_output_dir()
            .join("reticle-preview.svg")
            .to_string_lossy()
            .to_string();
        let profiles = list_profiles().unwrap_or_default();
        let status = seed_user_csvs().err();
        Self {
            config: CrosshairConfig::default(),
            output_path: default_file,
            csv_path: default_csv_path_string(),
            batch_dir: default_output_dir_string(),
            profile_name: String::new(),
            selected_profile: None,
            available_profiles: profiles,
            status,
            chain_canvas_and_radius: false,
        }
    }

    fn save_current_svg(&mut self) {
        let target = PathBuf::from(self.output_path.trim());
        if target.as_os_str().is_empty() {
            self.status = Some("Please enter an output file path.".to_string());
            return;
        }

        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(parent) {
                    self.status = Some(format!("Could not create folder: {}", err));
                    return;
                }
            }
        }

        match svg::save(&target, &generate_svg(&self.config)) {
            Ok(_) => self.status = Some(format!("Saved {}", target.display())),
            Err(err) => self.status = Some(format!("Save failed: {}", err)),
        }
    }

    fn generate_batch(&mut self) {
        let output_root = PathBuf::from(self.batch_dir.trim());
        match generate_batch_svgs(&self.config, self.csv_path.trim(), &output_root, false) {
            Ok(count) => {
                self.status = Some(format!(
                    "Generated {} SVGs into {}",
                    count,
                    output_root.display()
                ))
            }
            Err(err) => self.status = Some(format!("Batch failed: {}", err)),
        }
    }

    fn open_default_csv_directory(&mut self) {
        let default_dir = default_csv_path()
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(default_csv_path);

        match open_path_in_file_manager(default_dir.as_path()) {
            Ok(_) => self.status = Some(format!("Opened {}", default_dir.display())),
            Err(err) => self.status = Some(format!("Open failed: {}", err)),
        }
    }

    fn refresh_profiles(&mut self) {
        match list_profiles() {
            Ok(list) => self.available_profiles = list,
            Err(err) => self.status = Some(err),
        }
    }

    fn save_profile(&mut self) {
        let name = self.profile_name.trim();
        match save_profile_to_disk(&self.config, name) {
            Ok(path) => {
                self.status = Some(format!("Saved profile to {}", path.display()));
                self.refresh_profiles();
            }
            Err(err) => self.status = Some(err),
        }
    }

    fn load_profile(&mut self) {
        let preferred = if !self.profile_name.trim().is_empty() {
            Some(self.profile_name.trim().to_string())
        } else {
            self.selected_profile.clone()
        };

        let name = match preferred {
            Some(n) => n,
            None => {
                self.status = Some("Select or enter a profile name to load.".to_string());
                return;
            }
        };

        match load_profile_from_disk(&name) {
            Ok(cfg) => {
                self.config = cfg;
                self.profile_name = name.clone();
                self.selected_profile = Some(name.clone());
                self.status = Some(format!("Loaded profile '{}'", name));
            }
            Err(err) => self.status = Some(err),
        }
    }

    fn revert_defaults(&mut self) {
        self.config = CrosshairConfig::default();
        self.status = Some("Reverted to default settings.".to_string());
    }

    fn delete_profile(&mut self) {
        let preferred = if !self.profile_name.trim().is_empty() {
            Some(self.profile_name.trim().to_string())
        } else {
            self.selected_profile.clone()
        };

        let name = match preferred {
            Some(n) => n,
            None => {
                self.status = Some("Select or enter a profile name to delete.".to_string());
                return;
            }
        };

        match profile_path(&name)
            .and_then(|p| fs::remove_file(&p).map(|_| p).map_err(|e| e.to_string()))
        {
            Ok(_path) => {
                self.status = Some(format!("Deleted profile '{}'", name));
                if self.selected_profile.as_deref() == Some(&name) {
                    self.selected_profile = None;
                }
                if self.profile_name.trim() == name {
                    self.profile_name.clear();
                }
                self.refresh_profiles();
            }
            Err(err) => self.status = Some(format!("Delete failed: {}", err)),
        }
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Profiles");
        ui.horizontal(|ui| {
            ui.label("Profile name");
            ui.text_edit_singleline(&mut self.profile_name);
            if ui.button("Save profile").clicked() {
                self.save_profile();
            }
            if ui.button("Revert default").clicked() {
                self.revert_defaults();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Saved profiles");
            egui::ComboBox::from_id_source("profile_select")
                .selected_text(
                    self.selected_profile
                        .clone()
                        .unwrap_or_else(|| "Select profile".to_string()),
                )
                .show_ui(ui, |ui| {
                    for name in &self.available_profiles {
                        ui.selectable_value(&mut self.selected_profile, Some(name.clone()), name);
                    }
                });
            if ui.button("Load").clicked() {
                self.load_profile();
            }
            if ui.button("Refresh list").clicked() {
                self.refresh_profiles();
            }
            if ui.button("Delete").clicked() {
                self.delete_profile();
            }
        });
        ui.separator();

        ui.heading("Crosshair Settings");
        let mut canvas_changed = false;
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::Slider::new(&mut self.config.size, MIN_CANVAS_SIZE..=MAX_CANVAS_SIZE)
                    .text("Canvas size"),
            );
            canvas_changed |= response.changed();
            ui.toggle_value(&mut self.chain_canvas_and_radius, "Chain size <-> radius");
        });

        let mut ring_radius_changed = false;
        let ring_response = ui.add(
            egui::Slider::new(
                &mut self.config.ring_outer_radius,
                1.0..=MAX_RING_OUTER_RADIUS,
            )
            .text("Ring outer radius"),
        );
        ring_radius_changed |= ring_response.changed();

        if self.chain_canvas_and_radius {
            if canvas_changed {
                self.config.ring_outer_radius =
                    canvas_border_radius(self.config.size).clamp(0.0, MAX_RING_OUTER_RADIUS);
            } else if ring_radius_changed {
                let new_size = (self.config.ring_outer_radius * 2.0).round() as u32;
                self.config.size = new_size.clamp(MIN_CANVAS_SIZE, MAX_CANVAS_SIZE);
            }
        }

        ui.add(
            egui::Slider::new(&mut self.config.ring_thickness, 1.0..=2048.0).text("Ring thickness"),
        );
        ui.add(
            egui::Slider::new(&mut self.config.gap_from_ring, 0.0..=2048.0)
                .text("Arm gap from ring"),
        );
        ui.add(
            egui::Slider::new(&mut self.config.center_gap_radius, 0.0..=2048.0)
                .text("Center gap radius"),
        );
        ui.separator();

        ui.add(
            egui::Slider::new(&mut self.config.spoke_base_width, 1.0..=2048.0)
                .text("Spoke base width"),
        );
        ui.add(
            egui::Slider::new(&mut self.config.spoke_tip_width, 0.0..=1024.0)
                .text("Spoke tip width"),
        );
        ui.separator();

        ui.add(egui::Slider::new(&mut self.config.blur_radius, 0.0..=12.0).text("Blur radius"));
        ui.add(egui::Slider::new(&mut self.config.glow_radius, 0.0..=20.0).text("Glow radius"));
        ui.label("Blur/glow values are kept with the config; current renderer draws crisp edges.");
        ui.separator();

        ui.label("Rim color");
        let mut rim_rgba = tuple_to_rgba(self.config.rim_color);
        if color_picker::color_edit_button_rgba(ui, &mut rim_rgba, color_picker::Alpha::OnlyBlend)
            .changed()
        {
            self.config.rim_color = rgba_to_tuple(rim_rgba);
        }

        ui.label("Arm color");
        let mut arm_rgba = tuple_to_rgba(self.config.arm_color);
        if color_picker::color_edit_button_rgba(ui, &mut arm_rgba, color_picker::Alpha::OnlyBlend)
            .changed()
        {
            self.config.arm_color = rgba_to_tuple(arm_rgba);
        }

        ui.separator();
        ui.label("Spoke angles (degrees)");
        let mut remove_idx = None;
        for (idx, angle) in self.config.angles.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(angle)
                        .speed(0.5)
                        .clamp_range(0.0..=360.0)
                        .suffix("°"),
                );
                if ui.small_button("Remove").clicked() {
                    remove_idx = Some(idx);
                }
            });
        }
        if let Some(idx) = remove_idx {
            self.config.angles.remove(idx);
        }
        if ui.button("Add angle").clicked() {
            self.config.angles.push(0.0);
        }

        ui.separator();
        ui.heading("Export");
        ui.horizontal(|ui| {
            ui.label("Preview SVG path");
            ui.text_edit_singleline(&mut self.output_path);
            if ui.button("Pick folder").clicked() {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_directory(user_output_dir())
                    .pick_folder()
                {
                    let filename = PathBuf::from(&self.output_path)
                        .file_name()
                        .map(|s| s.to_owned())
                        .unwrap_or_else(|| "reticle-preview.svg".into());
                    let new_path = folder.join(filename);
                    self.output_path = new_path.to_string_lossy().to_string();
                }
            }
        });
        if ui.button("Save current SVG").clicked() {
            self.save_current_svg();
        }

        ui.separator();
        ui.heading("Batch from CSV");
        ui.horizontal(|ui| {
            ui.label("CSV with rim,arm hex pairs");
            if ui.button("Open default path").clicked() {
                self.open_default_csv_directory();
            }
        });
        ui.text_edit_singleline(&mut self.csv_path);
        ui.label("Output directory");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.batch_dir);
            if ui.button("Pick folder").clicked() {
                if let Some(folder) = rfd::FileDialog::new()
                    .set_directory(user_output_dir())
                    .pick_folder()
                {
                    self.batch_dir = folder.to_string_lossy().to_string();
                }
            }
        });
        if ui.button("Generate full set").clicked() {
            self.generate_batch();
        }

        if let Some(status) = &self.status {
            ui.separator();
            ui.label(status);
        }
    }
}

impl eframe::App for CrosshairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("controls")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                self.draw_controls(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Live preview");
            ui.label("Adjust settings on the left to see the updated reticle.");
            ui.add_space(8.0);
            draw_crosshair_preview(ui, &self.config);
        });
    }
}

// ------------------------------------------------------------
// ENTRYPOINTS
// ------------------------------------------------------------

fn run_gui() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Crosshair Configurator",
        options,
        Box::new(|_cc| Box::new(CrosshairApp::new())),
    )
}

fn run_batch() -> Result<(), Box<dyn std::error::Error>> {
    let config = CrosshairConfig::default();
    let out_dir = user_output_dir();
    let csv_path = default_csv_path_string();
    println!("Generating SVGs from {} ...", csv_path);
    let count = generate_batch_svgs(&config, &csv_path, &out_dir, true)?;
    println!("Generated {} SVG crosshairs.", count);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--batch") {
        run_batch()?;
        return Ok(());
    }

    run_gui()?;
    Ok(())
}
