use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wgpu::{self};
use wgpu::util::DeviceExt;
use wgpu_hal as hal;

use smithay::backend::allocator::{Buffer as BufferTrait, Fourcc};
use smithay::backend::renderer::{
    Bind, ContextId, DebugFlags, Frame, ImportDma, ImportMem, Renderer, RendererSuper, Texture, TextureFilter,
};
use smithay::backend::renderer::{ImportDmaWl, ImportMemWl};
use smithay::utils::{Buffer, Physical, Rectangle, Size, Transform};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GlobalUniforms {
    projection: [f32; 16],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RenderUniforms {
    color: [f32; 4],
    alpha: f32,
    has_texture: u32,
    _padding: [u32; 2],
}

/// A handle to a wgpu texture
#[derive(Debug, Clone)]
pub struct WgpuTexture {
    pub(super) texture: Arc<wgpu::Texture>,
    pub(super) view: Arc<wgpu::TextureView>,
    pub(super) size: Size<i32, Buffer>,
    pub(super) format: Option<Fourcc>,
    pub(super) has_alpha: bool,
}

impl WgpuTexture {
    /// Create a new WgpuTexture from an existing wgpu texture and view
    pub fn new(
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        size: Size<i32, Buffer>,
        format: Option<Fourcc>,
        has_alpha: bool,
    ) -> Self {
        Self {
            texture: Arc::new(texture),
            view: Arc::new(view),
            size,
            format,
            has_alpha,
        }
    }

    /// Get a reference to the underlying wgpu texture
    pub fn wgpu_texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

impl Texture for WgpuTexture {
    fn width(&self) -> u32 {
        self.size.w as u32
    }
    fn height(&self) -> u32 {
        self.size.h as u32
    }
    fn format(&self) -> Option<Fourcc> {
        self.format
    }
}

#[derive(Debug)]
struct WgpuRendererInner {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout_global: wgpu::BindGroupLayout,
    bind_group_layout_texture: wgpu::BindGroupLayout,
    bind_group_layout_render: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,

    vulkan_data: Option<VulkanData>,
}

struct VulkanData {
    memory_properties: ash::vk::PhysicalDeviceMemoryProperties,
}

impl std::fmt::Debug for VulkanData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanData").finish_non_exhaustive()
    }
}

/// A renderer using wgpu
#[derive(Debug)]
pub struct WgpuRenderer {
    inner: Arc<WgpuRendererInner>,
    context_id: ContextId<WgpuTexture>,
}

impl WgpuRenderer {
    /// Create a new wgpu renderer from an existing device and queue
    pub fn new(instance: &wgpu::Instance, device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smithay_wgpu_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bind_group_layout_global = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("global_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group_layout_texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        });

        let bind_group_layout_render = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("render_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smithay_wgpu_pipeline_layout"),
            bind_group_layouts: &[
                &bind_group_layout_global,
                &bind_group_layout_texture,
                &bind_group_layout_render,
            ],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smithay_wgpu_pipeline"),
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
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("smithay_wgpu_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let vertices = [
            Vertex {
                position: [0.0, 0.0],
                tex_coords: [0.0, 0.0],
            },
            Vertex {
                position: [0.0, 1.0],
                tex_coords: [0.0, 1.0],
            },
            Vertex {
                position: [1.0, 0.0],
                tex_coords: [1.0, 0.0],
            },
            Vertex {
                position: [1.0, 1.0],
                tex_coords: [1.0, 1.0],
            },
        ];
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smithay_wgpu_vertex_buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let vulkan_data = unsafe {
            device.as_hal::<hal::api::Vulkan>().and_then(|hal_device| {
                let physical_device = hal_device.raw_physical_device();
                instance.as_hal::<hal::api::Vulkan>().map(|hal_instance| {
                    let ash_instance = hal_instance.shared_instance().raw_instance();
                    let memory_properties =
                        ash_instance.get_physical_device_memory_properties(physical_device);
                    VulkanData { memory_properties }
                })
            })
        };

        Self {
            inner: Arc::new(WgpuRendererInner {
                device,
                queue,
                pipeline,
                bind_group_layout_global,
                bind_group_layout_texture,
                bind_group_layout_render,
                sampler,
                vertex_buffer,
                vulkan_data,
            }),
            context_id: ContextId::new(),
        }
    }

    /// Get the wgpu device
    pub fn device(&self) -> &wgpu::Device {
        &self.inner.device
    }

    /// Get the wgpu queue
    pub fn queue(&self) -> &wgpu::Queue {
        &self.inner.queue
    }

    fn find_memory_type(&self, type_filter: u32, properties: ash::vk::MemoryPropertyFlags) -> Option<u32> {
        let vulkan_data = self.inner.vulkan_data.as_ref()?;
        for i in 0..vulkan_data.memory_properties.memory_type_count {
            if (type_filter & (1 << i)) != 0
                && (vulkan_data.memory_properties.memory_types[i as usize].property_flags & properties)
                    == properties
            {
                return Some(i);
            }
        }
        None
    }
}

/// A frame for the wgpu renderer
pub struct WgpuFrame<'frame, 'buffer> {
    renderer: &'frame mut WgpuRenderer,
    framebuffer: &'frame mut WgpuTexture,
    encoder: wgpu::CommandEncoder,
    global_bind_group: wgpu::BindGroup,
    output_size: Size<i32, Physical>,
    transform: Transform,
    _phantom: std::marker::PhantomData<&'buffer ()>,
}


impl<'frame, 'buffer> std::fmt::Debug for WgpuFrame<'frame, 'buffer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuFrame")
            .field("output_size", &self.output_size)
            .field("transform", &self.transform)
            .finish()
    }
}

impl<'frame, 'buffer> Frame for WgpuFrame<'frame, 'buffer> {
    type Error = WgpuError;
    type TextureId = WgpuTexture;

    fn context_id(&self) -> ContextId<Self::TextureId> {
        self.renderer.context_id.clone()
    }

    fn clear(
        &mut self,
        color: smithay::backend::renderer::Color32F,
        at: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error> {
        for _rect in at {
            let _render_pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.framebuffer.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: color.r() as f64,
                            g: color.g() as f64,
                            b: color.b() as f64,
                            a: color.a() as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        Ok(())
    }

    fn draw_solid(
        &mut self,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        color: smithay::backend::renderer::Color32F,
    ) -> Result<(), Self::Error> {
        let uniforms = RenderUniforms {
            color: [color.r(), color.g(), color.b(), color.a()],
            alpha: color.a(),
            has_texture: 0,
            _padding: [0; 2],
        };
        let uniform_buffer =
            self.renderer
                .inner
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("solid_uniform_buffer"),
                    contents: bytemuck::cast_slice(&[uniforms]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
        let render_bind_group = self
            .renderer
            .inner
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.renderer.inner.bind_group_layout_render,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
                label: None,
            });

        let dummy_texture = self
            .renderer
            .inner
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
        let dummy_view = dummy_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let texture_bind_group = self
            .renderer
            .inner
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.renderer.inner.bind_group_layout_texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&dummy_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.renderer.inner.sampler),
                    },
                ],
                label: None,
            });

        for rect in damage {
            let intersection = rect.intersection(dst).unwrap_or_default();
            if intersection.is_empty() {
                continue;
            }

            let mut render_pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("solid_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.framebuffer.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.renderer.inner.pipeline);
            render_pass.set_bind_group(0, &self.global_bind_group, &[]);
            render_pass.set_bind_group(1, &texture_bind_group, &[]);
            render_pass.set_bind_group(2, &render_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.renderer.inner.vertex_buffer.slice(..));

            render_pass.draw(0..4, 0..1);
        }

        Ok(())
    }

    fn render_texture_from_to(
        &mut self,
        texture: &Self::TextureId,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _src_transform: Transform,
        alpha: f32,
    ) -> Result<(), Self::Error> {
        let uniforms = RenderUniforms {
            color: [0.0; 4],
            alpha,
            has_texture: 1,
            _padding: [0; 2],
        };
        let uniform_buffer =
            self.renderer
                .inner
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("texture_uniform_buffer"),
                    contents: bytemuck::cast_slice(&[uniforms]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
        let render_bind_group = self
            .renderer
            .inner
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.renderer.inner.bind_group_layout_render,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
                label: None,
            });

        let texture_bind_group = self
            .renderer
            .inner
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.renderer.inner.bind_group_layout_texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.renderer.inner.sampler),
                    },
                ],
                label: None,
            });

        for rect in damage {
            let intersection = rect.intersection(dst).unwrap_or_default();
            if intersection.is_empty() {
                continue;
            }

            let mut render_pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("texture_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.framebuffer.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.renderer.inner.pipeline);
            render_pass.set_bind_group(0, &self.global_bind_group, &[]);
            render_pass.set_bind_group(1, &texture_bind_group, &[]);
            render_pass.set_bind_group(2, &render_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.renderer.inner.vertex_buffer.slice(..));

            render_pass.draw(0..4, 0..1);
        }
        Ok(())
    }

    fn transformation(&self) -> Transform {
        self.transform
    }

    fn wait(&mut self, _sync: &smithay::backend::renderer::sync::SyncPoint) -> Result<(), Self::Error> {
        Ok(())
    }

    fn finish(self) -> Result<smithay::backend::renderer::sync::SyncPoint, Self::Error> {
        self.renderer
            .inner
            .queue
            .submit(std::iter::once(self.encoder.finish()));
        Ok(Default::default())
    }
}

/// Wgpu renderer error
#[derive(Debug, thiserror::Error)]
pub enum WgpuError {
    /// A wgpu error occurred
    #[error("Wgpu error: {0}")]
    Wgpu(String),
    /// The provided pixel format is not supported
    #[error("Unsupported pixel format: {0:?}")]
    UnsupportedPixelFormat(Fourcc),
    /// The provided shm format is not supported
    #[error("Unsupported shm format: {0:?}")]
    UnsupportedWlPixelFormat(wayland_server::protocol::wl_shm::Format),
    /// A buffer access error occurred
    #[error("Buffer access error: {0}")]
    BufferAccess(#[from] smithay::wayland::shm::BufferAccessError),
    /// DmaBuf import is not supported on this wgpu backend
    #[error("DmaBuf import is not supported")]
    DmaBufImportNotSupported,
    /// Failed to allocate memory on the GPU
    #[error("Failed to allocate memory on the GPU")]
    OutOfMemory,
}

impl RendererSuper for WgpuRenderer {
    type Error = WgpuError;
    type TextureId = WgpuTexture;
    type Framebuffer<'buffer> = WgpuTexture;
    type Frame<'frame, 'buffer>
        = WgpuFrame<'frame, 'buffer>
    where
        'buffer: 'frame,
        Self: 'frame;
}

impl Renderer for WgpuRenderer {
    fn context_id(&self) -> ContextId<Self::TextureId> {
        self.context_id.clone()
    }

    fn downscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        Ok(())
    }
    fn upscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_debug_flags(&mut self, _flags: DebugFlags) {}
    fn debug_flags(&self) -> DebugFlags {
        DebugFlags::empty()
    }

    fn render<'frame, 'buffer>(
        &'frame mut self,
        framebuffer: &'frame mut Self::Framebuffer<'buffer>,
        output_size: Size<i32, Physical>,
        dst_transform: Transform,
    ) -> Result<Self::Frame<'frame, 'buffer>, Self::Error>
    where
        'buffer: 'frame,
    {
        let encoder = self
            .inner
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("smithay_wgpu_render_encoder"),
            });

        let projection = [
            2.0 / output_size.w as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / output_size.h as f32,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            -1.0,
            1.0,
            0.0,
            1.0,
        ];
        let global_uniforms = GlobalUniforms { projection };
        let global_uniform_buffer = self
            .inner
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("global_uniform_buffer"),
                contents: bytemuck::cast_slice(&[global_uniforms]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let global_bind_group = self.inner.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.inner.bind_group_layout_global,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: global_uniform_buffer.as_entire_binding(),
            }],
            label: Some("global_bind_group"),
        });

        Ok(WgpuFrame {
            renderer: self,
            framebuffer,
            encoder,
            global_bind_group,
            output_size,
            transform: dst_transform,
            _phantom: std::marker::PhantomData,
        })
    }

    fn wait(&mut self, _sync: &smithay::backend::renderer::sync::SyncPoint) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Bind<WgpuTexture> for WgpuRenderer {
    fn bind<'a>(&mut self, target: &'a mut WgpuTexture) -> Result<Self::Framebuffer<'a>, Self::Error> {
        Ok(target.clone())
    }
}

impl ImportMem for WgpuRenderer {
    fn import_memory(
        &mut self,
        data: &[u8],
        format: Fourcc,
        size: Size<i32, Buffer>,
        _flipped: bool,
    ) -> Result<Self::TextureId, Self::Error> {
        let wgpu_format = match format {
            Fourcc::Argb8888 | Fourcc::Xrgb8888 => wgpu::TextureFormat::Bgra8Unorm,
            Fourcc::Abgr8888 | Fourcc::Xbgr8888 => wgpu::TextureFormat::Rgba8Unorm,
            _ => return Err(WgpuError::UnsupportedPixelFormat(format)),
        };

        let texture_extent = wgpu::Extent3d {
            width: size.w as u32,
            height: size.h as u32,
            depth_or_array_layers: 1,
        };

        let texture = self.inner.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("imported_memory_texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.inner.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size.w as u32),
                rows_per_image: Some(size.h as u32),
            },
            texture_extent,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(WgpuTexture {
            texture: Arc::new(texture),
            view: Arc::new(view),
            size,
            format: Some(format),
            has_alpha: smithay::backend::allocator::format::has_alpha(format),
        })
    }

    fn update_memory(
        &mut self,
        texture: &Self::TextureId,
        data: &[u8],
        region: Rectangle<i32, Buffer>,
    ) -> Result<(), Self::Error> {
        self.inner.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.loc.x as u32,
                    y: region.loc.y as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * texture.size.w as u32),
                rows_per_image: Some(region.size.h as u32),
            },
            wgpu::Extent3d {
                width: region.size.w as u32,
                height: region.size.h as u32,
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }

    fn mem_formats(&self) -> Box<dyn Iterator<Item = Fourcc>> {
        Box::new(
            vec![
                Fourcc::Argb8888,
                Fourcc::Xrgb8888,
                Fourcc::Abgr8888,
                Fourcc::Xbgr8888,
            ]
            .into_iter(),
        )
    }
}

impl ImportDma for WgpuRenderer {
    fn import_dmabuf(
        &mut self,
        dmabuf: &smithay::backend::allocator::dmabuf::Dmabuf,
        _damage: Option<&[Rectangle<i32, Buffer>]>,
    ) -> Result<Self::TextureId, Self::Error> {
        {
            let size = dmabuf.size();
            let format = dmabuf.format();
            let (vk_format, wgpu_format) = match format.code {
                Fourcc::Argb8888 => (ash::vk::Format::B8G8R8A8_UNORM, wgpu::TextureFormat::Bgra8Unorm),
                Fourcc::Xrgb8888 => (ash::vk::Format::B8G8R8A8_UNORM, wgpu::TextureFormat::Bgra8Unorm),
                Fourcc::Abgr8888 => (ash::vk::Format::R8G8B8A8_UNORM, wgpu::TextureFormat::Rgba8Unorm),
                Fourcc::Xbgr8888 => (ash::vk::Format::R8G8B8A8_UNORM, wgpu::TextureFormat::Rgba8Unorm),
                _ => (ash::vk::Format::B8G8R8A8_UNORM, wgpu::TextureFormat::Bgra8Unorm),
            };

            let hal_device = unsafe { self.inner.device.as_hal::<hal::api::Vulkan>() }
                .ok_or(WgpuError::DmaBufImportNotSupported)?;
            let ash_device = hal_device.raw_device();

            let mut external_memory_image_create_info = ash::vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(ash::vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut modifier_info = ash::vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .drm_format_modifier(format.modifier.into());
            let planes = dmabuf
                .offsets()
                .zip(dmabuf.strides())
                .enumerate()
                .map(|(_idx, (offset, stride))| ash::vk::SubresourceLayout {
                    offset: offset as u64,
                    size: 0,
                    row_pitch: stride as u64,
                    array_pitch: 0,
                    depth_pitch: 0,
                })
                .collect::<Vec<_>>();
            modifier_info = modifier_info.plane_layouts(&planes);
            let image_create_info = ash::vk::ImageCreateInfo::default()
                .image_type(ash::vk::ImageType::TYPE_2D)
                .format(vk_format)
                .extent(ash::vk::Extent3D {
                    width: size.w as u32,
                    height: size.h as u32,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(ash::vk::SampleCountFlags::TYPE_1)
                .tiling(ash::vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(ash::vk::ImageUsageFlags::SAMPLED | ash::vk::ImageUsageFlags::TRANSFER_SRC)
                .sharing_mode(ash::vk::SharingMode::EXCLUSIVE)
                .initial_layout(ash::vk::ImageLayout::UNDEFINED)
                .push_next(&mut external_memory_image_create_info)
                .push_next(&mut modifier_info);

            let image = unsafe { ash_device.create_image(&image_create_info, None) }
                .map_err(|e| WgpuError::Wgpu(e.to_string()))?;

            let memory_requirements = unsafe { ash_device.get_image_memory_requirements(image) };
            let memory_type_index = self
                .find_memory_type(
                    memory_requirements.memory_type_bits,
                    ash::vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
                .unwrap_or_else(|| {
                    self.find_memory_type(
                        memory_requirements.memory_type_bits,
                        ash::vk::MemoryPropertyFlags::empty(),
                    )
                    .unwrap_or(0)
                });

            use std::os::unix::io::AsRawFd;
            let mut import_memory_fd_info = ash::vk::ImportMemoryFdInfoKHR::default()
                .handle_type(ash::vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                .fd(dmabuf
                    .handles()
                    .next()
                    .ok_or(WgpuError::DmaBufImportNotSupported)?
                    .as_raw_fd());

            let memory_allocate_info = ash::vk::MemoryAllocateInfo::default()
                .allocation_size(memory_requirements.size)
                .memory_type_index(memory_type_index)
                .push_next(&mut import_memory_fd_info);

            let memory = unsafe { ash_device.allocate_memory(&memory_allocate_info, None) }
                .map_err(|e| WgpuError::Wgpu(e.to_string()))?;
            unsafe { ash_device.bind_image_memory(image, memory, 0) }
                .map_err(|e| WgpuError::Wgpu(e.to_string()))?;

            let desc = wgpu::TextureDescriptor {
                label: Some("imported_dmabuf"),
                size: wgpu::Extent3d {
                    width: size.w as u32,
                    height: size.h as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            };

            let ash_device_clone = ash_device.clone();
            let cleanup = Box::new(move || unsafe {
                ash_device_clone.destroy_image(image, None);
                ash_device_clone.free_memory(memory, None);
            });

            let texture = unsafe {
                self.inner.device.create_texture_from_hal::<hal::api::Vulkan>(
                    hal_device.texture_from_raw(
                        image,
                        &hal::TextureDescriptor {
                            label: Some("imported_dmabuf"),
                            size: wgpu::Extent3d {
                                width: size.w as u32,
                                height: size.h as u32,
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu_format,
                            usage: wgpu::TextureUses::RESOURCE,
                            memory_flags: hal::MemoryFlags::empty(),
                            view_formats: vec![wgpu_format],
                        },
                        Some(cleanup),
                    ),
                    &desc,
                )
            };

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            Ok(WgpuTexture {
                texture: Arc::new(texture),
                view: Arc::new(view),
                size: size.into(),
                format: Some(format.code),
                has_alpha: smithay::backend::allocator::format::has_alpha(format.code),
            })
        }
    }
}

impl ImportDmaWl for WgpuRenderer {}

impl ImportMemWl for WgpuRenderer {
    fn import_shm_buffer(
        &mut self,
        buffer: &wayland_server::protocol::wl_buffer::WlBuffer,
        surface: Option<&smithay::wayland::compositor::SurfaceData>,
        damage: &[Rectangle<i32, Buffer>],
    ) -> Result<Self::TextureId, Self::Error> {
        use smithay::wayland::shm::{shm_format_to_fourcc, with_buffer_contents};

        type CacheMap = HashMap<ContextId<WgpuTexture>, WgpuTexture>;

        let mut surface_lock = surface.as_ref().map(|surface_data| {
            surface_data
                .data_map
                .get_or_insert_threadsafe(|| Arc::new(Mutex::new(CacheMap::new())))
                .lock()
                .unwrap()
        });

        with_buffer_contents(buffer, |ptr, len, data| {
            let width = data.width;
            let height = data.height;
            let fourcc =
                shm_format_to_fourcc(data.format).ok_or(WgpuError::UnsupportedWlPixelFormat(data.format))?;

            let id = self.context_id();
            let cached_texture = surface_lock
                .as_ref()
                .and_then(|cache| cache.get(&id).cloned())
                .filter(|texture| texture.size == (width, height).into());

            let texture = if let Some(texture) = cached_texture {
                let data_slice = unsafe {
                    std::slice::from_raw_parts(ptr.add(data.offset as usize), len - data.offset as usize)
                };
                if !damage.is_empty() {
                    self.update_memory(&texture, data_slice, Rectangle::from_size((width, height).into()))?;
                }
                texture
            } else {
                let data_slice = unsafe {
                    std::slice::from_raw_parts(ptr.add(data.offset as usize), len - data.offset as usize)
                };
                let texture = self.import_memory(data_slice, fourcc, (width, height).into(), false)?;
                if let Some(cache) = surface_lock.as_mut() {
                    cache.insert(id, texture.clone());
                }
                texture
            };

            Ok(texture)
        })?
    }
}
