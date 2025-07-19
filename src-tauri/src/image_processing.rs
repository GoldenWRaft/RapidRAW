use std::{io::Read, sync::Arc};
use base64::{engine::general_purpose, Engine};
use bytemuck::{Pod, Zeroable};
use image::{buffer, DynamicImage, GenericImageView, Rgba};
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::f32::consts::PI;
use rawler::decoders::Orientation;

pub use crate::gpu_processing::{get_or_init_gpu_context, process_and_get_dynamic_image};
use crate::{AppState, mask_generation::MaskDefinition, load_settings};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageMetadata {
    pub version: u32,
    pub rating: u8,
    pub adjustments: Value,
}

impl Default for ImageMetadata {
    fn default() -> Self {
        ImageMetadata {
            version: 1,
            rating: 0,
            adjustments: Value::Null,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub fn apply_orientation(image: DynamicImage, orientation: Orientation) -> DynamicImage {
    match orientation {
        Orientation::Normal | Orientation::Unknown => image,
        Orientation::HorizontalFlip => image.fliph(),
        Orientation::Rotate180 => image.rotate180(),
        Orientation::VerticalFlip => image.flipv(),
        Orientation::Transpose => image.rotate90().flipv(),
        Orientation::Rotate90 => image.rotate90(),
        Orientation::Transverse => image.rotate90().fliph(),
        Orientation::Rotate270 => image.rotate270(),
    }
}

pub fn apply_rotation(image: &DynamicImage, rotation_degrees: f32) -> DynamicImage {
    if rotation_degrees % 360.0 == 0.0 {
        return image.clone();
    }

    let rgba_image = image.to_rgba8();
    
    let rotated = rotate_about_center(
        &rgba_image,
        rotation_degrees * PI / 180.0,
        Interpolation::Bilinear,
        Rgba([0u8, 0, 0, 0]),
    );

    DynamicImage::ImageRgba8(rotated)
}

pub fn apply_crop(mut image: DynamicImage, crop_value: &Value) -> DynamicImage {
    if crop_value.is_null() {
        return image;
    }
    if let Ok(crop) = serde_json::from_value::<Crop>(crop_value.clone()) {
        let x = crop.x.round() as u32;
        let y = crop.y.round() as u32;
        let width = crop.width.round() as u32;
        let height = crop.height.round() as u32;

        if width > 0 && height > 0 {
            let (img_w, img_h) = image.dimensions();
            if x < img_w && y < img_h {
                let new_width = (img_w - x).min(width);
                let new_height = (img_h - y).min(height);
                if new_width > 0 && new_height > 0 {
                    image = image.crop_imm(x, y, new_width, new_height);
                }
            }
        }
    }
    image
}

pub fn apply_flip(image: DynamicImage, horizontal: bool, vertical: bool) -> DynamicImage {
    let mut img = image;
    if horizontal {
        img = img.fliph();
    }
    if vertical {
        img = img.flipv();
    }
    img
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default)]
#[repr(C)]
pub struct Point {
    x: f32,
    y: f32,
    _pad1: f32,
    _pad2: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default)]
#[repr(C)]
pub struct HslColor {
    hue: f32,
    saturation: f32,
    luminance: f32,
    _pad: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default)]
#[repr(C)]
pub struct GlobalAdjustments {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    pub vibrance: f32,
    
    pub sharpness: f32,
    pub luma_noise_reduction: f32,
    pub color_noise_reduction: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub structure: f32,
    pub vignette_amount: f32,
    pub vignette_midpoint: f32,
    pub vignette_roundness: f32,
    pub vignette_feather: f32,
    pub grain_amount: f32,
    pub grain_size: f32,
    pub grain_roughness: f32,
    _pad1: f32,

    pub hsl: [HslColor; 8],
    pub luma_curve: [Point; 16],
    pub red_curve: [Point; 16],
    pub green_curve: [Point; 16],
    pub blue_curve: [Point; 16],
    pub luma_curve_count: u32,
    pub red_curve_count: u32,
    pub green_curve_count: u32,
    pub blue_curve_count: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default)]
#[repr(C)]
pub struct MaskAdjustments {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    pub vibrance: f32,
    
    pub sharpness: f32,
    pub luma_noise_reduction: f32,
    pub color_noise_reduction: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub structure: f32,
    
    _pad1: f32,
    _pad2: f32,
    _pad3: f32,
    _pad4: f32,

    pub hsl: [HslColor; 8],
    pub luma_curve: [Point; 16],
    pub red_curve: [Point; 16],
    pub green_curve: [Point; 16],
    pub blue_curve: [Point; 16],
    pub luma_curve_count: u32,
    pub red_curve_count: u32,
    pub green_curve_count: u32,
    pub blue_curve_count: u32,
}

#[derive(Debug, Clone, Copy, Pod, Zeroable, Default)]
#[repr(C)]
pub struct AllAdjustments {
    pub global: GlobalAdjustments,
    pub mask_adjustments: [MaskAdjustments; 16],
    pub mask_count: u32,
    pub tile_offset_x: u32,
    pub tile_offset_y: u32,
    _pad1: u32,
}

struct AdjustmentScales {
    exposure: f32,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
    vibrance: f32,
    
    sharpness: f32,
    luma_noise_reduction: f32,
    color_noise_reduction: f32,
    clarity: f32,
    dehaze: f32,
    structure: f32,

    vignette_amount: f32,
    vignette_midpoint: f32,
    vignette_roundness: f32,
    vignette_feather: f32,
    grain_amount: f32,
    grain_size: f32,
    grain_roughness: f32,

    hsl_hue_multiplier: f32,
    hsl_saturation: f32,
    hsl_luminance: f32,
}

const SCALES: AdjustmentScales = AdjustmentScales {
    exposure: 25.0,
    contrast: 100.0,
    highlights: 100.0,
    shadows: 500.0,
    whites: 30.0,
    blacks: 50.0,
    saturation: 80.0,
    temperature: 30.0,
    tint: 200.0,
    vibrance: 80.0,
    
    sharpness: 40.0,
    luma_noise_reduction: 100.0,
    color_noise_reduction: 100.0,
    clarity: 100.0,
    dehaze: 750.0,
    structure: 100.0,

    vignette_amount: 100.0,
    vignette_midpoint: 100.0,
    vignette_roundness: 100.0,
    vignette_feather: 100.0,
    grain_amount: 200.0,
    grain_size: 50.0,
    grain_roughness: 100.0,

    hsl_hue_multiplier: 0.3,
    hsl_saturation: 100.0,
    hsl_luminance: 100.0,
};

fn parse_hsl_adjustments(js_hsl: &serde_json::Value) -> [HslColor; 8] {
    let mut hsl_array = [HslColor::default(); 8];
    if let Some(hsl_map) = js_hsl.as_object() {
        let color_map = [
            ("reds", 0), ("oranges", 1), ("yellows", 2), ("greens", 3),
            ("aquas", 4), ("blues", 5), ("purples", 6), ("magentas", 7),
        ];
        for (name, index) in color_map.iter() {
            if let Some(color_data) = hsl_map.get(*name) {
                hsl_array[*index] = HslColor {
                    hue: color_data["hue"].as_f64().unwrap_or(0.0) as f32 * SCALES.hsl_hue_multiplier,
                    saturation: color_data["saturation"].as_f64().unwrap_or(0.0) as f32 / SCALES.hsl_saturation,
                    luminance: color_data["luminance"].as_f64().unwrap_or(0.0) as f32 / SCALES.hsl_luminance,
                    _pad: 0.0,
                };
            }
        }
    }
    hsl_array
}

fn convert_points_to_aligned(frontend_points: Vec<serde_json::Value>) -> [Point; 16] {
    let mut aligned_points = [Point::default(); 16];
    for (i, point) in frontend_points.iter().enumerate().take(16) {
        if let (Some(x), Some(y)) = (point["x"].as_f64(), point["y"].as_f64()) {
            aligned_points[i] = Point { x: x as f32, y: y as f32, _pad1: 0.0, _pad2: 0.0 };
        }
    }
    aligned_points
}

fn get_global_adjustments_from_json(js_adjustments: &serde_json::Value) -> GlobalAdjustments {
    if js_adjustments.is_null() {
        return GlobalAdjustments::default();
    }

    let visibility = js_adjustments.get("sectionVisibility");
    let is_visible = |section: &str| -> bool {
        visibility
            .and_then(|v| v.get(section))
            .and_then(|s| s.as_bool())
            .unwrap_or(true)
    };
    
    let get_val = |section: &str, key: &str, scale: f32, default: Option<f64>| -> f32 {
        if is_visible(section) {
            js_adjustments[key].as_f64().unwrap_or(default.unwrap_or(0.0)) as f32 / scale
        } else {
            if let Some(d) = default { d as f32 / scale } else { 0.0 }
        }
    };

    let curves_obj = js_adjustments.get("curves").cloned().unwrap_or_default();
    let luma_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["luma"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let red_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["red"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let green_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["green"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let blue_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["blue"].as_array().cloned().unwrap_or_default() } else { Vec::new() };

    GlobalAdjustments {
        exposure: get_val("basic", "exposure", SCALES.exposure, None),
        contrast: get_val("basic", "contrast", SCALES.contrast, None),
        highlights: get_val("basic", "highlights", SCALES.highlights, None),
        shadows: get_val("basic", "shadows", SCALES.shadows, None),
        whites: get_val("basic", "whites", SCALES.whites, None),
        blacks: get_val("basic", "blacks", SCALES.blacks, None),
        
        saturation: get_val("color", "saturation", SCALES.saturation, None),
        temperature: get_val("color", "temperature", SCALES.temperature, None),
        tint: get_val("color", "tint", SCALES.tint, None),
        vibrance: get_val("color", "vibrance", SCALES.vibrance, None),
        
        sharpness: get_val("details", "sharpness", SCALES.sharpness, None),
        luma_noise_reduction: get_val("details", "lumaNoiseReduction", SCALES.luma_noise_reduction, None),
        color_noise_reduction: get_val("details", "colorNoiseReduction", SCALES.color_noise_reduction, None),
        
        clarity: get_val("effects", "clarity", SCALES.clarity, None),
        dehaze: get_val("effects", "dehaze", SCALES.dehaze, None),
        structure: get_val("effects", "structure", SCALES.structure, None),
        vignette_amount: get_val("effects", "vignetteAmount", SCALES.vignette_amount, None),
        vignette_midpoint: get_val("effects", "vignetteMidpoint", SCALES.vignette_midpoint, Some(50.0)),
        vignette_roundness: get_val("effects", "vignetteRoundness", SCALES.vignette_roundness, Some(0.0)),
        vignette_feather: get_val("effects", "vignetteFeather", SCALES.vignette_feather, Some(50.0)),
        grain_amount: get_val("effects", "grainAmount", SCALES.grain_amount, None),
        grain_size: get_val("effects", "grainSize", SCALES.grain_size, Some(25.0)),
        grain_roughness: get_val("effects", "grainRoughness", SCALES.grain_roughness, Some(50.0)),
        _pad1: 0.0,

        hsl: if is_visible("color") { parse_hsl_adjustments(&js_adjustments.get("hsl").cloned().unwrap_or_default()) } else { [HslColor::default(); 8] },
        luma_curve: convert_points_to_aligned(luma_points.clone()),
        red_curve: convert_points_to_aligned(red_points.clone()),
        green_curve: convert_points_to_aligned(green_points.clone()),
        blue_curve: convert_points_to_aligned(blue_points.clone()),
        luma_curve_count: luma_points.len() as u32,
        red_curve_count: red_points.len() as u32,
        green_curve_count: green_points.len() as u32,
        blue_curve_count: blue_points.len() as u32,
    }
}

fn get_mask_adjustments_from_json(adj: &serde_json::Value) -> MaskAdjustments {
    if adj.is_null() {
        return MaskAdjustments::default();
    }

    let visibility = adj.get("sectionVisibility");
    let is_visible = |section: &str| -> bool {
        visibility
            .and_then(|v| v.get(section))
            .and_then(|s| s.as_bool())
            .unwrap_or(true)
    };
    
    let get_val = |section: &str, key: &str, scale: f32| -> f32 {
        if is_visible(section) {
            adj[key].as_f64().unwrap_or(0.0) as f32 / scale
        } else {
            0.0
        }
    };

    let curves_obj = adj.get("curves").cloned().unwrap_or_default();
    let luma_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["luma"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let red_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["red"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let green_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["green"].as_array().cloned().unwrap_or_default() } else { Vec::new() };
    let blue_points: Vec<serde_json::Value> = if is_visible("curves") { curves_obj["blue"].as_array().cloned().unwrap_or_default() } else { Vec::new() };

    MaskAdjustments {
        exposure: get_val("basic", "exposure", SCALES.exposure),
        contrast: get_val("basic", "contrast", SCALES.contrast),
        highlights: get_val("basic", "highlights", SCALES.highlights),
        shadows: get_val("basic", "shadows", SCALES.shadows),
        whites: get_val("basic", "whites", SCALES.whites),
        blacks: get_val("basic", "blacks", SCALES.blacks),
        
        saturation: get_val("color", "saturation", SCALES.saturation),
        temperature: get_val("color", "temperature", SCALES.temperature),
        tint: get_val("color", "tint", SCALES.tint),
        vibrance: get_val("color", "vibrance", SCALES.vibrance),
        
        sharpness: get_val("details", "sharpness", SCALES.sharpness),
        luma_noise_reduction: get_val("details", "lumaNoiseReduction", SCALES.luma_noise_reduction),
        color_noise_reduction: get_val("details", "colorNoiseReduction", SCALES.color_noise_reduction),
        
        clarity: get_val("effects", "clarity", SCALES.clarity),
        dehaze: get_val("effects", "dehaze", SCALES.dehaze),
        structure: get_val("effects", "structure", SCALES.structure),
        
        _pad1: 0.0, _pad2: 0.0, _pad3: 0.0, _pad4: 0.0,

        hsl: if is_visible("color") { parse_hsl_adjustments(&adj.get("hsl").cloned().unwrap_or_default()) } else { [HslColor::default(); 8] },
        luma_curve: convert_points_to_aligned(luma_points.clone()),
        red_curve: convert_points_to_aligned(red_points.clone()),
        green_curve: convert_points_to_aligned(green_points.clone()),
        blue_curve: convert_points_to_aligned(blue_points.clone()),
        luma_curve_count: luma_points.len() as u32,
        red_curve_count: red_points.len() as u32,
        green_curve_count: green_points.len() as u32,
        blue_curve_count: blue_points.len() as u32,
    }
}

pub fn get_all_adjustments_from_json(js_adjustments: &serde_json::Value) -> AllAdjustments {
    let global = get_global_adjustments_from_json(js_adjustments);
    let mut mask_adjustments = [MaskAdjustments::default(); 16];
    let mut mask_count = 0;

    let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_else(Vec::new);

    for (i, mask_def) in mask_definitions.iter().filter(|m| m.visible).enumerate().take(16) {
        mask_adjustments[i] = get_mask_adjustments_from_json(&mask_def.adjustments);
        mask_count += 1;
    }

    AllAdjustments {
        global,
        mask_adjustments,
        mask_count,
        tile_offset_x: 0,
        tile_offset_y: 0,
        _pad1: 0,
    }
}

#[derive(Clone)]
pub struct GpuContext {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub limits: wgpu::Limits,
}

impl GpuContext {
    pub async fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }).await.ok_or("Failed to find a suitable GPU adapter")?;

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("RapidRAW Device"),
                required_features: wgpu::Features::FLOAT32_FILTERABLE,
                required_limits: wgpu::Limits::default(),
            },
            None,
        ).await.map_err(|e| format!("Failed to create device: {}", e))?;

        Ok(GpuContext {
            device: Arc::new(device),
            queue: Arc::new(queue),
            limits: adapter.limits(),
        })
    }
}

#[derive(Serialize, Clone)]
pub struct HistogramData {
    red: Vec<f32>,
    green: Vec<f32>,
    blue: Vec<f32>,
    luma: Vec<f32>,
}

#[tauri::command]
pub fn generate_histogram(state: tauri::State<AppState>, app_handle: tauri::AppHandle) -> Result<HistogramData, String> {
    let cached_preview_lock = state.cached_preview.lock().unwrap();

    if let Some(cached) = &*cached_preview_lock {
        calculate_histogram_from_image(&cached.image)
    } else {
        drop(cached_preview_lock);
        let image = state.original_image.lock().unwrap().as_ref()
            .ok_or("No image loaded to generate histogram")?
            .image.clone();

        let settings = load_settings(app_handle).unwrap_or_default();
        let preview_dim = settings.editor_preview_resolution.unwrap_or(1920);
        let preview = image.thumbnail(preview_dim, preview_dim);
        calculate_histogram_from_image(&preview)
    }
}

pub fn calculate_histogram_from_image(image: &DynamicImage) -> Result<HistogramData, String> {
    let mut red_counts = vec![0u32; 256];
    let mut green_counts = vec![0u32; 256];
    let mut blue_counts = vec![0u32; 256];
    let mut luma_counts = vec![0u32; 256];

    for pixel in image.to_rgb8().pixels() {
        let r = pixel[0] as usize;
        let g = pixel[1] as usize;
        let b = pixel[2] as usize;
        red_counts[r] += 1;
        green_counts[g] += 1;
        blue_counts[b] += 1;
        let luma_val = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as usize;
        luma_counts[luma_val.min(255)] += 1;
    }

    let mut red: Vec<f32> = red_counts.into_iter().map(|c| c as f32).collect();
    let mut green: Vec<f32> = green_counts.into_iter().map(|c| c as f32).collect();
    let mut blue: Vec<f32> = blue_counts.into_iter().map(|c| c as f32).collect();
    let mut luma: Vec<f32> = luma_counts.into_iter().map(|c| c as f32).collect();

    let smoothing_sigma = 2.5;
    apply_gaussian_smoothing(&mut red, smoothing_sigma);
    apply_gaussian_smoothing(&mut green, smoothing_sigma);
    apply_gaussian_smoothing(&mut blue, smoothing_sigma);
    apply_gaussian_smoothing(&mut luma, smoothing_sigma);

    normalize_histogram_range(&mut red, 0.99);
    normalize_histogram_range(&mut green, 0.99);
    normalize_histogram_range(&mut blue, 0.99);
    normalize_histogram_range(&mut luma, 0.99);

    Ok(HistogramData { red, green, blue, luma })
}

fn apply_gaussian_smoothing(histogram: &mut Vec<f32>, sigma: f32) {
    if sigma <= 0.0 { return; }
    
    let kernel_radius = (sigma * 3.0).ceil() as usize;
    if kernel_radius == 0 || kernel_radius >= histogram.len() { return; }

    let kernel_size = 2 * kernel_radius + 1;
    let mut kernel = vec![0.0; kernel_size];
    let mut kernel_sum = 0.0;

    let two_sigma_sq = 2.0 * sigma * sigma;
    for i in 0..kernel_size {
        let x = (i as i32 - kernel_radius as i32) as f32;
        let val = (-x * x / two_sigma_sq).exp();
        kernel[i] = val;
        kernel_sum += val;
    }

    if kernel_sum > 0.0 {
        for val in &mut kernel {
            *val /= kernel_sum;
        }
    }

    let original = histogram.clone();
    let len = histogram.len();

    for i in 0..len {
        let mut smoothed_val = 0.0;
        for k in 0..kernel_size {
            let offset = k as i32 - kernel_radius as i32;
            let sample_index = i as i32 + offset;
            let clamped_index = sample_index.clamp(0, len as i32 - 1) as usize;
            smoothed_val += original[clamped_index] * kernel[k];
        }
        histogram[i] = smoothed_val;
    }
}

fn normalize_histogram_range(histogram: &mut Vec<f32>, percentile_clip: f32) {
    if histogram.is_empty() { return; }

    let mut sorted_data = histogram.clone();
    sorted_data.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let clip_index = ((sorted_data.len() - 1) as f32 * percentile_clip).round() as usize;
    let max_val = sorted_data[clip_index.min(sorted_data.len() - 1)];

    if max_val > 1e-6 {
        let scale_factor = 1.0 / max_val;
        for value in histogram.iter_mut() {
            *value = (*value * scale_factor).min(1.0);
        }
    } else {
        for value in histogram.iter_mut() {
            *value = 0.0;
        }
    }
}

#[derive(Serialize, Clone)]
pub struct WaveformData {
    red: Vec<u32>,
    green: Vec<u32>,
    blue: Vec<u32>,
    luma: Vec<u32>,
    width: u32,
    height: u32,
}

#[tauri::command]
pub fn generate_waveform(state: tauri::State<AppState>, app_handle: tauri::AppHandle) -> Result<WaveformData, String> {
    let cached_preview_lock = state.cached_preview.lock().unwrap();

    if let Some(cached) = &*cached_preview_lock {
        calculate_waveform_from_image(&cached.image)
    } else {
        drop(cached_preview_lock);
        let image = state.original_image.lock().unwrap().as_ref()
            .ok_or("No image loaded to generate waveform")?
            .image.clone();

        let settings = load_settings(app_handle).unwrap_or_default();
        let preview_dim = settings.editor_preview_resolution.unwrap_or(1920);
        let preview = image.thumbnail(preview_dim, preview_dim);
        calculate_waveform_from_image(&preview)
    }
}

pub fn calculate_waveform_from_image(image: &DynamicImage) -> Result<WaveformData, String> {
    const WAVEFORM_WIDTH: u32 = 256;
    const WAVEFORM_HEIGHT: u32 = 256;

    if image.width() == 0 || image.height() == 0 {
        return Err("Image has zero dimensions.".to_string());
    }
    let preview_height = (image.height() as f32 * (WAVEFORM_WIDTH as f32 / image.width() as f32)).round() as u32;
    if preview_height == 0 {
        return Err("Image has zero height after scaling for waveform.".to_string());
    }
    let preview = image.resize(WAVEFORM_WIDTH, preview_height, image::imageops::FilterType::Triangle);
    let rgb_image = preview.to_rgb8();

    let mut red = vec![0; (WAVEFORM_WIDTH * WAVEFORM_HEIGHT) as usize];
    let mut green = vec![0; (WAVEFORM_WIDTH * WAVEFORM_HEIGHT) as usize];
    let mut blue = vec![0; (WAVEFORM_WIDTH * WAVEFORM_HEIGHT) as usize];
    let mut luma = vec![0; (WAVEFORM_WIDTH * WAVEFORM_HEIGHT) as usize];

    for (x, _, pixel) in rgb_image.enumerate_pixels() {
        let r = pixel[0] as usize;
        let g = pixel[1] as usize;
        let b = pixel[2] as usize;

        let r_idx = (255 - r) * WAVEFORM_WIDTH as usize + x as usize;
        let g_idx = (255 - g) * WAVEFORM_WIDTH as usize + x as usize;
        let b_idx = (255 - b) * WAVEFORM_WIDTH as usize + x as usize;

        red[r_idx] += 1;
        green[g_idx] += 1;
        blue[b_idx] += 1;

        let luma_val = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32).round() as usize;
        let luma_idx = (255 - luma_val.min(255)) * WAVEFORM_WIDTH as usize + x as usize;
        luma[luma_idx] += 1;
    }

    Ok(WaveformData {
        red,
        green,
        blue,
        luma,
        width: WAVEFORM_WIDTH,
        height: WAVEFORM_HEIGHT,
    })
}

#[tauri::command]
pub fn load_file_data(path: &str) -> String {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {}", e));

    let buffer = std::io::BufReader::new(file.unwrap());

    let base64 = general_purpose::STANDARD.encode(buffer.get_ref().bytes()
        .map(|b| b.unwrap_or(0))
        .collect::<Vec<u8>>());
    base64
}

// read file data as string
#[tauri::command]
pub fn read_file_data(path: &str) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).map_err(|e| format!("Failed to read file: {}", e))?;
    Ok(contents)
}