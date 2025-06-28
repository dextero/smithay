//! A backend for smithay that renders to a tty.
use crate::backend::renderer::ratatui::RatatuiRenderer;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::{CrosstermBackend, Terminal};
use std::{io, time::Duration};

/// A backend for smithay that renders to a tty.
#[derive(Debug)]
pub struct RatatuiBackend {
    renderer: RatatuiRenderer<CrosstermBackend<io::Stdout>>,
}

impl RatatuiBackend {
    /// Create a new ratatui backend.
    pub fn new() -> Result<Self, io::Error> {
        let mut stdout = io::stdout();
        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        let renderer = RatatuiRenderer::new(terminal);
        Ok(RatatuiBackend { renderer })
    }

    /// Draw to the terminal.
    pub fn draw(&mut self) -> Result<(), io::Error> {
        self.renderer.terminal.draw(|f| {
            let size = f.size();
            let block = Paragraph::new("Hello, world!");
            f.render_widget(block, size);
        })?;
        Ok(())
    }

    /// Handle input from the terminal.
    ///
    /// Returns `true` if the compositor should exit.
    pub fn handle_input(&self) -> Result<bool, io::Error> {
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Get a mutable reference to the renderer.
    pub fn renderer(&mut self) -> &mut RatatuiRenderer<CrosstermBackend<io::Stdout>> {
        &mut self.renderer
    }
}

impl Drop for RatatuiBackend {
    fn drop(&mut self) {
        disable_raw_mode().unwrap();
        execute!(self.renderer.terminal.backend_mut(), LeaveAlternateScreen).unwrap();
        self.renderer.terminal.show_cursor().unwrap();
    }
}
