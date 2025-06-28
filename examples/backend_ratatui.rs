use smithay::backend::ratatui::RatatuiBackend;

fn main() -> Result<(), std::io::Error> {
    let mut backend = RatatuiBackend::new()?;

    loop {
        backend.draw()?;
        if backend.handle_input()? {
            break;
        }
    }

    Ok(())
}
