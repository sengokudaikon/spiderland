//! What a Slint [`spiderland::Host`] needs to do its work, built on Slint's event loop.
//!
//! - [`WindowHost`] and [`Windowed`]: a host handed its window on each call. It isn't asked to render once the window is gone,
//!   and still performs every effect.
//! - [`WindowView`]: a view in one window, which attaches to a store through [`Windowed`], so several windows can show one
//!   store. [`attach_window`] attaches one for as long as its window is open, and [`ask_before_closing`] lets a store
//!   decide whether a window closes.
//! - [`Background`]: work off the UI thread whose result comes back as an intent, and waiting on the UI thread, for work that
//!   can be let go of ([`Background::local`]) and for work that can't ([`Background::finish`]). It is
//!   [`spiderland::Background`] on [`Slint`]'s loop.
//! - [`every`] and [`after`]: a repeating intent and a delayed one, each ending with the timer it returns.
//! - [`Sampled`]: an input that comes fast, such as a drag, sent as its latest value on the next turn of the event loop, or at
//!   most once an interval.
//! - [`on_wake`] and [`on_watch`]: a service on another thread telling a feature that something changed.
//! - [`Rows`]: a list model updated row by row, so a row being dragged or typed in survives its neighbours' changes. [`InnerRows`]
//!   keeps a list inside each row.
//! - `testing`, behind that feature: the real window with no display, driven by real input.

mod rows;
mod sampled;
#[cfg(feature = "testing")]
pub mod testing;
mod window;

pub use rows::{InnerRows, Rows};
pub use sampled::Sampled;
pub use window::{WindowHost, WindowView, Windowed, ask_before_closing, attach_window};

use slint::{Timer, TimerMode};
use spiderland::{EventLoop, Intents, Listening, Waker};
use std::time::Duration;
use tokio::sync::watch;

/// Slint's event loop, for the work and listeners `spiderland` runs on the UI thread.
#[derive(Clone, Copy, Debug)]
pub struct Slint;

impl EventLoop for Slint {
    fn spawn_local(future: impl Future<Output = ()> + 'static) {
        // Fails only where no event loop can ever run, and then nothing could show the result anyway.
        let _ = slint::spawn_local(future);
    }
}

/// Starts work on the app's pool and waits on Slint's event loop. Clone it into whatever hosts a feature.
///
/// ```
/// let background = spiderland_slint::Background::new(|job| {
///     std::thread::spawn(job);
///     true
/// });
/// # drop(background);
/// ```
pub type Background = spiderland::Background<Slint>;

/// Sends `make()` on Slint's event loop each time a worker publishes on `updates`, and once more when it ends. This is
/// [`spiderland::on_watch`] on [`Slint`].
pub fn on_watch<T: 'static, I: 'static>(updates: watch::Receiver<T>, intents: &Intents<I>, make: impl Fn() -> I + 'static) -> Listening {
    spiderland::on_watch::<Slint, _, _>(updates, intents, make)
}

/// Sends `make()` on Slint's event loop each time the returned [`Waker`] fires. This is [`spiderland::on_wake`] on [`Slint`].
pub fn on_wake<I: 'static>(intents: &Intents<I>, make: impl Fn() -> I + 'static) -> (Waker, Listening) {
    spiderland::on_wake::<Slint, _>(intents, make)
}

/// Sends `make()` every `interval` until the returned timer is dropped.
#[must_use = "dropping the timer stops it"]
pub fn every<I: 'static>(interval: Duration, intents: &Intents<I>, make: impl Fn() -> I + 'static) -> Timer {
    let timer = Timer::default();
    let intents = intents.clone();
    timer.start(TimerMode::Repeated, interval, move || intents.send(make()));
    timer
}

/// Sends `make()` once, after `delay`, unless the returned timer is dropped first. Keeping the timer in one place makes this a
/// debounce: the next call's timer replaces it, and only the last one fires.
#[must_use = "dropping the timer stops it"]
pub fn after<I: 'static>(delay: Duration, intents: &Intents<I>, make: impl Fn() -> I + 'static) -> Timer {
    let timer = Timer::default();
    let intents = intents.clone();
    timer.start(TimerMode::SingleShot, delay, move || intents.send(make()));
    timer
}
