use image::{GrayImage};
use nalgebra::Matrix3;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::AppHandle;
use crate::panorama_stitching::Feature;
use crate::panorama_utils::processing::{
    self, calculate_downscale_dimensions, compute_homography, find_features, 
    find_homography_ransac, generate_brief_pairs, match_features, MIN_INLIERS_FOR_CONNECTION,
};
use std::collections::HashMap;

/// Helper to stretch contrast so feature detectors can see details in RAW files
fn normalize_image_contrast(img: &GrayImage) -> GrayImage {
    let mut min_val = 255u8;
    let mut max_val = 0u8;

    // Find min/max
    for &p in img.iter() {
        if p < min_val { min_val = p; }
        if p > max_val { max_val = p; }
    }

    if min_val == max_val {
        return img.clone();
    }

    // Stretch to 0-255 range
    let mut new_img = img.clone();
    for p in new_img.iter_mut() {
        *p = ((*p as f32 - min_val as f32) / (max_val as f32 - min_val as f32) * 255.0) as u8;
    }
    new_img
}

pub fn align_brackets(
    image_paths: &Vec<String>, 
    anchor_index: Option<usize>, 
    _app_handle: AppHandle
) -> Result<Vec<(Matrix3<f64>, u32, u32)>, String> { // <--- CHANGED RETURN TYPE
    
    let start_time = Instant::now();
    println!("------------------------------------------------");
    println!("Starting bracket alignment for {} images...", image_paths.len());

    // 1. Determine Anchor
    let anchor_idx = anchor_index.unwrap_or(image_paths.len() / 2);
    println!("Anchor image index: {} ({})", anchor_idx, image_paths[anchor_idx]);

    // 2. Load Proxies & Extract Features
    let brief_pairs = generate_brief_pairs();
    
    // CHANGED: Added u32, u32 to the result tuple to carry dimensions
    let processed_images: Vec<Result<(usize, Vec<Feature>, f64, u32, u32), String>> = image_paths
        .par_iter()
        .enumerate()
        .map(|(i, filename)| {
            // Load image (reuse your existing loader)
            let file_bytes = std::fs::read(filename)
                .map_err(|e| format!("Failed to read {}", filename))?;
            
            // NOTE: Ensure this loader is getting a preview or raw buffer. 
            // If it's a RAW file, we ask for a small preview (sample=2 or 4) to be fast.
            let dynamic_image = crate::image_loader::load_base_image_from_bytes(&file_bytes, filename, false, 4.0)
                .map_err(|e| format!("Load error: {}", e))?;

            let gray_full = dynamic_image.to_luma8();
            let (w, h) = gray_full.dimensions(); // <--- CAPTURE DIMENSIONS
            
            // Downscale for alignment speed
            let (new_w, new_h, scale_factor) = calculate_downscale_dimensions(w, h);
            let mut gray_small = image::imageops::resize(
                &gray_full,
                new_w,
                new_h,
                image::imageops::FilterType::Triangle,
            );

            // *** CRITICAL FIX: Normalize Contrast ***
            // RAW files converted to 8-bit are often too dark for feature detection.
            gray_small = normalize_image_contrast(&gray_small);

            let features = find_features(&gray_small, &brief_pairs);
            
            // Debug log per image
            println!("  [Img {}] Size: {}x{}, Features found: {}", i, new_w, new_h, features.len());

            // PASS DIMENSIONS OUT
            Ok((i, features, scale_factor, w, h)) 
        })
        .collect();

    let mut features_map = Vec::new();
    let mut scales = Vec::new();
    let mut dimensions_map = HashMap::new(); // <--- STORE DIMENSIONS
    
    for res in processed_images {
        match res {
            Ok((i, feats, scale, w, h)) => {
                features_map.push((i, feats));
                scales.push(scale);
                dimensions_map.insert(i, (w, h)); // Store for retrieval later
            }
            Err(e) => return Err(e),
        }
    }
    
    // Sort to ensure indices match 0,1,2...
    features_map.sort_by_key(|k| k.0);
    
    if features_map[anchor_idx].1.len() < 10 {
        println!("!! ERROR: Anchor image has too few features. Alignment impossible.");
    }

    let anchor_features = &features_map[anchor_idx].1;
    let anchor_scale = scales[anchor_idx];

    // 3. Match Every Image to the Anchor
    let results: Arc<Mutex<Vec<(usize, Matrix3<f64>)>>> = Arc::new(Mutex::new(Vec::new()));

    features_map.par_iter().for_each(|(i, feats)| {
        if *i == anchor_idx {
            results.lock().unwrap().push((*i, Matrix3::identity()));
            return;
        }

        let matches = match_features(feats, anchor_features);
        
        if matches.len() < MIN_INLIERS_FOR_CONNECTION {
             println!("  [Img {} -> Anchor] Failed: Only {} raw matches (Needs {})", i, matches.len(), MIN_INLIERS_FOR_CONNECTION);
             results.lock().unwrap().push((*i, Matrix3::identity()));
             return;
        }

        let kps_src: Vec<_> = feats.iter().map(|f| f.keypoint).collect();
        let kps_anchor: Vec<_> = anchor_features.iter().map(|f| f.keypoint).collect();

        if let Some((_, inliers)) = find_homography_ransac(&matches, &kps_src, &kps_anchor) {
            println!("  [Img {} -> Anchor] Success: {} inliers", i, inliers.len());
            
            let inlier_points: Vec<_> = inliers.iter().map(|m| {
                let p1 = kps_src[m.index1];
                let p2 = kps_anchor[m.index2];
                (
                    nalgebra::Point2::new(p1.x as f64, p1.y as f64),
                    nalgebra::Point2::new(p2.x as f64, p2.y as f64),
                )
            }).collect();

            if let Some(h_refined) = compute_homography(&inlier_points) {
                let s_src_factor = scales[*i];      
                let s_anchor_factor = anchor_scale; 

                let scale_down_src = Matrix3::new(
                    1.0 / s_src_factor, 0.0, 0.0,
                    0.0, 1.0 / s_src_factor, 0.0,
                    0.0, 0.0, 1.0
                );

                let scale_up_anchor = Matrix3::new(
                    s_anchor_factor, 0.0, 0.0,
                    0.0, s_anchor_factor, 0.0,
                    0.0, 0.0, 1.0
                );

                let h_final = scale_up_anchor * h_refined * scale_down_src;

                results.lock().unwrap().push((*i, h_final));
            } else {
                 results.lock().unwrap().push((*i, Matrix3::identity()));
            }
        } else {
             println!("  [Img {} -> Anchor] RANSAC failed to find coherent model.", i);
             results.lock().unwrap().push((*i, Matrix3::identity()));
        }
    });

    let mut final_results = results.lock().unwrap().clone();
    final_results.sort_by_key(|k| k.0);

    println!("Bracket alignment finished in {:.2?}", start_time.elapsed());
    println!("------------------------------------------------");

    // CHANGED: Zip with dimensions
    Ok(final_results.into_iter().map(|(i, m)| {
        let (w, h) = dimensions_map.get(&i).unwrap_or(&(0,0));
        (m, *w, *h)
    }).collect())
}