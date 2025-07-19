@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var hald_clut: texture_2d<f32>;
@group(0) @binding(3) var lut_sampler: sampler;

fn hald_lookup(color: vec3<f32>) -> vec3<f32> {
    let lut_dim = f32(textureDimensions(hald_clut).x);

    // -- FIX IS HERE --
    // Replace cbrt(lut_dim) with pow(lut_dim, 1.0 / 3.0)
    let level = round(pow(lut_dim, 1.0 / 3.0)); 
    // -- END FIX --

    let slice_size = level * level;
    let scaled_b = color.b * (slice_size - 1.0);
    let slice_z1 = floor(scaled_b);
    let slice_z2 = ceil(scaled_b);
    let b_frac = fract(scaled_b);

    let u1 = (slice_z1 % level + color.r) / level;
    let v1 = (floor(slice_z1 / level) + color.g) / level;
    let u2 = (slice_z2 % level + color.r) / level;
    let v2 = (floor(slice_z2 / level) + color.g) / level;
    
    let color1 = textureSampleLevel(hald_clut, lut_sampler, vec2<f32>(u1, v1), 0.0).rgb;
    let color2 = textureSampleLevel(hald_clut, lut_sampler, vec2<f32>(u2, v2), 0.0).rgb;

    return mix(color1, color2, b_frac);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let image_dims = textureDimensions(output_texture);
    if (global_id.x >= image_dims.x || global_id.y >= image_dims.y) {
        return;
    }
    let pixel_coords = vec2<i32>(i32(global_id.x), i32(global_id.y));
    let original_color = textureLoad(input_texture, pixel_coords, 0).rgb;
    let new_color = hald_lookup(original_color);
    textureStore(output_texture, pixel_coords, vec4<f32>(new_color, 1.0));
}