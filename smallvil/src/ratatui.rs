use std::time::Duration;

use crossterm::event::Event;
use smithay::{
    backend::{
        ratatui,
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement, gles::GlesRenderer,
            Renderer,
        },
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Rectangle, Size, Transform},
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
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);

    state.space.map_output(&output, (0, 0));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);

    event_loop.handle().insert_source(backend, move |event, _, data| {
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
            Event::Key(event) => state.process_input_event(event),
            Event::Mouse(event) => state.process_input_event(event),
        }
    });

    event_loop.handle().insert_idle(|data| {
        let display = &mut data.display_handle;
        let state = &mut data.state;

        smithay::desktop::space::render_output(
            &output,
            backend.renderer(),
            &mut framebuffer,
            1.0,
            0,
            [&state.space],
            &[],
            &mut damage_tracker,
            [0.1, 0.1, 0.1, 1.0],
        )
        .unwrap();

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
    });

    Ok(())
}
