struct NormUniforms {
    algo_mode: u32,
};

@group(0) @binding(0) var accum_color: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(1) var accum_weight: texture_storage_2d<r32float, read_write>;
@group(0) @binding(2) var output_texture: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(3) var<uniform> u: NormUniforms;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = vec2<i32>(global_id.xy);
    let dims = textureDimensions(output_texture);
    if (global_id.x >= dims.x || global_id.y >= dims.y) { return; }

    let sum_c = textureLoad(accum_color, pos);
    let sum_w = textureLoad(accum_weight, pos).r;

    var final_color = sum_c.rgb;

    if (u.algo_mode == 0u) {
        // HDR MODE: Weighted Average (Keep Division)
        if (sum_w > 0.00001) {
            final_color = final_color / sum_w;
        }
    } 
    else {
        // FOCUS MODE: Winner Takes All (NO DIVISION)
        // The accumulator already holds the raw pixel from the best frame.
        // Passing it through directly preserves 100% of the original sharpness.
    }

    // Output Gamma
    final_color = pow(final_color, vec3<f32>(1.0 / 2.2));

    textureStore(output_texture, pos, vec4<f32>(final_color, 1.0));
}