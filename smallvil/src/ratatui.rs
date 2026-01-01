use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use smithay::{
    backend::{
        input::InputEvent,
        ratatui::{self, RatatuiEvent, RatatuiInputBackend, RatatuiMouseEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::{
            protocol::{wl_shm, wl_surface::WlSurface},
            DisplayHandle,
        },
    },
    utils::{Logical, Point, Size, Transform},
    wayland::{
        compositor::{
            with_surface_tree_downward, SubsurfaceCachedState, SurfaceData, TraversalAction,
        },
        shm,
    },
};
use tracing::debug;

use crate::gpu_renderer::GpuRenderer;
use crate::vulkan_import::VulkanImport;
use crate::{CalloopData, Smallvil};
use gpu_ansi_encoder::GpuAnsiEncoder;

use tokio::sync::mpsc;

struct RatatuiHandler {
    wgpu_device: Arc<wgpu::Device>,
    wgpu_queue: Arc<wgpu::Queue>,
    gpu_renderer: GpuRenderer,
    vulkan_import: VulkanImport,
    output: Output,
    backend: ratatui::RatatuiBackend,

    tx: mpsc::Sender<Option<wgpu::Texture>>,
}

impl RatatuiHandler {
    fn redraw(&mut self, state: &mut Smallvil, display: &mut DisplayHandle) {
        // Composite scene into a texture
        let screen_size = self.output.current_mode().unwrap().size;

        // For now, always create a new texture to avoid race conditions with the encoding task.
        // We can optimize this later with a texture pool.
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
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Bgra8Unorm],
        };
        let screen_texture = self.wgpu_device.create_texture(&screen_desc);
        let screen_view = screen_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut windows_to_render = Vec::new();
        state.space.elements().for_each(|window| {
            self.import_window(state, window, &mut windows_to_render);
        });

        self.gpu_renderer.render_scene(
            &screen_view,
            Size::from((screen_size.w, screen_size.h)),
            &windows_to_render,
        );

        // Send to encoding task
        let _ = self.tx.try_send(Some(screen_texture));

        state.space.elements().for_each(|window| {
            window.send_frame(
                &self.output,
                state.start_time.elapsed(),
                Some(Duration::ZERO),
                |_, _| Some(self.output.clone()),
            )
        });

        state.space.refresh();
        state.popups.cleanup();
        let _ = display.flush_clients();
    }

    fn import_window(
        &self,
        state: &Smallvil,
        window: &smithay::desktop::Window,
        windows_to_render: &mut Vec<(wgpu::Texture, Point<i32, Logical>, Size<i32, Logical>)>,
    ) {
        let Some(window_location) = state.space.element_location(window) else {
            return;
        };

        let surface = window.toplevel().expect("Not a toplevel?").wl_surface();

        with_surface_tree_downward(
            surface,
            window_location,
            |_, states, &location| {
                let mut location = location;
                if let Some(subsurface) = states.data_map.get::<SubsurfaceCachedState>() {
                    location += subsurface.location;
                }
                TraversalAction::DoChildren(location)
            },
            |surface, states, &location| {
                if let Err(e) = self.import_surface(surface, states, location, windows_to_render) {
                    eprintln!("failed to import surface: {:#?}", e);
                }
            },
            |_, _, _| true,
        );
    }

    fn import_surface(
        &self,
        _surface: &WlSurface,
        states: &SurfaceData,
        location: Point<i32, Logical>,
        windows_to_render: &mut Vec<(wgpu::Texture, Point<i32, Logical>, Size<i32, Logical>)>,
    ) -> anyhow::Result<()> {
        let surface_state = states
            .data_map
            .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>()
            .unwrap()
            .lock()
            .unwrap();
        let Some(buffer) = surface_state.buffer() else {
            return Ok(());
        };

        let buffer_size = surface_state.buffer_size().unwrap_or_default();

        let texture = if let Ok(dmabuf) = smithay::wayland::dmabuf::get_dmabuf(buffer) {
            unsafe { self.vulkan_import.import_dmabuf(&self.wgpu_device, &dmabuf) }
        } else if let Ok(shm_texture) = shm::with_buffer_contents(buffer, |ptr, _len, data| {
            let offset = data.offset as usize;
            let stride = data.stride as usize;
            let height = data.height as usize;
            let width = data.width as usize;

            let size = wgpu::Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            };

            let wgpu_format = match data.format {
                wl_shm::Format::Argb8888 => wgpu::TextureFormat::Bgra8Unorm,
                wl_shm::Format::Xrgb8888 => wgpu::TextureFormat::Bgra8Unorm,
                wl_shm::Format::Abgr8888 => wgpu::TextureFormat::Rgba8Unorm,
                wl_shm::Format::Xbgr8888 => wgpu::TextureFormat::Rgba8Unorm,
                _ => panic!("unsupported format: {:?}", data.format),
            };

            let texture = self.wgpu_device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shm_texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[wgpu_format],
            });

            let data_ptr = unsafe { ptr.add(offset) };
            let data_slice = unsafe { std::slice::from_raw_parts(data_ptr, stride * height) };

            self.wgpu_queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data_slice,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(height as u32),
                },
                size,
            );
            texture
        }) {
            shm_texture
        } else {
            bail!("window has no dmabuf and shm failed");
        };
        windows_to_render.push((texture, location, buffer_size));
        Ok(())
    }

    fn handle_event(&mut self, event: RatatuiEvent, state: &mut Smallvil, display: &mut DisplayHandle) {
        match event {
            RatatuiEvent::Redraw => {
                self.redraw(state, display);
            }
            RatatuiEvent::Resize(_, _) => {
                self.output.change_current_state(
                    Some(Mode {
                        size: self.backend.window_size(),
                        refresh: 60_000,
                    }),
                    None,
                    None,
                    None,
                );
                let _ = self.tx.try_send(None); // Reset diffing on resize
            }
            event @ RatatuiEvent::Key { .. } => {
                debug!("Ratatui Key Event: {:?}", event);
                state
                    .process_input_event::<RatatuiInputBackend>(InputEvent::Keyboard { event: event.into() });
            }
            RatatuiEvent::Mouse(event) => {
                debug!("Ratatui Mouse Event: {:?}", event);
                let e = RatatuiMouseEvent::new(event, self.backend.window_size());
                let event = match event.kind {
                    crossterm::event::MouseEventKind::Down(_) | crossterm::event::MouseEventKind::Up(_) => {
                        InputEvent::PointerButton { event: e }
                    }
                    crossterm::event::MouseEventKind::Drag(_) | crossterm::event::MouseEventKind::Moved => {
                        InputEvent::PointerMotionAbsolute { event: e }
                    }
                    crossterm::event::MouseEventKind::ScrollDown
                    | crossterm::event::MouseEventKind::ScrollUp
                    | crossterm::event::MouseEventKind::ScrollLeft
                    | crossterm::event::MouseEventKind::ScrollRight => InputEvent::PointerAxis { event: e },
                };
                state.process_input_event::<RatatuiInputBackend>(event);
            }
        }
    }
}

pub fn init_ratatui(
    event_loop: &mut EventLoop<CalloopData>,
    data: &mut CalloopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &mut data.display_handle;
    let state = &mut data.state;

    let backend = ratatui::RatatuiBackend::new()?;

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
    let adapter =
        pollster::block_on(wgpu_instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (wgpu_device, wgpu_queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let wgpu_device = Arc::new(wgpu_device);
    let wgpu_queue = Arc::new(wgpu_queue);

    let ansi_encoder =
        pollster::block_on(GpuAnsiEncoder::new(wgpu_device.clone(), wgpu_queue.clone())).unwrap();
    let gpu_renderer = GpuRenderer::new(wgpu_device.clone(), wgpu_queue.clone());

    // Get raw Vulkan device for imports
    let ash_instance = unsafe {
        wgpu_instance
            .as_hal::<wgpu_hal::api::Vulkan>()
            .expect("Failed to get Vulkan instance from wgpu")
            .shared_instance()
            .raw_instance()
    };
    let (ash_device, ash_pdev) = unsafe {
        wgpu_device
            .as_hal::<wgpu_hal::api::Vulkan>()
            .map(|d| (d.raw_device().clone(), d.raw_physical_device()))
            .expect("Failed to get Vulkan device from wgpu")
    };
    let vulkan_import = VulkanImport::new(Arc::new(ash_device), ash_instance, ash_pdev);

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    let (tx, mut rx) = mpsc::channel::<Option<wgpu::Texture>>(2);
    let ansi_encoder = Arc::new(ansi_encoder);

    tokio::spawn(async move {
        let mut previous_texture: Option<wgpu::Texture> = None;
        let mut frames = 0;
        let mut render_start = std::time::Instant::now();

        while let Some(msg) = rx.recv().await {
            if let Some(current_texture) = msg {
                let ansi_string = ansi_encoder
                    .ansi_from_texture(previous_texture.as_ref(), &current_texture)
                    .await
                    .unwrap();
                print!("{}", &*ansi_string);
                let _ = std::io::stdout().flush();
                previous_texture = Some(current_texture);

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
            } else {
                previous_texture = None;
            }
        }
    });

    let mut handler = RatatuiHandler {
        wgpu_device,
        wgpu_queue,
        gpu_renderer,
        vulkan_import,
        output,
        backend,
        tx,
    };

    event_loop
        .handle()
        .insert_source(
            handler.backend.event_source(Duration::from_micros(
                1_000_000_000 / u64::try_from(mode.refresh).unwrap(),
            )),
            move |event, _, data| {
                handler.handle_event(event, &mut data.state, &mut data.display_handle);
            },
        )
        .unwrap();

    Ok(())
}
