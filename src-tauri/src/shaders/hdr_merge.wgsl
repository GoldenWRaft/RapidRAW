struct Uniforms {
    matrix: mat4x4<f32>,
    param_1: f32,
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

    let margin = 0.005; 
    if (input_uv.x < margin || input_uv.x > 1.0 - margin || 
        input_uv.y < margin || input_uv.y > 1.0 - margin) {
        return vec4<f32>(0.0);
    }
    
    return textureSampleLevel(input_texture, input_sampler, input_uv, 0.0);
}

// NEW: HIGHLIGHT RECONSTRUCTION
fn fix_magenta_highlights(c: vec3<f32>) -> vec3<f32> {
    let max_chan = max(c.r, max(c.g, c.b));
    
    // If pixel is bright...
    if (max_chan > 0.8) {
        // ...and Green is surprisingly weak compared to Red/Blue...
        if (c.g < c.r * 0.9 && c.g < c.b * 0.9) {
            // ...It's a clip artifact. Force Green to match the others.
            // This turns Pink (2.0, 1.0, 2.0) into White (2.0, 2.0, 2.0).
            let recovered_g = max(c.r, c.b);
            return vec3<f32>(c.r, recovered_g, c.b);
        }
    }
    return c;
}

fn calculate_weight(linear_color: vec3<f32>) -> f32 {
    // 1. Perceptual View (for judging exposure)
    let srgb = pow(linear_color, vec3<f32>(1.0 / 2.2));
    
    let brightness = max(srgb.r, max(srgb.g, srgb.b));
    let lum = dot(srgb, vec3<f32>(0.21, 0.72, 0.07));
    let sat = length(srgb - vec3<f32>(lum));

    let ideal = 0.5;
    let sigma = 0.4; // Wide tolerance for shadows
    
    var w = exp(-0.5 * pow((brightness - ideal) / sigma, 2.0));
    w = w * (1.0 + 2.0 * sat); // Prefer saturation

    // 2. Magenta Guard (Weighting)
    // Even though we fixed the color, we still trust this pixel LESS
    // because it was clipped.
    if (brightness > 0.95) { w = w * 0.1; }

    return w + 0.01;
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

    // 1. FIX THE PIXEL DATA (Pink -> White)
    let clean_rgb = fix_magenta_highlights(sample.rgb);

    // 2. CALCULATE WEIGHT
    let w = calculate_weight(clean_rgb);
    
    let cur_c = textureLoad(accum_color, coords);
    let cur_w = textureLoad(accum_weight, coords).r;
    
    textureStore(accum_color, coords, cur_c + (vec4<f32>(clean_rgb, 1.0) * w));
    textureStore(accum_weight, coords, vec4<f32>(cur_w + w, 0.0, 0.0, 0.0));
}

/*
struct Uniforms {
    matrix: mat4x4<f32>,
    param_1: f32, // Bias (0.0 = Prefer Darkest, 1.0 = Prefer Brightest)
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
    
    // MARGIN CHECK (Consolidated from your update)
    // Rejects edge artifacts
    let margin = 0.005; 
    if (input_uv.x < margin || input_uv.x > 1.0 - margin || 
        input_uv.y < margin || input_uv.y > 1.0 - margin) {
        return vec4<f32>(0.0);
    }
    
    return textureSampleLevel(input_texture, input_sampler, input_uv, 0.0);
}

fn fix_magenta_highlights(c: vec3<f32>) -> vec3<f32> {
    let max_chan = max(c.r, max(c.g, c.b));
    if (max_chan > 0.8) {
        if (c.g < c.r * 0.9 && c.g < c.b * 0.9) {
            let recovered_g = max(c.r, c.b);
            return vec3<f32>(c.r, recovered_g, c.b);
        }
    }
    return c;
}

fn calculate_weight(linear_color: vec3<f32>) -> f32 {
    let srgb = pow(linear_color, vec3<f32>(1.0 / 2.2));
    
    let brightness = max(srgb.r, max(srgb.g, srgb.b));
    let lum = dot(srgb, vec3<f32>(0.21, 0.72, 0.07));
    let sat = length(srgb - vec3<f32>(lum));

    // --- SLIDER LOGIC UPDATE ---
    
    // 1. Widen the Target Range
    // Old: 0.5 +/- 0.2 (Range 0.3 to 0.7) -> Too subtle.
    // New: 0.1 to 0.9.
    // Slider Left (0.0) -> Target 0.1 (Loves dark pixels/Sky)
    // Slider Right (1.0) -> Target 0.9 (Loves bright pixels/Shadows)
    let ideal = 0.1 + (u.param_1 * 0.8); 
    
    // 2. Tighten the Curve
    // Old: 0.4 -> Too loose, accepted everything.
    // New: 0.25 -> Pickier. Forces the algorithm to respect the 'ideal'.
    let sigma = 0.25; 
    
    var w = exp(-0.5 * pow((brightness - ideal) / sigma, 2.0));
    w = w * (1.0 + 2.0 * sat); 

    // Magenta Guard
    if (brightness > 0.95) { w = w * 0.01; }

    // Robust Floor (Small enough not to dilute the slider effect)
    return w + 0.05;
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

    let clean_rgb = fix_magenta_highlights(sample.rgb);
    let w = calculate_weight(clean_rgb);
    
    let cur_c = textureLoad(accum_color, coords);
    let cur_w = textureLoad(accum_weight, coords).r;
    
    textureStore(accum_color, coords, cur_c + (vec4<f32>(clean_rgb, 1.0) * w));
    textureStore(accum_weight, coords, vec4<f32>(cur_w + w, 0.0, 0.0, 0.0));
}
*/