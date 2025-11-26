use image::{DynamicImage, ImageBuffer, Rgba};
use wgpu::util::DeviceExt;
use crate::AlignedBracketFrame;
use crate::image_loader;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct MergeUniforms {
    matrix: [[f32; 4]; 4],
    param_1: f32,
    width: f32,
    height: f32,
    _pad: [u32; 4], // Padding to 16-byte align (total size check needed)
}

// 16-byte aligned uniform structure for the Shader
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct FocusUniforms {
    matrix: [[f32; 4]; 4], // 64 bytes
    param_1: f32,          // 4 bytes (Noise Threshold)
    width: f32,            // 4 bytes
    height: f32,           // 4 bytes
    _pad: f32,             // 4 bytes (Total 80 bytes, multiple of 16)
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
    param_1: f32, 
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

   let create_storage_tex = |label: &str, format: wgpu::TextureFormat| -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };

    let bg_color = create_storage_tex("BG Color", wgpu::TextureFormat::Rgba32Float);
    let bg_weight = create_storage_tex("BG Weight", wgpu::TextureFormat::R32Float);
    let detail_color = create_storage_tex("Detail Color", wgpu::TextureFormat::Rgba32Float);
    let detail_depth = create_storage_tex("Detail Depth", wgpu::TextureFormat::R32Float);

     let input_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Input"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[],
    });

    let output_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Output"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
    });

    // --- 3. ZEROING ---
    let zero_rgba = vec![0u8; (preview_width * preview_height * 16) as usize]; 
    let zero_r = vec![0u8; (preview_width * preview_height * 4) as usize];
    let layout_rgba = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(preview_width * 16), rows_per_image: Some(preview_height) };
    let layout_r = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(preview_width * 4), rows_per_image: Some(preview_height) };

    queue.write_texture(bg_color.as_image_copy(), &zero_rgba, layout_rgba, size);
    queue.write_texture(bg_weight.as_image_copy(), &zero_r, layout_r, size);
    queue.write_texture(detail_color.as_image_copy(), &zero_rgba, layout_rgba, size);
    queue.write_texture(detail_depth.as_image_copy(), &zero_r, layout_r, size);

    // --- 4. PIPELINE SETUP ---
    let merge_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/focus_merge.wgsl"));
    let norm_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/focus_norm.wgsl"));

    // A. MERGE LAYOUT (Uses 4 Storage Textures)
    let merge_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Merge Layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 5, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 6, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });

    // B. NORM LAYOUT (Uses 4 Storage Textures - No Depth, No Input)
    // We re-map indices to be 0..3 to keep it simple in the shader
    let norm_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Norm Layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadOnly, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadOnly, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
        ],
    });

    let merge_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Merge Pipeline"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&merge_layout], push_constant_ranges: &[] })),
        module: &merge_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    let norm_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Norm Pipeline"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&norm_layout], push_constant_ranges: &[] })),
        module: &norm_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge, address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default()
    });

    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Uniforms"), size: std::mem::size_of::<FocusUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
    });

    // Views
    let input_view = input_tex.create_view(&Default::default());
    let bg_color_view = bg_color.create_view(&Default::default());
    let bg_weight_view = bg_weight.create_view(&Default::default());
    let det_color_view = detail_color.create_view(&Default::default());
    let det_depth_view = detail_depth.create_view(&Default::default());
    let out_view = output_tex.create_view(&Default::default());

    let load_sample_size = if preview_width > 2500 { 1.0 } else { 8.0 };

    for (i, frame) in frames.iter().enumerate() {
        // Load & Upload
        let bytes = std::fs::read(&frame.path).map_err(|e| e.to_string())?;
        let img = image_loader::load_base_image_from_bytes(&bytes, &frame.path, false, load_sample_size).map_err(|e| e.to_string())?;
        let rgba = img.resize_exact(preview_width, preview_height, image::imageops::FilterType::Triangle).to_rgba8();

        queue.write_texture(input_tex.as_image_copy(), &rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * preview_width), rows_per_image: None }, size);

        // Uniforms
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

        let u_data = FocusUniforms { matrix: m_final.into(), param_1, width: preview_width as f32, height: preview_height as f32, _pad: 0.0 };
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[u_data]));

        // Bind & Dispatch
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &merge_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bg_color_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&bg_weight_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&det_color_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&det_depth_view) },
                wgpu::BindGroupEntry { binding: 6, resource: uniform_buffer.as_entire_binding() },
            ],
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&merge_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
        }
        queue.submit(Some(encoder.finish()));
    }

    // --- 6. NORMALIZE ---
    let norm_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &norm_layout,
        entries: &[
            // Remapped indices: 0=BG_C, 1=BG_W, 2=Det_C, 3=Out
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&bg_color_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&bg_weight_view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&det_color_view) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&out_view) },
        ],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&norm_pipeline);
        pass.set_bind_group(0, &norm_bind_group, &[]);
        pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
    }
    queue.submit(Some(encoder.finish()));

    // --- 7. READBACK ---
    let padded_bytes = (4 * preview_width + 255) & !255;
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Readback"), size: (padded_bytes * preview_height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &output_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &output_buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_bytes), rows_per_image: Some(preview_height) } },
        size
    );
    queue.submit(Some(encoder.finish()));

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
    for chunk in data.chunks(padded_bytes as usize) {
        pixels.extend_from_slice(&chunk[.. (preview_width * 4) as usize]);
    }

    ImageBuffer::<Rgba<u8>, _>::from_raw(preview_width, preview_height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or("Failed to create image buffer".to_string())
}

pub async fn run_focus_merge(
    frames: &[AlignedBracketFrame],
    preview_width: u32,
    preview_height: u32,
    param_1: f32, 
) -> Result<DynamicImage, String> {
    if frames.is_empty() { return Err("No frames provided".to_string()); }

    // --- 1. WGPU SETUP ---
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default()
    }).await.map_err(|e| format!("No graphics adapter found: {}", e))?;
    
    let mut limits = wgpu::Limits::default();
    limits.max_storage_textures_per_shader_stage = 8;

    let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Focus Device"),
        required_features: wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        memory_hints: wgpu::MemoryHints::Performance,
        required_limits: limits,
        ..Default::default()
    }).await.map_err(|e| e.to_string())?;

    let size = wgpu::Extent3d { width: preview_width, height: preview_height, depth_or_array_layers: 1 };

    // --- 2. TEXTURE ALLOCATION (The 4-Buffer System) ---
    
    // Helper to create storage textures
    let create_storage_tex = |label: &str, format: wgpu::TextureFormat| -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };

    // A. Smooth Background Layer
    let bg_color = create_storage_tex("BG Color", wgpu::TextureFormat::Rgba32Float);
    let bg_weight = create_storage_tex("BG Weight", wgpu::TextureFormat::R32Float);

    // B. Sharp Detail Layer
    let detail_color = create_storage_tex("Detail Color", wgpu::TextureFormat::Rgba32Float);
    let detail_depth = create_storage_tex("Detail Depth", wgpu::TextureFormat::R32Float);

    // C. Input & Output
    let input_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Input Frame"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, // Standard load format
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let output_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Final Output"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // --- 3. ZEROING BUFFERS ---
    // We must clear the accumulators to Black/0.0 before starting.
    // Writing a zero-filled buffer is the most portable way.
    let zero_rgba = vec![0u8; (preview_width * preview_height * 16) as usize]; // RGBA32F
    let zero_r = vec![0u8; (preview_width * preview_height * 4) as usize];    // R32F

    let copy_layout_rgba = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(preview_width * 16), rows_per_image: Some(preview_height) };
    let copy_layout_r = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(preview_width * 4), rows_per_image: Some(preview_height) };

    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &bg_color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &zero_rgba, copy_layout_rgba, size);
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &bg_weight, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &zero_r, copy_layout_r, size);
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &detail_color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &zero_rgba, copy_layout_rgba, size);
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &detail_depth, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &zero_r, copy_layout_r, size);

    // --- 4. PIPELINE SETUP ---
    let merge_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/focus_merge.wgsl"));
    let norm_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/focus_norm.wgsl"));

    // Single Bind Layout covering all bindings used in both shaders
    // 0:In, 1:Samp, 2:BG_C, 3:BG_W, 4:Det_C, 5:Det_D, 6:Uniforms, 7:Out
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Focus Layout"),
        entries: &[
            // 0: Input Texture
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            // 1: Sampler
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            // 2: BG Color (RW)
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            // 3: BG Weight (RW)
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            // 4: Detail Color (RW)
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            // 5: Detail Depth (RW)
            wgpu::BindGroupLayoutEntry { binding: 5, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            // 6: Uniforms
            wgpu::BindGroupLayoutEntry { binding: 6, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            // 7: Output (WriteOnly) - Used in Norm
            wgpu::BindGroupLayoutEntry { binding: 7, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
        ],
    });

    let merge_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Merge Pipeline"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bind_layout], push_constant_ranges: &[] })),
        module: &merge_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    let norm_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Norm Pipeline"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&bind_layout], push_constant_ranges: &[] })),
        module: &norm_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    // Sampler
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    // Uniform Buffer
    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Uniforms"),
        size: std::mem::size_of::<FocusUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Texture Views (Static)
    let input_view_base = input_tex.create_view(&Default::default());
    let bg_color_view = bg_color.create_view(&Default::default());
    let bg_weight_view = bg_weight.create_view(&Default::default());
    let det_color_view = detail_color.create_view(&Default::default());
    let det_depth_view = detail_depth.create_view(&Default::default());
    let out_view = output_tex.create_view(&Default::default());

    // --- 5. MERGE LOOP ---
    let load_sample_size = if preview_width > 2500 { 1.0 } else { 8.0 };

    for (i, frame) in frames.iter().enumerate() {
        // A. Load & Resize
        let bytes = std::fs::read(&frame.path).map_err(|e| e.to_string())?;
        let img = image_loader::load_base_image_from_bytes(&bytes, &frame.path, false, load_sample_size).map_err(|e| e.to_string())?;
        let resized = img.resize_exact(preview_width, preview_height, image::imageops::FilterType::Triangle);
        let rgba = resized.to_rgba8();

        // B. Upload
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &input_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * preview_width), rows_per_image: Some(preview_height) },
            size
        );

        // C. Matrix & Uniforms
        // Standard homography logic: Inverse(Downscale * Transform * Upscale)
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

        let u_data = FocusUniforms {
            matrix: m_final.into(),
            param_1,
            width: preview_width as f32,
            height: preview_height as f32,
            _pad: 0.0,
        };
        queue.write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&[u_data]));

        // D. Bind Group (Recreated per frame? Or create once if input_view is stable)
        // Since we reuse input_tex, we can actually create this once outside. 
        // But for safety (in case wgpu complains about updating active buffer), let's do it here.
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_view_base) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bg_color_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&bg_weight_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&det_color_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&det_depth_view) },
                wgpu::BindGroupEntry { binding: 6, resource: uniform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&out_view) }, // Bound but unused in merge
            ],
        });

        // E. Dispatch Merge
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&merge_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
        }
        queue.submit(Some(encoder.finish()));
    }

    // --- 6. NORMALIZE ---
    // We need a bind group for normalization. It uses the same layout and textures.
    let norm_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Norm Bind"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_view_base) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bg_color_view) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&bg_weight_view) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&det_color_view) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&det_depth_view) },
            wgpu::BindGroupEntry { binding: 6, resource: uniform_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&out_view) },
        ],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&norm_pipeline);
        pass.set_bind_group(0, &norm_bind_group, &[]);
        pass.dispatch_workgroups((preview_width + 15) / 16, (preview_height + 15) / 16, 1);
    }
    queue.submit(Some(encoder.finish()));

    // --- 7. READBACK ---
    let padded_bytes_per_row = (4 * preview_width + 255) & !255; // 256-byte alignment
    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Readback"),
        size: (padded_bytes_per_row * preview_height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &output_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &output_buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_bytes_per_row), rows_per_image: Some(preview_height) } },
        size
    );
    queue.submit(Some(encoder.finish()));

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
    
    // Unpad rows
    for chunk in data.chunks(padded_bytes_per_row as usize) {
        pixels.extend_from_slice(&chunk[.. (preview_width * 4) as usize]);
    }

    let mut final_img = ImageBuffer::<Rgba<u8>, _>::from_raw(preview_width, preview_height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or("Failed to create image buffer".to_string())?;

    // --- 8. AUTO-CROP ---
    let (cx, cy, cw, ch) = calculate_crop_rect(frames, preview_width, preview_height);
    
    // Only crop if we lose less than 20% of the image (sanity check)
    // preventing bugs from creating 1x1 images
    if cw > preview_width / 2 && ch > preview_height / 2 {
        final_img = final_img.crop_imm(cx, cy, cw, ch);
    }

    Ok(final_img)
}

fn calculate_crop_rect(
    frames: &[AlignedBracketFrame], 
    preview_width: u32, 
    preview_height: u32
) -> (u32, u32, u32, u32) { // x, y, w, h
    
    // Start with the full canvas
    let mut min_x = 0.0f32;
    let mut min_y = 0.0f32;
    let mut max_x = preview_width as f32;
    let mut max_y = preview_height as f32;

    for frame in frames {
        // 1. Reconstruct the Matrix for this frame (Same logic as loop)
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
        
        // We want the Forward transform (Image -> Canvas) to see where the corners land
        let m_final = m_down * m_orig * m_up;

        // 2. Transform the 4 corners of the source image
        let corners = [
            nalgebra::Vector4::new(0.0, 0.0, 0.0, 1.0),
            nalgebra::Vector4::new(preview_width as f32, 0.0, 0.0, 1.0),
            nalgebra::Vector4::new(preview_width as f32, preview_height as f32, 0.0, 1.0),
            nalgebra::Vector4::new(0.0, preview_height as f32, 0.0, 1.0),
        ];

        // 3. Find the Bounding Box of this frame on the canvas
        let mut f_min_x = std::f32::MAX;
        let mut f_min_y = std::f32::MAX;
        let mut f_max_x = std::f32::MIN;
        let mut f_max_y = std::f32::MIN;

        for p in corners {
            let warped = m_final * p;
            // Normalize homogenous coordinates
            let x = warped.x / warped.w;
            let y = warped.y / warped.w;

            if x < f_min_x { f_min_x = x; }
            if y < f_min_y { f_min_y = y; }
            if x > f_max_x { f_max_x = x; }
            if y > f_max_y { f_max_y = y; }
        }

        // 4. Shrink the global crop to fit this frame
        // We want the INTERSECTION of all frames. 
        // So we take the MAX of the minimums, and the MIN of the maximums.
        min_x = min_x.max(f_min_x);
        min_y = min_y.max(f_min_y);
        max_x = max_x.min(f_max_x);
        max_y = max_y.min(f_max_y);
    }

    // 5. Safety clamps
    min_x = min_x.max(0.0);
    min_y = min_y.max(0.0);
    max_x = max_x.min(preview_width as f32);
    max_y = max_y.min(preview_height as f32);

    if max_x <= min_x || max_y <= min_y {
        // Fallback if intersection failed
        return (0, 0, preview_width, preview_height);
    }

    (min_x as u32, min_y as u32, (max_x - min_x) as u32, (max_y - min_y) as u32)
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
    // Note: This calls the same function but with full resolution
    let result_img = run_merge_pass(frames, w, h, mode_str, enabled_indices, param).await?;
    result_img.save(output_path).map_err(|e| e.to_string())?;
    Ok(())
}