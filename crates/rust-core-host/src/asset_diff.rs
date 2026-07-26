use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageBuffer, ImageReader, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

const DEFAULT_MAX_DECODED_PIXELS: usize = 40_000_000;
const RGBA_BYTES_PER_PIXEL: usize = 4;

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ImageDiffOptions {
    file_path: Option<String>,
    dimension_policy: String,
    pixel_threshold: u8,
    region_min_area: usize,
    alpha_handling: String,
    max_decoded_pixels: usize,
}

impl Default for ImageDiffOptions {
    fn default() -> Self {
        Self {
            file_path: None,
            dimension_policy: "strict".to_string(),
            pixel_threshold: 16,
            region_min_area: 4,
            alpha_handling: "include".to_string(),
            max_decoded_pixels: DEFAULT_MAX_DECODED_PIXELS,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AssetDimensions {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone)]
struct PixelMetrics {
    changed_pixels: usize,
    raw_changed_pixels: usize,
    mean_absolute_error: f64,
    root_mean_squared_error: f64,
    centroid_x: f64,
    centroid_y: f64,
}

#[derive(Debug, Clone)]
struct ChangedRegion {
    pixels: Vec<usize>,
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
}

/// Perceptual image diff (#… asset diff) — parse options, run the diff, return JSON. The C ABI
/// (`intentdiff_call`) calls this directly.
pub(crate) fn diff_asset_image_impl(
    before_path: &str,
    after_path: &str,
    output_dir: &str,
    options_json: &str,
) -> Result<String, String> {
    let options: ImageDiffOptions =
        serde_json::from_str(options_json).map_err(|exc| format!("options JSON: {exc}"))?;
    diff_asset_image_value(
        Path::new(before_path),
        Path::new(after_path),
        Path::new(output_dir),
        &options,
    )
    .map(|value| value.to_string())
}

/// Perceptual image diff across a git range (base..head) — the changed-image variant.
pub(crate) fn diff_git_assets_impl(
    repo_path: &str,
    base: &str,
    head: &str,
    output_dir: &str,
    options_json: &str,
) -> Result<String, String> {
    let options: ImageDiffOptions =
        serde_json::from_str(options_json).map_err(|exc| format!("options JSON: {exc}"))?;
    diff_git_assets_value(
        Path::new(repo_path),
        base,
        head,
        Path::new(output_dir),
        &options,
    )
    .map(|value| value.to_string())
}

fn diff_git_assets_value(
    repo_path: &Path,
    base: &str,
    head: &str,
    output_dir: &Path,
    options: &ImageDiffOptions,
) -> Result<serde_json::Value, String> {
    let root = git_root(repo_path)?;
    let (effective_base, effective_head, name_status_args) = git_asset_diff_args(base, head)?;
    let diff_output = run_git_bytes(&root, &name_status_args)?;
    let mut entries = parse_name_status_z(&diff_output);
    if head.is_empty() {
        let untracked = git_untracked_images(&root)?;
        for path in &untracked {
            entries.push(("A".to_string(), path.clone(), path.clone()));
        }
    }

    fs::create_dir_all(output_dir)
        .map_err(|exc| format!("create output directory {}: {exc}", output_dir.display()))?;
    let temp_root = make_temp_dir("git-assets")?;

    let mut diffs = Vec::<serde_json::Value>::new();
    let mut skipped = Vec::<serde_json::Value>::new();
    for (change_type, old_path, new_path) in entries {
        if !is_supported_image_path(&old_path) && !is_supported_image_path(&new_path) {
            continue;
        }
        validate_git_rel_path(&old_path)?;
        validate_git_rel_path(&new_path)?;
        let display_path = if new_path.is_empty() {
            &old_path
        } else {
            &new_path
        };
        if change_type == "A" {
            skipped.push(skipped_asset(
                display_path,
                &change_type,
                "Added image has no before asset for perceptual comparison.",
            ));
            continue;
        }
        if change_type == "D" {
            skipped.push(skipped_asset(
                display_path,
                &change_type,
                "Deleted image has no after asset for perceptual comparison.",
            ));
            continue;
        }
        let before_bytes = git_show_bytes(&root, &effective_base, &old_path)?;
        let after_bytes = if head.is_empty() {
            fs::read(root.join(&new_path))
                .map_err(|exc| format!("read working-tree asset {new_path}: {exc}"))?
        } else if head == ":staged" {
            git_show_bytes(&root, "", &new_path)?
        } else {
            git_show_bytes(&root, &effective_head, &new_path)?
        };
        let before_path = write_temp_asset(&temp_root, "before", &old_path, &before_bytes)?;
        let after_path = write_temp_asset(&temp_root, "after", &new_path, &after_bytes)?;
        let mut item_options = ImageDiffOptions {
            file_path: Some(display_path.to_string()),
            dimension_policy: options.dimension_policy.clone(),
            pixel_threshold: options.pixel_threshold,
            region_min_area: options.region_min_area,
            alpha_handling: options.alpha_handling.clone(),
            max_decoded_pixels: options.max_decoded_pixels,
        };
        if item_options.file_path.is_none() {
            item_options.file_path = Some(display_path.to_string());
        }
        let item_out = output_dir.join(asset_output_leaf(display_path));
        diffs.push(diff_asset_image_value(
            &before_path,
            &after_path,
            &item_out,
            &item_options,
        )?);
    }
    let _ = fs::remove_dir_all(&temp_root);

    Ok(json!({
        "kind": "asset_diff_batch",
        "repo": path_string(&root),
        "base": effective_base,
        "head": if head.is_empty() { "working-tree" } else { effective_head.as_str() },
        "asset_diffs": diffs,
        "skipped": skipped,
    }))
}

fn diff_asset_image_value(
    before_path: &Path,
    after_path: &Path,
    output_dir: &Path,
    options: &ImageDiffOptions,
) -> Result<serde_json::Value, String> {
    let before = decode_image(before_path, options)?;
    let after = decode_image(after_path, options)?;
    let before_dimensions = AssetDimensions {
        width: before.width(),
        height: before.height(),
    };
    let after_dimensions = AssetDimensions {
        width: after.width(),
        height: after.height(),
    };
    let dimensions_changed = before_dimensions.width != after_dimensions.width
        || before_dimensions.height != after_dimensions.height;
    let decoded_cost = decoded_cost_json(&before_dimensions, &after_dimensions, options);

    let mut warnings = Vec::<String>::new();
    let (before_rgba, after_rgba, comparison_dimensions) =
        align_images(before, after, options, &mut warnings)?;

    if dimensions_changed && options.dimension_policy == "strict" {
        return Ok(json!({
            "kind": "asset_diff",
            "provider": "image",
            "status": "dimension_mismatch",
            "file_path": options.file_path.clone().unwrap_or_default(),
            "mime_type": mime_hint(after_path),
            "before_dimensions": before_dimensions,
            "after_dimensions": after_dimensions,
            "comparison_dimensions": comparison_dimensions,
            "decoded_cost": decoded_cost,
            "dimensions_changed": true,
            "changed_pixel_percentage": null,
            "mean_absolute_error": null,
            "root_mean_squared_error": null,
            "ssim": null,
            "ssim_available": false,
            "artifacts": {},
            "warnings": ["Dimensions differ; strict policy skipped perceptual comparison."],
            "summary": format!(
                "Image dimensions changed from {}x{} to {}x{}; strict comparison skipped.",
                before_dimensions.width,
                before_dimensions.height,
                after_dimensions.width,
                after_dimensions.height,
            ),
        }));
    }

    fs::create_dir_all(output_dir)
        .map_err(|exc| format!("create output directory {}: {exc}", output_dir.display()))?;

    let (raw_mask, filtered_mask, tiny_mask, metrics) =
        compare_pixels(&before_rgba, &after_rgba, options);
    let regions = collect_regions(
        &raw_mask,
        before_rgba.width() as usize,
        before_rgba.height() as usize,
    );
    let kept_regions = regions
        .iter()
        .filter(|region| region.pixels.len() >= options.region_min_area.max(1))
        .cloned()
        .collect::<Vec<_>>();
    if metrics.raw_changed_pixels != metrics.changed_pixels {
        warnings.push(format!(
            "Ignored {} tiny changed pixels below region_min_area={}.",
            metrics
                .raw_changed_pixels
                .saturating_sub(metrics.changed_pixels),
            options.region_min_area,
        ));
    }

    let diff_path = output_dir.join("diff.png");
    let mask_path = output_dir.join("mask.png");
    let overlay_path = output_dir.join("overlay.png");
    let contact_sheet_path = output_dir.join("contact-sheet.png");
    let heatmap_path = output_dir.join("heatmap.png");
    // The comparison-resized before/after images are surfaced as their own
    // artifacts so the VS Code viewer can render side-by-side and blend them for
    // onion-skin (they share `comparison_dimensions`, so they overlay 1:1).
    let before_render_path = output_dir.join("before.png");
    let after_render_path = output_dir.join("after.png");
    before_rgba
        .save(&before_render_path)
        .map_err(save_error(&before_render_path))?;
    after_rgba
        .save(&after_render_path)
        .map_err(save_error(&after_render_path))?;

    render_diff(&before_rgba, &after_rgba)
        .save(&diff_path)
        .map_err(save_error(&diff_path))?;
    render_mask(&filtered_mask, before_rgba.width(), before_rgba.height())
        .save(&mask_path)
        .map_err(save_error(&mask_path))?;
    render_overlay(&after_rgba, &filtered_mask, &tiny_mask)
        .save(&overlay_path)
        .map_err(save_error(&overlay_path))?;
    render_heatmap(&before_rgba, &after_rgba, options)
        .save(&heatmap_path)
        .map_err(save_error(&heatmap_path))?;
    render_contact_sheet(&before_rgba, &after_rgba, &overlay_path, &diff_path)?
        .save(&contact_sheet_path)
        .map_err(save_error(&contact_sheet_path))?;

    let total_pixels = (before_rgba.width() as usize).saturating_mul(before_rgba.height() as usize);
    let changed_pixel_percentage = if total_pixels == 0 {
        0.0
    } else {
        metrics.changed_pixels as f64 * 100.0 / total_pixels as f64
    };

    Ok(json!({
        "kind": "asset_diff",
        "provider": "image",
        "status": "compared",
        "file_path": options.file_path.clone().unwrap_or_default(),
        "mime_type": mime_hint(after_path),
        "before_dimensions": before_dimensions,
        "after_dimensions": after_dimensions,
        "comparison_dimensions": comparison_dimensions,
        "decoded_cost": decoded_cost,
        "dimensions_changed": dimensions_changed,
        "dimension_policy": options.dimension_policy,
        "pixel_threshold": options.pixel_threshold,
        "region_min_area": options.region_min_area,
        "alpha_handling": options.alpha_handling,
        "changed_pixel_percentage": round3(changed_pixel_percentage),
        "changed_pixels": metrics.changed_pixels,
        "raw_changed_pixels": metrics.raw_changed_pixels,
        "mean_absolute_error": round3(metrics.mean_absolute_error),
        "root_mean_squared_error": round3(metrics.root_mean_squared_error),
        "ssim": null,
        "ssim_available": false,
        "artifacts": {
            "before": path_string(&before_render_path),
            "after": path_string(&after_render_path),
            "diff": path_string(&diff_path),
            "mask": path_string(&mask_path),
            "overlay": path_string(&overlay_path),
            "heatmap": path_string(&heatmap_path),
            "contact_sheet": path_string(&contact_sheet_path),
        },
        "hotspots": hotspots_json(&kept_regions, before_rgba.width() as usize, before_rgba.height() as usize),
        "hotspot_navigation": hotspot_navigation_json(&kept_regions),
        "histograms": histograms_json(&before_rgba, &after_rgba, options),
        "warnings": warnings,
        "summary": summarize_image_diff(
            changed_pixel_percentage,
            metrics.changed_pixels,
            dimensions_changed,
            &metrics,
        ),
    }))
}

fn decode_image(path: &Path, options: &ImageDiffOptions) -> Result<DynamicImage, String> {
    let reader = ImageReader::open(path)
        .map_err(|exc| format!("open image {}: {exc}", path.display()))?
        .with_guessed_format()
        .map_err(|exc| format!("guess image format {}: {exc}", path.display()))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|exc| format!("read image dimensions {}: {exc}", path.display()))?;
    validate_decoded_image_cost(path, width, height, options.max_decoded_pixels)?;
    ImageReader::open(path)
        .map_err(|exc| format!("open image {}: {exc}", path.display()))?
        .with_guessed_format()
        .map_err(|exc| format!("guess image format {}: {exc}", path.display()))?
        .decode()
        .map_err(|exc| format!("decode image {}: {exc}", path.display()))
}

fn validate_decoded_image_cost(
    path: &Path,
    width: u32,
    height: u32,
    max_decoded_pixels: usize,
) -> Result<(), String> {
    let pixels = decoded_pixels(width, height);
    if pixels <= max_decoded_pixels {
        return Ok(());
    }
    Err(format!(
        "image decoded dimensions too large: {} is {}x{} ({} pixels, ~{} RGBA); limit is {} pixels (~{} RGBA). Resize the image or raise max_decoded_pixels for this asset diff.",
        path.display(),
        width,
        height,
        pixels,
        human_bytes(pixels.saturating_mul(RGBA_BYTES_PER_PIXEL)),
        max_decoded_pixels,
        human_bytes(max_decoded_pixels.saturating_mul(RGBA_BYTES_PER_PIXEL)),
    ))
}

fn decoded_pixels(width: u32, height: u32) -> usize {
    (width as usize).saturating_mul(height as usize)
}

fn decoded_cost_json(
    before_dimensions: &AssetDimensions,
    after_dimensions: &AssetDimensions,
    options: &ImageDiffOptions,
) -> serde_json::Value {
    let before_pixels = decoded_pixels(before_dimensions.width, before_dimensions.height);
    let after_pixels = decoded_pixels(after_dimensions.width, after_dimensions.height);
    json!({
        "before_pixels": before_pixels,
        "after_pixels": after_pixels,
        "max_decoded_pixels": options.max_decoded_pixels,
        "before_rgba_bytes": before_pixels.saturating_mul(RGBA_BYTES_PER_PIXEL),
        "after_rgba_bytes": after_pixels.saturating_mul(RGBA_BYTES_PER_PIXEL),
    })
}

fn human_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / KB)
    } else {
        format!("{bytes} B")
    }
}

fn align_images(
    before: DynamicImage,
    after: DynamicImage,
    options: &ImageDiffOptions,
    warnings: &mut Vec<String>,
) -> Result<(RgbaImage, RgbaImage, AssetDimensions), String> {
    let before_rgba = before.to_rgba8();
    let after_rgba = after.to_rgba8();
    let before_dims = before_rgba.dimensions();
    let after_dims = after_rgba.dimensions();
    if before_dims == after_dims {
        return Ok((
            before_rgba,
            after_rgba,
            AssetDimensions {
                width: before_dims.0,
                height: before_dims.1,
            },
        ));
    }
    match options.dimension_policy.as_str() {
        "strict" => Ok((
            before_rgba,
            after_rgba,
            AssetDimensions {
                width: before_dims.0,
                height: before_dims.1,
            },
        )),
        "resize" => {
            warnings.push(format!(
                "Resized after image from {}x{} to {}x{} for comparison.",
                after_dims.0, after_dims.1, before_dims.0, before_dims.1
            ));
            let resized = imageops::resize(
                &after_rgba,
                before_dims.0,
                before_dims.1,
                FilterType::Triangle,
            );
            Ok((
                before_rgba,
                resized,
                AssetDimensions {
                    width: before_dims.0,
                    height: before_dims.1,
                },
            ))
        }
        "pad" => {
            let width = before_dims.0.max(after_dims.0);
            let height = before_dims.1.max(after_dims.1);
            warnings.push(format!("Padded images to common canvas {width}x{height}."));
            Ok((
                pad_image(&before_rgba, width, height),
                pad_image(&after_rgba, width, height),
                AssetDimensions { width, height },
            ))
        }
        other => Err(format!(
            "unsupported dimension_policy {other:?}; expected strict, resize, or pad"
        )),
    }
}

fn pad_image(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut canvas = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    imageops::overlay(&mut canvas, source, 0, 0);
    canvas
}

fn compare_pixels(
    before: &RgbaImage,
    after: &RgbaImage,
    options: &ImageDiffOptions,
) -> (Vec<bool>, Vec<bool>, Vec<bool>, PixelMetrics) {
    let width = before.width();
    let height = before.height();
    let len = width as usize * height as usize;
    let mut raw_mask = vec![false; len];
    let mut mae_sum = 0.0;
    let mut mse_sum = 0.0;
    let mut raw_changed_pixels = 0usize;

    for y in 0..height {
        for x in 0..width {
            let idx = (y as usize * width as usize) + x as usize;
            let a = before.get_pixel(x, y).0;
            let b = after.get_pixel(x, y).0;
            let channels = if options.alpha_handling == "ignore" {
                3
            } else {
                4
            };
            let mut max_delta = 0u8;
            for channel in 0..channels {
                let delta = a[channel].abs_diff(b[channel]);
                max_delta = max_delta.max(delta);
                mae_sum += f64::from(delta);
                let delta_f = f64::from(delta);
                mse_sum += delta_f * delta_f;
            }
            if max_delta > options.pixel_threshold {
                raw_mask[idx] = true;
                raw_changed_pixels += 1;
            }
        }
    }

    let filtered_mask = filter_regions(
        &raw_mask,
        width as usize,
        height as usize,
        options.region_min_area,
    );
    let tiny_mask = raw_mask
        .iter()
        .zip(filtered_mask.iter())
        .map(|(raw, filtered)| *raw && !*filtered)
        .collect::<Vec<_>>();
    let mut changed_pixels = 0usize;
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    for y in 0..height {
        for x in 0..width {
            let idx = (y as usize * width as usize) + x as usize;
            if filtered_mask[idx] {
                changed_pixels += 1;
                x_sum += f64::from(x);
                y_sum += f64::from(y);
            }
        }
    }
    let divisor = (len
        * if options.alpha_handling == "ignore" {
            3
        } else {
            4
        })
    .max(1) as f64;
    let metrics = PixelMetrics {
        changed_pixels,
        raw_changed_pixels,
        mean_absolute_error: mae_sum / divisor,
        root_mean_squared_error: (mse_sum / divisor).sqrt(),
        centroid_x: if changed_pixels == 0 {
            0.5
        } else {
            x_sum / changed_pixels as f64 / width.max(1) as f64
        },
        centroid_y: if changed_pixels == 0 {
            0.5
        } else {
            y_sum / changed_pixels as f64 / height.max(1) as f64
        },
    };
    (raw_mask, filtered_mask, tiny_mask, metrics)
}

fn filter_regions(raw_mask: &[bool], width: usize, height: usize, min_area: usize) -> Vec<bool> {
    if min_area <= 1 {
        return raw_mask.to_vec();
    }
    let mut filtered = vec![false; raw_mask.len()];
    for region in collect_regions(raw_mask, width, height) {
        if region.pixels.len() >= min_area {
            for idx in region.pixels {
                filtered[idx] = true;
            }
        }
    }
    filtered
}

fn collect_regions(raw_mask: &[bool], width: usize, height: usize) -> Vec<ChangedRegion> {
    let mut visited = vec![false; raw_mask.len()];
    let mut regions = Vec::<ChangedRegion>::new();
    let mut queue = VecDeque::<usize>::new();
    for start in 0..raw_mask.len() {
        if visited[start] || !raw_mask[start] {
            continue;
        }
        let mut component = Vec::<usize>::new();
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        visited[start] = true;
        queue.push_back(start);
        while let Some(idx) = queue.pop_front() {
            component.push(idx);
            let x = idx % width;
            let y = idx / width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            let candidates = [
                (x.wrapping_sub(1), y, x > 0),
                (x + 1, y, x + 1 < width),
                (x, y.wrapping_sub(1), y > 0),
                (x, y + 1, y + 1 < height),
            ];
            for (nx, ny, valid) in candidates {
                if !valid {
                    continue;
                }
                let next = ny * width + nx;
                if !visited[next] && raw_mask[next] {
                    visited[next] = true;
                    queue.push_back(next);
                }
            }
        }
        regions.push(ChangedRegion {
            pixels: component,
            min_x,
            min_y,
            max_x,
            max_y,
        });
    }
    regions
}

fn render_diff(before: &RgbaImage, after: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::new(before.width(), before.height());
    for y in 0..before.height() {
        for x in 0..before.width() {
            let a = before.get_pixel(x, y).0;
            let b = after.get_pixel(x, y).0;
            out.put_pixel(
                x,
                y,
                Rgba([
                    a[0].abs_diff(b[0]),
                    a[1].abs_diff(b[1]),
                    a[2].abs_diff(b[2]),
                    255,
                ]),
            );
        }
    }
    out
}

fn render_mask(mask: &[bool], width: u32, height: u32) -> RgbaImage {
    let mut out = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    for y in 0..height {
        for x in 0..width {
            let idx = y as usize * width as usize + x as usize;
            if mask[idx] {
                out.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }
    out
}

fn render_overlay(after: &RgbaImage, filtered_mask: &[bool], tiny_mask: &[bool]) -> RgbaImage {
    let mut out = after.clone();
    for y in 0..after.height() {
        for x in 0..after.width() {
            let idx = y as usize * after.width() as usize + x as usize;
            if filtered_mask[idx] {
                blend_pixel(&mut out, x, y, [255, 48, 64, 210]);
            } else if tiny_mask[idx] {
                blend_pixel(&mut out, x, y, [255, 184, 64, 150]);
            }
        }
    }
    out
}

fn render_heatmap(before: &RgbaImage, after: &RgbaImage, options: &ImageDiffOptions) -> RgbaImage {
    let mut out = ImageBuffer::from_pixel(before.width(), before.height(), Rgba([7, 14, 26, 255]));
    let channels = if options.alpha_handling == "ignore" {
        3
    } else {
        4
    };
    for y in 0..before.height() {
        for x in 0..before.width() {
            let a = before.get_pixel(x, y).0;
            let b = after.get_pixel(x, y).0;
            let max_delta = (0..channels)
                .map(|channel| a[channel].abs_diff(b[channel]))
                .max()
                .unwrap_or_default();
            if max_delta <= options.pixel_threshold {
                continue;
            }
            let intensity = max_delta;
            out.put_pixel(
                x,
                y,
                Rgba([
                    intensity,
                    intensity.saturating_div(2).saturating_add(40),
                    255u8.saturating_sub(intensity.saturating_div(3)),
                    255,
                ]),
            );
        }
    }
    out
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, overlay: [u8; 4]) {
    let base = image.get_pixel(x, y).0;
    let alpha = f32::from(overlay[3]) / 255.0;
    image.put_pixel(
        x,
        y,
        Rgba([
            ((f32::from(base[0]) * (1.0 - alpha)) + (f32::from(overlay[0]) * alpha)) as u8,
            ((f32::from(base[1]) * (1.0 - alpha)) + (f32::from(overlay[1]) * alpha)) as u8,
            ((f32::from(base[2]) * (1.0 - alpha)) + (f32::from(overlay[2]) * alpha)) as u8,
            255,
        ]),
    );
}

fn render_contact_sheet(
    before: &RgbaImage,
    after: &RgbaImage,
    overlay_path: &Path,
    diff_path: &Path,
) -> Result<RgbaImage, String> {
    let overlay = decode_image(overlay_path, &ImageDiffOptions::default())?.to_rgba8();
    let diff = decode_image(diff_path, &ImageDiffOptions::default())?.to_rgba8();
    let width = before.width() * 2;
    let height = before.height() * 2;
    let mut sheet = ImageBuffer::from_pixel(width, height, Rgba([10, 18, 32, 255]));
    imageops::overlay(&mut sheet, before, 0, 0);
    imageops::overlay(&mut sheet, after, i64::from(before.width()), 0);
    imageops::overlay(&mut sheet, &overlay, 0, i64::from(before.height()));
    imageops::overlay(
        &mut sheet,
        &diff,
        i64::from(before.width()),
        i64::from(before.height()),
    );
    Ok(sheet)
}

fn hotspots_json(regions: &[ChangedRegion], width: usize, height: usize) -> serde_json::Value {
    let total_pixels = width.saturating_mul(height).max(1);
    let hotspots = regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            let pixel_count = region.pixels.len();
            let changed_percentage = pixel_count as f64 * 100.0 / total_pixels as f64;
            let bbox_width = region.max_x.saturating_sub(region.min_x) + 1;
            let bbox_height = region.max_y.saturating_sub(region.min_y) + 1;
            let (centroid_x, centroid_y) = region_centroid(region, width, height);
            let severity = hotspot_severity(changed_percentage);
            json!({
                "id": format!("hotspot-{}", index + 1),
                "label": format!("Hotspot {}", index + 1),
                "bbox": {
                    "x": region.min_x,
                    "y": region.min_y,
                    "width": bbox_width,
                    "height": bbox_height,
                },
                "centroid": {
                    "x": round3(centroid_x),
                    "y": round3(centroid_y),
                },
                "pixel_count": pixel_count,
                "changed_pixel_percentage": round3(changed_percentage),
                "severity": severity,
                "summary": format!(
                    "{} image change covering {:.1}% of pixels in the {} region.",
                    capitalize(severity),
                    changed_percentage,
                    region_name(centroid_x, centroid_y),
                ),
            })
        })
        .collect::<Vec<_>>();
    json!(hotspots)
}

fn hotspot_navigation_json(regions: &[ChangedRegion]) -> serde_json::Value {
    let order = (0..regions.len())
        .map(|index| format!("hotspot-{}", index + 1))
        .collect::<Vec<_>>();
    json!({
        "count": regions.len(),
        "order": order,
        "selected_id": if regions.is_empty() { serde_json::Value::Null } else { json!("hotspot-1") },
        "focus_artifacts": ["overlay", "heatmap", "contact_sheet"],
    })
}

fn region_centroid(region: &ChangedRegion, width: usize, height: usize) -> (f64, f64) {
    if region.pixels.is_empty() {
        return (0.5, 0.5);
    }
    let mut x_sum = 0.0;
    let mut y_sum = 0.0;
    for idx in &region.pixels {
        x_sum += (*idx % width) as f64;
        y_sum += (*idx / width) as f64;
    }
    (
        x_sum / region.pixels.len() as f64 / width.max(1) as f64,
        y_sum / region.pixels.len() as f64 / height.max(1) as f64,
    )
}

fn hotspot_severity(changed_percentage: f64) -> &'static str {
    if changed_percentage < 1.0 {
        "low"
    } else if changed_percentage < 10.0 {
        "medium"
    } else {
        "high"
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn histograms_json(
    before: &RgbaImage,
    after: &RgbaImage,
    options: &ImageDiffOptions,
) -> serde_json::Value {
    const BINS: usize = 16;
    let mut red = vec![0usize; BINS];
    let mut green = vec![0usize; BINS];
    let mut blue = vec![0usize; BINS];
    let mut alpha = vec![0usize; BINS];
    let mut brightness = vec![0usize; BINS];

    for y in 0..before.height() {
        for x in 0..before.width() {
            let a = before.get_pixel(x, y).0;
            let b = after.get_pixel(x, y).0;
            red[delta_bin(a[0].abs_diff(b[0]), BINS)] += 1;
            green[delta_bin(a[1].abs_diff(b[1]), BINS)] += 1;
            blue[delta_bin(a[2].abs_diff(b[2]), BINS)] += 1;
            if options.alpha_handling != "ignore" {
                alpha[delta_bin(a[3].abs_diff(b[3]), BINS)] += 1;
            }
            let before_brightness = (u16::from(a[0]) + u16::from(a[1]) + u16::from(a[2])) / 3;
            let after_brightness = (u16::from(b[0]) + u16::from(b[1]) + u16::from(b[2])) / 3;
            brightness[delta_bin(before_brightness.abs_diff(after_brightness) as u8, BINS)] += 1;
        }
    }

    json!({
        "bins": BINS,
        "red_delta": red,
        "green_delta": green,
        "blue_delta": blue,
        "brightness_delta": brightness,
        "alpha_delta": if options.alpha_handling == "ignore" { serde_json::Value::Null } else { json!(alpha) },
    })
}

fn delta_bin(delta: u8, bins: usize) -> usize {
    ((usize::from(delta) * bins) / 256).min(bins.saturating_sub(1))
}

fn summarize_image_diff(
    percentage: f64,
    changed_pixels: usize,
    dimensions_changed: bool,
    metrics: &PixelMetrics,
) -> String {
    if changed_pixels == 0 {
        return if dimensions_changed {
            "Image dimensions changed, but no pixels differ above the configured threshold after alignment.".to_string()
        } else {
            "Image unchanged: no pixels differ above the configured threshold.".to_string()
        };
    }
    let scale = if percentage < 1.0 {
        "slightly"
    } else if percentage < 10.0 {
        "moderately"
    } else {
        "substantially"
    };
    let area = region_name(metrics.centroid_x, metrics.centroid_y);
    let dimension_note = if dimensions_changed {
        "Dimensions changed. "
    } else {
        "Dimensions unchanged. "
    };
    format!(
        "Image changed {scale}: {:.1}% of pixels differ. {dimension_note}Most changes are concentrated in the {area} region.",
        percentage,
    )
}

fn region_name(x: f64, y: f64) -> &'static str {
    match (x, y) {
        (x, y) if x < 0.33 && y < 0.33 => "top-left",
        (x, y) if x > 0.66 && y < 0.33 => "top-right",
        (x, y) if x < 0.33 && y > 0.66 => "bottom-left",
        (x, y) if x > 0.66 && y > 0.66 => "bottom-right",
        (_, y) if y < 0.33 => "top",
        (_, y) if y > 0.66 => "bottom",
        (x, _) if x < 0.33 => "left",
        (x, _) if x > 0.66 => "right",
        _ => "center",
    }
}

fn mime_hint(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn save_error(path: &Path) -> impl FnOnce(image::ImageError) -> String + '_ {
    move |exc| format!("save image {}: {exc}", path.display())
}

fn git_root(repo_path: &Path) -> Result<PathBuf, String> {
    let output = run_git_bytes(
        repo_path,
        &["rev-parse".to_string(), "--show-toplevel".to_string()],
    )?;
    let root = String::from_utf8(output)
        .map_err(|exc| format!("git rev-parse output was not UTF-8: {exc}"))?;
    Ok(PathBuf::from(root.trim()))
}

fn git_asset_diff_args(base: &str, head: &str) -> Result<(String, String, Vec<String>), String> {
    if head == ":staged" {
        return Ok((
            base.to_string(),
            ":staged".to_string(),
            vec![
                "diff".to_string(),
                "--cached".to_string(),
                "--name-status".to_string(),
                "-z".to_string(),
                base.to_string(),
            ],
        ));
    }
    if head == ":unpushed" {
        return Ok((
            "@{u}".to_string(),
            "HEAD".to_string(),
            vec![
                "diff".to_string(),
                "--name-status".to_string(),
                "-z".to_string(),
                "@{u}".to_string(),
                "HEAD".to_string(),
            ],
        ));
    }
    if head.is_empty() {
        return Ok((
            base.to_string(),
            "working-tree".to_string(),
            vec![
                "diff".to_string(),
                "--name-status".to_string(),
                "-z".to_string(),
                base.to_string(),
            ],
        ));
    }
    Ok((
        base.to_string(),
        head.to_string(),
        vec![
            "diff".to_string(),
            "--name-status".to_string(),
            "-z".to_string(),
            base.to_string(),
            head.to_string(),
        ],
    ))
}

fn run_git_bytes(repo: &Path, args: &[String]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|exc| format!("run git: {exc}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn parse_name_status_z(data: &[u8]) -> Vec<(String, String, String)> {
    let tokens = data
        .split(|value| *value == 0)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut entries = Vec::<(String, String, String)>::new();
    let mut idx = 0usize;
    while idx < tokens.len() {
        let status = String::from_utf8_lossy(tokens[idx]).to_string();
        idx += 1;
        let code = status.chars().next().unwrap_or_default().to_string();
        if code == "R" || code == "C" {
            if idx + 1 >= tokens.len() {
                break;
            }
            let old_path = String::from_utf8_lossy(tokens[idx]).to_string();
            let new_path = String::from_utf8_lossy(tokens[idx + 1]).to_string();
            idx += 2;
            entries.push((code, old_path, new_path));
        } else {
            if idx >= tokens.len() {
                break;
            }
            let path = String::from_utf8_lossy(tokens[idx]).to_string();
            idx += 1;
            entries.push((code, path.clone(), path));
        }
    }
    entries
}

fn git_untracked_images(root: &Path) -> Result<Vec<String>, String> {
    let output = run_git_bytes(
        root,
        &[
            "ls-files".to_string(),
            "--others".to_string(),
            "--exclude-standard".to_string(),
            "-z".to_string(),
        ],
    )?;
    Ok(output
        .split(|value| *value == 0)
        .filter(|token| !token.is_empty())
        .map(|token| String::from_utf8_lossy(token).to_string())
        .filter(|path| is_supported_image_path(path))
        .collect())
}

fn git_show_bytes(root: &Path, rev: &str, rel_path: &str) -> Result<Vec<u8>, String> {
    validate_git_rel_path(rel_path)?;
    if rel_path.contains('\n') || rel_path.contains('\r') {
        return Err("git asset path contains a newline".to_string());
    }
    let spec = if rev.is_empty() {
        format!(":{rel_path}")
    } else {
        format!("{rev}:{rel_path}")
    };
    run_git_bytes(root, &["show".to_string(), spec])
}

fn validate_git_rel_path(rel_path: &str) -> Result<(), String> {
    let path = Path::new(rel_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("unsafe git asset path: {rel_path}"));
    }
    Ok(())
}

fn is_supported_image_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "png" | "jpg" | "jpeg" | "webp"
    )
}

fn make_temp_dir(label: &str) -> Result<PathBuf, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|exc| format!("system clock before epoch: {exc}"))?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("intentdiff-{label}-{nonce}"));
    fs::create_dir_all(&path)
        .map_err(|exc| format!("create temp dir {}: {exc}", path.display()))?;
    Ok(path)
}

fn write_temp_asset(
    root: &Path,
    label: &str,
    rel_path: &str,
    data: &[u8],
) -> Result<PathBuf, String> {
    let mut hasher = Sha256::new();
    hasher.update(format!("{label}:{rel_path}").as_bytes());
    let digest = hex::encode(hasher.finalize());
    let suffix = Path::new(rel_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_else(|| ".bin".to_string());
    let path = root.join(format!("{label}-{}{suffix}", &digest[..16]));
    fs::write(&path, data).map_err(|exc| format!("write temp asset {}: {exc}", path.display()))?;
    Ok(path)
}

fn asset_output_leaf(rel_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rel_path.as_bytes());
    let digest = hex::encode(hasher.finalize());
    let stem = Path::new(rel_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("asset");
    let safe = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(48)
        .collect::<String>();
    format!("{safe}-{}", &digest[..12])
}

fn skipped_asset(path: &str, change_type: &str, reason: &str) -> serde_json::Value {
    json!({
        "kind": "asset_diff",
        "provider": "image",
        "status": "skipped",
        "file_path": path,
        "change_type": change_type,
        "mime_type": mime_hint(Path::new(path)),
        "reason": reason,
        "summary": reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("intentdiff-asset-{name}-{nonce}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_png(path: &Path, pixels: &[[u8; 4]], width: u32, height: u32) {
        let mut image = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let idx = y as usize * width as usize + x as usize;
                image.put_pixel(x, y, Rgba(pixels[idx]));
            }
        }
        image.save(path).expect("save test png");
    }

    fn write_image(path: &Path, pixels: &[[u8; 4]], width: u32, height: u32) {
        let mut image = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let idx = y as usize * width as usize + x as usize;
                image.put_pixel(x, y, Rgba(pixels[idx]));
            }
        }
        DynamicImage::ImageRgba8(image)
            .save(path)
            .expect("save test image");
    }

    fn noisy_pixels(width: u32, height: u32) -> Vec<[u8; 4]> {
        let mut state = 0x9e37_79b9u32;
        (0..(width as usize * height as usize))
            .map(|idx| {
                state = state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223)
                    .wrapping_add(idx as u32);
                [
                    (state & 0xff) as u8,
                    ((state >> 8) & 0xff) as u8,
                    ((state >> 16) & 0xff) as u8,
                    255,
                ]
            })
            .collect()
    }

    fn run_test_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git").arg("-C").arg(repo).args(args).status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => panic!("git {} failed with {status}", args.join(" ")),
            Err(_) => {
                // Developer machines without git should still be able to run the
                // pure image tests; CI/release gates exercise this path.
            }
        }
    }

    #[test]
    fn identical_images_report_no_change() {
        let dir = temp_dir("identical");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        let pixels = [[10, 20, 30, 255]; 4];
        write_png(&before, &pixels, 2, 2);
        write_png(&after, &pixels, 2, 2);
        let result = diff_asset_image_value(
            &before,
            &after,
            &dir.join("out"),
            &ImageDiffOptions::default(),
        )
        .expect("diff");
        assert_eq!(result["changed_pixels"], 0);
        assert_eq!(result["changed_pixel_percentage"], 0.0);
    }

    #[test]
    fn image_sized_png_is_not_rejected_by_compressed_file_size() {
        let dir = temp_dir("image-sized");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        let width = 768;
        let height = 768;
        let pixels = noisy_pixels(width, height);
        let mut changed = pixels.clone();
        changed[42] = [255, 0, 255, 255];
        write_png(&before, &pixels, width, height);
        write_png(&after, &changed, width, height);
        assert!(
            fs::metadata(&after).expect("image metadata").len() > 1_411 * 1024,
            "fixture should exercise a normal image payload larger than the old 1.411 MB complaint"
        );

        let result = diff_asset_image_value(
            &before,
            &after,
            &dir.join("out"),
            &ImageDiffOptions {
                pixel_threshold: 1,
                region_min_area: 1,
                ..ImageDiffOptions::default()
            },
        )
        .expect("image-sized png should compare");

        assert_eq!(result["status"], "compared");
        assert_eq!(result["comparison_dimensions"]["width"], width);
        assert_eq!(result["decoded_cost"]["after_pixels"], (width as usize * height as usize));
        assert_eq!(
            result["decoded_cost"]["max_decoded_pixels"],
            DEFAULT_MAX_DECODED_PIXELS
        );
    }

    #[test]
    fn decoded_pixel_policy_reports_actionable_limit() {
        let dir = temp_dir("decoded-policy");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        let pixels = [[10, 20, 30, 255]; 25];
        write_png(&before, &pixels, 5, 5);
        write_png(&after, &pixels, 5, 5);

        let err = diff_asset_image_value(
            &before,
            &after,
            &dir.join("out"),
            &ImageDiffOptions {
                max_decoded_pixels: 24,
                ..ImageDiffOptions::default()
            },
        )
        .expect_err("tiny decoded pixel cap should reject before full comparison");

        assert!(err.contains("image decoded dimensions too large"));
        assert!(err.contains("5x5"));
        assert!(err.contains("25 pixels"));
        assert!(err.contains("max_decoded_pixels"));
    }

    #[test]
    fn one_pixel_change_generates_artifacts() {
        let dir = temp_dir("one-pixel");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        write_png(&before, &[[0, 0, 0, 255]; 4], 2, 2);
        write_png(
            &after,
            &[
                [255, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
            ],
            2,
            2,
        );
        let options = ImageDiffOptions {
            region_min_area: 1,
            ..ImageDiffOptions::default()
        };
        let result =
            diff_asset_image_value(&before, &after, &dir.join("out"), &options).expect("diff");
        assert_eq!(result["changed_pixels"], 1);
        assert_eq!(result["changed_pixel_percentage"], 25.0);
        assert!(dir.join("out").join("contact-sheet.png").exists());
        assert!(dir.join("out").join("heatmap.png").exists());
        assert_eq!(
            result["artifacts"]["heatmap"],
            path_string(&dir.join("out").join("heatmap.png"))
        );
        assert_eq!(result["hotspots"].as_array().expect("hotspots").len(), 1);
        assert_eq!(result["hotspots"][0]["bbox"]["width"], 1);
        assert_eq!(result["hotspot_navigation"]["selected_id"], "hotspot-1");
        assert_eq!(result["histograms"]["bins"], 16);
        assert!(
            result["histograms"]["red_delta"]
                .as_array()
                .expect("red histogram")
                .len()
                == 16
        );
    }

    #[test]
    fn strict_dimension_mismatch_reports_without_artifacts() {
        let dir = temp_dir("dimensions");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        write_png(&before, &[[0, 0, 0, 255]; 4], 2, 2);
        write_png(&after, &[[0, 0, 0, 255]; 6], 3, 2);
        let result = diff_asset_image_value(
            &before,
            &after,
            &dir.join("out"),
            &ImageDiffOptions::default(),
        )
        .expect("diff");
        assert_eq!(result["status"], "dimension_mismatch");
        assert_eq!(result["changed_pixel_percentage"], serde_json::Value::Null);
    }

    #[test]
    fn resize_and_pad_dimension_policies_compare_images() {
        let dir = temp_dir("dimension-policies");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        write_png(&before, &[[0, 0, 0, 255]; 4], 2, 2);
        write_png(&after, &[[255, 0, 0, 255]; 6], 3, 2);

        let resize = ImageDiffOptions {
            dimension_policy: "resize".to_string(),
            region_min_area: 1,
            ..ImageDiffOptions::default()
        };
        let resize_result = diff_asset_image_value(&before, &after, &dir.join("resize"), &resize)
            .expect("resize diff");
        assert_eq!(resize_result["status"], "compared");
        assert_eq!(resize_result["comparison_dimensions"]["width"], 2);
        assert!(resize_result["warnings"].as_array().expect("warnings")[0]
            .as_str()
            .expect("warning")
            .contains("Resized after image"));

        let pad = ImageDiffOptions {
            dimension_policy: "pad".to_string(),
            region_min_area: 1,
            ..ImageDiffOptions::default()
        };
        let pad_result =
            diff_asset_image_value(&before, &after, &dir.join("pad"), &pad).expect("pad diff");
        assert_eq!(pad_result["status"], "compared");
        assert_eq!(pad_result["comparison_dimensions"]["width"], 3);
    }

    #[test]
    fn alpha_handling_can_include_or_ignore_transparency_changes() {
        let dir = temp_dir("alpha");
        let before = dir.join("before.png");
        let after = dir.join("after.png");
        write_png(&before, &[[10, 20, 30, 255]; 4], 2, 2);
        write_png(&after, &[[10, 20, 30, 0]; 4], 2, 2);

        let include = ImageDiffOptions {
            pixel_threshold: 1,
            region_min_area: 1,
            alpha_handling: "include".to_string(),
            ..ImageDiffOptions::default()
        };
        let include_result =
            diff_asset_image_value(&before, &after, &dir.join("include"), &include)
                .expect("include alpha diff");
        assert_eq!(include_result["changed_pixels"], 4);
        assert!(include_result["histograms"]["alpha_delta"].is_array());

        let ignore = ImageDiffOptions {
            pixel_threshold: 1,
            region_min_area: 1,
            alpha_handling: "ignore".to_string(),
            ..ImageDiffOptions::default()
        };
        let ignore_result = diff_asset_image_value(&before, &after, &dir.join("ignore"), &ignore)
            .expect("ignore alpha diff");
        assert_eq!(ignore_result["changed_pixels"], 0);
        assert_eq!(
            ignore_result["histograms"]["alpha_delta"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn jpg_inputs_decode_through_rust_image_provider() {
        let dir = temp_dir("jpeg");
        let before = dir.join("before.jpg");
        let after = dir.join("after.jpg");
        write_image(&before, &[[24, 40, 80, 255]; 9], 3, 3);
        write_image(&after, &[[28, 42, 82, 255]; 9], 3, 3);

        let result = diff_asset_image_value(
            &before,
            &after,
            &dir.join("out"),
            &ImageDiffOptions {
                pixel_threshold: 240,
                region_min_area: 1,
                ..ImageDiffOptions::default()
            },
        )
        .expect("jpeg diff");
        assert_eq!(result["mime_type"], "image/jpeg");
        assert_eq!(result["changed_pixels"], 0);
    }

    #[test]
    fn unsupported_binary_file_returns_decode_error() {
        let dir = temp_dir("unsupported");
        let before = dir.join("before.bin");
        let after = dir.join("after.bin");
        fs::write(&before, [0, 1, 2, 3]).expect("write before");
        fs::write(&after, [4, 5, 6, 7]).expect("write after");

        let err = diff_asset_image_value(
            &before,
            &after,
            &dir.join("out"),
            &ImageDiffOptions::default(),
        )
        .expect_err("unsupported images should not decode");
        assert!(
            err.contains("decode image")
                || err.contains("guess image format")
                || err.contains("read image dimensions")
        );
    }

    #[test]
    fn git_working_tree_image_change_is_discovered_in_rust() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("git-image");
        run_test_git(&dir, &["init"]);
        run_test_git(&dir, &["config", "user.email", "intentdiff@example.test"]);
        run_test_git(&dir, &["config", "user.name", "IntentDiff Test"]);
        let asset = dir.join("asset.png");
        write_png(&asset, &[[0, 0, 0, 255]; 4], 2, 2);
        run_test_git(&dir, &["add", "asset.png"]);
        run_test_git(&dir, &["commit", "-m", "base"]);
        write_png(
            &asset,
            &[
                [255, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
            ],
            2,
            2,
        );

        let options = ImageDiffOptions {
            region_min_area: 1,
            ..ImageDiffOptions::default()
        };
        let result = diff_git_assets_value(&dir, "HEAD", "", &dir.join("out"), &options)
            .expect("git asset diff");

        assert_eq!(result["kind"], "asset_diff_batch");
        assert_eq!(
            result["asset_diffs"].as_array().expect("diff list").len(),
            1
        );
        assert_eq!(result["asset_diffs"][0]["file_path"], "asset.png");
    }

    #[test]
    fn git_added_deleted_and_staged_image_assets_are_classified_in_rust() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("git-lifecycle");
        run_test_git(&dir, &["init"]);
        run_test_git(&dir, &["config", "user.email", "intentdiff@example.test"]);
        run_test_git(&dir, &["config", "user.name", "IntentDiff Test"]);

        let modified = dir.join("modified.png");
        let deleted = dir.join("deleted.png");
        write_png(&modified, &[[0, 0, 0, 255]; 4], 2, 2);
        write_png(&deleted, &[[0, 0, 0, 255]; 4], 2, 2);
        run_test_git(&dir, &["add", "modified.png", "deleted.png"]);
        run_test_git(&dir, &["commit", "-m", "base"]);

        write_png(
            &modified,
            &[
                [255, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
                [0, 0, 0, 255],
            ],
            2,
            2,
        );
        fs::remove_file(&deleted).expect("delete image");
        write_png(&dir.join("added.png"), &[[0, 255, 0, 255]; 4], 2, 2);
        run_test_git(&dir, &["add", "modified.png"]);

        let options = ImageDiffOptions {
            region_min_area: 1,
            ..ImageDiffOptions::default()
        };
        let working = diff_git_assets_value(&dir, "HEAD", "", &dir.join("working"), &options)
            .expect("working asset diff");
        assert_eq!(working["asset_diffs"].as_array().expect("diffs").len(), 1);
        let skipped = working["skipped"].as_array().expect("skipped");
        assert!(skipped
            .iter()
            .any(|item| item["file_path"] == "added.png" && item["change_type"] == "A"));
        assert!(skipped
            .iter()
            .any(|item| item["file_path"] == "deleted.png" && item["change_type"] == "D"));

        let staged = diff_git_assets_value(&dir, "HEAD", ":staged", &dir.join("staged"), &options)
            .expect("staged asset diff");
        assert_eq!(
            staged["asset_diffs"]
                .as_array()
                .expect("staged diffs")
                .len(),
            1
        );
        assert_eq!(staged["head"], ":staged");
    }
}
