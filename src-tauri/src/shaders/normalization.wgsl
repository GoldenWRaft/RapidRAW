// src-tauri/src/shaders/normalization.wgsl

struct NormUniforms {
    algo_mode: u32, // 0=Exposure, 1=Focus
};

@group(0) @binding(0) var accum_color: texture_storage_2d<rgba32float, read>;
@group(0) @binding(1) var accum_weight: texture_storage_2d<r32float, read>;
@group(0) @binding(2) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> u: NormUniforms;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = vec2<i32>(global_id.xy);
    let dims = textureDimensions(output_texture);
    if (global_id.x >= dims.x || global_id.y >= dims.y) { return; }

    let sum_c = textureLoad(accum_color, pos);
    let sum_w = textureLoad(accum_weight, pos).r;

    var final_color = sum_c.rgb;

    // Exposure Fusion uses Weighted Average (SumColor / SumWeight)
    // Focus Stacking uses Replacement (Color is already correct), no division needed.
    if (u.algo_mode == 0u && sum_w > 0.0001) {
        final_color = final_color / sum_w;
    }

    // Gamma Correction (Linear -> sRGB)
    // Required because all inputs were linearized in the accumulation step
    final_color = pow(final_color, vec3<f32>(1.0 / 2.2));

    textureStore(output_texture, pos, vec4<f32>(final_color, 1.0));
}