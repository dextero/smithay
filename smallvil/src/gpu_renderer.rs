use smithay::utils::{Logical, Point, Size};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub struct GpuRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

impl GpuRenderer {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Compositor Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("compositor.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("compositor_bind_group_layout"),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Uint,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            sampler,
        }
    }

    pub fn render_scene(
        &self,
        target_view: &wgpu::TextureView,
        screen_size: Size<i32, Logical>,
        windows: &[(wgpu::Texture, Point<i32, Logical>, Size<i32, Logical>)],
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 100.0,
                            a: 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.pipeline);

            for (texture, pos, size) in windows {
                let x1 = (pos.x as f32 / screen_size.w as f32) * 2.0 - 1.0;
                let y1 = 1.0 - (pos.y as f32 / screen_size.h as f32) * 2.0;
                let x2 = ((pos.x + size.w) as f32 / screen_size.w as f32) * 2.0 - 1.0;
                let y2 = 1.0 - ((pos.y + size.h) as f32 / screen_size.h as f32) * 2.0;

                let vertices = [
                    Vertex {
                        position: [x1, y1],
                        tex_coords: [0.0, 0.0],
                    },
                    Vertex {
                        position: [x1, y2],
                        tex_coords: [0.0, 1.0],
                    },
                    Vertex {
                        position: [x2, y1],
                        tex_coords: [1.0, 0.0],
                    },
                    Vertex {
                        position: [x2, y1],
                        tex_coords: [1.0, 0.0],
                    },
                    Vertex {
                        position: [x1, y2],
                        tex_coords: [0.0, 1.0],
                    },
                    Vertex {
                        position: [x2, y2],
                        tex_coords: [1.0, 1.0],
                    },
                ];

                let vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Vertex Buffer"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

                let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&texture_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                    label: Some("window_bind_group"),
                });

                render_pass.set_bind_group(0, &bind_group, &[]);
                render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                render_pass.draw(0..6, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::utils::Point;

    async fn get_device() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("Failed to find wgpu adapter");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("Failed to create wgpu device");
        (Arc::new(device), Arc::new(queue))
    }

    #[tokio::test]
    async fn test_render_simple_rect() {
        let (device, queue) = get_device().await;
        let renderer = GpuRenderer::new(device.clone(), queue.clone());

        let width = 256;
        let height = 256;
        let screen_size = Size::from((width as i32, height as i32));

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("target_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let target_texture = device.create_texture(&texture_desc);
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create a 1x1 white source texture
        let src_texture_desc = wgpu::TextureDescriptor {
            label: Some("src_texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let src_texture = device.create_texture(&src_texture_desc);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &src_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        // Render a 50x50 rect at (25, 25)
        renderer.render_scene(
            &target_view,
            screen_size,
            &[(src_texture, Point::from((25, 25)), Size::from((50, 50)))],
        );

        // Read back
        let buffer_size = (width * height * 4) as wgpu::BufferAddress;
        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback_buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback_encoder"),
        });
        encoder.copy_texture_to_buffer(
            target_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll failed");
        rx.recv().unwrap().expect("map_async failed");

        let data = buffer_slice.get_mapped_range();

        // Check pixel at (50, 50) which should be white
        let pixel_offset = ((50 * width + 50) * 4) as usize;
        assert_eq!(data[pixel_offset..pixel_offset + 4], [255, 255, 255, 255]);

        // Check pixel at (10, 10) which should be black (clear color)
        let pixel_offset = ((10 * width + 10) * 4) as usize;
        assert_eq!(data[pixel_offset..pixel_offset + 4], [0, 0, 0, 1]);

        drop(data);
        output_buffer.unmap();
    }

    #[tokio::test]
    async fn test_render_multiple_windows() {
        let (device, queue) = get_device().await;
        let renderer = GpuRenderer::new(device.clone(), queue.clone());

        let width = 256;
        let height = 256;
        let screen_size = Size::from((width as i32, height as i32));

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("target_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let target_texture = device.create_texture(&texture_desc);
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create Red and Blue 1x1 source textures
        let src_texture_desc = wgpu::TextureDescriptor {
            label: Some("src_texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let red_texture = device.create_texture(&src_texture_desc);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &red_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let blue_texture = device.create_texture(&src_texture_desc);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &blue_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        // Render Red at (0,0) 100x100, then Blue at (50,50) 100x100 (Blue should be on top)
        renderer.render_scene(
            &target_view,
            screen_size,
            &[
                (red_texture, Point::from((0, 0)), Size::from((100, 100))),
                (blue_texture, Point::from((50, 50)), Size::from((100, 100))),
            ],
        );

        // Read back
        let buffer_size = (width * height * 4) as wgpu::BufferAddress;
        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback_buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            target_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll failed");
        rx.recv().unwrap().expect("map failed");

        let data = buffer_slice.get_mapped_range();

        // (25, 25) should be Red [255, 0, 0, 255]
        let off = ((25 * width + 25) * 4) as usize;
        assert_eq!(data[off..off + 4], [255, 0, 0, 255]);

        // (75, 75) should be Blue [0, 0, 255, 255] (overlap area)
        let off = ((75 * width + 75) * 4) as usize;
        assert_eq!(data[off..off + 4], [0, 0, 255, 255]);

        // (125, 125) should be Blue [0, 0, 255, 255]
        let off = ((125 * width + 125) * 4) as usize;
        assert_eq!(data[off..off + 4], [0, 0, 255, 255]);

        // (200, 200) should be Black [0, 0, 0, 1]
        let off = ((200 * width + 200) * 4) as usize;
        assert_eq!(data[off..off + 4], [0, 0, 0, 1]);

        drop(data);
        output_buffer.unmap();
    }
}
