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

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::thread;
use std::fs;
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use image::{DynamicImage, GenericImageView, ImageBuffer, Luma, Rgba, RgbaImage, ImageFormat, GrayImage};
use image::codecs::jpeg::JpegEncoder;
use tauri::{Manager, Emitter};
use base64::{Engine as _, engine::general_purpose};
use serde_json::Value;
use tokio::task::JoinHandle;
use window_vibrancy::{apply_acrylic, apply_vibrancy, NSVisualEffectMaterial};
use serde::{Serialize, Deserialize};
use chrono::Local;
use little_exif::metadata::Metadata;
use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::rational::uR64;

use crate::image_processing::{
    get_all_adjustments_from_json, get_or_init_gpu_context, GpuContext,
    ImageMetadata, process_and_get_dynamic_image, Crop, apply_crop, apply_rotation, apply_flip,
};
use crate::file_management::{get_sidecar_path, load_settings, AppSettings};
use crate::mask_generation::{MaskDefinition, generate_mask_bitmap};
use crate::ai_processing::{
    AiState, get_or_init_ai_models, generate_image_embeddings, run_sam_decoder,
    AiSubjectMaskParameters, run_u2netp_model, AiForegroundMaskParameters
};
use crate::formats::{is_raw_file};
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
    keep_metadata: bool,
    strip_gps: bool,
    filename_template: Option<String>,
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
    original_path: String,
    output_path: String,
    js_adjustments: Value,
    export_settings: ExportSettings,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    if state.export_task_handle.lock().unwrap().is_some() {
        return Err("An export is already in progress.".to_string());
    }

    let context = get_or_init_gpu_context(&state)?;
    let original_image_data = get_full_image_for_processing(&state)?;
    let context = Arc::new(context);

    let task = tokio::spawn(async move {
        let processing_result: Result<(), String> = (|| {
            let base_image = composite_patches_on_image(&original_image_data, &js_adjustments)
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

            let output_path_obj = std::path::Path::new(&output_path);
            let extension = output_path_obj.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
            
            let mut image_bytes = Vec::new();
            let mut cursor = Cursor::new(&mut image_bytes);

            match extension.as_str() {
                "jpg" | "jpeg" => {
                    let rgb_image = final_image.to_rgb8();
                    let encoder = JpegEncoder::new_with_quality(&mut cursor, export_settings.jpeg_quality);
                    rgb_image.write_with_encoder(encoder).map_err(|e| e.to_string())?;
                }
                "png" => {
                    final_image.write_to(&mut cursor, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                }
                "tiff" => {
                    final_image.write_to(&mut cursor, image::ImageFormat::Tiff).map_err(|e| e.to_string())?;
                }
                _ => return Err(format!("Unsupported file extension: {}", extension)),
            };

            write_image_with_metadata(
                &mut image_bytes,
                &original_path,
                &extension,
                export_settings.keep_metadata,
                export_settings.strip_gps,
            )?;

            fs::write(&output_path, image_bytes).map_err(|e| e.to_string())?;

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
        let output_folder_path = std::path::Path::new(&output_folder);
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

                let original_path = std::path::Path::new(image_path_str);
                let filename_template = export_settings.filename_template.as_deref().unwrap_or("{original_filename}_edited");
                let new_stem = generate_filename_from_template(filename_template, original_path, i + 1, total_paths);
                let new_filename = format!("{}.{}", new_stem, output_format);
                let output_path = output_folder_path.join(new_filename);

                let mut image_bytes = Vec::new();
                let mut cursor = Cursor::new(&mut image_bytes);

                match output_format.as_str() {
                    "jpg" | "jpeg" => {
                        let rgb_image = final_image.to_rgb8();
                        let encoder = JpegEncoder::new_with_quality(&mut cursor, export_settings.jpeg_quality);
                        rgb_image.write_with_encoder(encoder).map_err(|e| e.to_string())?;
                    }
                    "png" => {
                        final_image.write_to(&mut cursor, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                    }
                    "tiff" => {
                        final_image.write_to(&mut cursor, image::ImageFormat::Tiff).map_err(|e| e.to_string())?;
                    }
                    _ => return Err(format!("Unsupported file format: {}", output_format)),
                };

                write_image_with_metadata(
                    &mut image_bytes,
                    image_path_str,
                    &output_format,
                    export_settings.keep_metadata,
                    export_settings.strip_gps,
                )?;

                fs::write(&output_path, image_bytes).map_err(|e| e.to_string())?;

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

fn generate_filename_from_template(
    template: &str,
    original_path: &std::path::Path,
    sequence: usize,
    total: usize,
) -> String {
    let now = Local::now();
    let stem = original_path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let sequence_str = format!("{:0width$}", sequence, width = total.to_string().len().max(1));

    let mut result = template.to_string();
    result = result.replace("{original_filename}", stem);
    result = result.replace("{sequence}", &sequence_str);
    result = result.replace("{YYYY}", &now.format("%Y").to_string());
    result = result.replace("{MM}", &now.format("%m").to_string());
    result = result.replace("{DD}", &now.format("%d").to_string());
    result = result.replace("{hh}", &now.format("%H").to_string());
    result = result.replace("{mm}", &now.format("%M").to_string());

    result
}

fn write_image_with_metadata(
    image_bytes: &mut Vec<u8>,
    original_path_str: &str,
    output_format: &str,
    keep_metadata: bool,
    strip_gps: bool,
) -> Result<(), String> {
    if !keep_metadata || output_format.to_lowercase() == "tiff" { // FIXME: temporary solution until I find a way to write metadata to TIFF
        return Ok(());
    }

    let file_type = match output_format.to_lowercase().as_str() {
        "jpg" | "jpeg" => FileExtension::JPEG,
        "png" => FileExtension::PNG { as_zTXt_chunk: true },
        "tiff" => FileExtension::TIFF,
        _ => return Ok(()),
    };

    let original_path = std::path::Path::new(original_path_str);
    if !original_path.exists() {
        eprintln!("Original file not found, cannot copy metadata: {}", original_path_str);
        return Ok(());
    }

    if let Ok(mut metadata) = Metadata::new_from_path(original_path) {
        if strip_gps {
            let dummy_rational = uR64 { nominator: 0, denominator: 1 };
            let dummy_rational_vec1 = vec![dummy_rational.clone()];
            let dummy_rational_vec3 = vec![dummy_rational.clone(), dummy_rational.clone(), dummy_rational.clone()];

            metadata.remove_tag(ExifTag::GPSVersionID([0,0,0,0].to_vec()));
            metadata.remove_tag(ExifTag::GPSLatitudeRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSLatitude(dummy_rational_vec3.clone()));
            metadata.remove_tag(ExifTag::GPSLongitudeRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSLongitude(dummy_rational_vec3.clone()));
            metadata.remove_tag(ExifTag::GPSAltitudeRef(vec![0]));
            metadata.remove_tag(ExifTag::GPSAltitude(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSTimeStamp(dummy_rational_vec3.clone()));
            metadata.remove_tag(ExifTag::GPSSatellites("".to_string()));
            metadata.remove_tag(ExifTag::GPSStatus("".to_string()));
            metadata.remove_tag(ExifTag::GPSMeasureMode("".to_string()));
            metadata.remove_tag(ExifTag::GPSDOP(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSSpeedRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSSpeed(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSTrackRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSTrack(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSImgDirectionRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSImgDirection(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSMapDatum("".to_string()));
            metadata.remove_tag(ExifTag::GPSDestLatitudeRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSDestLatitude(dummy_rational_vec3.clone()));
            metadata.remove_tag(ExifTag::GPSDestLongitudeRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSDestLongitude(dummy_rational_vec3.clone()));
            metadata.remove_tag(ExifTag::GPSDestBearingRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSDestBearing(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSDestDistanceRef("".to_string()));
            metadata.remove_tag(ExifTag::GPSDestDistance(dummy_rational_vec1.clone()));
            metadata.remove_tag(ExifTag::GPSProcessingMethod(vec![]));
            metadata.remove_tag(ExifTag::GPSAreaInformation(vec![]));
            metadata.remove_tag(ExifTag::GPSDateStamp("".to_string()));
            metadata.remove_tag(ExifTag::GPSDifferential(vec![0u16]));
            metadata.remove_tag(ExifTag::GPSHPositioningError(dummy_rational_vec1.clone()));
        }

        metadata.set_tag(ExifTag::Orientation(vec![1u16]));

        if metadata.write_to_vec(image_bytes, file_type).is_err() {
            eprintln!("Failed to write metadata to image vector for {}", original_path_str);
        }
    } else {
        eprintln!("Failed to read metadata from original file: {}", original_path_str);
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

fn apply_window_effect(theme: String, window: impl raw_window_handle::HasWindowHandle) {
    #[cfg(target_os = "windows")]
    {
        let color = match theme.as_str() {
            "light" => Some((250, 250, 250, 150)),
            "muted-green" => Some((44, 56, 54, 100)),
            _ => Some((26, 29, 27, 60)),
        };

        let info = os_info::get();

        let is_win11_or_newer = match info.version() {
            os_info::Version::Semantic(major, _, build) => *major == 10 && *build >= 22000,
            _ => false,
        };

        if is_win11_or_newer {
            window_vibrancy::apply_acrylic(&window, color)
                .expect("Failed to apply acrylic effect on Windows 11");
        } else {
            window_vibrancy::apply_blur(&window, color)
                .expect("Failed to apply blur effect on Windows 10 or older");
        }
    }

    #[cfg(target_os = "macos")]
    {
        let material = match theme.as_str() {
            "light" => window_vibrancy::NSVisualEffectMaterial::ContentBackground,
            _ => window_vibrancy::NSVisualEffectMaterial::HudWindow,
        };
        window_vibrancy::apply_vibrancy(&window, material, None, None)
            .expect("Unsupported platform! 'apply_vibrancy' is only supported on macOS");
    }

    #[cfg(target_os = "linux")]
    {
    }
}

#[tauri::command]
fn update_window_effect(theme: String, window: tauri::Window) {
    apply_window_effect(theme, window);
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

#[tauri::command]
fn get_supported_file_types() -> Result<serde_json::Value, String> {
    let raw_extensions: Vec<&str> = crate::formats::RAW_EXTENSIONS.iter().map(|(ext, _)| *ext).collect();
    let non_raw_extensions: Vec<&str> = crate::formats::NON_RAW_EXTENSIONS.to_vec();
    
    Ok(serde_json::json!({
        "raw": raw_extensions,
        "nonRaw": non_raw_extensions
    }))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_os::init())
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
            let decorations = settings.decorations.unwrap_or(window_cfg.decorations);

            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &window_cfg)
                .unwrap()
                .transparent(transparent)
                .decorations(decorations)
                .build()
                .expect("Failed to build window");

            if transparent {
                let theme = settings.theme.unwrap_or("dark".to_string());
                apply_window_effect(theme, &window);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_image,
            apply_adjustments,
            export_image,
            batch_export_images,
            cancel_export,
            generate_fullscreen_preview,
            generate_preset_preview,
            generate_uncropped_preview,
            generate_mask_overlay,
            generate_ai_subject_mask,
            generate_ai_foreground_mask,
            update_window_effect,
            check_comfyui_status,
            test_comfyui_connection,
            invoke_generative_replace,
            get_supported_file_types,
            image_processing::generate_histogram,
            image_processing::generate_waveform,
            image_processing::load_file_data,
            image_processing::read_file_data,
            lut_cube::apply_lut_type_gpu,
            image_processing::calculate_auto_adjustments,
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
            file_management::show_in_finder,
            file_management::delete_files_from_disk,
            file_management::delete_files_with_associated,
            file_management::save_metadata_and_update_thumbnail,
            file_management::apply_adjustments_to_paths,
            file_management::load_metadata,
            file_management::load_presets,
            file_management::save_presets,
            file_management::load_settings,
            file_management::save_settings,
            file_management::reset_adjustments_for_paths,
            file_management::apply_auto_adjustments_to_paths,
            file_management::handle_import_presets_from_file,
            file_management::handle_export_presets_to_file,
            file_management::clear_all_sidecars,
            file_management::clear_thumbnail_cache
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}