// src-tauri/src/shaders/bracketing.wgsl

struct Uniforms {
    matrix: mat4x4<f32>,
    algo_mode: u32, // 0 = Exposure, 1 = Focus
    param_1: f32,   // Bias / Hardness
    input_gamma: f32,
    _pad: u32,
};

// Bindings match the Streaming Rust Layout
@group(0) @binding(0) var input_texture: texture_2d<f32>; // SINGLE TEXTURE
@group(0) @binding(1) var accum_color: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(2) var accum_weight: texture_storage_2d<r32float, read_write>;
@group(0) @binding(3) var<uniform> u: Uniforms;

// Helper: Get pixel from the single input frame
fn get_aligned_pixel(uv: vec2<f32>, dims: vec2<f32>) -> vec4<f32> {
    let pixel_coord = vec3<f32>(uv * dims, 1.0);
    let warped = u.matrix * vec4<f32>(pixel_coord, 1.0);
    
    if (abs(warped.z) < 0.00001) { return vec4<f32>(0.0); }
    let projected = warped.xy / warped.z;
    let sample_coord = vec2<i32>(round(projected));
    
    let input_dims = vec2<i32>(textureDimensions(input_texture));
    
    if (sample_coord.x < 0 || sample_coord.x >= input_dims.x || 
        sample_coord.y < 0 || sample_coord.y >= input_dims.y) {
        return vec4<f32>(0.0);
    }
    
    let raw = textureLoad(input_texture, sample_coord, 0);
    // Linearize
    return vec4<f32>(pow(raw.rgb, vec3<f32>(u.input_gamma)), raw.a);
}

fn calculate_exposure_weight(color: vec3<f32>) -> f32 {
    let mean = (color.r + color.g + color.b) / 3.0;
    let sat = sqrt(((color.r - mean)*(color.r - mean) + (color.g - mean)*(color.g - mean) + (color.b - mean)*(color.b - mean)) / 3.0);
    
    let ideal = 0.2 + (u.param_1 * 0.6); 
    let sigma = 0.2;
    
    let r_exp = exp(-0.5 * pow((color.r - ideal) / sigma, 2.0));
    let g_exp = exp(-0.5 * pow((color.g - ideal) / sigma, 2.0));
    let b_exp = exp(-0.5 * pow((color.b - ideal) / sigma, 2.0));
    
    return pow(sat, 1.0) * pow(r_exp * g_exp * b_exp, 1.0) + 0.0001; 
}

fn calculate_sharpness_metric(uv: vec2<f32>, dims: vec2<f32>) -> f32 {
    let step_x = 1.0 / dims.x;
    let step_y = 1.0 / dims.y;
    
    // Use Green channel for sharpness
    let c  = get_aligned_pixel(uv, dims).g;
    let l  = get_aligned_pixel(uv + vec2(-step_x, 0.0), dims).g;
    let r  = get_aligned_pixel(uv + vec2(step_x, 0.0), dims).g;
    let ua  = get_aligned_pixel(uv + vec2(0.0, -step_y), dims).g;
    let d  = get_aligned_pixel(uv + vec2(0.0, step_y), dims).g;
    
    let ul = get_aligned_pixel(uv + vec2(-step_x, -step_y), dims).g;
    let ur = get_aligned_pixel(uv + vec2(step_x, -step_y), dims).g;
    let dl = get_aligned_pixel(uv + vec2(-step_x, step_y), dims).g;
    let dr = get_aligned_pixel(uv + vec2(step_x, step_y), dims).g;

    let ml_x = abs(2.0 * c - l - r);
    let ml_y = abs(2.0 * c - ua - d);
    let ml_diag = abs(2.0 * c - ul - dr) + abs(2.0 * c - ur - dl);
    
    let energy = ml_x + ml_y + (ml_diag * 0.707);
    let val = max(0.0, energy - 0.005);
    let p = 1.0 + (u.param_1 * 3.0); 
    return pow(val, p);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let dims = vec2<f32>(textureDimensions(accum_color));
    if (global_id.x >= u32(dims.x) || global_id.y >= u32(dims.y)) { return; }
    
    let uv = vec2<f32>(f32(global_id.x) / dims.x, f32(global_id.y) / dims.y);
    let pos = vec2<i32>(global_id.xy);

    let sample = get_aligned_pixel(uv, dims);
    if (sample.a == 0.0) { return; }

    if (u.algo_mode == 0u) {
        // --- EXPOSURE FUSION (Add) ---
        let w = calculate_exposure_weight(sample.rgb);
        let cur_color = textureLoad(accum_color, pos);
        let cur_weight = textureLoad(accum_weight, pos).r;
        
        textureStore(accum_color, pos, cur_color + (sample * w));
        textureStore(accum_weight, pos, vec4<f32>(cur_weight + w, 0.0, 0.0, 0.0));
    } else {
        // --- FOCUS STACKING (Max) ---
        let sharpness = calculate_sharpness_metric(uv, dims);
        let max_sharpness = textureLoad(accum_weight, pos).r;
        
        if (sharpness > max_sharpness) {
            textureStore(accum_color, pos, sample);
            textureStore(accum_weight, pos, vec4<f32>(sharpness, 0.0, 0.0, 0.0));
        }
    }
}