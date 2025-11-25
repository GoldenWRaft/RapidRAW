struct Uniforms {
    matrix: mat4x4<f32>,
    param_1: f32, // Hardness (0.0 = Smooth Blend, 1.0 = High Contrast Select)
    width: f32,
    height: f32,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var accum_color: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(3) var accum_weight: texture_storage_2d<r32float, read_write>;
@group(0) @binding(4) var<uniform> u: Uniforms;

fn get_aligned_pixel(uv: vec2<f32>) -> vec4<f32> {
    let output_pt = vec4<f32>(uv.x * u.width, uv.y * u.height, 1.0, 1.0);
    let warped = u.matrix * output_pt;
    if (abs(warped.z) < 0.0001) { return vec4<f32>(0.0); }
    let input_uv = (warped.xy / warped.z) / vec2<f32>(u.width, u.height);
    
    // Safety Margin to kill edge slices
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

// Single Pixel Laplacian
fn raw_metric(uv: vec2<f32>) -> f32 {
    let dx = 1.0 / u.width;
    let dy = 1.0 / u.height;
    
    let c  = get_lum(uv);
    let l  = get_lum(uv + vec2<f32>(-dx, 0.0));
    let r  = get_lum(uv + vec2<f32>(dx, 0.0));
    let up = get_lum(uv + vec2<f32>(0.0, -dy));
    let dn = get_lum(uv + vec2<f32>(0.0, dy));

    // SML Metric
    let val = abs(2.0 * c - l - r) + abs(2.0 * c - up - dn);
    
    // NORMALIZATION (Fixes Bright Noise vs Dark Detail)
    // We divide by brightness. Bright pixels need MORE edge contrast to count.
    // Dark pixels need LESS. This equalizes the field.
    return val / (c + 0.1); 
}

// 3x3 Box Average (The Denoiser)
fn calculate_robust_sharpness(uv: vec2<f32>) -> f32 {
    let dx = 1.0 / u.width;
    let dy = 1.0 / u.height;
    
    var sum = 0.0;
    
    // INCREASED KERNEL SIZE: 5x5
    // We loop from -2 to +2.
    // This averages 25 pixels instead of 9.
    // It creates a much stronger "Denoising" effect for the decision map.
    for (var i = -2.0; i <= 2.0; i = i + 1.0) {
        for (var j = -2.0; j <= 2.0; j = j + 1.0) {
            sum = sum + raw_metric(uv + vec2<f32>(i * dx, j * dy));
        }
    }
    
    // Normalize by 25.0 (Total sample count)
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

    // 1. Get Denoised Sharpness
    let sharpness = calculate_robust_sharpness(uv);
    
    // 2. POWER CURVE (Signal Boosting)
    // Since we are averaging 25 pixels, the average score drops slightly.
    // We compensate by boosting the multiplier from 50.0 -> 80.0.
    let boost = sharpness * 80.0; 
    
    // Slider Controls Separation:
    // 0% -> Power 2.0 (Gentle weighting, reduces noise, smooths parallax)
    // 100% -> Power 12.0 (Aggressive, crispest text but potential artifacts)
    let power = 2.0 + (u.param_1 * 10.0); 
    
    let w = pow(boost, power) + 0.0001; // Tiny floor prevents black holes

    // 3. WEIGHTED ACCUMULATION (Soft Stack)
    // We use the HDR-style accumulation now. 
    // This blends overlapping text instead of cutting it in half.
    let cur_c = textureLoad(accum_color, coords);
    let cur_w = textureLoad(accum_weight, coords).r;

    textureStore(accum_color, coords, cur_c + (sample * w));
    textureStore(accum_weight, coords, vec4<f32>(cur_w + w, 0.0, 0.0, 0.0));
}