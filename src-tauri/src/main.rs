#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod comfyui_connector;
mod image_processing;
mod file_management;
mod gpu_processing;
mod raw_processing;
mod mask_generation;
mod ai_processing;
mod formats;
mod image_loader;
mod lut_processes;

use std::io::{Cursor, BufWriter};
use std::sync::{Arc, Mutex};
use std::thread;
use std::fs;
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::path::Path;
use std::process::Command;
use std::hash::{Hash, Hasher};

use image::{DynamicImage, GenericImageView, ImageBuffer, Luma, Rgba, RgbaImage, ImageFormat, GrayImage};
use image::codecs::jpeg::JpegEncoder;
use tauri::{Manager, Emitter};
use base64::{Engine as _, engine::general_purpose};
use serde_json::Value;
use tokio::task::JoinHandle;
use walkdir::WalkDir;
use window_vibrancy::{apply_acrylic, apply_vibrancy, NSVisualEffectMaterial};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use rayon::prelude::*;

use crate::image_processing::{
    get_all_adjustments_from_json, get_or_init_gpu_context, GpuContext,
    ImageMetadata, process_and_get_dynamic_image, Crop, apply_crop, apply_rotation, apply_flip,
};
use crate::file_management::get_sidecar_path;
use crate::mask_generation::{MaskDefinition, generate_mask_bitmap};
use crate::ai_processing::{
    AiState, get_or_init_ai_models, generate_image_embeddings, run_sam_decoder,
    AiSubjectMaskParameters, run_u2netp_model, AiForegroundMaskParameters
};
use crate::formats::is_raw_file;
use crate::image_loader::{load_base_image_from_bytes, composite_patches_on_image, load_and_composite};

use crate::lut_processes::*;

#[derive(Clone)]
pub struct LoadedImage {
    image: DynamicImage,
    full_width: u32,
    full_height: u32,
}

#[derive(Clone)]
pub struct CachedPreview {
    image: DynamicImage,
    transform_hash: u64,
    scale: f32,
    unscaled_crop_offset: (f32, f32),
}

pub struct AppState {
    original_image: Mutex<Option<LoadedImage>>,
    cached_preview: Mutex<Option<CachedPreview>>,
    pub gpu_context: Mutex<Option<GpuContext>>,
    ai_state: Mutex<Option<AiState>>,
    export_task_handle: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Preset {
    id: String,
    name: String,
    adjustments: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PresetFile {
    presets: Vec<Preset>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct SortCriteria {
    key: String,
    order: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct LastFolderState {
    current_folder_path: String,
    expanded_folders: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    last_root_path: Option<String>,
    editor_preview_resolution: Option<u32>,
    sort_criteria: Option<SortCriteria>,
    theme: Option<String>,
    transparent: Option<bool>,
    comfyui_address: Option<String>,
    last_folder_state: Option<LastFolderState>,
}

#[derive(serde::Serialize)]
struct LoadImageResult {
    original_base64: String,
    width: u32,
    height: u32,
    metadata: ImageMetadata,
    exif: HashMap<String, String>,
    is_raw: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
enum ResizeMode {
    LongEdge,
    Width,
    Height,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct ResizeOptions {
    mode: ResizeMode,
    value: u32,
    dont_enlarge: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct ExportSettings {
    jpeg_quality: u8,
    resize: Option<ResizeOptions>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            last_root_path: None,
            editor_preview_resolution: Some(1920),
            sort_criteria: None,
            theme: Some("dark".to_string()),
            transparent: Some(true),
            comfyui_address: None,
            last_folder_state: None,
        }
    }
}

fn apply_all_transformations(
    image: &DynamicImage,
    adjustments: &serde_json::Value,
    scale: f32,
) -> (DynamicImage, (f32, f32)) {
    let rotation_degrees = adjustments["rotation"].as_f64().unwrap_or(0.0) as f32;
    let flip_horizontal = adjustments["flipHorizontal"].as_bool().unwrap_or(false);
    let flip_vertical = adjustments["flipVertical"].as_bool().unwrap_or(false);

    let flipped_image = apply_flip(image.clone(), flip_horizontal, flip_vertical);
    let rotated_image = apply_rotation(&flipped_image, rotation_degrees);

    let crop_data: Option<Crop> = serde_json::from_value(adjustments["crop"].clone()).ok();
    
    let scaled_crop_json = if let Some(c) = &crop_data {
        serde_json::to_value(Crop {
            x: c.x * scale as f64,
            y: c.y * scale as f64,
            width: c.width * scale as f64,
            height: c.height * scale as f64,
        }).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };

    let cropped_image = apply_crop(rotated_image, &scaled_crop_json);
    
    let unscaled_crop_offset = crop_data.map_or((0.0, 0.0), |c| (c.x as f32, c.y as f32));

    (cropped_image, unscaled_crop_offset)
}

fn calculate_transform_hash(adjustments: &serde_json::Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    
    let rotation = adjustments["rotation"].as_f64().unwrap_or(0.0);
    (rotation.to_bits()).hash(&mut hasher);

    let flip_h = adjustments["flipHorizontal"].as_bool().unwrap_or(false);
    flip_h.hash(&mut hasher);
    
    let flip_v = adjustments["flipVertical"].as_bool().unwrap_or(false);
    flip_v.hash(&mut hasher);

    if let Some(crop_val) = adjustments.get("crop") {
        if !crop_val.is_null() {
            crop_val.to_string().hash(&mut hasher);
        }
    }
    
    if let Some(patches_val) = adjustments.get("aiPatches") {
        if let Some(patches_arr) = patches_val.as_array() {
            for patch in patches_arr {
                if let Some(id) = patch.get("id").and_then(|v| v.as_str()) {
                    id.hash(&mut hasher);
                }
                let is_visible = patch.get("visible").and_then(|v| v.as_bool()).unwrap_or(true);
                is_visible.hash(&mut hasher);

                let data_exists = patch.get("patchDataBase64").is_some();
                data_exists.hash(&mut hasher);
            }
        }
    }

    hasher.finish()
}

fn generate_transformed_preview(
    loaded_image: &LoadedImage,
    adjustments: &serde_json::Value,
    app_handle: &tauri::AppHandle,
) -> Result<(DynamicImage, f32, (f32, f32)), String> {
    let patched_original_image = composite_patches_on_image(&loaded_image.image, adjustments)
        .map_err(|e| format!("Failed to composite AI patches: {}", e))?;
    
    let (full_w, full_h) = (loaded_image.full_width, loaded_image.full_height);

    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let final_preview_dim = settings.editor_preview_resolution.unwrap_or(1920);

    let (processing_base, scale_for_gpu) = 
        if full_w > final_preview_dim || full_h > final_preview_dim {
            let base = patched_original_image.thumbnail(final_preview_dim, final_preview_dim);
            let scale = if full_w > 0 { base.width() as f32 / full_w as f32 } else { 1.0 };
            (base, scale)
        } else {
            (patched_original_image.clone(), 1.0)
        };

    let (final_preview_base, unscaled_crop_offset) = 
        apply_all_transformations(&processing_base, adjustments, scale_for_gpu);
    
    Ok((final_preview_base, scale_for_gpu, unscaled_crop_offset))
}

fn encode_to_base64(image: &DynamicImage, quality: u8) -> Result<String, String> {
    let rgb_image = image.to_rgb8();

    let mut buf = Cursor::new(Vec::new());
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    rgb_image.write_with_encoder(encoder).map_err(|e| e.to_string())?;
    
    let base64_str = general_purpose::STANDARD.encode(buf.get_ref());
    Ok(format!("data:image/jpeg;base64,{}", base64_str))
}

fn encode_to_base64_png(image: &GrayImage) -> Result<String, String> {
    let mut buf = Cursor::new(Vec::new());
    image.write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
    let base64_str = general_purpose::STANDARD.encode(buf.get_ref());
    Ok(format!("data:image/png;base64,{}", base64_str))
}

fn read_exif_data(file_bytes: &[u8]) -> HashMap<String, String> {
    let mut exif_data = HashMap::new();
    let exif_reader = exif::Reader::new();
    if let Ok(exif) = exif_reader.read_from_container(&mut Cursor::new(file_bytes)) {
        for field in exif.fields() {
            exif_data.insert(
                field.tag.to_string(),
                field.display_value().with_unit(&exif).to_string(),
            );
        }
    }
    exif_data
}

#[tauri::command]
async fn load_image(path: String, state: tauri::State<'_, AppState>, app_handle: tauri::AppHandle) -> Result<LoadImageResult, String> {
    let sidecar_path = get_sidecar_path(&path);
    let metadata: ImageMetadata = if sidecar_path.exists() {
        let file_content = fs::read_to_string(sidecar_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&file_content).unwrap_or_default()
    } else {
        ImageMetadata::default()
    };

    let file_bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let pristine_img = load_base_image_from_bytes(&file_bytes, &path, false)
        .map_err(|e| e.to_string())?;

    let (orig_width, orig_height) = pristine_img.dimensions();
    let is_raw = is_raw_file(&path);

    let exif_data = read_exif_data(&file_bytes);

    let settings = load_settings(app_handle).unwrap_or_default();
    let display_preview_dim = settings.editor_preview_resolution.unwrap_or(1920);
    let display_preview = pristine_img.thumbnail(display_preview_dim, display_preview_dim);
    let original_base64 = encode_to_base64(&display_preview, 85)?;

    *state.cached_preview.lock().unwrap() = None;
    *state.original_image.lock().unwrap() = Some(LoadedImage {
        image: pristine_img,
        full_width: orig_width,
        full_height: orig_height,
    });
    
    Ok(LoadImageResult {
        original_base64,
        width: orig_width,
        height: orig_height,
        metadata,
        exif: exif_data,
        is_raw,
    })
}

#[tauri::command]
fn apply_adjustments(
    js_adjustments: serde_json::Value,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let context = get_or_init_gpu_context(&state)?;
    let adjustments_clone = js_adjustments.clone();
    
    let loaded_image = state.original_image.lock().unwrap().clone().ok_or("No original image loaded")?;
    let new_transform_hash = calculate_transform_hash(&adjustments_clone);

    let mut cached_preview_lock = state.cached_preview.lock().unwrap();
    
    let (final_preview_base, scale_for_gpu, unscaled_crop_offset) = 
        if let Some(cached) = &*cached_preview_lock {
            if cached.transform_hash == new_transform_hash {
                (cached.image.clone(), cached.scale, cached.unscaled_crop_offset)
            } else {
                let (base, scale, offset) = generate_transformed_preview(&loaded_image, &adjustments_clone, &app_handle)?;
                *cached_preview_lock = Some(CachedPreview {
                    image: base.clone(),
                    transform_hash: new_transform_hash,
                    scale,
                    unscaled_crop_offset: offset,
                });
                (base, scale, offset)
            }
        } else {
            let (base, scale, offset) = generate_transformed_preview(&loaded_image, &adjustments_clone, &app_handle)?;
            *cached_preview_lock = Some(CachedPreview {
                image: base.clone(),
                transform_hash: new_transform_hash,
                scale,
                unscaled_crop_offset: offset,
            });
            (base, scale, offset)
        };
    
    drop(cached_preview_lock);
    
    thread::spawn(move || {
        let (preview_width, preview_height) = final_preview_base.dimensions();

        let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_else(Vec::new);

        let scaled_crop_offset = (unscaled_crop_offset.0 * scale_for_gpu, unscaled_crop_offset.1 * scale_for_gpu);

        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions.iter()
            .filter_map(|def| generate_mask_bitmap(def, preview_width, preview_height, scale_for_gpu, scaled_crop_offset))
            .collect();

        let final_adjustments = get_all_adjustments_from_json(&adjustments_clone);

        if let Ok(final_processed_image) = process_and_get_dynamic_image(&context, &final_preview_base, final_adjustments, &mask_bitmaps) {
            if let Ok(histogram_data) = image_processing::calculate_histogram_from_image(&final_processed_image) {
                let _ = app_handle.emit("histogram-update", histogram_data);
            }

            if let Ok(waveform_data) = image_processing::calculate_waveform_from_image(&final_processed_image) {
                let _ = app_handle.emit("waveform-update", waveform_data);
            }

            if let Ok(base64_str) = encode_to_base64(&final_processed_image, 88) {
                let _ = app_handle.emit("preview-update-final", base64_str);
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn generate_uncropped_preview(
    js_adjustments: serde_json::Value,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let context = get_or_init_gpu_context(&state)?;
    let adjustments_clone = js_adjustments.clone();
    let loaded_image = state.original_image.lock().unwrap().clone().ok_or("No original image loaded")?;

    thread::spawn(move || {
        let patched_image = match composite_patches_on_image(&loaded_image.image, &adjustments_clone) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("Failed to composite patches for uncropped preview: {}", e);
                loaded_image.image
            },
        };
        
        let (full_w, full_h) = (loaded_image.full_width, loaded_image.full_height);

        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        let preview_dim = settings.editor_preview_resolution.unwrap_or(1920);

        let (processing_base, scale_for_gpu) = 
            if full_w > preview_dim || full_h > preview_dim {
                let base = patched_image.thumbnail(preview_dim, preview_dim);
                let scale = if full_w > 0 { base.width() as f32 / full_w as f32 } else { 1.0 };
                (base, scale)
            } else {
                (patched_image.clone(), 1.0)
            };
        
        let (preview_width, preview_height) = processing_base.dimensions();

        let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_else(Vec::new);

        let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions.iter()
            .filter_map(|def| generate_mask_bitmap(def, preview_width, preview_height, scale_for_gpu, (0.0, 0.0)))
            .collect();

        let uncropped_adjustments = get_all_adjustments_from_json(&adjustments_clone);

        if let Ok(processed_image) = process_and_get_dynamic_image(&context, &processing_base, uncropped_adjustments, &mask_bitmaps) {
            if let Ok(base64_str) = encode_to_base64(&processed_image, 85) {
                let _ = app_handle.emit("preview-update-uncropped", base64_str);
            }
        }
    });

    Ok(())
}

fn get_full_image_for_processing(state: &tauri::State<AppState>) -> Result<DynamicImage, String> {
    let original_image_lock = state.original_image.lock().unwrap();
    let loaded_image = original_image_lock.as_ref().ok_or("No original image loaded")?;
    Ok(loaded_image.image.clone())
}

#[tauri::command]
fn generate_fullscreen_preview(
    js_adjustments: serde_json::Value,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let context = get_or_init_gpu_context(&state)?;
    let original_image = get_full_image_for_processing(&state)?;
    let base_image = composite_patches_on_image(&original_image, &js_adjustments)
        .map_err(|e| format!("Failed to composite AI patches for fullscreen: {}", e))?;
    
    let (transformed_image, unscaled_crop_offset) = 
        apply_all_transformations(&base_image, &js_adjustments, 1.0);
    let (img_w, img_h) = transformed_image.dimensions();
    
    let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_else(Vec::new);

    let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions.iter()
        .filter_map(|def| generate_mask_bitmap(def, img_w, img_h, 1.0, unscaled_crop_offset))
        .collect();

    let all_adjustments = get_all_adjustments_from_json(&js_adjustments);
    let final_image = process_and_get_dynamic_image(&context, &transformed_image, all_adjustments, &mask_bitmaps)?;
    
    encode_to_base64(&final_image, 95)
}

#[tauri::command]
async fn export_image(
    path: String,
    js_adjustments: Value,
    export_settings: ExportSettings,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    if state.export_task_handle.lock().unwrap().is_some() {
        return Err("An export is already in progress.".to_string());
    }

    let context = get_or_init_gpu_context(&state)?;
    let original_image = get_full_image_for_processing(&state)?;
    let context = Arc::new(context);

    let task = tokio::spawn(async move {
        let processing_result: Result<(), String> = (|| {
            let base_image = composite_patches_on_image(&original_image, &js_adjustments)
                .map_err(|e| format!("Failed to composite AI patches for export: {}", e))?;

            let (transformed_image, unscaled_crop_offset) = 
                apply_all_transformations(&base_image, &js_adjustments, 1.0);
            let (img_w, img_h) = transformed_image.dimensions();

            let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
                .and_then(|m| serde_json::from_value(m.clone()).ok())
                .unwrap_or_else(Vec::new);

            let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions.iter()
                .filter_map(|def| generate_mask_bitmap(def, img_w, img_h, 1.0, unscaled_crop_offset))
                .collect();

            let all_adjustments = get_all_adjustments_from_json(&js_adjustments);
            let mut final_image = process_and_get_dynamic_image(&context, &transformed_image, all_adjustments, &mask_bitmaps)?;

            if let Some(resize_opts) = export_settings.resize {
                let (current_w, current_h) = final_image.dimensions();
                let should_resize = if resize_opts.dont_enlarge {
                    match resize_opts.mode {
                        ResizeMode::LongEdge => current_w.max(current_h) > resize_opts.value,
                        ResizeMode::Width => current_w > resize_opts.value,
                        ResizeMode::Height => current_h > resize_opts.value,
                    }
                } else { true };

                if should_resize {
                    final_image = match resize_opts.mode {
                        ResizeMode::LongEdge => {
                            let (w, h) = if current_w > current_h {
                                (resize_opts.value, (resize_opts.value as f32 * (current_h as f32 / current_w as f32)).round() as u32)
                            } else {
                                ((resize_opts.value as f32 * (current_w as f32 / current_h as f32)).round() as u32, resize_opts.value)
                            };
                            final_image.thumbnail(w, h)
                        },
                        ResizeMode::Width => final_image.thumbnail(resize_opts.value, u32::MAX),
                        ResizeMode::Height => final_image.thumbnail(u32::MAX, resize_opts.value),
                    };
                }
            }

            let output_path = std::path::Path::new(&path);
            let extension = output_path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();

            let mut file = BufWriter::new(fs::File::create(&path).map_err(|e| e.to_string())?);
            match extension.as_str() {
                "jpg" | "jpeg" => {
                    let rgb_image = final_image.to_rgb8();
                    let encoder = JpegEncoder::new_with_quality(&mut file, export_settings.jpeg_quality);
                    rgb_image.write_with_encoder(encoder).map_err(|e| e.to_string())?;
                }
                "png" => {
                    final_image.write_to(&mut file, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                }
                "tiff" => {
                    final_image.write_to(&mut file, image::ImageFormat::Tiff).map_err(|e| e.to_string())?;
                }
                _ => return Err(format!("Unsupported file extension: {}", extension)),
            };
            Ok(())
        })();

        if let Err(e) = processing_result {
            let _ = app_handle.emit("export-error", e);
        } else {
            let _ = app_handle.emit("export-complete", ());
        }

        *app_handle.state::<AppState>().export_task_handle.lock().unwrap() = None;
    });

    *state.export_task_handle.lock().unwrap() = Some(task);
    Ok(())
}

#[tauri::command]
async fn batch_export_images(
    output_folder: String,
    paths: Vec<String>,
    export_settings: ExportSettings,
    output_format: String,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    if state.export_task_handle.lock().unwrap().is_some() {
        return Err("An export is already in progress.".to_string());
    }

    let context = get_or_init_gpu_context(&state)?;
    let context = Arc::new(context);

    let task = tokio::spawn(async move {
        let output_folder_path = Path::new(&output_folder);
        let total_paths = paths.len();

        for (i, image_path_str) in paths.iter().enumerate() {
            if app_handle.state::<AppState>().export_task_handle.lock().unwrap().is_none() {
                println!("Export cancelled during batch processing.");
                let _ = app_handle.emit("export-cancelled", ());
                return;
            }

            let _ = app_handle.emit("batch-export-progress", serde_json::json!({ "current": i, "total": total_paths, "path": image_path_str }));

            let processing_result: Result<(), String> = (|| {
                let sidecar_path = get_sidecar_path(image_path_str);
                let metadata: ImageMetadata = if sidecar_path.exists() {
                    let file_content = fs::read_to_string(sidecar_path).map_err(|e| e.to_string())?;
                    serde_json::from_str(&file_content).unwrap_or_default()
                } else {
                    ImageMetadata::default()
                };
                let js_adjustments = metadata.adjustments;

                let base_image = load_and_composite(image_path_str, &js_adjustments, false)
                    .map_err(|e| e.to_string())?;
                
                let (transformed_image, unscaled_crop_offset) = 
                    apply_all_transformations(&base_image, &js_adjustments, 1.0);
                let (img_w, img_h) = transformed_image.dimensions();

                let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
                    .and_then(|m| serde_json::from_value(m.clone()).ok())
                    .unwrap_or_else(Vec::new);

                let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions.iter()
                    .filter_map(|def| generate_mask_bitmap(def, img_w, img_h, 1.0, unscaled_crop_offset))
                    .collect();

                let all_adjustments = get_all_adjustments_from_json(&js_adjustments);
                let mut final_image = process_and_get_dynamic_image(&context, &transformed_image, all_adjustments, &mask_bitmaps)?;

                if let Some(resize_opts) = &export_settings.resize {
                    let (current_w, current_h) = final_image.dimensions();
                    let should_resize = if resize_opts.dont_enlarge {
                        match resize_opts.mode {
                            ResizeMode::LongEdge => current_w.max(current_h) > resize_opts.value,
                            ResizeMode::Width => current_w > resize_opts.value,
                            ResizeMode::Height => current_h > resize_opts.value,
                        }
                    } else { true };

                    if should_resize {
                        final_image = match resize_opts.mode {
                            ResizeMode::LongEdge => {
                                let (w, h) = if current_w > current_h {
                                    (resize_opts.value, (resize_opts.value as f32 * (current_h as f32 / current_w as f32)).round() as u32)
                                } else {
                                    ((resize_opts.value as f32 * (current_w as f32 / current_h as f32)).round() as u32, resize_opts.value)
                                };
                                final_image.thumbnail(w, h)
                            },
                            ResizeMode::Width => final_image.thumbnail(resize_opts.value, u32::MAX),
                            ResizeMode::Height => final_image.thumbnail(u32::MAX, resize_opts.value),
                        };
                    }
                }

                let original_path = Path::new(image_path_str);
                let original_stem = original_path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
                let new_filename = format!("{}_edited.{}", original_stem, output_format);
                let output_path = output_folder_path.join(new_filename);

                let mut file = BufWriter::new(fs::File::create(&output_path).map_err(|e| e.to_string())?);
                match output_format.as_str() {
                    "jpg" | "jpeg" => {
                        let rgb_image = final_image.to_rgb8();
                        let encoder = JpegEncoder::new_with_quality(&mut file, export_settings.jpeg_quality);
                        rgb_image.write_with_encoder(encoder).map_err(|e| e.to_string())?;
                    }
                    "png" => {
                        final_image.write_to(&mut file, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                    }
                    "tiff" => {
                        final_image.write_to(&mut file, image::ImageFormat::Tiff).map_err(|e| e.to_string())?;
                    }
                    _ => return Err(format!("Unsupported file format: {}", output_format)),
                };
                Ok(())
            })();

            if let Err(e) = processing_result {
                eprintln!("Failed to export {}: {}", image_path_str, e);
                let _ = app_handle.emit("export-error", e);
                *app_handle.state::<AppState>().export_task_handle.lock().unwrap() = None;
                return;
            }
        }

        let _ = app_handle.emit("batch-export-progress", serde_json::json!({ "current": total_paths, "total": total_paths, "path": "" }));
        let _ = app_handle.emit("export-complete", ());
        *app_handle.state::<AppState>().export_task_handle.lock().unwrap() = None;
    });

    *state.export_task_handle.lock().unwrap() = Some(task);
    Ok(())
}

#[tauri::command]
fn cancel_export(state: tauri::State<AppState>) -> Result<(), String> {
    if let Some(handle) = state.export_task_handle.lock().unwrap().take() {
        handle.abort();
        println!("Export task cancellation requested.");
    } else {
        return Err("No export task is currently running.".to_string());
    }
    Ok(())
}

#[tauri::command]
fn generate_mask_overlay(
    mask_def: MaskDefinition,
    width: u32,
    height: u32,
    scale: f32,
    crop_offset: (f32, f32),
) -> Result<String, String> {

    let scaled_crop_offset = (crop_offset.0 * scale, crop_offset.1 * scale);

    if let Some(gray_mask) = generate_mask_bitmap(&mask_def, width, height, scale, scaled_crop_offset) {
        let mut rgba_mask = RgbaImage::new(width, height);
        for (x, y, pixel) in gray_mask.enumerate_pixels() {
            let intensity = pixel[0];
            let alpha = (intensity as f32 * 0.5) as u8;
            rgba_mask.put_pixel(x, y, Rgba([255, 0, 0, alpha]));
        }

        let mut buf = Cursor::new(Vec::new());
        rgba_mask.write_to(&mut buf, ImageFormat::Png).map_err(|e| e.to_string())?;
        
        let base64_str = general_purpose::STANDARD.encode(buf.get_ref());
        Ok(format!("data:image/png;base64,{}", base64_str))
    } else {
        Ok("".to_string())
    }
}

#[tauri::command]
async fn generate_ai_foreground_mask(
    rotation: f32,
    flip_horizontal: bool,
    flip_vertical: bool,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<AiForegroundMaskParameters, String> {
    let models = state.ai_state.lock().unwrap().as_ref().map(|s| s.models.clone());

    let models = if let Some(models) = models {
        models
    } else {
        drop(models);
        let new_models = get_or_init_ai_models(&app_handle).await.map_err(|e| e.to_string())?;
        let mut ai_state_lock = state.ai_state.lock().unwrap();
        if let Some(ai_state) = &mut *ai_state_lock {
            ai_state.models.clone()
        } else {
            *ai_state_lock = Some(AiState {
                models: new_models.clone(),
                embeddings: None,
            });
            new_models
        }
    };

    let full_image = get_full_image_for_processing(&state)?;
    let full_mask_image = run_u2netp_model(&full_image, &models.u2netp).map_err(|e| e.to_string())?;
    let base64_data = encode_to_base64_png(&full_mask_image)?;

    Ok(AiForegroundMaskParameters {
        mask_data_base64: Some(base64_data),
        rotation: Some(rotation),
        flip_horizontal: Some(flip_horizontal),
        flip_vertical: Some(flip_vertical),
    })
}

#[tauri::command]
async fn generate_ai_subject_mask(
    path: String,
    start_point: (f64, f64),
    end_point: (f64, f64),
    rotation: f32,
    flip_horizontal: bool,
    flip_vertical: bool,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<AiSubjectMaskParameters, String> {
    let models = state.ai_state.lock().unwrap().as_ref().map(|s| s.models.clone());

    let models = if let Some(models) = models {
        models
    } else {
        drop(models);
        let new_models = get_or_init_ai_models(&app_handle).await.map_err(|e| e.to_string())?;
        let mut ai_state_lock = state.ai_state.lock().unwrap();
        if let Some(ai_state) = &mut *ai_state_lock {
            ai_state.models.clone()
        } else {
            *ai_state_lock = Some(AiState {
                models: new_models.clone(),
                embeddings: None,
            });
            new_models
        }
    };

    let embeddings = {
        let mut ai_state_lock = state.ai_state.lock().unwrap();
        let ai_state = ai_state_lock.as_mut().unwrap();

        let mut hasher = blake3::Hasher::new();
        hasher.update(path.as_bytes());
        let path_hash = hasher.finalize().to_hex().to_string();

        if let Some(cached_embeddings) = &ai_state.embeddings {
            if cached_embeddings.path_hash == path_hash {
                cached_embeddings.clone()
            } else {
                let full_image = get_full_image_for_processing(&state)?;
                let mut new_embeddings = generate_image_embeddings(&full_image, &models.sam_encoder).map_err(|e| e.to_string())?;
                new_embeddings.path_hash = path_hash;
                ai_state.embeddings = Some(new_embeddings.clone());
                new_embeddings
            }
        } else {
            let full_image = get_full_image_for_processing(&state)?;
            let mut new_embeddings = generate_image_embeddings(&full_image, &models.sam_encoder).map_err(|e| e.to_string())?;
            new_embeddings.path_hash = path_hash;
            ai_state.embeddings = Some(new_embeddings.clone());
            new_embeddings
        }
    };

    let (img_w, img_h) = embeddings.original_size;
    let center = (img_w as f64 / 2.0, img_h as f64 / 2.0);

    let p1 = start_point;
    let p2 = (start_point.0, end_point.1);
    let p3 = end_point;
    let p4 = (end_point.0, start_point.1);

    let angle_rad = (rotation as f64).to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let unrotate = |p: (f64, f64)| {
        let px = p.0 - center.0;
        let py = p.1 - center.1;
        let new_px = px * cos_a + py * sin_a + center.0;
        let new_py = -px * sin_a + py * cos_a + center.1;
        (new_px, new_py)
    };

    let up1 = unrotate(p1);
    let up2 = unrotate(p2);
    let up3 = unrotate(p3);
    let up4 = unrotate(p4);

    let unflip = |p: (f64, f64)| {
        let mut new_px = p.0;
        let mut new_py = p.1;
        if flip_horizontal {
            new_px = img_w as f64 - p.0;
        }
        if flip_vertical {
            new_py = img_h as f64 - p.1;
        }
        (new_px, new_py)
    };

    let ufp1 = unflip(up1);
    let ufp2 = unflip(up2);
    let ufp3 = unflip(up3);
    let ufp4 = unflip(up4);

    let min_x = ufp1.0.min(ufp2.0).min(ufp3.0).min(ufp4.0);
    let min_y = ufp1.1.min(ufp2.1).min(ufp3.1).min(ufp4.1);
    let max_x = ufp1.0.max(ufp2.0).max(ufp3.0).max(ufp4.0);
    let max_y = ufp1.1.max(ufp2.1).max(ufp3.1).max(ufp4.1);

    let unrotated_start_point = (min_x, min_y);
    let unrotated_end_point = (max_x, max_y);

    let mask_bitmap = run_sam_decoder(&models.sam_decoder, &embeddings, unrotated_start_point, unrotated_end_point).map_err(|e| e.to_string())?;
    let base64_data = encode_to_base64_png(&mask_bitmap)?;

    Ok(AiSubjectMaskParameters {
        start_x: start_point.0,
        start_y: start_point.1,
        end_x: end_point.0,
        end_y: end_point.1,
        mask_data_base64: Some(base64_data),
        rotation: Some(rotation),
        flip_horizontal: Some(flip_horizontal),
        flip_vertical: Some(flip_vertical),
    })
}

#[tauri::command]
fn save_metadata_and_update_thumbnail(
    path: String,
    adjustments: Value,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let sidecar_path = get_sidecar_path(&path);

    let metadata = ImageMetadata {
        version: 1,
        rating: adjustments["rating"].as_u64().unwrap_or(0) as u8,
        adjustments,
    };

    let json_string = serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?;
    std::fs::write(sidecar_path, json_string).map_err(|e| e.to_string())?;

    thread::spawn(move || {
        let _ = app_handle.emit("thumbnail-progress", serde_json::json!({ "completed": 0, "total": 1 }));
        let _ = file_management::generate_thumbnails_progressive(vec![path], app_handle);
    });

    Ok(())
}

#[tauri::command]
fn apply_adjustments_to_paths(
    paths: Vec<String>,
    adjustments: Value,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    paths.par_iter().for_each(|path| {
        let sidecar_path = get_sidecar_path(path);

        let existing_metadata: ImageMetadata = if sidecar_path.exists() {
            fs::read_to_string(&sidecar_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_default()
        } else {
            ImageMetadata::default()
        };

        let mut new_adjustments = existing_metadata.adjustments;
        if new_adjustments.is_null() {
            new_adjustments = serde_json::json!({});
        }
        
        if let (Some(new_map), Some(pasted_map)) = (new_adjustments.as_object_mut(), adjustments.as_object()) {
            for (k, v) in pasted_map {
                new_map.insert(k.clone(), v.clone());
            }
        }

        let metadata = ImageMetadata {
            version: 1,
            rating: new_adjustments["rating"].as_u64().unwrap_or(0) as u8,
            adjustments: new_adjustments,
        };

        if let Ok(json_string) = serde_json::to_string_pretty(&metadata) {
            let _ = std::fs::write(sidecar_path, json_string);
        }
    });

    thread::spawn(move || {
        let _ = file_management::generate_thumbnails_progressive(paths, app_handle);
    });

    Ok(())
}

#[tauri::command]
fn reset_adjustments_for_paths(paths: Vec<String>, app_handle: tauri::AppHandle) -> Result<(), String> {
    paths.par_iter().for_each(|path| {
        let sidecar_path = get_sidecar_path(path);

        let existing_metadata: ImageMetadata = if sidecar_path.exists() {
            fs::read_to_string(&sidecar_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_default()
        } else {
            ImageMetadata::default()
        };

        let new_adjustments = serde_json::json!({
            "rating": existing_metadata.rating
        });
        
        let metadata = ImageMetadata {
            version: 1,
            rating: existing_metadata.rating,
            adjustments: new_adjustments,
        };

        if let Ok(json_string) = serde_json::to_string_pretty(&metadata) {
            let _ = std::fs::write(sidecar_path, json_string);
        }
    });

    thread::spawn(move || {
        let _ = file_management::generate_thumbnails_progressive(paths, app_handle);
    });

    Ok(())
}

#[tauri::command]
fn load_metadata(path: String) -> Result<ImageMetadata, String> {
    let sidecar_path = get_sidecar_path(&path);
    if sidecar_path.exists() {
        let file_content = std::fs::read_to_string(sidecar_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&file_content).map_err(|e| e.to_string())
    } else {
        Ok(ImageMetadata::default())
    }
}

fn get_presets_path(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let presets_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("presets");

    if !presets_dir.exists() {
        fs::create_dir_all(&presets_dir).map_err(|e| e.to_string())?;
    }

    Ok(presets_dir.join("presets.json"))
}

#[tauri::command]
fn load_presets(app_handle: tauri::AppHandle) -> Result<Vec<Preset>, String> {
    let path = get_presets_path(&app_handle)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_presets(presets: Vec<Preset>, app_handle: tauri::AppHandle) -> Result<(), String> {
    let path = get_presets_path(&app_handle)?;
    let json_string = serde_json::to_string_pretty(&presets).map_err(|e| e.to_string())?;
    fs::write(path, json_string).map_err(|e| e.to_string())
}

fn get_settings_path(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let settings_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    if !settings_dir.exists() {
        fs::create_dir_all(&settings_dir).map_err(|e| e.to_string())?;
    }

    Ok(settings_dir.join("settings.json"))
}

#[tauri::command]
fn load_settings(app_handle: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = get_settings_path(&app_handle)?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_settings(settings: AppSettings, app_handle: tauri::AppHandle) -> Result<(), String> {
    let path = get_settings_path(&app_handle)?;
    let json_string = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(path, json_string).map_err(|e| e.to_string())
}

#[tauri::command]
fn generate_preset_preview(
    js_adjustments: serde_json::Value,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let context = get_or_init_gpu_context(&state)?;

    let loaded_image = state.original_image.lock().unwrap().clone()
        .ok_or("No original image loaded for preset preview")?;
    let original_image = loaded_image.image;
    
    const PRESET_PREVIEW_DIM: u32 = 200;
    let preview_base = original_image.thumbnail(PRESET_PREVIEW_DIM, PRESET_PREVIEW_DIM);

    let (transformed_image, unscaled_crop_offset) = 
        apply_all_transformations(&preview_base, &js_adjustments, 1.0);
    let (img_w, img_h) = transformed_image.dimensions();

    let mask_definitions: Vec<MaskDefinition> = js_adjustments.get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_else(Vec::new);

    let mask_bitmaps: Vec<ImageBuffer<Luma<u8>, Vec<u8>>> = mask_definitions.iter()
        .filter_map(|def| generate_mask_bitmap(def, img_w, img_h, 1.0, unscaled_crop_offset))
        .collect();

    let all_adjustments = get_all_adjustments_from_json(&js_adjustments);
    
    let processed_image = process_and_get_dynamic_image(&context, &transformed_image, all_adjustments, &mask_bitmaps)?;
    
    encode_to_base64(&processed_image, 50)
}

#[tauri::command]
fn handle_import_presets_from_file(file_path: String, app_handle: tauri::AppHandle) -> Result<Vec<Preset>, String> {
    let content = fs::read_to_string(file_path).map_err(|e| format!("Failed to read preset file: {}", e))?;
    let imported_preset_file: PresetFile = serde_json::from_str(&content).map_err(|e| format!("Failed to parse preset file: {}", e))?;

    let mut current_presets = load_presets(app_handle.clone())?;
    let mut current_preset_names: HashMap<String, usize> = current_presets.iter().map(|p| (p.name.clone(), 1)).collect();

    for mut imported_preset in imported_preset_file.presets {
        imported_preset.id = Uuid::new_v4().to_string();

        let mut new_name = imported_preset.name.clone();
        let mut counter = 1;
        while current_preset_names.contains_key(&new_name) {
            new_name = format!("{} ({})", imported_preset.name, counter);
            counter += 1;
        }
        imported_preset.name = new_name;
        current_preset_names.insert(imported_preset.name.clone(), 1);
        current_presets.push(imported_preset);
    }

    save_presets(current_presets.clone(), app_handle)?;
    Ok(current_presets)
}

#[tauri::command]
fn handle_export_presets_to_file(presets_to_export: Vec<Preset>, file_path: String) -> Result<(), String> {
    let preset_file = PresetFile { presets: presets_to_export };
    let json_string = serde_json::to_string_pretty(&preset_file).map_err(|e| format!("Failed to serialize presets: {}", e))?;
    fs::write(file_path, json_string).map_err(|e| format!("Failed to write preset file: {}", e))
}

#[tauri::command]
fn clear_all_sidecars(root_path: String) -> Result<usize, String> {
    if !Path::new(&root_path).exists() {
        return Err(format!("Root path does not exist: {}", root_path));
    }

    let mut deleted_count = 0;
    let walker = WalkDir::new(root_path).into_iter();

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "rrdata" {
                    if fs::remove_file(path).is_ok() {
                        deleted_count += 1;
                    } else {
                        eprintln!("Failed to delete sidecar file: {:?}", path);
                    }
                }
            }
        }
    }

    Ok(deleted_count)
}

#[tauri::command]
fn clear_thumbnail_cache(app_handle: tauri::AppHandle) -> Result<(), String> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?;
    let thumb_cache_dir = cache_dir.join("thumbnails");

    if thumb_cache_dir.exists() {
        fs::remove_dir_all(&thumb_cache_dir).map_err(|e| format!("Failed to remove thumbnail cache: {}", e))?;
    }
    
    fs::create_dir_all(&thumb_cache_dir).map_err(|e| format!("Failed to recreate thumbnail cache directory: {}", e))?;

    Ok(())
}

#[tauri::command]
fn show_in_finder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        if let Some(parent) = Path::new(&path).parent() {
            Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            return Err("Could not get parent directory".into());
        }
    }

    Ok(())
}

#[tauri::command]
fn delete_files_from_disk(paths: Vec<String>) -> Result<(), String> {
    trash::delete_all(&paths).map_err(|e| e.to_string())?;

    for path in paths {
        let sidecar_path = get_sidecar_path(&path);
        if sidecar_path.exists() {
            let _ = trash::delete(&sidecar_path);
        }
    }

    Ok(())
}

#[tauri::command]
fn update_window_effect(theme: String, window: tauri::Window) {
    #[cfg(target_os = "windows")]
    {
        let color = if theme == "light" {
            Some((250, 250, 250, 150))
        } else if theme == "muted-green" {
            Some((44, 56, 54, 100))
        } else {
            Some((26, 29, 27, 60))
        };
        window_vibrancy::apply_acrylic(&window, color)
            .expect("Unsupported platform! 'apply_acrylic' is only supported on Windows");
    }

    #[cfg(target_os = "macos")]
    {
        let material = if theme == "light" {
            window_vibrancy::NSVisualEffectMaterial::ContentBackground
        } else {
            window_vibrancy::NSVisualEffectMaterial::HudWindow
        };
        window_vibrancy::apply_vibrancy(&window, material, None, None)
            .expect("Unsupported platform! 'apply_vibrancy' is only supported on macOS");
    }
}

#[tauri::command]
async fn check_comfyui_status(app_handle: tauri::AppHandle) {
    let settings = load_settings(app_handle.clone()).unwrap_or_default();
    let is_connected = if let Some(address) = settings.comfyui_address {
        comfyui_connector::ping_server(&address).await.is_ok()
    } else {
        false
    };
    let _ = app_handle.emit("comfyui-status-update", serde_json::json!({ "connected": is_connected }));
}

#[tauri::command]
async fn test_comfyui_connection(address: String) -> Result<(), String> {
    comfyui_connector::ping_server(&address)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn invoke_generative_replace(
    _path: String,
    mask_data_base64: String,
    prompt: String,
    current_adjustments: Value,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let settings = load_settings(app_handle).unwrap_or_default();
    let address = settings.comfyui_address
        .ok_or_else(|| "ComfyUI address is not configured in settings.".to_string())?;

    let base_image = get_full_image_for_processing(&state)?;
    let source_image = composite_patches_on_image(&base_image, &current_adjustments)
        .map_err(|e| format!("Failed to prepare source image: {}", e))?;

    let b64_data = if let Some(idx) = mask_data_base64.find(',') {
        &mask_data_base64[idx + 1..]
    } else {
        &mask_data_base64
    };
    let mask_bytes = general_purpose::STANDARD.decode(b64_data)
        .map_err(|e| format!("Failed to decode mask: {}", e))?;
    let mask_image = image::load_from_memory(&mask_bytes)
        .map_err(|e| format!("Failed to load mask image: {}", e))?;

    let workflow_inputs = comfyui_connector::WorkflowInputs {
        source_image_node_id: "11".to_string(),
        mask_image_node_id: Some("148".to_string()),
        text_prompt_node_id: Some("6".to_string()),
        final_output_node_id: "252".to_string(),
    };

    let result_png_bytes = comfyui_connector::execute_workflow(
        &address,
        "generative_replace",
        workflow_inputs,
        source_image,
        Some(mask_image),
        Some(prompt)
    ).await.map_err(|e| e.to_string())?;

    Ok(general_purpose::STANDARD.encode(&result_png_bytes))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            
            let gpu_context_result = pollster::block_on(GpuContext::new());

            match gpu_context_result {
                Ok(gpu_context) => {
                    // On success, create and manage your AppState
                    app.manage(AppState {
                        original_image: Mutex::new(None),
                        cached_preview: Mutex::new(None),
                        gpu_context: Mutex::new(Some(gpu_context)),
                        ai_state: Mutex::new(None),
                        export_task_handle: Mutex::new(None),
                    });
                },
                Err(e) => {
                    // On failure, show a friendly error dialog and exit
                    use tauri_plugin_dialog::DialogExt;
                    let handle = app.handle().clone();
                    handle.dialog().message(format!("Fatal Graphics Error\n\n{}\n\nThe application will now close.", e))
                        .title("Error")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                        .blocking_show();
                    
                    std::process::exit(1);
                }
            }

            let app_handle = app.handle().clone();

            let resource_path = app_handle.path()
                .resolve("resources", tauri::path::BaseDirectory::Resource)
                .expect("failed to resolve resource directory");
            
            let ort_library_name = {
                #[cfg(target_os = "windows")] { "onnxruntime.dll" }
                #[cfg(target_os = "linux")] { "libonnxruntime.so" }
                #[cfg(target_os = "macos")] { "libonnxruntime.dylib" }
            };

            let ort_library_path = resource_path.join(ort_library_name);
            std::env::set_var("ORT_DYLIB_PATH", &ort_library_path);
            println!("Set ORT_DYLIB_PATH to: {}", ort_library_path.display());

            let settings: AppSettings = load_settings(app_handle.clone()).unwrap_or_default();
            let window_cfg = app.config().app.windows.get(0).unwrap().clone();
            let transparent = settings.transparent.unwrap_or(window_cfg.transparent);

            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_cfg)
                .unwrap()
                .transparent(transparent)
                .build()
                .expect("Failed to build window");

            if transparent {
                let theme = settings.theme.unwrap_or_else(|| "dark".to_string());

                #[cfg(target_os = "macos")]
                {
                    let material = if theme == "light" {
                        NSVisualEffectMaterial::ContentBackground
                    } else {
                        NSVisualEffectMaterial::HudWindow
                    };
                    apply_vibrancy(&window, material, None, None).expect(
                        "Unsupported platform! 'apply_vibrancy' is only supported on macOS",
                    );
                }

                #[cfg(target_os = "windows")]
                {
                    let color = if theme == "light" {
                        Some((250, 250, 250, 150))
                    } else if theme == "muted-green" {
                        Some((44, 56, 54, 100))
                    } else {
                        Some((26, 29, 27, 60))
                    };
                    apply_acrylic(&window, color).expect(
                        "Unsupported platform! 'apply_acrylic' is only supported on Windows",
                    );
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            show_in_finder,
            delete_files_from_disk,
            load_image,
            apply_adjustments,
            export_image,
            batch_export_images,
            cancel_export,
            generate_fullscreen_preview,
            save_metadata_and_update_thumbnail,
            apply_adjustments_to_paths,
            load_metadata,
            image_processing::generate_histogram,
            image_processing::generate_waveform,
            image_processing::load_file_data,
            image_processing::read_file_data,
            lut_cube::apply_lut_type_gpu,
            file_management::list_images_in_dir,
            file_management::get_folder_tree,
            file_management::get_file_tree,
            file_management::generate_thumbnails,
            file_management::generate_thumbnails_progressive,
            file_management::create_folder,
            file_management::delete_folder,
            file_management::copy_files,
            file_management::move_files,
            file_management::rename_folder,
            file_management::duplicate_file,
            load_presets,
            save_presets,
            generate_preset_preview,
            generate_uncropped_preview,
            generate_mask_overlay,
            generate_ai_subject_mask,
            generate_ai_foreground_mask,
            load_settings,
            save_settings,
            reset_adjustments_for_paths,
            handle_import_presets_from_file,
            handle_export_presets_to_file,
            clear_all_sidecars,
            clear_thumbnail_cache,
            update_window_effect,
            check_comfyui_status,
            test_comfyui_connection,
            invoke_generative_replace
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}