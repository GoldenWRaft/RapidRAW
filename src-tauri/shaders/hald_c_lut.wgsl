@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var hald_clut: texture_2d<f32>;

fn hald_lookup(color: vec3<f32>) -> vec3<f32> {
    let dim = textureDimensions(hald_clut).x;
    let level = round(pow(f32(dim), 1.0 / 3.0));         // e.g. 12 for 1728x1728
    let cubeSize = level * level;                        // = 144
    // Scale by (level^2 - 1) to cover all entries
    let scaled = color * (cubeSize - 1.0);
    let r0 = floor(scaled.r);
    let g0 = floor(scaled.g);
    let b0 = floor(scaled.b);
    let r1 = min(r0 + 1.0, cubeSize - 1.0);
    let g1 = min(g0 + 1.0, cubeSize - 1.0);
    let b1 = min(b0 + 1.0, cubeSize - 1.0);
    let fr = fract(scaled.r);
    let fg = fract(scaled.g);
    let fb = fract(scaled.b);

    // Convert to integer coordinates for textureLoad
    let L = i32(level);
    let cube = i32(cubeSize);
    let r0_i = i32(r0);
    let g0_i = i32(g0); 
    let b0_i = i32(b0);
    let r1_i = i32(r1);
    let g1_i = i32(g1); 
    let b1_i = i32(b1);

    // Compute 2D indices per known Hald mapping:contentReference[oaicite:2]{index=2}
    let x000 = r0_i + (g0_i % L) * cube;
    let y000 = b0_i * L + (g0_i / L);
    let c000 = textureLoad(hald_clut, vec2<i32>(x000, y000), 0).rgb;

    let x100 = r1_i + (g0_i % L) * cube;
    let y100 = b0_i * L + (g0_i / L);
    let c100 = textureLoad(hald_clut, vec2<i32>(x100, y100), 0).rgb;

    let x010 = r0_i + (g1_i % L) * cube;
    let y010 = b0_i * L + (g1_i / L);
    let c010 = textureLoad(hald_clut, vec2<i32>(x010, y010), 0).rgb;

    let x110 = r1_i + (g1_i % L) * cube;
    let y110 = b0_i * L + (g1_i / L);
    let c110 = textureLoad(hald_clut, vec2<i32>(x110, y110), 0).rgb;

    let x001 = r0_i + (g0_i % L) * cube;
    let y001 = b1_i * L + (g0_i / L);
    let c001 = textureLoad(hald_clut, vec2<i32>(x001, y001), 0).rgb;

    let x101 = r1_i + (g0_i % L) * cube;
    let y101 = b1_i * L + (g0_i / L);
    let c101 = textureLoad(hald_clut, vec2<i32>(x101, y101), 0).rgb;

    let x011 = r0_i + (g1_i % L) * cube;
    let y011 = b1_i * L + (g1_i / L);
    let c011 = textureLoad(hald_clut, vec2<i32>(x011, y011), 0).rgb;

    let x111 = r1_i + (g1_i % L) * cube;
    let y111 = b1_i * L + (g1_i / L);
    let c111 = textureLoad(hald_clut, vec2<i32>(x111, y111), 0).rgb;

    // Trilinear interpolation
    let c00 = mix(c000, c100, fr);
    let c01 = mix(c001, c101, fr);
    let c10 = mix(c010, c110, fr);
    let c11 = mix(c011, c111, fr);
    let c0 = mix(c00, c10, fg);
    let c1 = mix(c01, c11, fg);
    return mix(c0, c1, fb);
}

fn map_coord(r: i32, g: i32, b: i32, L_i: i32) -> vec2<i32> {
    let slice_y = b / L_i;
    let slice_x = b % L_i;
    let slice_origin_x = slice_x * L_i;
    let slice_origin_y = slice_y * L_i;
    return vec2<i32>(slice_origin_x + r, slice_origin_y + g);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let dims = textureDimensions(output_texture);
    if (global_id.x >= dims.x || global_id.y >= dims.y) { return; }
    let coord = vec2<i32>(i32(global_id.x), i32(global_id.y));
    let rgb = textureLoad(input_texture, coord, 0).rgb;
    // let lin = srgb_to_linear(rgb);
    let looked = hald_lookup(rgb);
    let out = linear_to_srgb(looked);
    textureStore(output_texture, coord, vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(
        c / 12.92,
        pow((c + 0.055) / 1.055, vec3<f32>(2.4)),
        c > vec3<f32>(0.04045)
    );
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(
        c * 12.92,
        1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055,
        c > vec3<f32>(0.0031308)
    );
}