use smithay::backend::renderer::Renderer;
use smithay::backend::ratatui::RatatuiBackend;
use smithay::reexports::calloop::{EventLoop, LoopSignal};
use smithay::reexports::wayland_server::Display;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocket;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop = EventLoop::try_new()?;
    let mut display = Display::new()?;
    let _shm_state = ShmState::new(&mut display, vec![]);
    let _compositor_state = CompositorState::new(&mut display);

    let socket = ListeningSocket::bind("wayland-5").unwrap();

    let mut backend = RatatuiBackend::new()?;

    let loop_signal = event_loop.get_signal();

    loop {
        let mut frame = backend.renderer().render(backend.renderer().size(), backend.renderer().transform())?;
        frame.clear([0.0, 0.0, 0.0, 1.0])?;
        frame.finish()?;

        if backend.handle_input()? {
            break;
        }

        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut ())?;
        display.flush_clients()?;
    }

    Ok(())
}