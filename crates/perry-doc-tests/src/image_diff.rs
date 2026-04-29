//! Perceptual screenshot diff for the widget gallery.
//!
//! Uses `dssim-core`'s multi-scale SSIM so we tolerate the small
//! anti-aliasing differences between dev boxes and CI runners but still
//! catch real widget regressions. Thresholds are per-(baseline, OS) and
//! live in `docs/examples/_baselines/thresholds.json`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

const DEFAULT_THRESHOLD: f64 = 0.010;

/// Diff outcome. `distance` is dssim's raw SSIM distance (0 = identical).
/// `threshold` is what was compared against.
#[derive(Debug)]
pub struct DiffOutcome {
    pub distance: f64,
    pub threshold: f64,
}

impl DiffOutcome {
    pub fn passed(&self) -> bool {
        self.distance <= self.threshold
    }
}

/// Diff `actual_png` against `baseline_png` using SSIM.
/// Returns Err if either image is missing or malformed.
///
/// Retina / HiDPI tolerance: if the actual screenshot is exactly 2× the
/// baseline in both dimensions (a retina macOS capture vs a 1× baseline),
/// the actual is downsampled with a 2×2 box filter before comparison.
/// Any other size mismatch is still an error.
pub fn diff(actual_png: &Path, baseline_png: &Path, threshold: f64) -> Result<DiffOutcome> {
    let actual = load(actual_png)
        .with_context(|| format!("loading actual screenshot {}", actual_png.display()))?;
    let baseline = load(baseline_png)
        .with_context(|| format!("loading baseline {}", baseline_png.display()))?;

    // Auto-correct for retina (2× backing scale) captures against 1× baselines.
    let halved;
    let actual_ref: &image::RgbaImage =
        if actual.width() == baseline.width() * 2 && actual.height() == baseline.height() * 2 {
            halved = halve(&actual);
            &halved
        } else {
            if actual.width() != baseline.width() || actual.height() != baseline.height() {
                return Err(anyhow!(
                    "size mismatch: actual {}x{} vs baseline {}x{} \
                     (only exact 2× retina scaling is auto-corrected)",
                    actual.width(),
                    actual.height(),
                    baseline.width(),
                    baseline.height()
                ));
            }
            &actual
        };

    let attr = dssim_core::Dssim::new();
    let actual_img = to_dssim(actual_ref, &attr)?;
    let baseline_img = to_dssim(&baseline, &attr)?;
    let (val, _maps) = attr.compare(&baseline_img, &actual_img);
    Ok(DiffOutcome {
        distance: val.into(),
        threshold,
    })
}

/// Downsample a 2× retina image to 1× using a 2×2 box filter.
fn halve(img: &image::RgbaImage) -> image::RgbaImage {
    let w = img.width() / 2;
    let h = img.height() / 2;
    let mut out = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let avg = |a: u8, b: u8, c: u8, d: u8| -> u8 {
                (((a as u32) + (b as u32) + (c as u32) + (d as u32) + 2) / 4) as u8
            };
            let p00 = img.get_pixel(x * 2, y * 2).0;
            let p10 = img.get_pixel(x * 2 + 1, y * 2).0;
            let p01 = img.get_pixel(x * 2, y * 2 + 1).0;
            let p11 = img.get_pixel(x * 2 + 1, y * 2 + 1).0;
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    avg(p00[0], p10[0], p01[0], p11[0]),
                    avg(p00[1], p10[1], p01[1], p11[1]),
                    avg(p00[2], p10[2], p01[2], p11[2]),
                    avg(p00[3], p10[3], p01[3], p11[3]),
                ]),
            );
        }
    }
    out
}

/// Look up the threshold for a given baseline name + host OS.
/// Falls back to `DEFAULT_THRESHOLD` if not specified. Unknown keys at the top
/// level (`_comment`, anything else) are ignored, so the JSON file can carry
/// human-readable notes alongside real entries.
pub fn threshold_for(thresholds_file: &Path, baseline_name: &str, host_os: &str) -> f64 {
    let Ok(text) = std::fs::read_to_string(thresholds_file) else {
        return DEFAULT_THRESHOLD;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&text) else {
        return DEFAULT_THRESHOLD;
    };
    root.get(baseline_name)
        .and_then(|v| v.get(host_os))
        .and_then(|v| v.as_f64())
        .unwrap_or(DEFAULT_THRESHOLD)
}

fn load(path: &Path) -> Result<image::RgbaImage> {
    let img = image::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(img.to_rgba8())
}

fn to_dssim(
    img: &image::RgbaImage,
    attr: &dssim_core::Dssim,
) -> Result<dssim_core::DssimImage<f32>> {
    let width = img.width() as usize;
    let height = img.height() as usize;
    let pixels: Vec<rgb::RGBA8> = img
        .pixels()
        .map(|p| rgb::RGBA8 {
            r: p[0],
            g: p[1],
            b: p[2],
            a: p[3],
        })
        .collect();
    attr.create_image_rgba(&pixels, width, height)
        .ok_or_else(|| anyhow!("dssim failed to ingest image"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> image::RgbaImage {
        image::RgbaImage::from_fn(w, h, |_, _| image::Rgba([r, g, b, 255]))
    }

    #[test]
    fn halve_averages_2x2_blocks() {
        // 4×2 image with two distinct colors side by side (each 2×2 block is one color).
        let mut img = image::RgbaImage::new(4, 2);
        for y in 0..2u32 {
            for x in 0..2u32 {
                img.put_pixel(x, y, image::Rgba([200, 100, 50, 255]));
                img.put_pixel(x + 2, y, image::Rgba([100, 200, 150, 255]));
            }
        }
        let out = halve(&img);
        assert_eq!(out.width(), 2);
        assert_eq!(out.height(), 1);
        // Left pixel should be the average of (200,100,50) × 4 → (200,100,50)
        assert_eq!(out.get_pixel(0, 0).0, [200, 100, 50, 255]);
        // Right pixel should be the average of (100,200,150) × 4 → (100,200,150)
        assert_eq!(out.get_pixel(1, 0).0, [100, 200, 150, 255]);
    }

    #[test]
    fn diff_identical_same_size_passes() {
        // Write two identical tiny PNGs to temp files and diff them.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        solid(4, 4, 128, 128, 128).save(&a).unwrap();
        solid(4, 4, 128, 128, 128).save(&b).unwrap();
        let outcome = diff(&a, &b, 0.01).unwrap();
        assert!(outcome.passed(), "identical images should pass");
    }

    #[test]
    fn diff_retina_2x_against_1x_baseline_passes() {
        // Simulate a retina capture: baseline 2×2, actual 4×4 (same solid color).
        let dir = tempfile::tempdir().unwrap();
        let actual_path = dir.path().join("actual.png");
        let baseline_path = dir.path().join("baseline.png");
        solid(4, 4, 64, 128, 192).save(&actual_path).unwrap();
        solid(2, 2, 64, 128, 192).save(&baseline_path).unwrap();
        let outcome = diff(&actual_path, &baseline_path, 0.05).unwrap();
        assert!(outcome.passed(), "2× retina capture should pass after downsampling");
    }

    #[test]
    fn diff_arbitrary_size_mismatch_errors() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        solid(6, 4, 0, 0, 0).save(&a).unwrap();
        solid(4, 4, 0, 0, 0).save(&b).unwrap();
        let result = diff(&a, &b, 0.05);
        assert!(result.is_err(), "non-2× mismatch should be an error");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("size mismatch"));
    }
}
