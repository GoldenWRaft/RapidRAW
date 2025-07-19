use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use image::{ImageReader, ImageBuffer, Rgba};
use std::io::Cursor;
use std::sync::Mutex;
use crate::AppState;
use wgpu::util::DeviceExt;

use crate::GpuContext;

// -- Cube LUT Parser --
#[derive(Debug)]
pub struct CubeLut {
    size: u32,
    data: Vec<f32>, // Stored as [r,g,b, r,g,b, ...]
}

pub fn parse_cube_lut(lut_text: &str) -> Result<CubeLut> {
    let mut size: Option<u32> = None;
    let mut data: Vec<f32> = Vec::new();

    for line in lut_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0] {
            "LUT_3D_SIZE" => {
                size = Some(parts[1].parse()?);
            }
            // Ignore other metadata like DOMAIN_MIN/MAX for this example
            _ => {
                // This must be data line
                if parts.len() == 3 {
                    let r = parts[0].parse::<f32>()?;
                    let g = parts[1].parse::<f32>()?;
                    let b = parts[2].parse::<f32>()?;
                    data.extend_from_slice(&[r, g, b]);
                }
            }
        }
    }

    let size = size.ok_or_else(|| anyhow!("LUT_3D_SIZE not found in .cube file"))?;
    if data.len() != (size * size * size * 3) as usize {
        return Err(anyhow!("LUT data size does not match LUT_3D_SIZE"));
    }
    
    // Convert to RGBA f32 for texture alignment
    let rgba_data = data
        .chunks_exact(3)
        .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 1.0])
        .collect();

    Ok(CubeLut { size, data: rgba_data })
}

#[tauri::command]
pub async fn apply_lut_type_gpu(
    image_data: String,
    lut_data: String,
    lut_type: String,
    app_state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    // Lock the GPU context to get access to the device and queue
    let gpu_context_mutex = &app_state.gpu_context;
    let mut gpu_context_guard = gpu_context_mutex.lock().unwrap();
    
    let context = gpu_context_guard
        .as_mut()
        .ok_or_else(|| "GPU context is not initialized".to_string())?;
    let device = &context.device;
    let queue = &context.queue;

    // ---- 1. Decode Image and LUT from Frontend Data ----
    let image_bytes = general_purpose::STANDARD
        .decode(image_data.split(',').nth(1).unwrap_or(""))
        .map_err(|e| e.to_string())?;
    println!("Here 1");
    let image = ImageReader::new(Cursor::new(&image_bytes))
        .with_guessed_format().map_err(|e| e.to_string())?
        .decode().map_err(|e| e.to_string())?
        .to_rgba8();
    println!("Here 2");
    let (width, height) = image.dimensions();

    // ---- 2. Create GPU Textures ----
    let texture_size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    
    // Input texture for the source image
    let input_texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("Input Image Texture"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &image,
    );
    
    // Output texture where the shader will write results
    let output_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Output Image Texture"),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // ---- 3. Process LUT and Create LUT Texture ----
    let (lut_texture, shader_source, bind_group_layout) = if lut_type == "cube" {
        let cube_lut = parse_cube_lut(&lut_data).map_err(|e| e.to_string())?;
        let lut_size = cube_lut.size;
        
        let lut_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Cube LUT Texture"),
                size: wgpu::Extent3d { width: lut_size, height: lut_size, depth_or_array_layers: lut_size },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&cube_lut.data),
        );
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Cube LUT Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { // Input Image
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { // Output Image
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { // 3D LUT
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { // Sampler for LUT
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        (lut_texture, include_str!("../../shaders/cube_lut.wgsl"), layout)
    } else { // "hald"
        let lut_bytes = general_purpose::STANDARD.decode(lut_data.split(',').nth(1).unwrap_or("")).map_err(|e| e.to_string())?;
        let lut_image = ImageReader::new(Cursor::new(&lut_bytes)).with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?.to_rgba8();
        let (lut_width, lut_height) = lut_image.dimensions();

        let lut_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Hald CLUT Texture"),
                size: wgpu::Extent3d { width: lut_width, height: lut_height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &lut_image,
        );

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Hald CLUT Bind Group Layout"),
            entries: &[
                // Bindings 0, 1, and 3 are identical to the Cube LUT
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false, }, count: None, },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2, }, count: None, },
                wgpu::BindGroupLayoutEntry { // 2D Hald LUT
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None, },
            ],
        });
        (lut_texture, include_str!("../../shaders/hald_c_lut.wgsl"), layout)
    };

    // ---- 4. Setup Compute Pipeline and Bind Group ----
    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("LUT Shader Module"),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("LUT Pipeline Layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("LUT Compute Pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader_module,
        entry_point: "main",
    });

    let lut_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("LUT Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("LUT Bind Group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_texture.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output_texture.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lut_texture.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&lut_sampler) },
        ],
    });

    // ---- 5. Dispatch Compute Job ----
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("LUT Command Encoder") });
    {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("LUT Compute Pass"), timestamp_writes: None });
        compute_pass.set_pipeline(&compute_pipeline);
        compute_pass.set_bind_group(0, &bind_group, &[]);
        let workgroup_count_x = (width + 7) / 8; // Ceil division
        let workgroup_count_y = (height + 7) / 8; // Ceil division
        compute_pass.dispatch_workgroups(workgroup_count_x, workgroup_count_y, 1);
    }
    
    // ---- 6. Read Data Back from GPU (With Alignment Correction) ----
    let pixel_size = std::mem::size_of::<u32>() as u32; // Rgba8 is 4 bytes

    // Calculate the unpadded size of a single row.
    let unpadded_bytes_per_row = pixel_size * width;
    
    // The required alignment for buffer copies
    let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

    // Calculate the padded bytes per row by rounding up to the next multiple of the alignment.
    let padded_bytes_per_row = (unpadded_bytes_per_row + alignment - 1) & !(alignment - 1);
    
    // The size of the output buffer must use the padded size.
    let output_buffer_size = (padded_bytes_per_row * height) as wgpu::BufferAddress;

    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("LUT Output Buffer"),
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &output_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                // Use the PADDED bytes_per_row value here.
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        texture_size,
    );
    
    queue.submit(Some(encoder.finish()));

    // ---- 7. Map Buffer, Remove Padding, and Encode Result ----
    let buffer_slice = output_buffer.slice(..);
    buffer_slice.map_async(wgpu::MapMode::Read, |result| {
        if let Err(e) = result {
            // It's good practice to log or handle the error
            eprintln!("Failed to map buffer: {:?}", e);
        }
    });
    
    // Block until the GPU is done and the buffer is ready.
    device.poll(wgpu::Maintain::Wait);

    let padded_data = buffer_slice.get_mapped_range();

    // Create a new, clean vector to hold the final image data without padding.
    let mut final_data = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    
    // Iterate over each row of the padded data.
    for chunk in padded_data.chunks(padded_bytes_per_row as usize) {
        // For each chunk (a full padded row), take only the part that contains actual image data.
        final_data.extend_from_slice(&chunk[..unpadded_bytes_per_row as usize]);
    }
    
    // Drop the mapped range to unmap the buffer. This must be done before the buffer goes out of scope.
    drop(padded_data);
    output_buffer.unmap();
    
    // Now create the image from the clean, unpadded data.
    let result_image: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, final_data)
        .ok_or_else(|| "Failed to create image from buffer data".to_string())?;

    let mut png_bytes: Vec<u8> = Vec::new();
    result_image.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    let base64_img = format!("data:image/png;base64,{}", general_purpose::STANDARD.encode(&png_bytes));
    
    Ok(base64_img)
}
