use ash::vk;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::{Buffer, Fourcc};
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use wgpu::TextureFormat;
use wgpu_hal as hal;

pub struct VulkanImport {
    pub device: Arc<ash::Device>,

    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
}

impl VulkanImport {
    pub fn new(device: Arc<ash::Device>, instance: &ash::Instance, pdev: vk::PhysicalDevice) -> Self {
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(pdev) };

        Self {
            device,
            memory_properties,
        }
    }

    fn find_memory_type(&self, type_filter: u32, properties: vk::MemoryPropertyFlags) -> Option<u32> {
        for i in 0..self.memory_properties.memory_type_count {
            if (type_filter & (1 << i)) != 0
                && (self.memory_properties.memory_types[i as usize].property_flags & properties) == properties
            {
                return Some(i);
            }
        }

        None
    }

    pub unsafe fn import_dmabuf(&self, wgpu_device: &wgpu::Device, dmabuf: &Dmabuf) -> wgpu::Texture {
        let size = dmabuf.size();
        let format = dmabuf.format();
        let (vk_format, wgpu_format) = match format.code {
            Fourcc::Argb8888 => (vk::Format::B8G8R8A8_UNORM, TextureFormat::Bgra8Unorm),
            Fourcc::Xrgb8888 => (vk::Format::B8G8R8A8_UNORM, TextureFormat::Bgra8Unorm),
            Fourcc::Abgr8888 => (vk::Format::R8G8B8A8_UNORM, TextureFormat::Rgba8Unorm),
            Fourcc::Xbgr8888 => (vk::Format::R8G8B8A8_UNORM, TextureFormat::Rgba8Unorm),
            _ => (vk::Format::R8G8B8A8_UNORM, TextureFormat::Rgba8Unorm),
        };

        let mut external_memory_image_create_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(format.modifier.into());
        let planes = dmabuf
            .offsets()
            .zip(dmabuf.strides())
            .enumerate()
            .map(|(_idx, (offset, stride))| vk::SubresourceLayout {
                offset: offset as u64,
                size: 0,
                row_pitch: stride as u64,
                array_pitch: 0,
                depth_pitch: 0,
            })
            .collect::<Vec<_>>();
        modifier_info = modifier_info.plane_layouts(&planes);
        let image_create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk_format)
            .extent(vk::Extent3D {
                width: size.w as u32,

                height: size.h as u32,

                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external_memory_image_create_info)
            .push_next(&mut modifier_info);
        let image = self.device.create_image(&image_create_info, None).unwrap();

        let memory_requirements = self.device.get_image_memory_requirements(image);
        let memory_type_index = self
            .find_memory_type(
                memory_requirements.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
            .unwrap_or_else(|| {
                self.find_memory_type(
                    memory_requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::empty(),
                )
                .unwrap()
            });
        let mut import_memory_fd_info = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(dmabuf.handles().next().unwrap().as_raw_fd());
        let memory_allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(memory_requirements.size)
            .memory_type_index(memory_type_index)
            .push_next(&mut import_memory_fd_info);
        let memory = self.device.allocate_memory(&memory_allocate_info, None).unwrap();
        self.device.bind_image_memory(image, memory, 0).unwrap();

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
            view_formats: &[TextureFormat::Rgba8Unorm],
        };
        let hal_device = wgpu_device
            .as_hal::<hal::api::Vulkan>()
            .expect("Not using Vulkan");
        let device_clone = self.device.clone();
        let cleanup = Box::new(move || unsafe {
            device_clone.destroy_image(image, None);
            device_clone.free_memory(memory, None);
        });
        wgpu_device.create_texture_from_hal::<hal::api::Vulkan>(
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
                    view_formats: vec![TextureFormat::Rgba8Uint],
                },
                Some(cleanup),
            ),
            &desc,
        )
    }
}
