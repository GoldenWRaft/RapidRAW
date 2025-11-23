// use image::{DynamicImage, ImageBuffer, Rgba};
// use wgpu::util::DeviceExt;
// use crate::AlignedBracketFrame;
// use crate::image_loader; 
// use crate::formats::is_raw_file;

// #[repr(C)]
// #[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
// struct AlignmentUniforms {
//     matrices: [[[f32; 4]; 4]; 8],
//     visibility_1: [f32; 4], 
//     visibility_2: [f32; 4],
//     num_frames: u32,
//     algo_mode: u32, // New Field
//     param_1: f32,
//     input_gamma: f32,
// }

// pub async fn run_merge_pass(
//     frames: &[AlignedBracketFrame],
//     preview_width: u32,
//     preview_height: u32,
//     mode_str: &str,
//     enabled_indices: &[bool],
//     parameter: f32, 
// ) -> Result<DynamicImage, String> {
//     if frames.is_empty() {
//         return Err("No frames provided".to_string());
//     }

//     // --- 1. SETUP WGPU (v27.0) ---
//     let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
//     let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
//         power_preference: wgpu::PowerPreference::HighPerformance,
//         compatible_surface: None,
//         force_fallback_adapter: false,
//     }).await.map_err(|e| format!("No graphics adapter found: {}", e))?;

//     let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
//         label: Some("Merge Device"),
//         required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
//         required_limits: wgpu::Limits::default(),
//         memory_hints: wgpu::MemoryHints::Performance,
//         ..Default::default()
//     }).await.map_err(|e| e.to_string())?;

//     // --- 2. LOAD IMAGES ---
//     let mut loaded_buffers: Vec<Vec<u8>> = Vec::new();
//     let mut original_dims = (0, 0);

//     let load_sample_size = if preview_width > 2500 { 1.0 } else { 8.0 };

//     for (i, frame) in frames.iter().enumerate() {
//         let bytes = std::fs::read(&frame.path)
//             .map_err(|e| format!("Failed to read file: {}", e))?;
        
//         let img = image_loader::load_base_image_from_bytes(
//             &bytes, 
//             &frame.path, 
//             false, 
//             load_sample_size
//         ).map_err(|e| format!("Failed to decode {}: {}", frame.path, e))?;

//         if i == 0 {
//             // Use dimensions from struct to ensure alignment matches logic
//             original_dims = (frame.orig_w, frame.orig_h);
//         }
        
//         let resized = img.resize_exact(preview_width, preview_height, image::imageops::FilterType::Triangle);
//         loaded_buffers.push(resized.to_rgba8().into_raw());
//     }

//     // --- 3. AUTO-DETECT GAMMA ---
//     // If Raw, use Gamma 2.2 (brighten linear data). If JPEG, use 1.0 (keep as is).
//     let is_raw = is_raw_file(&frames[0].path);
//     let input_gamma = if is_raw { 1.0 } else { 2.2 };

//     // --- 4. ADJUST MATRICES ---
//     let scale_x = preview_width as f32 / original_dims.0 as f32;
//     let scale_y = preview_height as f32 / original_dims.1 as f32;

//     let m_up = nalgebra::Matrix4::new_nonuniform_scaling(&nalgebra::Vector3::new(1.0/scale_x, 1.0/scale_y, 1.0));
//     let m_down = nalgebra::Matrix4::new_nonuniform_scaling(&nalgebra::Vector3::new(scale_x, scale_y, 1.0));

//     let mut final_matrices = [[[0.0; 4]; 4]; 8];

//     for (i, frame) in frames.iter().enumerate() {
//         if i >= 8 { break; }
//         let m_arr = frame.transform;
        
//         // Row-Major construction (Fixed orientation)
//         let m_orig = nalgebra::Matrix4::new(
//             m_arr[0][0], m_arr[0][1], m_arr[0][2], m_arr[0][3], 
//             m_arr[1][0], m_arr[1][1], m_arr[1][2], m_arr[1][3], 
//             m_arr[2][0], m_arr[2][1], m_arr[2][2], m_arr[2][3], 
//             m_arr[3][0], m_arr[3][1], m_arr[3][2], m_arr[3][3], 
//         );

//         let m_scaled = m_down * m_orig * m_up;
//         // Invert for shader backward-lookup
//         let m_final = m_scaled.try_inverse().unwrap_or_else(nalgebra::Matrix4::identity);

//         final_matrices[i] = m_final.into(); 
//     }

//     // --- 5. TEXTURE ARRAY ---
//     let size = wgpu::Extent3d { width: preview_width, height: preview_height, depth_or_array_layers: frames.len() as u32 };
    
//     let input_texture = device.create_texture(&wgpu::TextureDescriptor {
//         label: Some("Input Array"),
//         size,
//         mip_level_count: 1,
//         sample_count: 1,
//         dimension: wgpu::TextureDimension::D2,
//         format: wgpu::TextureFormat::Rgba8Unorm,
//         usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
//         view_formats: &[],
//     });

//     for (i, data) in loaded_buffers.iter().enumerate() {
//         queue.write_texture(
//             wgpu::TexelCopyTextureInfo { 
//                 texture: &input_texture,
//                 mip_level: 0,
//                 origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
//                 aspect: wgpu::TextureAspect::All,
//             },
//             data,
//             wgpu::TexelCopyBufferLayout { 
//                 offset: 0,
//                 bytes_per_row: Some(4 * preview_width),
//                 rows_per_image: Some(preview_height),
//             },
//             wgpu::Extent3d { width: preview_width, height: preview_height, depth_or_array_layers: 1 },
//         );
//     }

//     // --- 6. OUTPUT TEXTURE ---
//     let output_texture = device.create_texture(&wgpu::TextureDescriptor {
//         label: Some("Output"),
//         size: wgpu::Extent3d { width: preview_width, height: preview_height, depth_or_array_layers: 1 },
//         mip_level_count: 1,
//         sample_count: 1,
//         dimension: wgpu::TextureDimension::D2,
//         format: wgpu::TextureFormat::Rgba8Unorm,
//         usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
//         view_formats: &[],
//     });

//     // --- 7. UNIFORMS & PIPELINE ---
//     let mut v1 = [0.0f32; 4];
//     let mut v2 = [0.0f32; 4];

//     for (i, &enabled) in enabled_indices.iter().enumerate() {
//         let val = if enabled { 1.0 } else { 0.0 };
//         if i < 4 { v1[i] = val; } else if i < 8 { v2[i - 4] = val; }
//     }

//     let algo_mode = if mode_str == "focus" { 1 } else { 0 };

//     let uniforms = AlignmentUniforms { 
//         matrices: final_matrices,  
//         visibility_1: v1,
//         visibility_2: v2,
//         num_frames: frames.len() as u32, 
//         algo_mode, 
//         param_1: parameter,
//         input_gamma, // Pass detection result
//     };

//     let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
//         label: Some("Uniforms"),
//         contents: bytemuck::cast_slice(&[uniforms]),
//         usage: wgpu::BufferUsages::UNIFORM,
//     });

//     let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/bracketing.wgsl"));
    
//     let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
//         label: None,
//         entries: &[
//             wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2Array, sample_type: wgpu::TextureSampleType::Float { filterable: false } }, count: None },
//             wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
//             wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
//         ],
//     });

//     let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
//         label: None,
//         layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bind_group_layout], push_constant_ranges: &[] })),
//         module: &shader,
//         entry_point: Some("main"),
//         compilation_options: Default::default(),
//         cache: None
//     });

//     let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
//         label: None,
//         layout: &bind_group_layout,
//         entries: &[
//             wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })) },
//             wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output_texture.create_view(&Default::default())) },
//             wgpu::BindGroupEntry { binding: 2, resource: uniform_buf.as_entire_binding() },
//         ],
//     });

//     // --- 8. EXECUTE ---
//     let mut encoder = device.create_command_encoder(&Default::default());
//     {
//         let mut pass = encoder.begin_compute_pass(&Default::default());
//         pass.set_pipeline(&pipeline);
//         pass.set_bind_group(0, &bind_group, &[]);
//         pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
//     }

//     // --- 9. READBACK ---
//     let unpadded = preview_width * 4;
//     let align = 256;
//     let padding = (align - unpadded % align) % align;
//     let padded = unpadded + padding;

//     let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
//         label: Some("Readback"),
//         size: (padded * preview_height) as u64,
//         usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
//         mapped_at_creation: false,
//     });

//     encoder.copy_texture_to_buffer(
//         wgpu::TexelCopyTextureInfo { 
//             texture: &output_texture, 
//             mip_level: 0, 
//             origin: wgpu::Origin3d::ZERO, 
//             aspect: wgpu::TextureAspect::All 
//         },
//         wgpu::TexelCopyBufferInfo { 
//             buffer: &output_buffer, 
//             layout: wgpu::TexelCopyBufferLayout { 
//                 offset: 0, 
//                 bytes_per_row: Some(padded), 
//                 rows_per_image: Some(preview_height) 
//             } 
//         },
//         wgpu::Extent3d { width: preview_width, height: preview_height, depth_or_array_layers: 1 },
//     );

//     queue.submit(Some(encoder.finish()));

//     let slice = output_buffer.slice(..);
//     let (tx, rx) = std::sync::mpsc::channel();
//     slice.map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    
//     device
//         .poll(wgpu::PollType::Wait {
//             submission_index: None,
//             timeout: Some(std::time::Duration::from_secs(60)),
//         })
//         .unwrap();

//     rx.recv().unwrap().map_err(|e| e.to_string())?;

//     let data = slice.get_mapped_range();
//     let mut pixels: Vec<u8> = Vec::with_capacity((preview_width * preview_height * 4) as usize);
//     for chunk in data.chunks(padded as usize) {
//         pixels.extend_from_slice(&chunk[..unpadded as usize]);
//     }

//     ImageBuffer::<Rgba<u8>, _>::from_raw(preview_width, preview_height, pixels)
//         .map(DynamicImage::ImageRgba8)
//         .ok_or("Failed to create image buffer".to_string())
// }

// pub async fn save_high_res_merge(
//     frames: &[AlignedBracketFrame],
//     mode_str: &str,
//     enabled_indices: &[bool],
//     output_path: &str,
//     param: f32
// ) -> Result<(), String> {
//     if frames.is_empty() { return Err("No frames".to_string()); }

//     // 1. Get Full Dimensions from first image
//     let w = frames[0].orig_w;
//     let h = frames[0].orig_h;

//     println!("Starting High-Res Merge: {}x{}", w, h);

//     // 2. Run the existing pipeline (it handles resizing internally if we pass w, h)
//     // Note: Ensure run_merge_pass loads FULL images if w/h matches original.
//     // You might need to tweak run_merge_pass to NOT downscale loading if (w,h) are huge.
    
//     let result_img = run_merge_pass(frames, w, h, mode_str, enabled_indices, param).await?;

//     // 3. Save to Disk
//     // Suggestion: Save as 16-bit TIFF if possible for editing, or High-Quality JPG
//     result_img.save(output_path).map_err(|e| e.to_string())?;

//     Ok(())
// }

use image::{DynamicImage, ImageBuffer, Rgba};
use wgpu::util::DeviceExt;
use crate::AlignedBracketFrame;
use crate::image_loader;
use crate::formats; 

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct AccumulationUniforms {
    matrix: [[f32; 4]; 4],
    algo_mode: u32,
    param_1: f32,
    input_gamma: f32,
    _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct NormalizationUniforms {
    algo_mode: u32,
}

pub async fn run_merge_pass(
    frames: &[AlignedBracketFrame],
    preview_width: u32,
    preview_height: u32,
    mode_str: &str,
    enabled_indices: &[bool],
    parameter: f32, 
) -> Result<DynamicImage, String> {
    if frames.is_empty() { return Err("No frames provided".to_string()); }

    // --- 1. SETUP WGPU ---
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default()
    }).await.map_err(|e| format!("No graphics adapter found: {}", e))?;

    let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Merge Device"),
        required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        ..Default::default()
    }).await.map_err(|e| e.to_string())?;

    let algo_mode = if mode_str == "focus" { 1 } else { 0 };

    // --- 2. CREATE TEXTURES ---
    let size = wgpu::Extent3d { width: preview_width, height: preview_height, depth_or_array_layers: 1 };

    // Accumulators (32-bit Float for precision)
    let accum_desc = wgpu::TextureDescriptor {
        label: Some("Accumulator"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };

    // Create two descriptors: one for color (RGBA32F) and one for weight (R32F)
    let accum_color_desc = accum_desc.clone();
    let mut accum_weight_desc = accum_desc.clone();
    accum_weight_desc.format = wgpu::TextureFormat::R32Float;

    let accum_color = device.create_texture(&accum_color_desc);
    let accum_weight = device.create_texture(&accum_weight_desc);

    // Input (Single Frame)
    let input_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Input Frame"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // Output (Final)
    let output_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Output"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // --- 3. INITIALIZE ACCUMULATORS TO ZERO ---
    // We can do this by writing a zero buffer
    let zero_buffer_color = vec![0u8; (preview_width * preview_height * 16) as usize]; // 16 bytes per pixel (RGBA32F)
    let zero_buffer_weight = vec![0u8; (preview_width * preview_height * 4) as usize]; // 4 bytes per pixel (R32F)
    
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &accum_color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &zero_buffer_color,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(preview_width * 16), rows_per_image: Some(preview_height) },
        size
    );
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &accum_weight, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &zero_buffer_weight,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(preview_width * 4), rows_per_image: Some(preview_height) },
        size
    );

    // --- 4. PREPARE PIPELINES ---
    let accum_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/bracketing.wgsl"));
    let norm_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/normalization.wgsl"));

    // -- Accumulation Pipeline --
    // Note: We need a Buffer for Uniforms that we can update per frame
    let uniform_size = std::mem::size_of::<AccumulationUniforms>() as u64;
    let accum_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Accum Uniforms"),
        size: uniform_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let accum_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { multisampled: false, view_dimension: wgpu::TextureViewDimension::D2, sample_type: wgpu::TextureSampleType::Float { filterable: false } }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });

    let accum_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Accumulation"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&accum_bind_layout], push_constant_ranges: &[] })),
        module: &accum_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    let accum_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &accum_bind_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_texture.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&accum_color.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&accum_weight.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 3, resource: accum_uniform_buffer.as_entire_binding() },
        ],
    });

    // --- 5. STREAMING LOOP ---
    // Determine global sizing info from first frame
    // (Assuming all frames match first frame in original size, which is standard for bracketing)
    // If not, we need to look up each frame's orig_w/h separately.
    let load_sample_size = if preview_width > 2500 { 1.0 } else { 8.0 };

    for (i, frame) in frames.iter().enumerate() {
        // Skip if disabled
        if i < enabled_indices.len() && !enabled_indices[i] { continue; }

        // A. Load Image (CPU)
        let bytes = std::fs::read(&frame.path).map_err(|e| e.to_string())?;
        let img = image_loader::load_base_image_from_bytes(&bytes, &frame.path, false, load_sample_size).map_err(|e| e.to_string())?;
        let resized = img.resize_exact(preview_width, preview_height, image::imageops::FilterType::Triangle);
        let rgba = resized.to_rgba8();

        // B. Upload to GPU (Reuse input_texture)
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &input_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * preview_width), rows_per_image: Some(preview_height) },
            size
        );

        // C. Matrix Calculation
        let scale_x = preview_width as f32 / frame.orig_w as f32;
        let scale_y = preview_height as f32 / frame.orig_h as f32;
        let m_up = nalgebra::Matrix4::new_nonuniform_scaling(&nalgebra::Vector3::new(1.0/scale_x, 1.0/scale_y, 1.0));
        let m_down = nalgebra::Matrix4::new_nonuniform_scaling(&nalgebra::Vector3::new(scale_x, scale_y, 1.0));
        
        let m_arr = frame.transform;
        let m_orig = nalgebra::Matrix4::new(
            m_arr[0][0], m_arr[0][1], m_arr[0][2], m_arr[0][3], 
            m_arr[1][0], m_arr[1][1], m_arr[1][2], m_arr[1][3], 
            m_arr[2][0], m_arr[2][1], m_arr[2][2], m_arr[2][3], 
            m_arr[3][0], m_arr[3][1], m_arr[3][2], m_arr[3][3], 
        );
        let m_final = (m_down * m_orig * m_up).try_inverse().unwrap_or(nalgebra::Matrix4::identity());

        // D. Update Uniforms
        let is_raw = formats::is_raw_file(&frame.path);
        let input_gamma = if is_raw { 1.0 / 2.2 } else { 1.0 };

        let u_data = AccumulationUniforms {
            matrix: m_final.into(),
            algo_mode,
            param_1: parameter,
            input_gamma,
            _pad: 0,
        };
        queue.write_buffer(&accum_uniform_buffer, 0, bytemuck::cast_slice(&[u_data]));

        // E. Dispatch
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&accum_pipeline);
            pass.set_bind_group(0, &accum_bind_group, &[]);
            pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
        }
        queue.submit(Some(encoder.finish()));
    }

    // --- 6. NORMALIZATION PASS ---
    let norm_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Norm Uniforms"),
        contents: bytemuck::cast_slice(&[NormalizationUniforms { algo_mode }]),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let norm_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadOnly, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });

    let norm_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Normalization"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&norm_bind_layout], push_constant_ranges: &[] })),
        module: &norm_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    let norm_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &norm_bind_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&accum_color.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&accum_weight.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&output_texture.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 3, resource: norm_uniform_buffer.as_entire_binding() },
        ],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&norm_pipeline);
        pass.set_bind_group(0, &norm_bind_group, &[]);
        pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
    }

    // --- 7. READBACK ---
    let unpadded = preview_width * 4;
    let align = 256;
    let padding = (align - unpadded % align) % align;
    let padded = unpadded + padding;

    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Readback"),
        size: (padded * preview_height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &output_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &output_buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(preview_height) } },
        size
    );

    let submission_index = queue.submit(Some(encoder.finish()));

    let slice = output_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    rx.recv().unwrap().map_err(|e| e.to_string())?;

    let data = slice.get_mapped_range();
    let mut pixels: Vec<u8> = Vec::with_capacity((preview_width * preview_height * 4) as usize);
    for chunk in data.chunks(padded as usize) {
        pixels.extend_from_slice(&chunk[..unpadded as usize]);
    }

    ImageBuffer::<Rgba<u8>, _>::from_raw(preview_width, preview_height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or("Failed to create image buffer".to_string())
}

pub async fn save_high_res_merge(
    frames: &[AlignedBracketFrame],
    mode_str: &str,
    enabled_indices: &[bool],
    output_path: &str,
    param: f32
) -> Result<(), String> {
    if frames.is_empty() { return Err("No frames".to_string()); }

    let w = frames[0].orig_w;
    let h = frames[0].orig_h;

    println!("Starting High-Res Merge: {}x{}", w, h);
    let result_img = run_merge_pass(frames, w, h, mode_str, enabled_indices, param).await?;
    result_img.save(output_path).map_err(|e| e.to_string())?;
    Ok(())
}