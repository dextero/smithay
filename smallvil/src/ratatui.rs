use std::os::fd::FromRawFd;
use std::{fs::File, io::Write};
use std::sync::Arc;
use std::time::Duration;

use smithay::{
    backend::{
        input::InputEvent,
        ratatui::{self, RatatuiEvent, RatatuiInputBackend, RatatuiMouseEvent},
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            wgpu::{WgpuRenderer, WgpuTexture},
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        wayland_server::DisplayHandle,
    },
    utils::Transform,
};
use tracing::debug;

use crate::{CalloopData, Smallvil};
use gpu_ansi_encoder::GpuAnsiEncoder;

use tokio::sync::mpsc;

struct RatatuiHandler {
    renderer: WgpuRenderer,
    output: Output,
    damage_tracker: OutputDamageTracker,
    backend: ratatui::RatatuiBackend,

    tx: mpsc::Sender<Option<wgpu::Texture>>,
}

impl RatatuiHandler {
    fn redraw(&mut self, state: &mut Smallvil, display: &mut DisplayHandle) {
        let screen_size = self.output.current_mode().unwrap().size;

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
        
        let screen_texture = self.renderer.device().create_texture(&screen_desc);
        let screen_view = screen_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut screen_wgpu_texture = WgpuTexture::new(
            screen_texture.clone(),
            screen_view,
            (screen_size.w, screen_size.h).into(),
            None,
            false,
        );

        smithay::desktop::space::render_output::<
            _,
            WaylandSurfaceRenderElement<WgpuRenderer>,
            _,
            _,
        >(
            &self.output,
            &mut self.renderer,
            &mut screen_wgpu_texture,
            1.0,
            0,
            [&state.space],
            &[],
            &mut self.damage_tracker,
            [0.1, 0.1, 0.1, 1.0],
        )
        .unwrap();

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
    let renderer = WgpuRenderer::new(wgpu_device.clone(), wgpu_queue.clone());

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