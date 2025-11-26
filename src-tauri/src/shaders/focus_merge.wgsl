struct Uniforms {
    matrix: mat4x4<f32>,
    param_1: f32, 
    width: f32,
    height: f32,
};


// BINDINGS
@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;

// BACKGROUND LAYER (Smooth)
@group(0) @binding(2) var accum_bg_color: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(3) var accum_bg_weight: texture_storage_2d<r32float, read_write>;

// DETAIL LAYER (Sharp)
@group(0) @binding(4) var accum_detail_color: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(5) var accum_detail_depth: texture_storage_2d<r32float, read_write>;

@group(0) @binding(6) var<uniform> u: Uniforms;

fn get_aligned_pixel(uv: vec2<f32>) -> vec4<f32> {
    let output_pt = vec4<f32>(uv.x * u.width, uv.y * u.height, 1.0, 1.0);
    let warped = u.matrix * output_pt;
    if (abs(warped.z) < 0.0001) { return vec4<f32>(0.0); }
    let input_uv = (warped.xy / warped.z) / vec2<f32>(u.width, u.height);
    
    let margin = 0.005;
    if (input_uv.x < margin || input_uv.x > 1.0 - margin || 
        input_uv.y < margin || input_uv.y > 1.0 - margin) {
        return vec4<f32>(0.0);
    }
    return textureSampleLevel(input_texture, input_sampler, input_uv, 0.0);
}

fn get_lum(uv: vec2<f32>) -> f32 {
    let linear = get_aligned_pixel(uv).rgb;
    let srgb = pow(linear, vec3<f32>(1.0 / 2.2));
    return dot(srgb, vec3<f32>(0.21, 0.72, 0.07));
}

fn raw_metric(uv: vec2<f32>) -> f32 {
    let dx = 1.0 / u.width;
    let dy = 1.0 / u.height;
    let c  = get_lum(uv);
    let l  = get_lum(uv + vec2<f32>(-dx, 0.0));
    let r  = get_lum(uv + vec2<f32>(dx, 0.0));
    let up = get_lum(uv + vec2<f32>(0.0, -dy));
    let dn = get_lum(uv + vec2<f32>(0.0, dy));
    let val = abs(2.0 * c - l - r) + abs(2.0 * c - up - dn);
    return val / (c + 0.1); 
}

// Wide 5x5 Smoothing to ensure Noise doesn't trigger "Sharp" logic
fn calculate_robust_sharpness(uv: vec2<f32>) -> f32 {
    let dx = 1.0 / u.width;
    let dy = 1.0 / u.height;
    var sum = 0.0;
    for (var i = -2.0; i <= 2.0; i = i + 1.0) {
        for (var j = -2.0; j <= 2.0; j = j + 1.0) {
            sum = sum + raw_metric(uv + vec2<f32>(i * dx, j * dy));
        }
    }
    return sum / 25.0;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;
    if (x >= u32(u.width) || y >= u32(u.height)) { return; }
    
    let uv = vec2<f32>(f32(x) / u.width, f32(y) / u.height);
    let coords = vec2<i32>(i32(x), i32(y));

    let sample = get_aligned_pixel(uv);
    if (sample.a < 0.1) { return; }

    // 1. ALWAYS UPDATE BACKGROUND (Averaging)
    // This builds the smooth blue cover. Noise cancels out.
    let cur_bg = textureLoad(accum_bg_color, coords);
    let cur_bg_w = textureLoad(accum_bg_weight, coords).r;
    textureStore(accum_bg_color, coords, cur_bg + sample);
    textureStore(accum_bg_weight, coords, vec4<f32>(cur_bg_w + 1.0, 0.0, 0.0, 0.0));

    // 2. CONDITIONAL UPDATE DETAIL (Selection)
    // This captures only the text.
    let sharpness = calculate_robust_sharpness(uv);
    
    // Noise Gate: Only consider pixels that are actually sharp.
    // If it's below threshold, we assume it's part of the background.
    let threshold = u.param_1 * 0.05;
    
    if (sharpness > threshold) {
        let cur_depth = textureLoad(accum_detail_depth, coords).r;
        
        // Winner Takes All
        if (sharpness > cur_depth) {
            // Store the color. Alpha = 1.0 means "I have detail".
            textureStore(accum_detail_color, coords, vec4<f32>(sample.rgb, 1.0));
            textureStore(accum_detail_depth, coords, vec4<f32>(sharpness, 0.0, 0.0, 0.0));
        }
    }
}