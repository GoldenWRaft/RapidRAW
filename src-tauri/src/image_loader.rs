use anyhow::{Result, Context};
use base64::{engine::general_purpose, Engine as _};
use image::{imageops, DynamicImage, ImageReader, RgbaImage, Rgba};
use rawler::Orientation;
use std::io::Cursor;
use rayon::prelude::*;
use serde_json::Value;
use std::fs;

use exif::{Reader as ExifReader, Tag};
use crate::image_processing::apply_orientation;

use crate::formats::is_raw_file;
use crate::raw_processing::develop_raw_image;

pub fn load_and_composite(
    path: &str,
    adjustments: &Value,
    use_fast_raw_dev: bool,
) -> Result<DynamicImage> {
    let file_bytes = fs::read(path)?;
    let base_image = load_base_image_from_bytes(&file_bytes, path, use_fast_raw_dev)?;
    composite_patches_on_image(&base_image, adjustments)
}

pub fn load_base_image_from_bytes(
    bytes: &[u8],
    path_for_ext_check: &str,
    use_fast_raw_dev: bool,
) -> Result<DynamicImage> {
    if is_raw_file(path_for_ext_check) {
        develop_raw_image(bytes, use_fast_raw_dev)
    } else {
        load_image_with_orientation(bytes)
    }
}

pub fn load_image_with_orientation(bytes: &[u8]) -> Result<DynamicImage> {
    let cursor = Cursor::new(bytes);
    let mut reader = ImageReader::new(cursor.clone())
        .with_guessed_format()
        .context("Failed to guess image format")?;

    reader.no_limits();
    let image = reader.decode().context("Failed to decode image")?;

    let exif_reader = ExifReader::new();
    if let Ok(exif) = exif_reader.read_from_container(&mut cursor.clone()) {
        if let Some(orientation) = exif.get_field(Tag::Orientation, exif::In::PRIMARY)
                                       .and_then(|f| f.value.get_uint(0)) {
            return Ok(apply_orientation(image, Orientation::from_u16(orientation as u16)));
        }
    }

    Ok(image)
}

pub fn composite_patches_on_image(
    base_image: &DynamicImage,
    current_adjustments: &Value,
) -> Result<DynamicImage> {
    let patches_val = match current_adjustments.get("aiPatches") {
        Some(val) => val,
        None => return Ok(base_image.clone()),
    };

    let patches_arr = match patches_val.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return Ok(base_image.clone()),
    };

    let visible_patches: Vec<&Value> = patches_arr
        .par_iter()
        .filter(|patch_obj| {
            let is_visible = patch_obj
                .get("visible")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            
            let has_data = patch_obj.get("patchData").is_some();

            is_visible && has_data
        })
        .collect();

    if visible_patches.is_empty() {
        return Ok(base_image.clone());
    }

    let patch_layers: Result<Vec<RgbaImage>> = visible_patches
        .par_iter()
        .filter_map(|patch_obj| {
            let patch_data = patch_obj.get("patchData")?;
            
            let color_b64 = patch_data.get("color").and_then(|v| v.as_str())?;
            let mask_b64 = patch_data.get("mask").and_then(|v| v.as_str())?;

            let result: Result<RgbaImage> = (|| {
                let color_bytes = general_purpose::STANDARD.decode(color_b64)?;
                let color_image = image::load_from_memory(&color_bytes)?.to_rgb8();

                let mask_bytes = general_purpose::STANDARD.decode(mask_b64)?;
                let mask_image = image::load_from_memory(&mask_bytes)?.to_luma8();

                let (width, height) = color_image.dimensions();
                let mut patch_rgba = RgbaImage::new(width, height);

                for y in 0..height {
                    for x in 0..width {
                        let color_pixel = color_image.get_pixel(x, y);
                        let mask_pixel = mask_image.get_pixel(x, y);
                        patch_rgba.put_pixel(x, y, Rgba([
                            color_pixel[0],
                            color_pixel[1],
                            color_pixel[2],
                            mask_pixel[0],
                        ]));
                    }
                }
                Ok(patch_rgba)
            })();

            Some(result)
        })
        .collect();

    let patch_layers = patch_layers?;
    let mut composited_rgba = base_image.to_rgba8();
    for patch_layer in &patch_layers {
        imageops::overlay(&mut composited_rgba, patch_layer, 0, 0);
    }

    Ok(DynamicImage::ImageRgba8(composited_rgba))
}