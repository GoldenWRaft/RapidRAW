struct Point {
    x: f32,
    y: f32,
    _pad1: f32,
    _pad2: f32,
}

struct HslColor {
    hue: f32,
    saturation: f32,
    luminance: f32,
    _pad: f32,
}

struct GlobalAdjustments {
    exposure: f32,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
    vibrance: f32,
    
    sharpness: f32,
    luma_noise_reduction: f32,
    color_noise_reduction: f32,
    clarity: f32,
    dehaze: f32,
    structure: f32,
    vignette_amount: f32,
    vignette_midpoint: f32,
    vignette_roundness: f32,
    vignette_feather: f32,
    grain_amount: f32,
    grain_size: f32,
    grain_roughness: f32,
    _pad1: f32,

    hsl: array<HslColor, 8>,
    luma_curve: array<Point, 16>,
    red_curve: array<Point, 16>,
    green_curve: array<Point, 16>,
    blue_curve: array<Point, 16>,
    luma_curve_count: u32,
    red_curve_count: u32,
    green_curve_count: u32,
    blue_curve_count: u32,
}

struct MaskAdjustments {
    exposure: f32,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
    vibrance: f32,
    
    sharpness: f32,
    luma_noise_reduction: f32,
    color_noise_reduction: f32,
    clarity: f32,
    dehaze: f32,
    structure: f32,
    
    _pad1: f32,
    _pad2: f32,
    _pad3: f32,
    _pad4: f32,

    hsl: array<HslColor, 8>,
    luma_curve: array<Point, 16>,
    red_curve: array<Point, 16>,
    green_curve: array<Point, 16>,
    blue_curve: array<Point, 16>,
    luma_curve_count: u32,
    red_curve_count: u32,
    green_curve_count: u32,
    blue_curve_count: u32,
}

struct AllAdjustments {
    global: GlobalAdjustments,
    mask_adjustments: array<MaskAdjustments, 16>,
    mask_count: u32,
    tile_offset_x: u32,
    tile_offset_y: u32,
    _pad1: u32,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> adjustments: AllAdjustments;
@group(0) @binding(3) var mask_textures: texture_2d_array<f32>;

const LUMA_COEFF = vec3<f32>(0.2126, 0.7152, 0.0722);

fn get_luma(c: vec3<f32>) -> f32 {
    return dot(c, LUMA_COEFF);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = vec3<f32>(0.04045);
    let a = vec3<f32>(0.055);
    let higher = pow((c + a) / (1.0 + a), vec3<f32>(2.4));
    let lower = c / 12.92;
    return select(higher, lower, c <= cutoff);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let c_clamped = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    let cutoff = vec3<f32>(0.0031308);
    let a = vec3<f32>(0.055);
    let higher = (1.0 + a) * pow(c_clamped, vec3<f32>(1.0 / 2.4)) - a;
    let lower = c_clamped * 12.92;
    return select(higher, lower, c_clamped <= cutoff);
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let c_max = max(c.r, max(c.g, c.b));
    let c_min = min(c.r, min(c.g, c.b));
    let delta = c_max - c_min;
    var h: f32 = 0.0;
    if (delta > 0.0) {
        if (c_max == c.r) { h = 60.0 * (((c.g - c.b) / delta) % 6.0); }
        else if (c_max == c.g) { h = 60.0 * (((c.b - c.r) / delta) + 2.0); }
        else { h = 60.0 * (((c.r - c.g) / delta) + 4.0); }
    }
    if (h < 0.0) { h += 360.0; }
    let s = select(0.0, delta / c_max, c_max > 0.0);
    return vec3<f32>(h, s, c_max);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let h = c.x; let s = c.y; let v = c.z;
    let C = v * s;
    let X = C * (1.0 - abs((h / 60.0) % 2.0 - 1.0));
    let m = v - C;
    var rgb_prime: vec3<f32>;
    if (h < 60.0) { rgb_prime = vec3<f32>(C, X, 0.0); }
    else if (h < 120.0) { rgb_prime = vec3<f32>(X, C, 0.0); }
    else if (h < 180.0) { rgb_prime = vec3<f32>(0.0, C, X); }
    else if (h < 240.0) { rgb_prime = vec3<f32>(0.0, X, C); }
    else if (h < 300.0) { rgb_prime = vec3<f32>(X, 0.0, C); }
    else { rgb_prime = vec3<f32>(C, 0.0, X); }
    return rgb_prime + vec3<f32>(m, m, m);
}

fn get_hsl_influence(hue: f32, center_hue: f32, range_width: f32) -> f32 {
    let diff1 = abs(hue - center_hue);
    let diff2 = 360.0 - diff1;
    let distance = min(diff1, diff2);
    let normalized_distance = distance / (range_width * 0.5);
    return 1.0 - smoothstep(0.0, 1.0, normalized_distance);
}

fn hash(p: vec2<f32>) -> f32 {
    var p_mut = p * mat2x2<f32>(vec2<f32>(127.1, 311.7), vec2<f32>(269.5, 183.3));
    return fract(sin(p_mut.x + p_mut.y) * 43758.5453123);
}

fn gradient_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);

    let u = f * f * (3.0 - 2.0 * f);

    let grad_00 = (vec2<f32>(hash(i), hash(i + 17.0)) * 2.0 - 1.0);
    let grad_01 = (vec2<f32>(hash(i + vec2(0.0, 1.0)), hash(i + vec2(0.0, 1.0) + 17.0)) * 2.0 - 1.0);
    let grad_10 = (vec2<f32>(hash(i + vec2(1.0, 0.0)), hash(i + vec2(1.0, 0.0) + 17.0)) * 2.0 - 1.0);
    let grad_11 = (vec2<f32>(hash(i + vec2(1.0, 1.0)), hash(i + vec2(1.0, 1.0) + 17.0)) * 2.0 - 1.0);

    let dot_00 = dot(grad_00, f - vec2(0.0, 0.0));
    let dot_01 = dot(grad_01, f - vec2(0.0, 1.0));
    let dot_10 = dot(grad_10, f - vec2(1.0, 0.0));
    let dot_11 = dot(grad_11, f - vec2(1.0, 1.0));

    let bottom_interp = mix(dot_00, dot_10, u.x);
    let top_interp = mix(dot_01, dot_11, u.x);
    let final_interp = mix(bottom_interp, top_interp, u.y);

    return final_interp;
}

fn interpolate_cubic_hermite(x: f32, p1: Point, p2: Point, m1: f32, m2: f32) -> f32 {
    let dx = p2.x - p1.x;
    if (dx <= 0.0) {
        return p1.y;
    }
    let t = (x - p1.x) / dx;
    let t2 = t * t;
    let t3 = t2 * t;

    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;

    return h00 * p1.y + h10 * m1 * dx + h01 * p2.y + h11 * m2 * dx;
}

fn apply_curve(val: f32, points: array<Point, 16>, count: u32) -> f32 {
    if (count < 2u) { return val; }

    var local_points = points;
    let x = val * 255.0;

    if (x <= local_points[0].x) {
        return local_points[0].y / 255.0;
    }
    if (x >= local_points[count - 1u].x) {
        return local_points[count - 1u].y / 255.0;
    }

    for (var i = 0u; i < 15u; i = i + 1u) {
        if (i >= count - 1u) {
            break;
        }

        let p1 = local_points[i];
        let p2 = local_points[i + 1u];

        if (x <= p2.x) {
            let p0 = local_points[max(0u, i - 1u)];
            let p3 = local_points[min(count - 1u, i + 2u)];

            let delta_before = (p1.y - p0.y) / max(0.001, p1.x - p0.x);
            let delta_current = (p2.y - p1.y) / max(0.001, p2.x - p1.x);
            let delta_after = (p3.y - p2.y) / max(0.001, p3.x - p2.x);

            var tangent_at_p1: f32;
            var tangent_at_p2: f32;

            if (i == 0u) {
                tangent_at_p1 = delta_current;
            } else {
                if (delta_before * delta_current <= 0.0) {
                    tangent_at_p1 = 0.0;
                } else {
                    tangent_at_p1 = (delta_before + delta_current) / 2.0;
                }
            }

            if (i + 1u == count - 1u) {
                tangent_at_p2 = delta_current;
            } else {
                if (delta_current * delta_after <= 0.0) {
                    tangent_at_p2 = 0.0;
                } else {
                    tangent_at_p2 = (delta_current + delta_after) / 2.0;
                }
            }

            if (delta_current != 0.0) {
                let alpha = tangent_at_p1 / delta_current;
                let beta = tangent_at_p2 / delta_current;
                if (alpha * alpha + beta * beta > 9.0) {
                    let tau = 3.0 / sqrt(alpha * alpha + beta * beta);
                    tangent_at_p1 = tangent_at_p1 * tau;
                    tangent_at_p2 = tangent_at_p2 * tau;
                }
            }

            let result_y = interpolate_cubic_hermite(x, p1, p2, tangent_at_p1, tangent_at_p2);
            return clamp(result_y / 255.0, 0.0, 1.0);
        }
    }

    return local_points[count - 1u].y / 255.0;
}

fn apply_tonal_adjustments(color: vec3<f32>, con: f32, hi: f32, sh: f32, wh: f32, bl: f32) -> vec3<f32> {
    var rgb = color;
    let white_level = 1.0 - wh * 0.25;
    rgb = rgb / max(white_level, 0.01);
    let luma = get_luma(rgb);

    if (hi < 0.0) {
        let amount = abs(hi);
        let highlight_mask = smoothstep(0.5, 1.0, luma);
        if (highlight_mask > 0.001) {
            let highlight_gamma = 1.0 + amount * 2.5;
            let recovered_rgb = pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(highlight_gamma));
            rgb = mix(rgb, recovered_rgb, highlight_mask);
        }
    } else if (hi > 0.0) {
        let highlight_range = smoothstep(0.5, 1.0, luma);
        rgb = rgb + (hi * 0.3 * highlight_range * (1.0 - luma));
    }

    if (sh != 0.0) {
        let shadow_range = 1.0 - smoothstep(0.0, 0.4, luma);
        if (sh > 0.0) {
            let lift_amount = sh * shadow_range;
            let shadow_gamma = 1.0 / (1.0 + lift_amount * 2.0);
            rgb = pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(shadow_gamma));
        } else {
            rgb = rgb * (1.0 + sh * shadow_range);
        }
    }

    if (bl != 0.0) {
        let blacks_range = 1.0 - smoothstep(0.0, 0.2, luma);
        if (blacks_range > 0.001) {
            let safe_rgb = max(rgb, vec3<f32>(0.0));
            let black_gamma = pow(2.0, -bl * 0.75);
            let adjusted = pow(safe_rgb, vec3<f32>(black_gamma));
            rgb = mix(rgb, adjusted, blacks_range);
        }
    }

    if (con != 0.0) {
        let safe_rgb = max(rgb, vec3<f32>(0.0));
        let g = 2.2;
        let perceptual = pow(safe_rgb, vec3<f32>(1.0 / g));
        let clamped_perceptual = clamp(perceptual, vec3<f32>(0.0), vec3<f32>(1.0));
        let strength = pow(2.0, con * 1.25);
        let condition = clamped_perceptual < vec3<f32>(0.5);
        let high_part = 1.0 - 0.5 * pow(2.0 * (1.0 - clamped_perceptual), vec3<f32>(strength));
        let low_part = 0.5 * pow(2.0 * clamped_perceptual, vec3<f32>(strength));
        let curved_perceptual = select(high_part, low_part, condition);
        let contrast_adjusted_rgb = pow(curved_perceptual, vec3<f32>(g));
        let mix_factor = smoothstep(vec3<f32>(1.0), vec3<f32>(1.01), safe_rgb);
        rgb = mix(contrast_adjusted_rgb, rgb, mix_factor);
    }

    return clamp(rgb, vec3<f32>(-0.1), vec3<f32>(1.5));
}

fn apply_white_balance(color: vec3<f32>, temp: f32, tnt: f32) -> vec3<f32> {
    var rgb = color;
    let temp_kelvin_mult = vec3<f32>(1.0 + temp * 0.2, 1.0 + temp * 0.05, 1.0 - temp * 0.2);
    let tint_mult = vec3<f32>(1.0 - tnt * 0.25, 1.0 + tnt * 0.25, 1.0 - tnt * 0.25);
    rgb *= temp_kelvin_mult * tint_mult;
    return rgb;
}

fn apply_creative_color(color: vec3<f32>, sat: f32, vib: f32) -> vec3<f32> {
    if (sat == 0.0 && vib == 0.0) { return color; }
    let luma = get_luma(color);
    var sat_rgb = mix(vec3<f32>(luma), color, 1.0 + sat);
    if (vib != 0.0) {
        let luma_for_vib = get_luma(sat_rgb);
        let current_saturation = distance(sat_rgb, vec3<f32>(luma_for_vib));
        let saturation_mask = 1.0 - smoothstep(0.1, 0.7, current_saturation);
        let shadow_boost = smoothstep(0.0, 0.2, luma_for_vib);
        let highlight_protection = 1.0 - smoothstep(0.4, 0.9, luma_for_vib);
        let luminance_mask = shadow_boost * highlight_protection;
        let final_mask = saturation_mask * luminance_mask;
        if (vib > 0.0) {
            let strength_multiplier = 2.5;
            let vibrance_amount = vib * final_mask * strength_multiplier;
            sat_rgb = mix(vec3<f32>(luma_for_vib), sat_rgb, 1.0 + vibrance_amount);
        } else {
            let skin_luma_protection = 1.0 - smoothstep(0.3, 0.6, luma_for_vib);
            let skin_sat_protection = smoothstep(0.1, 0.3, current_saturation);
            let protection_mask = skin_luma_protection * skin_sat_protection;
            let vibrance_amount = vib * (1.0 - protection_mask);
            sat_rgb = mix(vec3<f32>(luma_for_vib), sat_rgb, 1.0 + vibrance_amount);
        }
    }
    return sat_rgb;
}

fn apply_hsl_panel(color: vec3<f32>, hsl_adjustments: array<HslColor, 8>) -> vec3<f32> {
    var hsv = rgb_to_hsv(color);
    if (hsv.y < 0.01) { return color; }
    var total_hue_shift: f32 = 0.0;
    var total_sat_adjust: f32 = 0.0;
    var total_lum_adjust: f32 = 0.0;
    var total_influence: f32 = 0.0;
    var influence = get_hsl_influence(hsv.x, 0.0, 80.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[0].hue * influence; total_sat_adjust += hsl_adjustments[0].saturation * influence; total_lum_adjust += hsl_adjustments[0].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 30.0, 70.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[1].hue * influence; total_sat_adjust += hsl_adjustments[1].saturation * influence; total_lum_adjust += hsl_adjustments[1].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 60.0, 70.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[2].hue * influence; total_sat_adjust += hsl_adjustments[2].saturation * influence; total_lum_adjust += hsl_adjustments[2].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 120.0, 100.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[3].hue * influence; total_sat_adjust += hsl_adjustments[3].saturation * influence; total_lum_adjust += hsl_adjustments[3].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 180.0, 80.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[4].hue * influence; total_sat_adjust += hsl_adjustments[4].saturation * influence; total_lum_adjust += hsl_adjustments[4].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 240.0, 90.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[5].hue * influence; total_sat_adjust += hsl_adjustments[5].saturation * influence; total_lum_adjust += hsl_adjustments[5].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 285.0, 80.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[6].hue * influence; total_sat_adjust += hsl_adjustments[6].saturation * influence; total_lum_adjust += hsl_adjustments[6].luminance * influence; total_influence += influence; }
    influence = get_hsl_influence(hsv.x, 330.0, 80.0); if (influence > 0.001) { total_hue_shift += hsl_adjustments[7].hue * influence; total_sat_adjust += hsl_adjustments[7].saturation * influence; total_lum_adjust += hsl_adjustments[7].luminance * influence; total_influence += influence; }
    if (total_influence > 0.001) {
        let norm_factor = 1.0 / total_influence;
        hsv.x = (hsv.x + total_hue_shift * norm_factor + 360.0) % 360.0;
        hsv.y = clamp(hsv.y * (1.0 + total_sat_adjust * norm_factor), 0.0, 1.0);
        hsv.z = clamp(hsv.z * (1.0 + total_lum_adjust * norm_factor), 0.0, 1.5);
    }
    return hsv_to_rgb(hsv);
}

fn apply_local_contrast(processed_color: vec3<f32>, coords_i: vec2<i32>, radius: i32, amount: f32) -> vec3<f32> {
    if (amount == 0.0) { return processed_color; }
    let max_coords = vec2<i32>(textureDimensions(input_texture) - 1u);
    let original_luma = get_luma(processed_color);
    var blurred_linear = vec3<f32>(0.0);
    var total_weight = 0.0;
    let spatial_sigma = f32(radius);
    let range_sigma = 0.25;
    for (var y = -radius; y <= radius; y += 1) {
        for (var x = -radius; x <= radius; x += 1) {
            let offset = vec2<i32>(x, y);
            let sample_coords = clamp(coords_i + offset, vec2<i32>(0), max_coords);
            let sample_linear = srgb_to_linear(textureLoad(input_texture, sample_coords, 0).rgb);
            let sample_luma = get_luma(sample_linear);
            let spatial_dist_sq = f32(x * x + y * y);
            let luma_dist = sample_luma - original_luma;
            let spatial_weight = exp(-spatial_dist_sq / (2.0 * spatial_sigma * spatial_sigma));
            let range_weight = exp(-(luma_dist * luma_dist) / (2.0 * range_sigma * range_sigma));
            let weight = spatial_weight * range_weight;
            blurred_linear += sample_linear * weight;
            total_weight += weight;
        }
    }
    if (total_weight > 0.0) { blurred_linear /= total_weight; } else { blurred_linear = processed_color; }
    let detail_linear = processed_color - blurred_linear;
    let shadow_protection = smoothstep(0.0, 0.25, original_luma);
    let highlight_protection = 1.0 - smoothstep(0.75, 1.0, original_luma);
    let midtone_mask = shadow_protection * highlight_protection;
    let scaled_amount = amount * 0.8;
    let enhanced_detail = detail_linear * scaled_amount * midtone_mask;
    return processed_color + enhanced_detail;
}

fn apply_dehaze(color: vec3<f32>, amount: f32) -> vec3<f32> {
    if (amount == 0.0) { return color; }
    let atmospheric_light = vec3<f32>(0.95, 0.97, 1.0);
    if (amount > 0.0) {
        let dark_channel = min(color.r, min(color.g, color.b));
        let transmission_estimate = 1.0 - dark_channel;
        let t = 1.0 - amount * transmission_estimate;
        let recovered = (color - atmospheric_light) / max(t, 0.1) + atmospheric_light;
        var result = mix(color, recovered, amount);
        result = 0.5 + (result - 0.5) * (1.0 + amount * 0.15);
        let luma = get_luma(result);
        result = mix(vec3<f32>(luma), result, 1.0 + amount * 0.1);
        return result;
    } else {
        return mix(color, atmospheric_light, abs(amount) * 0.7);
    }
}

fn apply_noise_reduction(color: vec3<f32>, coords_i: vec2<i32>, luma_amount: f32, color_amount: f32) -> vec3<f32> {
    if (luma_amount == 0.0 && color_amount == 0.0) { return color; }
    var accum_color = vec3<f32>(0.0);
    var total_weight = 0.0;
    let center_luma = get_luma(color);
    let max_coords = vec2<i32>(textureDimensions(input_texture) - 1u);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let offset = vec2<i32>(x, y);
            let sample_coords = clamp(coords_i + offset, vec2<i32>(0), max_coords);
            let sample_color = srgb_to_linear(textureLoad(input_texture, sample_coords, 0).rgb);
            let luma_diff = abs(get_luma(sample_color) - center_luma);
            let color_diff = distance(sample_color, color);
            let luma_weight = 1.0 - smoothstep(0.0, 0.1, luma_diff / max(luma_amount, 0.001));
            let color_weight = 1.0 - smoothstep(0.0, 0.2, color_diff / max(color_amount, 0.001));
            let weight = luma_weight * color_weight;
            accum_color += sample_color * weight;
            total_weight += weight;
        }
    }
    if (total_weight > 0.0) { return accum_color / total_weight; }
    return color;
}

fn apply_all_curves(
    color_srgb: vec3<f32>, 
    luma_curve: array<Point, 16>, luma_curve_count: u32,
    red_curve: array<Point, 16>, red_curve_count: u32,
    green_curve: array<Point, 16>, green_curve_count: u32,
    blue_curve: array<Point, 16>, blue_curve_count: u32
) -> vec3<f32> {
    let luma_val = get_luma(color_srgb);
    let luma_curved = apply_curve(luma_val, luma_curve, luma_curve_count);
    
    var luma_adjusted_srgb: vec3<f32>;
    if (luma_val > 0.001) {
        let ratio = luma_curved / luma_val;
        luma_adjusted_srgb = color_srgb * ratio;
    } else {
        luma_adjusted_srgb = vec3<f32>(luma_curved);
    }

    let max_component = max(luma_adjusted_srgb.r, max(luma_adjusted_srgb.g, luma_adjusted_srgb.b));
    if (max_component > 1.0) {
        luma_adjusted_srgb = luma_adjusted_srgb / max_component;
    }

    let curved_srgb = vec3<f32>(
        apply_curve(luma_adjusted_srgb.r, red_curve, red_curve_count),
        apply_curve(luma_adjusted_srgb.g, green_curve, green_curve_count),
        apply_curve(luma_adjusted_srgb.b, blue_curve, blue_curve_count)
    );

    return curved_srgb;
}

fn apply_all_adjustments(initial_rgb: vec3<f32>, adj: GlobalAdjustments, coords_i: vec2<i32>) -> vec3<f32> {
    var processed_rgb = initial_rgb;
    processed_rgb = apply_noise_reduction(processed_rgb, coords_i, adj.luma_noise_reduction, adj.color_noise_reduction);
    processed_rgb = apply_white_balance(processed_rgb, adj.temperature, adj.tint);
    processed_rgb = processed_rgb * pow(2.0, adj.exposure);
    processed_rgb = apply_tonal_adjustments(processed_rgb, adj.contrast, adj.highlights, adj.shadows, adj.whites, adj.blacks);
    processed_rgb = apply_dehaze(processed_rgb, adj.dehaze);
    processed_rgb = apply_local_contrast(processed_rgb, coords_i, 2, adj.sharpness);
    processed_rgb = apply_local_contrast(processed_rgb, coords_i, 8, adj.clarity);
    processed_rgb = apply_local_contrast(processed_rgb, coords_i, 20, adj.structure);
    processed_rgb = apply_creative_color(processed_rgb, adj.saturation, adj.vibrance);
    processed_rgb = apply_hsl_panel(processed_rgb, adj.hsl);

    let srgb_for_curves = linear_to_srgb(processed_rgb);
    let curved_srgb = apply_all_curves(srgb_for_curves,
        adj.luma_curve, adj.luma_curve_count,
        adj.red_curve, adj.red_curve_count,
        adj.green_curve, adj.green_curve_count,
        adj.blue_curve, adj.blue_curve_count
    );
    processed_rgb = srgb_to_linear(curved_srgb);
    return processed_rgb;
}

fn apply_all_mask_adjustments(initial_rgb: vec3<f32>, adj: MaskAdjustments, coords_i: vec2<i32>) -> vec3<f32> {
    var processed_rgb = initial_rgb;
    processed_rgb = apply_noise_reduction(processed_rgb, coords_i, adj.luma_noise_reduction, adj.color_noise_reduction);
    processed_rgb = apply_white_balance(processed_rgb, adj.temperature, adj.tint);
    processed_rgb = processed_rgb * pow(2.0, adj.exposure);
    processed_rgb = apply_tonal_adjustments(processed_rgb, adj.contrast, adj.highlights, adj.shadows, adj.whites, adj.blacks);
    processed_rgb = apply_dehaze(processed_rgb, adj.dehaze);
    processed_rgb = apply_local_contrast(processed_rgb, coords_i, 2, adj.sharpness);
    processed_rgb = apply_local_contrast(processed_rgb, coords_i, 8, adj.clarity);
    processed_rgb = apply_local_contrast(processed_rgb, coords_i, 20, adj.structure);
    processed_rgb = apply_creative_color(processed_rgb, adj.saturation, adj.vibrance);
    processed_rgb = apply_hsl_panel(processed_rgb, adj.hsl);

    let srgb_for_curves = linear_to_srgb(processed_rgb);
    let curved_srgb = apply_all_curves(srgb_for_curves,
        adj.luma_curve, adj.luma_curve_count,
        adj.red_curve, adj.red_curve_count,
        adj.green_curve, adj.green_curve_count,
        adj.blue_curve, adj.blue_curve_count
    );
    processed_rgb = srgb_to_linear(curved_srgb);
    return processed_rgb;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let in_dims = vec2<u32>(textureDimensions(input_texture));
    if (id.x >= in_dims.x || id.y >= in_dims.y) { return; }

    let original_color = textureLoad(input_texture, id.xy, 0);
    let initial_linear_rgb = srgb_to_linear(original_color.rgb);
    let absolute_coord_i = vec2<i32>(id.xy) + vec2<i32>(i32(adjustments.tile_offset_x), i32(adjustments.tile_offset_y));

    var processed_rgb = apply_all_adjustments(initial_linear_rgb, adjustments.global, absolute_coord_i);

    for (var i = 0u; i < adjustments.mask_count; i = i + 1u) {
        let influence = textureLoad(mask_textures, id.xy, i, 0).r;
        if (influence > 0.001) {
            let mask_adjusted_rgb = apply_all_mask_adjustments(processed_rgb, adjustments.mask_adjustments[i], absolute_coord_i);
            processed_rgb = mix(processed_rgb, mask_adjusted_rgb, influence);
        }
    }

    if (adjustments.global.grain_amount > 0.0) {
        let g = adjustments.global;
        let coord = vec2<f32>(absolute_coord_i);

        let amount = g.grain_amount * 0.5;
        
        let scale = 1.0 / max(g.grain_size, 0.1);
        let roughness = g.grain_roughness;

        let luma = max(0.0, get_luma(processed_rgb));
        let luma_mask = smoothstep(0.0, 0.15, luma) * (1.0 - smoothstep(0.6, 1.0, luma));

        let base_coord = coord * scale;
        let rough_coord = coord * scale * 0.6;

        let noise1 = vec3<f32>(
            gradient_noise(base_coord),
            gradient_noise(base_coord + 11.3),
            gradient_noise(base_coord + 23.7)
        );
        let noise2 = vec3<f32>(
            gradient_noise(rough_coord + 35.1),
            gradient_noise(rough_coord + 43.9),
            gradient_noise(rough_coord + 57.5)
        );
        
        let noise = mix(noise1, noise2, roughness);

        processed_rgb += noise * amount * luma_mask;
    }

    var final_rgb = linear_to_srgb(processed_rgb);

    let g = adjustments.global;
    if (g.vignette_amount != 0.0) {
        let out_coord = vec2<f32>(f32(id.x), f32(id.y));
        let v_amount = g.vignette_amount;
        let v_mid = g.vignette_midpoint;
        let v_round = 1.0 - g.vignette_roundness;
        let v_feather = g.vignette_feather * 0.5;
        let aspect = f32(in_dims.y) / f32(in_dims.x);
        let uv_centered = (out_coord / vec2<f32>(in_dims) - 0.5) * 2.0;
        let uv_round = sign(uv_centered) * pow(abs(uv_centered), vec2<f32>(v_round, v_round));
        let d = length(uv_round * vec2<f32>(1.0, aspect)) * 0.5;
        let vignette_mask = smoothstep(v_mid - v_feather, v_mid + v_feather, d);
        if (v_amount < 0.0) { final_rgb *= (1.0 + v_amount * vignette_mask); } else { final_rgb = mix(final_rgb, vec3<f32>(1.0), v_amount * vignette_mask); }
    }

    textureStore(output_texture, id.xy, vec4<f32>(clamp(final_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), original_color.a));
}