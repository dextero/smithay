//! A backend for smithay that renders to a tty.
use calloop::{EventSource, Interest, Mode, PostAction};

use crate::{
    backend::renderer::ratatui::RatatuiRenderer,
    utils::Size,
};
use std::{
    io,
    time::{Duration, Instant},
};

/// A backend for smithay that renders to a tty.
#[derive(Debug)]
pub struct RatatuiBackend {
    renderer: RatatuiRenderer,
}

impl RatatuiBackend {
    /// Create a new ratatui backend.
    pub fn new() -> Result<Self, io::Error> {
        let renderer = RatatuiRenderer::new();
        Ok(RatatuiBackend { renderer })
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

/// TODO doc
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

mod input {
    use std::time::Instant;

    use crossterm::event::{KeyCode, KeyEventKind, MouseButton, MouseEventKind};

    use crate::backend::input;

    /// TODO doc
    #[derive(Debug)]
    pub struct Backend;

    impl input::InputBackend for Backend {
        type Device = Device;

        type KeyboardKeyEvent = KeyEvent;

        type PointerAxisEvent = MouseEvent;
        type PointerButtonEvent = MouseEvent;
        type PointerMotionEvent = MouseEvent;
        type PointerMotionAbsoluteEvent = MouseEvent;

        type GestureSwipeBeginEvent = input::UnusedEvent;
        type GestureSwipeUpdateEvent = input::UnusedEvent;
        type GestureSwipeEndEvent = input::UnusedEvent;
        type GesturePinchBeginEvent = input::UnusedEvent;
        type GesturePinchUpdateEvent = input::UnusedEvent;
        type GesturePinchEndEvent = input::UnusedEvent;
        type GestureHoldBeginEvent = input::UnusedEvent;
        type GestureHoldEndEvent = input::UnusedEvent;
        type TouchDownEvent = input::UnusedEvent;
        type TouchUpEvent = input::UnusedEvent;
        type TouchMotionEvent = input::UnusedEvent;
        type TouchCancelEvent = input::UnusedEvent;
        type TouchFrameEvent = input::UnusedEvent;
        type TabletToolAxisEvent = input::UnusedEvent;
        type TabletToolProximityEvent = input::UnusedEvent;
        type TabletToolTipEvent = input::UnusedEvent;
        type TabletToolButtonEvent = input::UnusedEvent;
        type SwitchToggleEvent = input::UnusedEvent;
        type SpecialEvent = input::UnusedEvent;
    }

    #[derive(Debug, Clone, Hash, PartialEq, Eq)]
    pub struct Device;

    impl input::Device for Device {
        fn id(&self) -> String {
            "ratatui-input-device-id".to_owned()
        }

        fn name(&self) -> String {
            "ratatui-input-device".to_owned()
        }

        fn has_capability(&self, capability: input::DeviceCapability) -> bool {
            match capability {
                input::DeviceCapability::Keyboard => true,
                input::DeviceCapability::Pointer => true,
                _ => false,
            }
        }

        fn usb_id(&self) -> Option<(u32, u32)> {
            None
        }

        fn syspath(&self) -> Option<std::path::PathBuf> {
            None
        }
    }

    /// TODO doc
    #[derive(Debug)]
    pub struct KeyEvent {
        time: Instant,
        event: crossterm::event::KeyEvent,
    }

    impl From<crossterm::event::KeyEvent> for KeyEvent {
        fn from(event: crossterm::event::KeyEvent) -> Self {
            Self {
                time: Instant::now(),
                event,
            }
        }
    }

    impl crate::backend::input::Event<Backend> for KeyEvent {
        fn time(&self) -> u64 {
            self.time.elapsed().as_millis() as u64
        }

        fn device(&self) -> <Backend as input::InputBackend>::Device {
            Device
        }
    }

    impl input::KeyboardKeyEvent<Backend> for KeyEvent {
        fn key_code(&self) -> xkbcommon::xkb::Keycode {
            let code: u32 = match self.event.code {
                KeyCode::Char(c) if ('A'..='Z').contains(&c) => 4 + (c as u32 - b'A' as u32),
                KeyCode::Char(c) if ('a'..='z').contains(&c) => 4 + (c as u32 - b'a' as u32),
                KeyCode::Char(c) if ('1'..='9').contains(&c) => 30 + (c as u32 - b'1' as u32),
                KeyCode::Char('0') => 39,
                KeyCode::Enter => 40,
                KeyCode::Esc => 41,
                KeyCode::Backspace => 42,
                KeyCode::Tab => 43,
                KeyCode::Char(' ') => 44,
                KeyCode::Char('-') => 45,
                KeyCode::Char('=') => 46,
                KeyCode::Char('[') => 47,
                KeyCode::Char(']') => 48,
                KeyCode::Char('\\') => 49,
                KeyCode::Char(';') => 51,
                KeyCode::Char('\'') => 52,
                KeyCode::Char('`') => 53,
                KeyCode::Char(',') => 54,
                KeyCode::Char('.') => 55,
                KeyCode::Char('/') => 56,
                KeyCode::CapsLock => 57,
                KeyCode::F(n) if (1..=12).contains(&n) => 57 + n as u32,
                KeyCode::PrintScreen => 70,
                KeyCode::ScrollLock => 71,
                KeyCode::Pause => 72,
                KeyCode::Insert => 73,
                KeyCode::Home => 74,
                KeyCode::PageUp => 75,
                KeyCode::Delete => 76,
                KeyCode::End => 77,
                KeyCode::PageDown => 78,
                KeyCode::Right => 79,
                KeyCode::Left => 80,
                KeyCode::Down => 81,
                KeyCode::Up => 82,
                KeyCode::NumLock => 83,
                _ => /* duuno, handle as esc? */ 41,
            };
            code.into()
        }

        fn state(&self) -> input::KeyState {
            match self.event.kind {
                KeyEventKind::Press | KeyEventKind::Repeat => input::KeyState::Pressed,
                KeyEventKind::Release => input::KeyState::Released,
            }
        }

        fn count(&self) -> u32 {
            match self.event.kind {
                KeyEventKind::Press | KeyEventKind::Repeat => 1,
                KeyEventKind::Release => 0,
            }
        }
    }

    /// TODO doc
    #[derive(Debug)]
    pub struct MouseEvent {
        time: Instant,
        event: crossterm::event::MouseEvent,
    }

    impl From<crossterm::event::MouseEvent> for MouseEvent {
        fn from(event: crossterm::event::MouseEvent) -> Self {
            Self {
                time: Instant::now(),
                event,
            }
        }
    }

    impl input::Event<Backend> for MouseEvent {
        fn time(&self) -> u64 {
            self.time.elapsed().as_millis() as u64
        }

        fn device(&self) -> <Backend as crate::backend::input::InputBackend>::Device {
            Device
        }
    }

    impl input::PointerAxisEvent<Backend> for MouseEvent {
        fn amount(&self, _axis: input::Axis) -> Option<f64> {
            None
        }

        fn amount_v120(&self, _axis: input::Axis) -> Option<f64> {
            None
        }

        fn source(&self) -> input::AxisSource {
            input::AxisSource::Wheel
        }

        fn relative_direction(&self, _axis: input::Axis) -> input::AxisRelativeDirection {
            input::AxisRelativeDirection::Identical
        }
    }

    impl input::PointerMotionEvent<Backend> for MouseEvent {
        fn delta_x(&self) -> f64 {
            0.0f64
        }

        fn delta_y(&self) -> f64 {
            0.0f64
        }

        fn delta_x_unaccel(&self) -> f64 {
            0.0f64
        }

        fn delta_y_unaccel(&self) -> f64 {
            0.0f64
        }
    }

    impl input::AbsolutePositionEvent<Backend> for MouseEvent {
        fn x(&self) -> f64 {
            self.event.column as _
        }

        fn y(&self) -> f64 {
            self.event.row as _
        }

        fn x_transformed(&self, _width: i32) -> f64 {
            0.0f64
        }

        fn y_transformed(&self, _height: i32) -> f64 {
            0.0f64
        }
    }

    impl input::PointerMotionAbsoluteEvent<Backend> for MouseEvent {}

    impl input::PointerButtonEvent<Backend> for MouseEvent {
        fn button_code(&self) -> u32 {
            const BTN_LEFT: u32 = 0x110;
            const BTN_RIGHT: u32 = 0x111;
            const BTN_MIDDLE: u32 = 0x112;

            match self.event.kind {
                MouseEventKind::Down(MouseButton::Left)
                | MouseEventKind::Up(MouseButton::Left)
                | MouseEventKind::Drag(MouseButton::Left) => BTN_LEFT,
                MouseEventKind::Down(MouseButton::Right)
                | MouseEventKind::Up(MouseButton::Right)
                | MouseEventKind::Drag(MouseButton::Right) => BTN_RIGHT,
                MouseEventKind::Down(MouseButton::Middle)
                | MouseEventKind::Up(MouseButton::Middle)
                | MouseEventKind::Drag(MouseButton::Middle) => BTN_MIDDLE,
                _ => todo!(),
            }
        }

        fn state(&self) -> input::ButtonState {
            match self.event.kind {
                MouseEventKind::Down(_) => input::ButtonState::Pressed,
                MouseEventKind::Drag(_) => input::ButtonState::Pressed,
                MouseEventKind::Up(_) => input::ButtonState::Released,
                _ => todo!(),
            }
        }
    }
}

pub use input::Backend as RatatuiInputBackend;