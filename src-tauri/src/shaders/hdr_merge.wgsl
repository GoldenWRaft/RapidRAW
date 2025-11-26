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

fn calculate_weight(c: vec3<f32>) -> f32 {
    let brightness = max(c.r, max(c.g, c.b));
    let lum = dot(c, vec3<f32>(0.21, 0.72, 0.07));
    let sat = length(c - vec3<f32>(lum));

    // TARGET: 0.1 to 0.9
    let _target = 0.1 + (u.param_1 * 0.8); 

    // --- RELAXED GAUSSIAN ---
    // Previous "Power 5" was too narrow (approx sigma 0.1).
    // Sigma 0.25 allows a pixel that is 0.3 away from target to still have ~50% weight.
    // This allows Frame 1 (Dark) and Frame 5 (Bright) to blend together
    // even if Frame 3 is missing.
    let sigma = 0.25;
    
    var w = exp(-0.5 * pow((brightness - _target) / sigma, 2.0));

    // Saturation Bonus
    w = w * (1.0 + sat); 

    // Magenta Guard
    if (brightness > 0.95) { w = w * 0.001; }

    // Tiny Floor
    return w + 0.001;
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