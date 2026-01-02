use std::os::fd::FromRawFd;
use std::{fs::File, io::Write, path::Path};
use image::{ImageBuffer, Rgba};
use std::sync::Arc;
use std::time::Duration;

use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc, Modifier, Allocator,
        },
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        ratatui::{self, RatatuiEvent, RatatuiInputBackend, RatatuiMouseEvent},
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
            Bind, ImportDma,
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::DisplayHandle,
    },
    utils::Transform,
};
use crate::wgpu_renderer::WgpuRenderer;
use tracing::{debug, error};

use crate::{CalloopData, Smallvil};
use gpu_ansi_encoder::GpuAnsiEncoder;

use tokio::sync::mpsc;

struct RatatuiHandler {
    renderer: GlesRenderer,
    wgpu_renderer: WgpuRenderer,
    allocator: GbmAllocator<Arc<File>>,
    output: Output,
    damage_tracker: OutputDamageTracker,
    backend: ratatui::RatatuiBackend,
    tx: mpsc::Sender<Option<wgpu::Texture>>,
}

impl RatatuiHandler {
    fn handle_event(&mut self, event: RatatuiEvent, state: &mut Smallvil, display: &mut DisplayHandle) {
        match event {
            RatatuiEvent::Redraw => {
                self.redraw(state, display);
            }
            RatatuiEvent::Resize(_, _) => {
                let size = self.backend.window_size();
                self.output.change_current_state(
                    Some(Mode {
                        size,
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

    fn redraw(&mut self, state: &mut Smallvil, display: &mut DisplayHandle) {
        let size = self.output.current_mode().unwrap().size;
        
        // Allocate a Dmabuf
        let mut dmabuf = match self.allocator.create_buffer(
            size.w as u32,
            size.h as u32,
            Fourcc::Xrgb8888,
            &[Modifier::Linear],
        ) {
            Ok(bo) => {
                use smithay::backend::allocator::dmabuf::AsDmabuf;
                bo.export().expect("Failed to export dmabuf")
            },
            Err(err) => {
                error!("Failed to allocate dmabuf: {}", err);
                return;
            }
        };

        // Bind and render
        {
            let mut target = self.renderer.bind(&mut dmabuf).expect("Failed to bind dmabuf");
            
            smithay::desktop::space::render_output(
                &self.output,
                &mut self.renderer,
                &mut target,
                1.0,
                0,
                [&state.space],
                &[] as &[WaylandSurfaceRenderElement<GlesRenderer>],
                &mut self.damage_tracker,
                [0.1f32, 0.1, 0.4, 1.0],
            ).expect("Failed to render output");
        }

        // Import to WGPU
        let wgpu_texture = self.wgpu_renderer.import_dmabuf(&dmabuf, None).expect("Failed to import dmabuf to wgpu");
        
        // Send to encoding task
        let _ = self.tx.try_send(Some(wgpu_texture.wgpu_texture().clone()));

        // Frame callbacks
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
}

pub async fn save_texture_to_file(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    path: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let size = texture.size();
    let width = size.width;
    let height = size.height;

    let bytes_per_pixel = 4;
    let unpadded_bytes_per_row = width * bytes_per_pixel;
    let align = 256;
    let padded_bytes_per_row_padding = (align - unpadded_bytes_per_row % align) % align;
    let padded_bytes_per_row = unpadded_bytes_per_row + padded_bytes_per_row_padding;

    let buffer_size = (padded_bytes_per_row * height) as u64;
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Texture Save Staging Buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Texture Save Encoder"),
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        size,
    );

    queue.submit(std::iter::once(encoder.finish()));

    let buffer_slice = staging_buffer.slice(..);
    let (tx, rx) = tokio::sync::oneshot::channel();
    buffer_slice.map_async(wgpu::MapMode::Read, move |v| {
        tx.send(v).ok();
    });
    device.poll(wgpu::PollType::wait_indefinitely());

    if let Ok(Ok(())) = rx.await {
        let data = buffer_slice.get_mapped_range();
        let mut result = Vec::with_capacity((width * height * bytes_per_pixel) as usize);

        for chunk in data.chunks(padded_bytes_per_row as usize) {
            result.extend_from_slice(&chunk[..unpadded_bytes_per_row as usize]);
        }

        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        
        for (idx, pixel) in result.chunks_exact(4).enumerate() {
            let x = (idx as u32) % width;
            let y = (idx as u32) / width;
            // BGRA -> RGBA
            img.put_pixel(x, y, Rgba([pixel[2], pixel[1], pixel[0], 255]));
        }
        
        img.save(path)?;
        drop(data);
        staging_buffer.unmap();
        Ok(())
    } else {
        Err("Failed to map buffer".into())
    }
}

pub fn init_ratatui(
    event_loop: &mut EventLoop<CalloopData>,
    data: &mut CalloopData,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &mut data.display_handle;
    let state = &mut data.state;

    // DRM / GBM / EGL / GLES Setup
    // Manual scan for render node since DrmNode::ty() is acting up
    let drm_node = (128..136)
        .map(|i| format!("/dev/dri/renderD{}", i))
        .filter_map(|path| smithay::backend::drm::DrmNode::from_path(path).ok())
        .next()
        .ok_or_else(|| Box::<dyn std::error::Error>::from("No render node found"))?;
    
    let fd = Arc::new(std::fs::OpenOptions::new().read(true).write(true).open(drm_node.dev_path().unwrap())?);
    let gbm_egl = GbmDevice::new(fd.clone())?;
    let gbm_alloc = GbmDevice::new(fd)?;
    
    let egl_display = unsafe { EGLDisplay::new(gbm_egl) }.expect("Failed to create EGLDisplay");
    let egl_context = EGLContext::new(&egl_display).expect("Failed to create EGLContext");
    let renderer = unsafe { GlesRenderer::new(egl_context).expect("Failed to create GlesRenderer") };
    let allocator = GbmAllocator::new(gbm_alloc, GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT);

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

    // WGPU Initialization (for encoding)
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
    let wgpu_renderer = WgpuRenderer::new(&wgpu_instance, wgpu_device.clone(), wgpu_queue.clone());

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    let (tx, mut rx) = mpsc::channel::<Option<wgpu::Texture>>(2);
    let ansi_encoder = Arc::new(ansi_encoder);

    tokio::spawn(async move {
        let mut previous_texture: Option<wgpu::Texture> = None;
        let mut frames = 0;
        let mut render_start = std::time::Instant::now();
        let mut stdout = unsafe { File::from_raw_fd(1) };

        while let Some(msg) = rx.recv().await {
            if let Some(current_texture) = msg {
                let ansi_string = ansi_encoder
                    .ansi_from_texture(previous_texture.as_ref(), &current_texture)
                    .await
                    .unwrap();
                {
                    const START_BUFFERING: &str = "\x1b[?2026h";
                    const STOP_BUFFERING: &str = "\x1b[?2026l";
                    stdout.write_all(START_BUFFERING.as_bytes()).unwrap();
                    stdout.write_all(ansi_string.as_bytes()).unwrap();
                    stdout.write_all(STOP_BUFFERING.as_bytes()).unwrap();
                    let _ = stdout.flush().unwrap();
                }
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

    let damage_tracker = OutputDamageTracker::from_output(&output);

    let mut handler = RatatuiHandler {
        renderer,
        wgpu_renderer,
        allocator,
        output,
        damage_tracker,
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
