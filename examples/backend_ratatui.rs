use smithay::backend::renderer::{Renderer, Frame};
use smithay::backend::ratatui::RatatuiBackend;
use smithay::reexports::calloop::EventLoop;
use wayland_server::{Display, socket::ListeningSocket, DisplayHandle};
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::shm::ShmState;
use smithay::utils::{Size, Physical, Transform};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop = EventLoop::try_new()?;
    let mut display = Display::new()?;
    let display_handle = display.handle();
    let _shm_state = ShmState::new(&display_handle, vec![]);
    let _compositor_state = CompositorState::new(&display_handle);

    let socket = ListeningSocket::bind("wayland-5").unwrap();

    let mut backend = RatatuiBackend::new()?;

    let output_size = Size::from((80, 24)); // Example size
    let dst_transform = Transform::Normal; // Example transform

    loop {
        let mut frame = backend.renderer().render(backend.renderer().terminal.backend_mut().buffer(), output_size, dst_transform)?;
        frame.clear([0.0, 0.0, 0.0, 1.0], &[])?;
        frame.finish()?;

        if backend.handle_input()? {
            break;
        }

        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut ())?;
        display.flush_clients()?;
    }

    Ok(())
}
