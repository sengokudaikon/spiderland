//! The real window with no display, for tests that drive it by real input: Slint's own screenshot tests take this route. A
//! software renderer draws the real components with the bundled fonts, and [`Headless`] sends real pointer and key events.
//! Background work runs for real and comes back through the platform's event-loop proxy, which [`settle`] drains.
//!
//! Slint's platform can be set once per process and its components are `!Send`, so such a test runs on the main thread: one test
//! in its binary, or `harness = false`.
//!
//! ```no_run
//! # slint::slint! { export component MainWindow { } }
//! use slint::ComponentHandle;
//! use spiderland_slint::testing::{Headless, settle};
//!
//! let headless = Headless::install();
//! let ui = MainWindow::new().unwrap();
//! headless.resize(1100, 700);
//! ui.show().unwrap();
//! headless.click(120.0, 40.0);
//! settle();
//! let frame = ui.window().take_snapshot().unwrap();
//! # let _ = frame;
//! ```
//!
//! `take_snapshot` gives the frame as RGBA, for the test to save or compare. Software-rendered pixels differ a little from
//! FemtoVG's and Skia's, so a capture that must match the app exactly needs an app shipped with `renderer-software`.
//!
//! Slint's other ways of looking at a window, when this doesn't suit:
//! - `i-slint-backend-testing` finds elements by ID, accessible label, type or role and drives them (`single_click`, `drag`,
//!   `set_accessible_value`). It is internal, so pin it exactly. It needs `SLINT_EMIT_DEBUG_INFO=1`, measures text with a fixed font
//!   size, and draws nothing unless `TestingBackendOptions` asks for the `software` renderer. Using its element handles on this
//!   module's window hasn't been tried.
//! - `slint-viewer --screenshot out.png --load-data data.json --size 940x580 file.slint` renders one component with no Rust.
//! - The embedded MCP server (`--features slint/mcp`, `SLINT_EMIT_DEBUG_INFO=1`, `SLINT_MCP_PORT`, optionally
//!   `SLINT_BACKEND=headless`) lets an agent take screenshots, click and type against the running app.

use crate::Background;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Clipboard, EventLoopProxy, Key, Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{EventLoopError, LogicalPosition, PhysicalSize, PlatformError, SharedString};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

type Event = Box<dyn FnOnce() + Send>;

static EVENTS: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
/// How far [`advance`] has moved Slint's clock, in milliseconds.
static ADVANCED_MS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static WINDOW: RefCell<Weak<MinimalSoftwareWindow>> = const { RefCell::new(Weak::new()) };
    static CLIPBOARD: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Proxy;

impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), EventLoopError> {
        // Slint asks to quit when the last window hides, and a test may well hide one window and open another. A quit while a
        // window shows, or from another thread, is a fault in what is being tested.
        let visible = WINDOW.try_with(|slot| slot.borrow().upgrade().map(|window| window.window().is_visible())).ok().flatten();
        if visible != Some(false) {
            QUIT_REQUESTED.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn invoke_from_event_loop(&self, event: Event) -> Result<(), EventLoopError> {
        EVENTS.lock().unwrap().push_back(event);
        Ok(())
    }
}

struct HeadlessPlatform {
    window: Rc<MinimalSoftwareWindow>,
    /// Real time counts as well, for what a test waits on for real. None holds the clock still between [`advance`]s.
    started: Option<Instant>,
}

impl Platform for HeadlessPlatform {
    fn set_clipboard_text(&self, text: &str, _: Clipboard) {
        CLIPBOARD.with_borrow_mut(|clipboard| *clipboard = text.to_owned());
    }

    fn clipboard_text(&self, _: Clipboard) -> Option<String> {
        Some(CLIPBOARD.with_borrow(Clone::clone))
    }

    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(Proxy))
    }

    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.started.map_or(Duration::ZERO, |started| started.elapsed()) + Duration::from_millis(ADVANCED_MS.load(Ordering::SeqCst))
    }
}

/// The process's one window, and real input to it. Every action ends with a [`settle`].
pub struct Headless {
    /// The window every component shows in, for events this has no method for.
    pub window: Rc<MinimalSoftwareWindow>,
}

impl fmt::Debug for Headless {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Headless").field("size", &self.window.size()).finish_non_exhaustive()
    }
}

impl Headless {
    /// Sets the platform, whose clock runs in real time and also moves with [`advance`].
    pub fn install() -> Headless {
        Self::with_clock(Some(Instant::now()))
    }

    /// Sets the platform, whose clock moves only with [`advance`], so a test can count a timer's ticks.
    pub fn frozen() -> Headless {
        Self::with_clock(None)
    }

    fn with_clock(started: Option<Instant>) -> Headless {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        WINDOW.with(|slot| *slot.borrow_mut() = Rc::downgrade(&window));
        slint::platform::set_platform(Box::new(HeadlessPlatform { window: window.clone(), started })).expect("the platform is set once per process");
        Headless { window }
    }

    /// A left click at `x`, `y`, in logical pixels.
    pub fn click(&self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        self.window.dispatch_event(WindowEvent::PointerMoved { position });
        self.window.dispatch_event(WindowEvent::PointerPressed { position, button });
        self.window.dispatch_event(WindowEvent::PointerReleased { position, button });
        settle();
    }

    /// A scroll of `delta_y` logical pixels with the pointer at `x`, `y`.
    pub fn scroll(&self, x: f32, y: f32, delta_y: f32) {
        let position = LogicalPosition::new(x, y);
        self.window.dispatch_event(WindowEvent::PointerMoved { position });
        self.window.dispatch_event(WindowEvent::PointerScrolled { position, delta_x: 0.0, delta_y });
        settle();
    }

    /// Types `text`, one key per character.
    pub fn type_text(&self, text: &str) {
        for c in text.chars() {
            let text = SharedString::from(c.to_string());
            self.window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            self.window.dispatch_event(WindowEvent::KeyReleased { text });
        }
        settle();
    }

    /// `key` with Control held.
    pub fn control_key(&self, key: &str) {
        let control = SharedString::from(Key::Control);
        self.window.dispatch_event(WindowEvent::KeyPressed { text: control.clone() });
        self.type_text(key);
        self.window.dispatch_event(WindowEvent::KeyReleased { text: control });
        settle();
    }

    /// Presses and releases `key`.
    pub fn key(&self, key: Key) {
        let text = SharedString::from(key);
        self.window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        self.window.dispatch_event(WindowEvent::KeyReleased { text });
        settle();
    }

    /// Resizes the window to `width` by `height` physical pixels.
    pub fn resize(&self, width: u32, height: u32) {
        self.window.set_size(PhysicalSize::new(width, height));
        settle();
    }

    /// What the app last copied, in the test's own clipboard.
    pub fn clipboard(&self) -> String {
        CLIPBOARD.with_borrow(Clone::clone)
    }
}

/// Moves Slint's clock on by `by`, then runs what came due: a timer fires without waiting it out.
pub fn advance(by: Duration) {
    ADVANCED_MS.fetch_add(by.as_millis() as u64, Ordering::SeqCst);
    settle();
}

/// Runs what came due on the UI thread, until nothing more is due. It goes in rounds: the due timers and `changed` callbacks, then
/// the events queued for the UI thread. It stops after a round that ran no event and left no timer due, so a timer that an event
/// sets to fire at once fires in the same call. Panics if something asked the app to quit while a window showed, or if what is
/// due never runs out.
pub fn settle() {
    let mut steps = 0;
    loop {
        slint::platform::update_timers_and_animations();
        let mut ran = false;
        loop {
            let event = EVENTS.lock().unwrap().pop_front();
            let Some(event) = event else {
                break;
            };
            event();
            ran = true;
            steps += 1;
            assert!(steps < 10_000, "the event queue did not settle");
        }
        let due = slint::platform::duration_until_next_timer_update() == Some(Duration::ZERO);
        if !ran && !due {
            assert!(!QUIT_REQUESTED.load(Ordering::SeqCst), "the app was asked to quit while a window showed");
            return;
        }
        steps += 1;
        assert!(steps < 10_000, "the timers did not settle");
    }
}

/// Settles until `done` says so. It checks `done` only once a [`settle`] has run out of things to do, so when it returns, `done`
/// holds and nothing is left to run. `what` names it if it never does.
pub fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        settle();
        if done() {
            return;
        }
        assert!(Instant::now() < deadline, "{what} must finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Settles until all of `background`'s work has come back, and nothing that came back has started more: a test then sees what the
/// window would show.
pub fn idle(background: &Background) {
    until("background work", || background.pending() == 0);
}
