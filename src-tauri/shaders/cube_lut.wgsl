@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var lut_texture: texture_3d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;


// --- Add the color space helper functions back in ---
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(
        c * 12.92,
        1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055,
        c > vec3<f32>(0.0031308)
    );
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let image_dims = textureDimensions(output_texture);
    if (global_id.x >= image_dims.x || global_id.y >= image_dims.y) {
        return;
    }

    let pixel_coords = vec2<i32>(i32(global_id.x), i32(global_id.y));
    let original_color = textureLoad(input_texture, pixel_coords, 0).rgb;
    let new_color_linear = textureSampleLevel(lut_texture, lut_sampler, original_color, 0.0).rgb;
    let final_color_srgb = linear_to_srgb(new_color_linear);
    textureStore(output_texture, pixel_coords, vec4<f32>(final_color_srgb.rgb, 1.0));
}