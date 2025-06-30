//! A backend for smithay that renders to a tty.
use crate::backend::renderer::ratatui::RatatuiRenderer;
use crossterm::
    event::{self, Event, KeyCode}
;
use ratatui::{prelude::{CrosstermBackend, Terminal}, widgets::Paragraph};
use std::{io, time::Duration};

/// A backend for smithay that renders to a tty.
#[derive(Debug)]
pub struct RatatuiBackend {
    renderer: RatatuiRenderer,
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl RatatuiBackend {
    /// Create a new ratatui backend.
    pub fn new() -> Result<Self, io::Error> {
        let terminal = ratatui::init();
        let renderer = RatatuiRenderer;
        Ok(RatatuiBackend { renderer, terminal })
    }

    /// Draw to the terminal.
    pub fn draw(&mut self) -> Result<(), io::Error> {
        self.terminal.draw(|f| {
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
    pub fn renderer(&mut self) -> &mut RatatuiRenderer {
        &mut self.renderer
    }
}

impl Drop for RatatuiBackend {
    fn drop(&mut self) {
        ratatui::restore();
    }
}