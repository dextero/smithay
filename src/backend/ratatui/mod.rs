//! A backend for smithay that renders to a tty.
use calloop::{EventSource, Interest, Mode, PostAction};
use timerfd::{SetTimeFlags, TimerFd, TimerState};

use crate::{backend::renderer::ratatui::RatatuiRenderer, utils::Size};
use std::{
    io,
    os::{fd::AsFd, unix::prelude::BorrowedFd},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Timer {
    interval: Duration,
    token: calloop::Token,
    timer: timerfd::TimerFd,
}

impl Timer {
    pub fn new(token: calloop::Token, interval: Duration) -> Self {
        let mut timer = Self {
            interval,
            token,
            timer: TimerFd::new().unwrap(),
        };
        timer.reset();
        timer
    }

    pub fn reset(&mut self) {
        let state = TimerState::Periodic {
            current: Instant::now().elapsed(),
            interval: self.interval,
        };
        self.timer.set_state(state, SetTimeFlags::Default);
    }

    pub fn set_interval(&mut self, interval: Duration) {
        self.interval = interval;
        self.reset();
    }
}

impl AsFd for Timer {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.timer.as_fd()
    }
}

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

    /// TODO doc
    pub fn event_source(&self, refresh_interval: Duration) -> RatatuiEventSource {
        RatatuiEventSource {
            event_token: None,
            timer: None,
            refresh_interval,
        }
    }
}

/// TODO doc
#[derive(Debug)]
pub struct RatatuiEventSource {
    event_token: Option<calloop::Token>,
    timer: Option<Timer>,
    refresh_interval: Duration,
}

/// TODO doc
#[derive(Debug)]
pub enum RatatuiEvent {
    /// TODO doc
    Redraw,
    /// TODO doc
    Resize(u16, u16),
    /// TODO doc
    Key(crossterm::event::KeyEvent),
    /// TODO doc
    Mouse(crossterm::event::MouseEvent),
}

impl EventSource for RatatuiEventSource {
    type Event = RatatuiEvent;

    type Metadata = ();

    type Ret = ();

    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn process_events<F>(
        &mut self,
        readiness: calloop::Readiness,
        token: calloop::Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        let data = &mut ();
        if let Some(ref timer) = self.timer {
            if token == timer.token {
                timer.timer.read();
                callback(RatatuiEvent::Redraw, data);
                return Ok(PostAction::Continue);
            }
        }

        if readiness.error {
            // TODO?
            return Ok(PostAction::Disable);
        }
        if !readiness.readable {
            return Ok(PostAction::Continue);
        }

        while crossterm::event::poll(Duration::from_millis(0))? {
            let event = match crossterm::event::read()? {
                crossterm::event::Event::Resize(width, height) => RatatuiEvent::Resize(width, height),
                crossterm::event::Event::Key(event) => RatatuiEvent::Key(event),
                crossterm::event::Event::Mouse(event) => RatatuiEvent::Mouse(event),
                _ => continue,
            };
            callback(event, data);
        }
        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut calloop::Poll,
        token_factory: &mut calloop::TokenFactory,
    ) -> calloop::Result<()> {
        let timer_token = token_factory.token();
        let timer = Timer::new(timer_token, self.refresh_interval);
        // SAFETY: TODO
        unsafe {
            poll.register(&timer, Interest::READ, Mode::Level, timer_token)?;
        }
        self.timer = Some(timer);
        tracing::debug!("timer registered with token {timer_token:?}");

        let token = token_factory.token();
        // SAFETY: stdin stays valid for the entire process lifetime.
        unsafe {
            poll.register(std::io::stdin(), Interest::READ, Mode::Level, token)?;
        };
        tracing::debug!("stdin registered with token {token:?}");
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
        self.event_token = None;
        tracing::debug!("stdin unregistered");

        if let Some(timer) = self.timer.take() {
            poll.unregister(timer)?;
            tracing::debug!("timer unregistered");
        }
        Ok(())
    }
}

mod input {
    use std::time::Instant;

    use crossterm::event::{KeyCode, KeyEventKind, MouseButton, MouseEventKind};

    use crate::{
        backend::input::{self, KeyboardKeyEvent},
        utils::Size,
    };

    /// TODO doc
    #[derive(Debug)]
    pub struct Backend;

    impl input::InputBackend for Backend {
        type Device = Device;

        type KeyboardKeyEvent = KeyEvent;

        type PointerAxisEvent = MouseEvent;
        type PointerButtonEvent = MouseEvent;
        type PointerMotionEvent = input::UnusedEvent;
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
            let ret = Self {
                time: Instant::now(),
                event,
            };
            tracing::trace!(
                "key event: code {:?}, state {:?}, count {:?}",
                ret.key_code(),
                ret.state(),
                ret.count()
            );
            ret
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
            // https://gitlab.freedesktop.org/libinput/libinput/-/blob/main/include/linux/linux/input-event-codes.h
            let code: u32 = match self.event.code {
                KeyCode::Esc => 1,
                KeyCode::Char('1') => 2,
                KeyCode::Char('2') => 3,
                KeyCode::Char('3') => 4,
                KeyCode::Char('4') => 5,
                KeyCode::Char('5') => 6,
                KeyCode::Char('6') => 7,
                KeyCode::Char('7') => 8,
                KeyCode::Char('8') => 9,
                KeyCode::Char('9') => 10,
                KeyCode::Char('0') => 11,
                KeyCode::Char('-') => 12,
                KeyCode::Char('=') => 13,
                KeyCode::Backspace => 14,
                KeyCode::Tab => 15,
                KeyCode::Char('Q') | KeyCode::Char('q') => 16,
                KeyCode::Char('W') | KeyCode::Char('w') => 17,
                KeyCode::Char('E') | KeyCode::Char('e') => 18,
                KeyCode::Char('R') | KeyCode::Char('r') => 19,
                KeyCode::Char('T') | KeyCode::Char('t') => 20,
                KeyCode::Char('Y') | KeyCode::Char('y') => 21,
                KeyCode::Char('U') | KeyCode::Char('u') => 22,
                KeyCode::Char('I') | KeyCode::Char('i') => 23,
                KeyCode::Char('O') | KeyCode::Char('o') => 24,
                KeyCode::Char('P') | KeyCode::Char('p') => 25,
                KeyCode::Char('[') => 26,
                KeyCode::Char(']') => 27,
                KeyCode::Enter => 28,
                KeyCode::Char('A') | KeyCode::Char('a') => 30,
                KeyCode::Char('S') | KeyCode::Char('s') => 31,
                KeyCode::Char('D') | KeyCode::Char('d') => 32,
                KeyCode::Char('F') | KeyCode::Char('f') => 33,
                KeyCode::Char('G') | KeyCode::Char('g') => 34,
                KeyCode::Char('H') | KeyCode::Char('h') => 35,
                KeyCode::Char('J') | KeyCode::Char('j') => 36,
                KeyCode::Char('K') | KeyCode::Char('k') => 37,
                KeyCode::Char('L') | KeyCode::Char('l') => 38,
                KeyCode::Char(';') => 39,
                KeyCode::Char('\'') => 40,
                KeyCode::Char('`') => 41,
                KeyCode::Char('\\') => 42,
                KeyCode::Char('Z') | KeyCode::Char('z') => 44,
                KeyCode::Char('X') | KeyCode::Char('x') => 45,
                KeyCode::Char('C') | KeyCode::Char('c') => 46,
                KeyCode::Char('V') | KeyCode::Char('v') => 47,
                KeyCode::Char('B') | KeyCode::Char('b') => 48,
                KeyCode::Char('N') | KeyCode::Char('n') => 49,
                KeyCode::Char('M') | KeyCode::Char('m') => 50,
                KeyCode::Char(',') => 51,
                KeyCode::Char('.') => 52,
                KeyCode::Char('/') => 53,
                KeyCode::Char(' ') => 57,
                KeyCode::F(n) => 58 + n as u32,
                KeyCode::NumLock => 69,
                KeyCode::CapsLock => 70,
                KeyCode::Left => 105,
                c => todo!("unsupported key: {c:?}"),
            };
            (code + 8).into()
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
        window_size: Size<i32, crate::utils::Physical>,
    }

    impl MouseEvent {
        /// TODO: doc
        pub fn new(
            mut event: crossterm::event::MouseEvent,
            window_size: Size<i32, crate::utils::Physical>,
        ) -> Self {
            event.row *= 2;
            Self {
                time: Instant::now(),
                event,
                window_size,
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

    impl input::AbsolutePositionEvent<Backend> for MouseEvent {
        fn x(&self) -> f64 {
            self.event.column as _
        }

        fn y(&self) -> f64 {
            self.event.row as _
        }

        fn x_transformed(&self, width: i32) -> f64 {
            self.x() / self.window_size.w as f64 * width as f64
        }

        fn y_transformed(&self, height: i32) -> f64 {
            self.y() / self.window_size.h as f64 * height as f64
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
pub use input::KeyEvent as RatatuiKeyEvent;
pub use input::MouseEvent as RatatuiMouseEvent;
