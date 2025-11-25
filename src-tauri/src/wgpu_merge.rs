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

    // Accumulators (Float32)
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

    let accum_color = device.create_texture(&accum_desc);
    
    // Weight Buffer (R32Float)
    let mut weight_desc = accum_desc.clone();
    weight_desc.format = wgpu::TextureFormat::R32Float;
    let accum_weight = device.create_texture(&weight_desc);

    // Input Texture (Reusable, RGBA8, converted to f32 in shader)
    let input_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Input Frame"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, // Standard loading format
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    
    // Output Texture (For Normalization result)
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

    // --- 3. CREATE SAMPLER (NEW) ---
    // This enables the smooth bilinear filtering
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Bilinear Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    // --- 4. CLEAR ACCUMULATORS ---
    // (Explicitly zeroing to avoid garbage data)
    let zero_buffer_color = vec![0u8; (preview_width * preview_height * 16) as usize];
    let zero_buffer_weight = vec![0u8; (preview_width * preview_height * 4) as usize];
    
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

    // --- 5. PIPELINE SETUP ---
    let shader_source = if mode_str == "focus" {
        wgpu::include_wgsl!("shaders/focus_merge.wgsl")
    } else {
        wgpu::include_wgsl!("shaders/hdr_merge.wgsl")
    };
    let accum_shader = device.create_shader_module(shader_source);
    let norm_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/normalization.wgsl"));

    // Accumulation Bind Layout
    let accum_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Accum Layout"),
        entries: &[
            // 0: Input Texture
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            // 1: Sampler
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            // 2: Accum Color
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::Rgba32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            // 3: Accum Weight
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::ReadWrite, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            // 4: Uniforms
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });

    let accum_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Accum Pipeline"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&accum_bind_layout], push_constant_ranges: &[] })),
        module: &accum_shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None
    });

    // Uniform Buffer (Created once, updated per frame)
    let uniform_size = std::mem::size_of::<MergeUniforms>() as u64;
    let accum_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Accum Uniforms"),
        size: uniform_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Create View & BindGroup (Can be done once since we reuse input_texture)
    let input_view = input_texture.create_view(&Default::default());
    let accum_color_view = accum_color.create_view(&Default::default());
    let accum_weight_view = accum_weight.create_view(&Default::default());

    let accum_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Accum Bind Group"),
        layout: &accum_bind_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&accum_color_view) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&accum_weight_view) },
            wgpu::BindGroupEntry { binding: 4, resource: accum_uniform_buffer.as_entire_binding() },
        ],
    });

    // --- 6. STREAMING EXECUTION ---
    let load_sample_size = if preview_width > 2500 { 1.0 } else { 8.0 };

    for (i, frame) in frames.iter().enumerate() {
        if i < enabled_indices.len() && !enabled_indices[i] { continue; }

        // A. Load & Resize (CPU)
        let bytes = std::fs::read(&frame.path).map_err(|e| e.to_string())?;
        let img = image_loader::load_base_image_from_bytes(&bytes, &frame.path, false, load_sample_size).map_err(|e| e.to_string())?;
        let resized = img.resize_exact(preview_width, preview_height, image::imageops::FilterType::Triangle);
        let rgba = resized.to_rgba8();

        // B. Upload to GPU
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &input_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * preview_width), rows_per_image: Some(preview_height) },
            size
        );

        // C. Calculate Matrix & Gamma
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
        let u_data = MergeUniforms {
            matrix: m_final.into(),
            param_1: parameter,
            width: preview_width as f32,
            height: preview_height as f32,
            _pad: [0; 4],
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
        
        // F. Submit Immediately (Pipelining)
        queue.submit(Some(encoder.finish()));
    }

    // --- 7. NORMALIZATION PASS ---
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
        label: Some("Norm Pipeline"),
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
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&accum_color_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&accum_weight_view) },
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
    queue.submit(Some(encoder.finish()));

    // --- 8. READBACK ---
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

    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &output_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &output_buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(preview_height) } },
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
    // Note: This calls the same function but with full resolution
    let result_img = run_merge_pass(frames, w, h, mode_str, enabled_indices, param).await?;
    result_img.save(output_path).map_err(|e| e.to_string())?;
    Ok(())
}