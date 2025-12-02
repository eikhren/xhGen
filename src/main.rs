use image::{ImageBuffer, Luma, Rgba, RgbaImage};
use imageproc::drawing::draw_polygon_mut;
use imageproc::point::Point;

const SUPERSAMPLE: u32 = 4;

#[derive(Clone)]
struct CrosshairConfig {
    size: u32,
    ring_outer_radius: f64,
    ring_thickness: f64,
    ring_alpha: u8,
    ring_color: (u8, u8, u8),
    gap_from_ring: f64,
    center_gap_radius: f64,
    spoke_base_width: f64,
    spoke_tip_width: f64,
    angles: Vec<f64>,
}

impl Default for CrosshairConfig {
    fn default() -> Self {
        Self {
            size: 256,
            ring_outer_radius: 116.0,
            ring_thickness: 18.0,
            ring_alpha: 255,
            ring_color: (255, 255, 255),
            gap_from_ring: 16.0,
            center_gap_radius: 6.0,
            spoke_base_width: 16.0,
            spoke_tip_width: 0.0,
            angles: vec![45.0, 135.0, 225.0, 315.0],
        }
    }
}

fn add_triangle(
    mask: &mut ImageBuffer<Luma<u8>, Vec<u8>>,
    center: (f64, f64),
    tip_r: f64,
    base_r: f64,
    angle_deg: f64,
    base_width: f64,
    tip_width: f64,
) {
    let (cx, cy) = center;
    let th = angle_deg.to_radians();
    let ux = th.cos();
    let uy = th.sin();

    let px = -uy;
    let py = ux;

    let (tip_cx, tip_cy) = (cx + tip_r * ux, cy + tip_r * uy);
    let (base_cx, base_cy) = (cx + base_r * ux, cy + base_r * uy);

    let bw = base_width / 2.0;
    let tw = tip_width / 2.0;

    let base_left = (base_cx - px * bw, base_cy - py * bw);
    let base_right = (base_cx + px * bw, base_cy + py * bw);
    let tip_left = (tip_cx - px * tw, tip_cy - py * tw);
    let tip_right = (tip_cx + px * tw, tip_cy + py * tw);

    let points = [
        Point::new(base_left.0.round() as i32, base_left.1.round() as i32),
        Point::new(base_right.0.round() as i32, base_right.1.round() as i32),
        Point::new(tip_right.0.round() as i32, tip_right.1.round() as i32),
        Point::new(tip_left.0.round() as i32, tip_left.1.round() as i32),
    ];

    draw_polygon_mut(mask, &points, Luma([255u8]));
}

fn build_alpha_mask(config: &CrosshairConfig) -> Vec<u8> {
    let scale = SUPERSAMPLE as f64;
    let supersized = config.size * SUPERSAMPLE;
    let mut mask: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(supersized, supersized, Luma([0u8]));

    let cx = (config.size as f64 * scale) / 2.0;
    let cy = cx;

    let ring_outer = config.ring_outer_radius * scale;
    let ring_inner = (config.ring_outer_radius - config.ring_thickness) * scale;

    for y in 0..supersized {
        for x in 0..supersized {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= ring_inner && dist <= ring_outer {
                mask.put_pixel(x, y, Luma([255u8]));
            }
        }
    }

    let ring_inner_radius = config.ring_outer_radius - config.ring_thickness;
    let base_r = (ring_inner_radius - config.gap_from_ring) * scale;
    let tip_r = config.center_gap_radius * scale;
    let base_width = config.spoke_base_width * scale;
    let tip_width = config.spoke_tip_width * scale;

    for angle in &config.angles {
        add_triangle(
            &mut mask,
            (cx, cy),
            tip_r,
            base_r,
            *angle,
            base_width,
            tip_width,
        );
    }

    downsample_alpha(&mask, config.size, SUPERSAMPLE, config.ring_alpha)
}

fn downsample_alpha(
    mask: &ImageBuffer<Luma<u8>, Vec<u8>>,
    target_size: u32,
    scale: u32,
    ring_alpha: u8,
) -> Vec<u8> {
    let mut alphas = vec![0u8; (target_size * target_size) as usize];
    let block_area = (scale * scale) as u32;

    for y in 0..target_size {
        for x in 0..target_size {
            let mut sum: u32 = 0;
            let start_x = x * scale;
            let start_y = y * scale;

            for sy in 0..scale {
                for sx in 0..scale {
                    let pixel = mask.get_pixel(start_x + sx, start_y + sy);
                    sum += pixel[0] as u32;
                }
            }

            let average = (sum + block_area / 2) / block_area;
            let alpha = ((average as u16 * ring_alpha as u16 + 127) / 255) as u8;
            alphas[(y * target_size + x) as usize] = alpha;
        }
    }

    alphas
}

fn colorize(alpha: &[u8], size: u32, color: (u8, u8, u8)) -> RgbaImage {
    let mut img = RgbaImage::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) as usize;
            let a = alpha[idx];
            img.put_pixel(x, y, Rgba([color.0, color.1, color.2, a]));
        }
    }
    img
}

fn generate_crosshair_pair(config: &CrosshairConfig) -> (RgbaImage, RgbaImage) {
    let alpha = build_alpha_mask(config);
    let white = colorize(&alpha, config.size, config.ring_color);
    let black = colorize(&alpha, config.size, (0, 0, 0));
    (white, black)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = CrosshairConfig::default();
    let (white, black) = generate_crosshair_pair(&config);

    white.save("crosshair_reticle_256_white.png")?;
    black.save("crosshair_reticle_256_black.png")?;
    println!("Wrote crosshair_reticle_256_white.png and crosshair_reticle_256_black.png");

    Ok(())
}
