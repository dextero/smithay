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
    utils::{Transform, Size},
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
    let ash_device = unsafe {
        wgpu_device.as_hal::<wgpu_hal::api::Vulkan>()
            .map(|d| d.raw_device().clone())
            .expect("Failed to get Vulkan device from wgpu")
    };
    let vulkan_import = VulkanImport::new(Arc::new(ash_device));

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    let mut previous_texture: Option<wgpu::Texture> = None;
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
                        let screen_texture = wgpu_device.create_texture(&screen_desc);
                        let screen_view = screen_texture.create_view(&wgpu::TextureViewDescriptor::default());

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

                        gpu_renderer.render_scene(&screen_view, Size::from((screen_size.w, screen_size.h)), &windows_to_render);

                        // Encode to ANSI and print
                        let ansi_string = pollster::block_on(ansi_encoder.ansi_from_texture(previous_texture.as_ref(), &screen_texture)).unwrap();
                        print!("{ansi_string}");
                        let _ = std::io::stdout().flush();

                        previous_texture = Some(screen_texture);

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