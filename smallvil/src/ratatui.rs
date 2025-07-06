use std::time::Duration;

use ::ratatui::{buffer::Buffer, layout::Rect};
use crossterm::event::{KeyCode, KeyModifiers, KeyEventKind};
use smithay::{
    backend::{
        input::InputEvent,
        ratatui::{self, RatatuiEvent, RatatuiInputBackend, RatatuiMouseEvent},
        renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            ratatui::{RatatuiRenderer, RatatuiTexture},
            Color32F,
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Size, Transform},
};

use crate::{CalloopData, Smallvil};

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
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "Ratatui".into(),
        },
    );
    let _global = output.create_global::<Smallvil>(display_handle);
    output.change_current_state(Some(mode), Some(Transform::Normal), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    let size = backend.renderer().window_size();
    eprintln!("window size: {size:?}");
    let mut framebuffer = Some(RatatuiTexture::from(Buffer::empty(Rect::new(
        0,
        0,
        u16::try_from(size.w).unwrap(),
        u16::try_from(size.h).unwrap(),
    ))));

    let mut frames = 0;
    let mut render_start = std::time::Instant::now();

    let output = output.clone();
    event_loop
        .handle()
        .insert_source(
            backend.event_source(Duration::from_micros(1_000_000_000 / u64::try_from(mode.refresh).unwrap())),
            move |event, _, data| {
                let display = &mut data.display_handle;
                let state = &mut data.state;

                match event {
                    RatatuiEvent::Redraw => {
                        if frames == 0 {
                            render_start = std::time::Instant::now();
                        }
                        frames += 1;
                        let render_end = std::time::Instant::now();
                        eprintln!("FPS = {}", frames as f64 / render_end.duration_since(render_start).as_secs_f64());

                        smithay::desktop::space::render_output::<
                            _,
                            WaylandSurfaceRenderElement<RatatuiRenderer>,
                            _,
                            _,
                        >(
                            &output,
                            backend.renderer(),
                            framebuffer.as_mut().unwrap(),
                            1.0,
                            0,
                            [&state.space],
                            &[],
                            &mut damage_tracker,
                            Color32F::BLACK,
                        )
                        .unwrap();
                        framebuffer = Some(
                            backend
                                .renderer()
                                .swap_buffers(framebuffer.take().unwrap())
                                .unwrap(),
                        );

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
                    RatatuiEvent::Resize(width, height) => {
                        output.change_current_state(
                            Some(Mode {
                                size: Size::new(width.into(), height.into()),
                                refresh: 60_000,
                            }),
                            None,
                            None,
                            None,
                        );
                    }
                    RatatuiEvent::Key(mut event) => {
                        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            state.loop_signal.stop();
                        }

                        state.process_input_event::<RatatuiInputBackend>(InputEvent::Keyboard { event: event.clone().into() });

                        event.kind = KeyEventKind::Release;
                        state.process_input_event::<RatatuiInputBackend>(InputEvent::Keyboard { event: event.into() });
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
                        eprintln!("event: {event:?}");
                        state.process_input_event::<RatatuiInputBackend>(event);
                    }
                    _ => {}
                }
            },
        )
        .unwrap();

    Ok(())
}
