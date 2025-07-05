use std::time::Duration;

use ::ratatui::{buffer::Buffer, layout::Rect};
use crossterm::event::Event;
use smithay::{
    backend::{
        input::InputEvent, ratatui::{self, RatatuiInputBackend}, renderer::{
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            ratatui::{RatatuiRenderer, RatatuiTexture},
            Color32F,
        }
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
    let mut framebuffer = Some(RatatuiTexture::from(Buffer::empty(Rect::new(0, 0, size.w as u16, size.h as u16))));

    let output = output.clone();
    event_loop.handle().insert_source(backend.event_source(), move |event, _, data| {
        let display = &mut data.display_handle;
        let state = &mut data.state;

        match event {
            Event::Resize(width, height) => {
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
            Event::Key(event) => {
                state.process_input_event::<RatatuiInputBackend>(InputEvent::Keyboard {
                    event: event.into()
                });
            }
            Event::Mouse(event) => {
                let event = match event.kind {
                    crossterm::event::MouseEventKind::Down(_)
                    | crossterm::event::MouseEventKind::Up(_) => {
                        InputEvent::PointerButton { event: event.into() }
                    },
                    crossterm::event::MouseEventKind::Drag(_)
                    | crossterm::event::MouseEventKind::Moved => {
                        InputEvent::PointerMotionAbsolute { event: event.into() }
                    }
                    crossterm::event::MouseEventKind::ScrollDown
                    | crossterm::event::MouseEventKind::ScrollUp
                    | crossterm::event::MouseEventKind::ScrollLeft
                    | crossterm::event::MouseEventKind::ScrollRight => {
                        InputEvent::PointerAxis { event: event.into() }
                    }
                };
                state.process_input_event::<RatatuiInputBackend>(event);
            }
            _ => {}
        }

        smithay::desktop::space::render_output::<_, WaylandSurfaceRenderElement<RatatuiRenderer>, _, _>(
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
        framebuffer = Some(backend.renderer().swap_buffers(framebuffer.take().unwrap()).unwrap());

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
    }).unwrap();

    Ok(())
}
