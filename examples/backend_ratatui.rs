use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::{ratatui::RatatuiBackend, renderer::Color32F};
use smithay::output::{Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::EventLoop;
use wayland_server::{Display, ListeningSocket};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop = EventLoop::try_new()?;
    let mut display = Display::<()>::new()?;
    let display_handle = display.handle();

    let output = Output::new(
        "ratatui".to_string(),
        PhysicalProperties {
            size: (200, 100).into(),
            subpixel: Subpixel::None,
            make: "Smithay".into(),
            model: "Ratatui".into(),
        },
    );
    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    let socket = ListeningSocket::bind("wayland-5").unwrap();

    let mut backend = RatatuiBackend::new()?;

    let size = backend.renderer().window_size();
    let mut framebuffer = RatatuiTexture::from(Buffer::new(size.w as u16, size.h as u16));

    loop {
        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut ())?;

        let renderer = backend.renderer();
        smithay::desktop::space::render_output(
            &output,
            renderer,
            &mut framebuffer,
            0.5f32,
            0,
            None,
            &[],
            &mut damage_tracker,
            Color32F::BLACK,
        )?;
        framebuffer = renderer.swap_buffers(framebuffer);

        display.flush_clients()?;
    }
}
