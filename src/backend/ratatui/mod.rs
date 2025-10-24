//! A backend for smithay that renders to a tty.
use calloop::{EventSource, Interest, Mode, PostAction};
use timerfd::{SetTimeFlags, TimerFd, TimerState};

use crate::{backend::renderer::ratatui::RatatuiRenderer, utils::Size};
use std::{
    collections::HashSet,
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
            keyboard_state: KeyboardState::new(),
        }
    }
}

/// TODO doc
#[derive(Debug)]
pub struct RatatuiEventSource {
    event_token: Option<calloop::Token>,
    timer: Option<Timer>,
    refresh_interval: Duration,
    keyboard_state: KeyboardState,
}

/// TODO doc
#[derive(Debug)]
pub enum RatatuiEvent {
    /// TODO doc
    Redraw,
    /// TODO doc
    Resize(u16, u16),
    /// TODO doc
    Key {
        code: u32,
        kind: crossterm::event::KeyEventKind,
    },
    /// TODO doc
    Mouse(crossterm::event::MouseEvent),
}

fn to_input_code(code: crossterm::event::KeyCode) -> Option<u32> {
    use crossterm::event::KeyCode;
    use input_event_codes::*;

    Some(
        match code {
            KeyCode::Esc => KEY_ESC!(),
            KeyCode::Char('1') => KEY_1!(),
            KeyCode::Char('2') => KEY_2!(),
            KeyCode::Char('3') => KEY_3!(),
            KeyCode::Char('4') => KEY_4!(),
            KeyCode::Char('5') => KEY_5!(),
            KeyCode::Char('6') => KEY_6!(),
            KeyCode::Char('7') => KEY_7!(),
            KeyCode::Char('8') => KEY_8!(),
            KeyCode::Char('9') => KEY_9!(),
            KeyCode::Char('0') => KEY_0!(),
            KeyCode::Char('-') => KEY_MINUS!(),
            KeyCode::Char('=') => KEY_EQUAL!(),
            KeyCode::Backspace => KEY_BACKSPACE!(),
            KeyCode::Tab => KEY_TAB!(),
            KeyCode::Char('Q') | KeyCode::Char('q') => KEY_Q!(),
            KeyCode::Char('W') | KeyCode::Char('w') => KEY_W!(),
            KeyCode::Char('E') | KeyCode::Char('e') => KEY_E!(),
            KeyCode::Char('R') | KeyCode::Char('r') => KEY_R!(),
            KeyCode::Char('T') | KeyCode::Char('t') => KEY_T!(),
            KeyCode::Char('Y') | KeyCode::Char('y') => KEY_Y!(),
            KeyCode::Char('U') | KeyCode::Char('u') => KEY_U!(),
            KeyCode::Char('I') | KeyCode::Char('i') => KEY_I!(),
            KeyCode::Char('O') | KeyCode::Char('o') => KEY_O!(),
            KeyCode::Char('P') | KeyCode::Char('p') => KEY_P!(),
            KeyCode::Char('[') | KeyCode::Char('{') => KEY_LEFTBRACE!(),
            KeyCode::Char(']') | KeyCode::Char('}') => KEY_RIGHTBRACE!(),
            KeyCode::Enter => KEY_ENTER!(),
            KeyCode::Char('A') | KeyCode::Char('a') => KEY_A!(),
            KeyCode::Char('S') | KeyCode::Char('s') => KEY_S!(),
            KeyCode::Char('D') | KeyCode::Char('d') => KEY_D!(),
            KeyCode::Char('F') | KeyCode::Char('f') => KEY_F!(),
            KeyCode::Char('G') | KeyCode::Char('g') => KEY_G!(),
            KeyCode::Char('H') | KeyCode::Char('h') => KEY_H!(),
            KeyCode::Char('J') | KeyCode::Char('j') => KEY_J!(),
            KeyCode::Char('K') | KeyCode::Char('k') => KEY_K!(),
            KeyCode::Char('L') | KeyCode::Char('l') => KEY_L!(),
            KeyCode::Char(';') | KeyCode::Char(':') => KEY_SEMICOLON!(),
            KeyCode::Char('\'') | KeyCode::Char('"') => KEY_APOSTROPHE!(),
            KeyCode::Char('`') | KeyCode::Char('~') => KEY_GRAVE!(),
            KeyCode::Char('\\') | KeyCode::Char('|') => KEY_BACKSLASH!(),
            KeyCode::Char('Z') | KeyCode::Char('z') => KEY_Z!(),
            KeyCode::Char('X') | KeyCode::Char('x') => KEY_X!(),
            KeyCode::Char('C') | KeyCode::Char('c') => KEY_C!(),
            KeyCode::Char('V') | KeyCode::Char('v') => KEY_V!(),
            KeyCode::Char('B') | KeyCode::Char('b') => KEY_B!(),
            KeyCode::Char('N') | KeyCode::Char('n') => KEY_N!(),
            KeyCode::Char('M') | KeyCode::Char('m') => KEY_M!(),
            KeyCode::Char(',') | KeyCode::Char('<') => KEY_COMMA!(),
            KeyCode::Char('.') | KeyCode::Char('>') => KEY_DOT!(),
            KeyCode::Char('/') | KeyCode::Char('?') => KEY_SLASH!(),
            KeyCode::Char(' ') => KEY_SPACE!(),
            KeyCode::F(1) => KEY_F1!(),
            KeyCode::F(2) => KEY_F2!(),
            KeyCode::F(3) => KEY_F3!(),
            KeyCode::F(4) => KEY_F4!(),
            KeyCode::F(5) => KEY_F5!(),
            KeyCode::F(6) => KEY_F6!(),
            KeyCode::F(7) => KEY_F7!(),
            KeyCode::F(8) => KEY_F8!(),
            KeyCode::F(9) => KEY_F9!(),
            KeyCode::F(10) => KEY_F10!(),
            KeyCode::F(11) => KEY_F11!(),
            KeyCode::F(12) => KEY_F12!(),
            KeyCode::F(13) => KEY_F13!(),
            KeyCode::F(14) => KEY_F14!(),
            KeyCode::F(15) => KEY_F15!(),
            KeyCode::F(16) => KEY_F16!(),
            KeyCode::F(17) => KEY_F17!(),
            KeyCode::F(18) => KEY_F18!(),
            KeyCode::F(19) => KEY_F19!(),
            KeyCode::F(20) => KEY_F20!(),
            KeyCode::F(21) => KEY_F21!(),
            KeyCode::F(22) => KEY_F22!(),
            KeyCode::F(23) => KEY_F23!(),
            KeyCode::F(24) => KEY_F24!(),
            KeyCode::NumLock => KEY_NUMLOCK!(),
            KeyCode::CapsLock => KEY_CAPSLOCK!(),
            KeyCode::Left => KEY_LEFT!(),
            KeyCode::Right => KEY_RIGHT!(),
            KeyCode::Up => KEY_UP!(),
            KeyCode::Down => KEY_DOWN!(),
            c => {
                eprintln!("unsupported key code: {c:?}");
                return None;
            }
        } + 8, /* +8 maps scancode to x11 keycode, see MIN_KEYCODE in evdev */
               // TODO: type-based scancode -> keycode map
    )
}

// One Ratatui key event may resolve to multiple events, if for example a modifier key changed in
// the meantime.
#[derive(Debug)]
struct KeyboardState {
    modifiers: crossterm::event::KeyModifiers,
    keys_down: HashSet<crossterm::event::KeyCode>,
}

impl KeyboardState {
    fn new() -> Self {
        Self {
            modifiers: crossterm::event::KeyModifiers::empty(),
            keys_down: HashSet::new(),
        }
    }
    fn update(&mut self, event: crossterm::event::KeyEvent) -> Vec<RatatuiEvent> {
        use crossterm::event::KeyEventKind;
        use crossterm::event::KeyModifiers;
        use input_event_codes::*;

        let mut events = Vec::new();
        let mut emit = |code, kind| events.push(RatatuiEvent::Key { code, kind });
        let flag_state = |flag: KeyModifiers| {
            if !(flag & event.modifiers).is_empty() {
                KeyEventKind::Press
            } else {
                KeyEventKind::Release
            }
        };

        for flag in self.modifiers ^ event.modifiers {
            match flag {
                // No idea _which_ one was changed, emit both
                crossterm::event::KeyModifiers::SHIFT => {
                    emit(KEY_LEFTSHIFT!() + 8, flag_state(flag));
                    emit(KEY_RIGHTSHIFT!() + 8, flag_state(flag));
                }
                crossterm::event::KeyModifiers::CONTROL => {
                    emit(KEY_LEFTCTRL!() + 8, flag_state(flag));
                    emit(KEY_RIGHTCTRL!() + 8, flag_state(flag));
                }
                crossterm::event::KeyModifiers::ALT => {
                    emit(KEY_LEFTALT!() + 8, flag_state(flag));
                    emit(KEY_RIGHTALT!() + 8, flag_state(flag));
                }
                crossterm::event::KeyModifiers::META => {
                    emit(KEY_LEFTMETA!() + 8, flag_state(flag));
                    emit(KEY_RIGHTMETA!() + 8, flag_state(flag));
                }
                _ => todo!("unsupported modifier: {flag:?}"),
            }
        }
        self.modifiers = event.modifiers;

        // We only get Press events?
        for key in self.keys_down.drain() {
            if let Some(code) = to_input_code(key) {
                emit(code, KeyEventKind::Release);
            } else {
                eprintln!("unsupported event code {:?}", event.code);
            }
        }

        match event.kind {
            KeyEventKind::Press => {
                self.keys_down.insert(event.code);
                if let Some(code) = to_input_code(event.code) {
                    emit(code, event.kind);
                } else {
                    eprintln!("unsupported event code {:?}", event.code);
                }
            }
            KeyEventKind::Release => todo!("???? HOW ????"),
            KeyEventKind::Repeat => { /* ignore */ }
        };

        events
    }
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
            let events = match crossterm::event::read()? {
                crossterm::event::Event::Resize(width, height) => vec![RatatuiEvent::Resize(width, height)],
                crossterm::event::Event::Key(event) => self.keyboard_state.update(event),
                crossterm::event::Event::Mouse(event) => vec![RatatuiEvent::Mouse(event)],
                _ => continue,
            };

            for event in events {
                callback(event, data);
            }
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
        code: u32,
        kind: crossterm::event::KeyEventKind,
    }

    impl crate::backend::input::Event<Backend> for KeyEvent {
        fn time(&self) -> u64 {
            self.time.elapsed().as_millis() as u64
        }

        fn device(&self) -> <Backend as input::InputBackend>::Device {
            Device
        }
    }

    // TODO: it's a mess
    impl From<super::RatatuiEvent> for KeyEvent {
        fn from(event: super::RatatuiEvent) -> Self {
            let super::RatatuiEvent::Key { code, kind } = event else {
                todo!("unreachable, sort this out at compile time");
            };
            let ret = Self {
                time: Instant::now(),
                code,
                kind,
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

    impl input::KeyboardKeyEvent<Backend> for KeyEvent {
        fn key_code(&self) -> xkbcommon::xkb::Keycode {
            dbg!(self);
            self.code.into()
        }

        fn state(&self) -> input::KeyState {
            use crossterm::event::KeyEventKind;

            match self.kind {
                KeyEventKind::Press => input::KeyState::Pressed,
                KeyEventKind::Release => input::KeyState::Released,
                _ => todo!(),
            }
        }

        fn count(&self) -> u32 {
            1
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
