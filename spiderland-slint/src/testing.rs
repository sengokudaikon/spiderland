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
//! Every component shows in the one [`Headless::window`] until [`Headless::separate_windows`]. From then on, each component made
//! gets a window of its own, and [`Headless::on`] sends input to one of them:
//!
//! ```no_run
//! # slint::slint! { export component Editor { } }
//! use slint::ComponentHandle;
//! use spiderland_slint::testing::Headless;
//!
//! let headless = Headless::install();
//! headless.separate_windows();
//! let (first, second) = (Editor::new().unwrap(), Editor::new().unwrap());
//! headless.on(second.window()).resize(400, 300);
//! headless.on(second.window()).type_text("hello");
//! # let _ = first;
//! ```
//!
//! A window laid out by Slint has no fixed places to click. [`element`] finds an element by its accessible label, and clicks or
//! types into its middle with real input, wherever the layout put it:
//!
//! ```no_run
//! # slint::slint! { import { Button } from "std-widgets.slint"; export component Editor { Button { text: "Save"; } } }
//! use spiderland_slint::testing::{Headless, element, find};
//!
//! let headless = Headless::install();
//! let editor = Editor::new().unwrap();
//! element(&editor, "Save").click();
//! assert!(find(&editor, "Delete").is_none());
//! ```
//!
//! A standard widget is labelled by its text, and any element can say `accessible-label`. The search needs the names Slint keeps
//! only in a build with its debug info: `with_debug_info(true)` in a `build.rs`, or `SLINT_EMIT_DEBUG_INFO=1` while `slint!`
//! builds.
//!
//! `take_snapshot` gives the frame as RGBA, for the test to save or compare. Software-rendered pixels differ a little from
//! FemtoVG's and Skia's, so a capture that must match the app exactly needs an app shipped with `renderer-software`.
//!
//! Slint's other ways of looking at a window, when this doesn't suit:
//! - `i-slint-backend-testing`, which [`element`] is built on, finds elements by ID, type or role too, and drives them without
//!   input (`set_accessible_value`, `invoke_accessible_default_action`). [`Element::handle`] hands over its element for those.
//!   Its own platform measures text with a fixed font size and draws nothing unless `TestingBackendOptions` asks for the
//!   `software` renderer, so this module keeps its own.
//! - `slint-viewer --screenshot out.png --load-data data.json --size 940x580 file.slint` renders one component with no Rust.
//! - The embedded MCP server (`--features slint/mcp`, `SLINT_EMIT_DEBUG_INFO=1`, `SLINT_MCP_PORT`, optionally
//!   `SLINT_BACKEND=headless`) lets an agent take screenshots, click and type against the running app.

use crate::Background;
use i_slint_backend_testing::ElementHandle;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Clipboard, EventLoopProxy, Key, Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, EventLoopError, LogicalPosition, PhysicalSize, PlatformError, SharedString, Window};
use std::cell::{Cell, RefCell};
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
    /// Every window made, the first one included.
    static WINDOWS: RefCell<Vec<Weak<MinimalSoftwareWindow>>> = const { RefCell::new(Vec::new()) };
    /// Whether each component made from now on gets a window of its own.
    static SEPARATE: Cell<bool> = const { Cell::new(false) };
    static CLIPBOARD: RefCell<String> = const { RefCell::new(String::new()) };
}

/// A window for a component to show in, kept in `WINDOWS`.
fn new_window() -> Rc<MinimalSoftwareWindow> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    WINDOWS.with_borrow_mut(|windows| {
        windows.retain(|kept| kept.strong_count() > 0);
        windows.push(Rc::downgrade(&window));
    });
    window
}

struct Proxy;

impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), EventLoopError> {
        // Slint asks to quit when the last window that shows hides, and a test may well hide one window and open another. A quit
        // while any window shows, or from another thread, is a fault in what is being tested.
        let showing = WINDOWS.try_with(|windows| windows.borrow().iter().filter_map(Weak::upgrade).any(|window| window.is_visible()));
        if showing != Ok(false) {
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
    /// What every component shows in, until [`Headless::separate_windows`].
    first: Rc<MinimalSoftwareWindow>,
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
        if SEPARATE.get() { Ok(new_window()) } else { Ok(self.first.clone()) }
    }

    fn duration_since_start(&self) -> Duration {
        self.started.map_or(Duration::ZERO, |started| started.elapsed()) + Duration::from_millis(ADVANCED_MS.load(Ordering::SeqCst))
    }
}

/// The platform with no display, and real input to its windows. Every action ends with a [`settle`].
///
/// Its own input methods go to [`Headless::window`]. Those of [`Headless::on`] go to the window it is given.
pub struct Headless {
    /// The window every component shows in, until [`Headless::separate_windows`]. It is here for events this has no method for.
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
        let window = new_window();
        let platform = HeadlessPlatform { first: window.clone(), started };
        slint::platform::set_platform(Box::new(platform)).expect("the platform is set once per process");
        Headless { window }
    }

    /// From now on, each component made gets a window of its own, as it would on a desktop. Until then, every component shows in
    /// [`Headless::window`], and a new one takes the place of the one before. Input to a window of its own goes through
    /// [`Headless::on`]. Such a window starts with no size, so a test resizes it first.
    pub fn separate_windows(&self) {
        SEPARATE.set(true);
    }

    /// Real input to `window`, such as `component.window()`.
    pub fn on<'a>(&self, window: &'a Window) -> Input<'a> {
        Input(window)
    }

    /// A left click at `x`, `y`, in logical pixels.
    pub fn click(&self, x: f32, y: f32) {
        self.on(&self.window).click(x, y);
    }

    /// A scroll of `delta_y` logical pixels with the pointer at `x`, `y`.
    pub fn scroll(&self, x: f32, y: f32, delta_y: f32) {
        self.on(&self.window).scroll(x, y, delta_y);
    }

    /// Types `text`, one key per character.
    pub fn type_text(&self, text: &str) {
        self.on(&self.window).type_text(text);
    }

    /// `key` with Control held.
    pub fn control_key(&self, key: &str) {
        self.on(&self.window).control_key(key);
    }

    /// Presses and releases `key`.
    pub fn key(&self, key: Key) {
        self.on(&self.window).key(key);
    }

    /// Resizes the window to `width` by `height` physical pixels.
    pub fn resize(&self, width: u32, height: u32) {
        self.on(&self.window).resize(width, height);
    }

    /// What the app last copied, in the test's own clipboard.
    pub fn clipboard(&self) -> String {
        CLIPBOARD.with_borrow(Clone::clone)
    }
}

/// Real input to one window, from [`Headless::on`]. Every action ends with a [`settle`].
#[derive(Clone, Copy)]
pub struct Input<'a>(&'a Window);

impl fmt::Debug for Input<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input").field("size", &self.0.size()).finish_non_exhaustive()
    }
}

impl Input<'_> {
    /// A left click at `x`, `y`, in logical pixels.
    pub fn click(self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        self.0.dispatch_event(WindowEvent::PointerMoved { position });
        self.0.dispatch_event(WindowEvent::PointerPressed { position, button });
        self.0.dispatch_event(WindowEvent::PointerReleased { position, button });
        settle();
    }

    /// A scroll of `delta_y` logical pixels with the pointer at `x`, `y`.
    pub fn scroll(self, x: f32, y: f32, delta_y: f32) {
        let position = LogicalPosition::new(x, y);
        self.0.dispatch_event(WindowEvent::PointerMoved { position });
        self.0.dispatch_event(WindowEvent::PointerScrolled { position, delta_x: 0.0, delta_y });
        settle();
    }

    /// Types `text`, one key per character.
    pub fn type_text(self, text: &str) {
        for c in text.chars() {
            let text = SharedString::from(c.to_string());
            self.0.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
            self.0.dispatch_event(WindowEvent::KeyReleased { text });
        }
        settle();
    }

    /// `key` with Control held.
    pub fn control_key(self, key: &str) {
        let control = SharedString::from(Key::Control);
        self.0.dispatch_event(WindowEvent::KeyPressed { text: control.clone() });
        self.type_text(key);
        self.0.dispatch_event(WindowEvent::KeyReleased { text: control });
        settle();
    }

    /// Presses and releases `key`.
    pub fn key(self, key: Key) {
        let text = SharedString::from(key);
        self.0.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        self.0.dispatch_event(WindowEvent::KeyReleased { text });
        settle();
    }

    /// Resizes the window to `width` by `height` physical pixels.
    pub fn resize(self, width: u32, height: u32) {
        self.0.set_size(PhysicalSize::new(width, height));
        settle();
    }

    /// Asks the window to close, as its title bar's close button does. What happens next is up to its `on_close_requested`.
    pub fn close(self) {
        self.0.dispatch_event(WindowEvent::CloseRequested);
        settle();
    }
}

/// The element of `component` whose accessible label is `label`, for real input to its middle. Panics unless exactly one such
/// element shows, so a test names what it clicks.
pub fn element<'a, C: ComponentHandle>(component: &'a C, label: &str) -> Element<'a> {
    let mut found = labelled(component, label);
    match found.len() {
        1 => found.remove(0),
        0 => panic!("no element labelled {label:?} shows"),
        n => panic!("{n} elements labelled {label:?} show: give each its own `accessible-label`"),
    }
}

/// The element of `component` whose accessible label is `label`, if one shows. Panics if several do.
pub fn find<'a, C: ComponentHandle>(component: &'a C, label: &str) -> Option<Element<'a>> {
    let mut found = labelled(component, label);
    assert!(found.len() < 2, "{} elements labelled {label:?} show: give each its own `accessible-label`", found.len());
    found.pop()
}

fn labelled<'a, C: ComponentHandle>(component: &'a C, label: &str) -> Vec<Element<'a>> {
    ElementHandle::find_by_accessible_label(component, label)
        .filter(|handle| handle.is_valid() && handle.computed_opacity() > 0.0)
        .map(|handle| Element {
            handle,
            window: component.window(),
        })
        .collect()
}

/// One element of a component, from [`element`] or [`find`]. Its actions are real input to its window, at its middle as the
/// layout placed it, and each ends with a [`settle`].
pub struct Element<'a> {
    handle: ElementHandle,
    window: &'a Window,
}

impl Element<'_> {
    /// A left click in its middle.
    pub fn click(&self) {
        let (x, y) = self.middle();
        Input(self.window).click(x, y);
    }

    /// Clicks it, then types `text` into what has the focus, such as this text box.
    pub fn type_text(&self, text: &str) {
        self.click();
        Input(self.window).type_text(text);
    }

    /// Where its middle is in its window, in logical pixels.
    pub fn middle(&self) -> (f32, f32) {
        let (at, size) = (self.handle.absolute_position(), self.handle.size());
        (at.x + size.width / 2.0, at.y + size.height / 2.0)
    }

    /// Slint's handle on it, for what else it offers, such as its accessible value.
    pub fn handle(&self) -> &ElementHandle {
        &self.handle
    }
}

impl fmt::Debug for Element<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Element")
            .field("label", &self.handle.accessible_label())
            .field("middle", &self.middle())
            .finish()
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
