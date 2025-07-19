use std::sync::Arc;

use bytemuck;
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, Luma};
use wgpu::util::{DeviceExt, TextureDataOrder};

use crate::AppState;
use crate::image_processing::{AllAdjustments, GpuContext};

pub fn get_or_init_gpu_context(state: &tauri::State<AppState>) -> Result<GpuContext, String> {
    let mut context_lock = state.gpu_context.lock().unwrap();
    if let Some(context) = &*context_lock {
        return Ok(context.clone());
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .ok_or("Failed to find a wgpu adapter.")?;

    let mut required_features = wgpu::Features::TEXTURE_BINDING_ARRAY;
    if adapter.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
        required_features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    }

    let limits = adapter.limits();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("Processing Device"),
            required_features,
            required_limits: limits.clone(),
        },
        None,
    )).map_err(|e| e.to_string())?;

    let new_context = GpuContext {
        device: Arc::new(device),
        queue: Arc::new(queue),
        limits,
    };
    *context_lock = Some(new_context.clone());
    Ok(new_context)
}

fn read_texture_data(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: wgpu::Extent3d,
) -> Result<Vec<u8>, String> {
    let unpadded_bytes_per_row = 4 * size.width;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) & !(align - 1);
    let output_buffer_size = (padded_bytes_per_row * size.height) as u64;

    let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Readback Buffer"),
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Readback Encoder") });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(padded_bytes_per_row), rows_per_image: Some(size.height) },
        },
        size,
    );

    queue.submit(Some(encoder.finish()));
    let buffer_slice = output_buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |result| { tx.send(result).unwrap(); });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().map_err(|e| e.to_string())?;

    let padded_data = buffer_slice.get_mapped_range().to_vec();
    output_buffer.unmap();

    if padded_bytes_per_row == unpadded_bytes_per_row {
        Ok(padded_data)
    } else {
        let mut unpadded_data = Vec::with_capacity((unpadded_bytes_per_row * size.height) as usize);
        for chunk in padded_data.chunks(padded_bytes_per_row as usize) {
            unpadded_data.extend_from_slice(&chunk[..unpadded_bytes_per_row as usize]);
        }
        Ok(unpadded_data)
    }
}

pub fn run_gpu_processing(
    context: &GpuContext,
    image: &DynamicImage,
    adjustments: AllAdjustments,
    mask_bitmaps: &[ImageBuffer<Luma<u8>, Vec<u8>>],
) -> Result<Vec<u8>, String> {
    let device = &context.device;
    let queue = &context.queue;
    let (width, height) = image.dimensions();
    let max_dim = context.limits.max_texture_dimension_2d;

    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Image Processing Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/shader.wgsl").into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Bind Group Layout"),
        entries: &[
            // Input Image
            wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2, multisampled: false,
                }, count: None,
            },
            // Output Image
            wgpu::BindGroupLayoutEntry {
                binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    view_dimension: wgpu::TextureViewDimension::D2,
                }, count: None,
            },
            // Adjustments Uniform
            wgpu::BindGroupLayoutEntry {
                binding: 2, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                }, count: None,
            },
            // Mask Texture Array
            wgpu::BindGroupLayoutEntry {
                binding: 3, visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Pipeline Layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Compute Pipeline"), layout: Some(&pipeline_layout),
        module: &shader_module, entry_point: "main",
    });

    let num_masks = mask_bitmaps.len();
    // Create the texture once. It's cheap and can be reused to create views.
    let empty_mask_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Empty Mask Texture"),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    if width <= max_dim && height <= max_dim {
        let img_rgba = image.to_rgba8();
        let texture_size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };

        let adjustments_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Adjustments Buffer"),
            contents: bytemuck::bytes_of(&adjustments),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let input_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Input Texture"), size: texture_size, mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[],
            },
            TextureDataOrder::MipMajor, &img_rgba,
        );

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Output Texture"), size: texture_size, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });

        let mask_texture_array_view = if num_masks > 0 {
            let mask_texture_array = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Mask Texture Array"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: num_masks as u32 },
                mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            for (i, mask_bitmap) in mask_bitmaps.iter().enumerate() {
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &mask_texture_array,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    mask_bitmap,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(width),
                        rows_per_image: Some(height),
                    },
                    texture_size,
                );
            }
            mask_texture_array.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        } else {
            // Create a new view from the empty texture.
            empty_mask_texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Single Texture Bind Group"), layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_texture.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output_texture.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: adjustments_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&mask_texture_array_view) },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            compute_pass.set_pipeline(&compute_pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups((width + 7) / 8, (height + 7) / 8, 1);
        }

        queue.submit(Some(encoder.finish()));
        return read_texture_data(device, queue, &output_texture, texture_size);
    }

    // Tiling logic for very large images
    let tile_size = (max_dim / 2).min(2048);
    let img_rgba = image.to_rgba8();
    let mut final_pixels = vec![0u8; (width * height * 4) as usize];

    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;

    let raw_buffer = img_rgba.as_raw();

    for tile_y in 0..tiles_y {
        for tile_x in 0..tiles_x {
            let x_start = tile_x * tile_size;
            let y_start = tile_y * tile_size;
            let x_end = (x_start + tile_size).min(width);
            let y_end = (y_start + tile_size).min(height);

            let tile_width = x_end - x_start;
            let tile_height = y_end - y_start;

            let mut tile_pixels = Vec::with_capacity((tile_width * tile_height * 4) as usize);

            for y in y_start..y_end {
                let pixel_row_start = (y * width + x_start) as usize * 4;
                let pixel_row_end = pixel_row_start + (tile_width as usize * 4);
                tile_pixels.extend_from_slice(&raw_buffer[pixel_row_start..pixel_row_end]);
            }

            let texture_size = wgpu::Extent3d { width: tile_width, height: tile_height, depth_or_array_layers: 1 };

            let mut tile_adjustments = adjustments;
            tile_adjustments.tile_offset_x = x_start;
            tile_adjustments.tile_offset_y = y_start;

            let adjustments_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Tile Adjustments Buffer"),
                contents: bytemuck::bytes_of(&tile_adjustments),
                usage: wgpu::BufferUsages::UNIFORM,
            });

            let input_texture = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("Input Tile Texture"), size: texture_size, mip_level_count: 1, sample_count: 1,
                    dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[],
                },
                TextureDataOrder::MipMajor, &tile_pixels,
            );

            let output_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Output Tile Texture"), size: texture_size, mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
            });

            // Create a texture array with cropped versions of the masks for this tile
            let mask_texture_array_view = if num_masks > 0 {
                let mask_texture_array = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Tile Mask Texture Array"),
                    size: wgpu::Extent3d { width: tile_width, height: tile_height, depth_or_array_layers: num_masks as u32 },
                    mip_level_count: 1, sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                for (i, full_mask_bitmap) in mask_bitmaps.iter().enumerate() {
                    let cropped_mask = image::imageops::crop_imm(full_mask_bitmap, x_start, y_start, tile_width, tile_height).to_image();
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &mask_texture_array,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &cropped_mask,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(tile_width),
                            rows_per_image: Some(tile_height),
                        },
                        texture_size,
                    );
                }
                mask_texture_array.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                })
            } else {
                // Create a new view from the empty texture for this tile.
                empty_mask_texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2Array),
                    ..Default::default()
                })
            };

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Tile Bind Group"), layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&input_texture.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&output_texture.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 2, resource: adjustments_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&mask_texture_array_view) },
                ],
            });

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("Tile Encoder") });
            {
                let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                compute_pass.set_pipeline(&compute_pipeline);
                compute_pass.set_bind_group(0, &bind_group, &[]);
                compute_pass.dispatch_workgroups((tile_width + 7) / 8, (tile_height + 7) / 8, 1);
            }
            queue.submit(Some(encoder.finish()));

            let processed_tile_data = read_texture_data(device, queue, &output_texture, texture_size)?;

            for row in 0..tile_height {
                let final_y = y_start + row;
                let final_row_offset = (final_y * width + x_start) as usize * 4;
                let tile_row_offset = (row * tile_width) as usize * 4;
                let copy_bytes = (tile_width * 4) as usize;

                final_pixels[final_row_offset..final_row_offset + copy_bytes]
                    .copy_from_slice(&processed_tile_data[tile_row_offset..tile_row_offset + copy_bytes]);
            }
        }
    }

    Ok(final_pixels)
}

pub fn process_and_get_dynamic_image(
    context: &GpuContext,
    base_image: &DynamicImage,
    all_adjustments: AllAdjustments,
    mask_bitmaps: &[ImageBuffer<Luma<u8>, Vec<u8>>],
) -> Result<DynamicImage, String> {
    let processed_pixels = run_gpu_processing(context, base_image, all_adjustments, mask_bitmaps)?;
    let (width, height) = base_image.dimensions();
    let img_buf = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, processed_pixels)
        .ok_or("Failed to create image buffer from GPU data")?;
    Ok(DynamicImage::ImageRgba8(img_buf))
}