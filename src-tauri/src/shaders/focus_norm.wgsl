// UNIFIED LAYOUT MATCHING
// Bindings must be 'read_write' to match the Rust BindGroupLayout,
// even though we only read from them here.

@group(0) @binding(2) var accum_bg_color: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(3) var accum_bg_weight: texture_storage_2d<r32float, read_write>;
@group(0) @binding(4) var accum_detail_color: texture_storage_2d<rgba32float, read_write>;

// Binding 7: Output
@group(0) @binding(7) var output_texture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pos = vec2<i32>(global_id.xy);
    let dims = textureDimensions(output_texture);
    if (global_id.x >= dims.x || global_id.y >= dims.y) { return; }
    
    // 1. Resolve Background (Average)
    let bg_sum = textureLoad(accum_bg_color, pos);
    let bg_w = textureLoad(accum_bg_weight, pos).r;
    
    var final_color = vec3<f32>(0.0);
    
    if (bg_w > 0.0001) {
        final_color = bg_sum.rgb / bg_w;
    }

    // 2. Resolve Detail (Overlay)
    let detail = textureLoad(accum_detail_color, pos);
    
    // If alpha > 0.5, it means this pixel was selected as a sharp detail.
    if (detail.a > 0.5) {
        final_color = detail.rgb;
    }

    // 3. Gamma Correction
    final_color = pow(final_color, vec3<f32>(1.0 / 2.2));
    
    textureStore(output_texture, pos, vec4<f32>(final_color, 1.0));
}