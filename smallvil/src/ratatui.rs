use std::time::Duration;
use std::sync::Arc;
use std::io::Write;

use smithay::{
    backend::{
        input::InputEvent,
        ratatui::{self, RatatuiEvent, RatatuiInputBackend, RatatuiMouseEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Transform, Size, Physical},
    wayland::compositor::with_states,
};

use crate::{CalloopData, Smallvil};
use crate::gpu_renderer::GpuRenderer;
use crate::vulkan_import::VulkanImport;
use gpu_ansi_encoder::GpuAnsiEncoder;

pub fn init_ratatui(
    event_loop: &mut EventLoop<CalloopData>,
    data: &mut CalloopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &mut data.display_handle;
    let state = &mut data.state;

    let mut backend = ratatui::RatatuiBackend::new()?;

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "ratatui".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "GpuRatatui".into(),
        },
    );
    let _global = output.create_global::<Smallvil>(display_handle);
    output.change_current_state(Some(mode), Some(Transform::Normal), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    // WGPU Initialization
    let wgpu_instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let adapter = pollster::block_on(wgpu_instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (wgpu_device, wgpu_queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let wgpu_device = Arc::new(wgpu_device);
    let wgpu_queue = Arc::new(wgpu_queue);

    let ansi_encoder = pollster::block_on(GpuAnsiEncoder::new(wgpu_device.clone(), wgpu_queue.clone())).unwrap();
    let gpu_renderer = GpuRenderer::new(wgpu_device.clone(), wgpu_queue.clone());
    
    // Get raw Vulkan device for imports
    let ash_instance = unsafe {
        wgpu_instance.as_hal::<wgpu_hal::api::Vulkan>()
            .expect("Failed to get Vulkan instance from wgpu")
            .shared_instance()
            .raw_instance()
    };
    let (ash_device, ash_pdev) = unsafe {
        wgpu_device.as_hal::<wgpu_hal::api::Vulkan>()
            .map(|d| (d.raw_device().clone(), d.raw_physical_device()))
            .expect("Failed to get Vulkan device from wgpu")
    };
    let vulkan_import = VulkanImport::new(Arc::new(ash_device), ash_instance, ash_pdev);

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    let mut previous_texture: Option<wgpu::Texture> = None;
    let mut current_screen_texture: Option<(wgpu::Texture, wgpu::TextureView, Size<i32, Physical>)> = None;
    let mut frames = 0;
    let mut render_start = std::time::Instant::now();

    let output = output.clone();
    event_loop
        .handle()
        .insert_source(
            backend.event_source(Duration::from_micros(
                1_000_000_000 / u64::try_from(mode.refresh).unwrap(),
            )),
            move |event, _, data| {
                let display = &mut data.display_handle;
                let state = &mut data.state;

                match event {
                    RatatuiEvent::Redraw => {
                        frames += 1;
                        if frames >= 60 {
                            let render_end = std::time::Instant::now();
                            eprintln!(
                                "FPS = {}",
                                frames as f64 / render_end.duration_since(render_start).as_secs_f64()
                            );
                            frames = 0;
                            render_start = std::time::Instant::now();
                        }

                        // Composite scene into a texture
                        let screen_size = output.current_mode().unwrap().size;
                        
                        if current_screen_texture.as_ref().map(|(_, _, size)| *size != screen_size).unwrap_or(true) {
                            let screen_desc = wgpu::TextureDescriptor {
                                label: Some("screen_texture"),
                                size: wgpu::Extent3d {
                                    width: screen_size.w as u32,
                                    height: screen_size.h as u32,
                                    depth_or_array_layers: 1,
                                },
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba8Uint,
                                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                                view_formats: &[],
                            };
                            let tex = wgpu_device.create_texture(&screen_desc);
                            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                            current_screen_texture = Some((tex, view, screen_size));
                        }

                        let (screen_texture, screen_view, _) = current_screen_texture.as_ref().unwrap();

                        let mut windows_to_render = Vec::new();
                        state.space.elements().for_each(|window| {
                            let surface = window.toplevel().expect("Not a toplevel?").wl_surface();
                            let location = state.space.element_location(window).unwrap();
                            
                            with_states(surface, |surface_data| {
                                let surface_state = surface_data.data_map.get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>().unwrap().lock().unwrap();
                                if let Some(buffer) = surface_state.buffer() {
                                    if let Ok(dmabuf) = smithay::wayland::dmabuf::get_dmabuf(buffer) {
                                        let texture = unsafe { vulkan_import.import_dmabuf(&wgpu_device, &dmabuf) };
                                        windows_to_render.push((texture, location, window.geometry().size));
                                    }
                                }
                            });
                        });

                        gpu_renderer.render_scene(screen_view, Size::from((screen_size.w, screen_size.h)), &windows_to_render);

                        // Encode to ANSI and print
                        let ansi_string = pollster::block_on(ansi_encoder.ansi_from_texture(previous_texture.as_ref(), screen_texture)).unwrap();
                        print!("{}", &*ansi_string);
                        let _ = std::io::stdout().flush();

                        if !ansi_string.is_empty() {
                            // Save screenshot before exiting
                            let (width, height) = (screen_size.w as u32, screen_size.h as u32);
                            let buffer_size = (width * height * 4) as wgpu::BufferAddress;
                            let output_buffer = wgpu_device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("screenshot_buffer"),
                                size: buffer_size,
                                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                                mapped_at_creation: false,
                            });

                            let mut encoder = wgpu_device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("screenshot_encoder"),
                            });
                            encoder.copy_texture_to_buffer(
                                screen_texture.as_image_copy(),
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
                            wgpu_queue.submit(std::iter::once(encoder.finish()));

                            let buffer_slice = output_buffer.slice(..);
                            let (tx, rx) = std::sync::mpsc::channel();
                            buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
                                tx.send(result).unwrap();
                            });
                            let _ = wgpu_device.poll(wgpu::PollType::wait_indefinitely());
                            rx.recv().unwrap().unwrap();

                            let data = buffer_slice.get_mapped_range();
                            let is_single_color = if data.len() >= 4 {
                                let first_pixel = &data[0..4];
                                data.chunks_exact(4).all(|pixel| pixel == first_pixel)
                            } else {
                                true
                            };

                            if !is_single_color {
                                image::save_buffer(
                                    "/tmp/screenshot.png",
                                    &data,
                                    width,
                                    height,
                                    image::ExtendedColorType::Rgba8,
                                ).expect("Failed to save screenshot");
                                drop(data);
                                output_buffer.unmap();

                                std::process::exit(0);
                            }
                            drop(data);
                            output_buffer.unmap();
                        }

                        // We need a persistent copy for diffing if current_screen_texture is repurposed
                        // Actually, GpuAnsiEncoder diffs current against previous.
                        // If we reuse screen_texture, we must clone it or copy it.
                        // For now, let's keep it simple and just clone the texture handle if possible, 
                        // but wgpu::Texture is just a handle. The content changes.
                        // So we MUST have a separate "previous" texture with copied content.
                        
                        let prev_tex = if let Some(ref prev) = previous_texture {
                            if prev.size() == screen_texture.size() {
                                prev.clone()
                            } else {
                                let desc = wgpu::TextureDescriptor {
                                    label: Some("previous_texture"),
                                    size: screen_texture.size(),
                                    mip_level_count: 1,
                                    sample_count: 1,
                                    dimension: wgpu::TextureDimension::D2,
                                    format: wgpu::TextureFormat::Rgba8Uint,
                                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                                    view_formats: &[],
                                };
                                wgpu_device.create_texture(&desc)
                            }
                        } else {
                            let desc = wgpu::TextureDescriptor {
                                label: Some("previous_texture"),
                                size: screen_texture.size(),
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba8Uint,
                                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                                view_formats: &[],
                            };
                            wgpu_device.create_texture(&desc)
                        };

                        let mut encoder = wgpu_device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("copy_to_prev") });
                        encoder.copy_texture_to_texture(
                            screen_texture.as_image_copy(),
                            prev_tex.as_image_copy(),
                            screen_texture.size()
                        );
                        wgpu_queue.submit(std::iter::once(encoder.finish()));
                        
                        previous_texture = Some(prev_tex);

                        state.space.elements().for_each(|window| {
                            window.send_frame(
                                &output,
                                state.start_time.elapsed(),
                                Some(Duration::ZERO),
                                |_, _| Some(output.clone()),
                            )
                        });

                        state.space.refresh();
                        state.popups.cleanup();
                        let _ = display.flush_clients();
                    }
                    RatatuiEvent::Resize(_, _) => {
                        output.change_current_state(
                            Some(Mode {
                                size: backend.renderer().window_size(),
                                refresh: 60_000,
                            }),
                            None,
                            None,
                            None,
                        );
                        previous_texture = None; // Reset diffing on resize
                    }
                    event @ RatatuiEvent::Key { .. } => {
                        state.process_input_event::<RatatuiInputBackend>(InputEvent::Keyboard {
                            event: event.into(),
                        });
                    }
                    RatatuiEvent::Mouse(event) => {
                        let e = RatatuiMouseEvent::new(event, backend.window_size());
                        let event = match event.kind {
                            crossterm::event::MouseEventKind::Down(_)
                            | crossterm::event::MouseEventKind::Up(_) => {
                                InputEvent::PointerButton { event: e }
                            }
                            crossterm::event::MouseEventKind::Drag(_)
                            | crossterm::event::MouseEventKind::Moved => {
                                InputEvent::PointerMotionAbsolute { event: e }
                            }
                            crossterm::event::MouseEventKind::ScrollDown
                            | crossterm::event::MouseEventKind::ScrollUp
                            | crossterm::event::MouseEventKind::ScrollLeft
                            | crossterm::event::MouseEventKind::ScrollRight => {
                                InputEvent::PointerAxis { event: e }
                            }
                        };
                        state.process_input_event::<RatatuiInputBackend>(event);
                    }
                }
            },
        )
        .unwrap();

    Ok(())
}