//! A backend for smithay that renders to a tty.
use calloop::{EventSource, Interest, Mode, PostAction};

use crate::{backend::renderer::ratatui::RatatuiRenderer, utils::Size};
use std::{io, time::Duration};

/// A backend for smithay that renders to a tty.
#[derive(Debug)]
pub struct RatatuiBackend {
    renderer: RatatuiRenderer,
}

impl RatatuiBackend {
    /// Create a new ratatui backend.
    pub fn new() -> Result<Self, io::Error> {
        let renderer = RatatuiRenderer::new();
        Ok(RatatuiBackend {
            renderer,
        })
    }

    /// Get a mutable reference to the renderer.
    pub fn renderer(&mut self) -> &mut RatatuiRenderer {
        &mut self.renderer
    }

    /// Return window size, in cells
    pub fn window_size(&self) -> Size<i32, crate::utils::Physical> {
        self.renderer.window_size()
    }

    pub fn event_source(&self) -> RatatuiEventSource {
        RatatuiEventSource { event_token: None }
    }
}

pub struct RatatuiEventSource {
    event_token: Option<calloop::Token>,
}

impl EventSource for RatatuiEventSource {
    type Event = crossterm::event::Event;

    type Metadata = ();

    type Ret = ();

    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn process_events<F>(
        &mut self,
        readiness: calloop::Readiness,
        _token: calloop::Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        if readiness.error {
            // TODO?
            return Ok(PostAction::Disable);
        }
        if !readiness.readable {
            return Ok(PostAction::Continue);
        }

        while crossterm::event::poll(Duration::from_millis(0))? {
            callback(crossterm::event::read()?, &mut ());
        }
        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut calloop::Poll,
        token_factory: &mut calloop::TokenFactory,
    ) -> calloop::Result<()> {
        let token = token_factory.token();
        // SAFETY: stdin stays valid for the entire process lifetime.
        unsafe {
            poll.register(std::io::stdin(), Interest::READ, Mode::Level, token)?;
        };
        self.event_token = Some(token);
        Ok(())
    }

    fn reregister(
        &mut self,
        poll: &mut calloop::Poll,
        token_factory: &mut calloop::TokenFactory,
    ) -> calloop::Result<()> {
        self.unregister(poll)?;
        self.register(poll, token_factory)?;
        Ok(())
    }

    fn unregister(&mut self, poll: &mut calloop::Poll) -> calloop::Result<()> {
        poll.unregister(std::io::stdin())?;
        Ok(())
    }
}
