//! What a Slint [`spiderland::Host`] needs to do its work, built on Slint's event loop.
//!
//! - [`Background`]: work off the UI thread whose result comes back as an intent, and waiting on the UI thread, for work that
//!   can be let go of ([`Background::local`]) and for work that can't ([`Background::finish`]). It is
//!   [`spiderland::Background`] on [`Slint`]'s loop.
//! - [`every`] and [`after`]: a repeating intent and a delayed one, each ending with the timer it returns.
//! - [`on_wake`] and [`on_watch`]: a service on another thread telling a feature that something changed.
//! - [`Rows`]: a list model updated row by row, so a row being dragged or typed in survives its neighbours' changes. [`InnerRows`]
//!   keeps a list inside each row.
//! - [`select_language`]: the user's language, for the views and the phrases alike.
//! - `testing`, behind that feature: the real window with no display, driven by real input.

mod rows;
#[cfg(feature = "testing")]
pub mod testing;

pub use rows::{InnerRows, Rows};

use slint::{Timer, TimerMode};
use spiderland::{EventLoop, Intents, Listening, Waker};
use spiderland_phrases::{Catalog, Translations};
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
pub fn every<I: 'static>(interval: Duration, intents: &Intents<I>, make: impl Fn() -> I + 'static) -> Timer {
    let timer = Timer::default();
    let intents = intents.clone();
    timer.start(TimerMode::Repeated, interval, move || intents.send(make()));
    timer
}

/// Sends `make()` once, after `delay`, unless the returned timer is dropped first. Keeping the timer in one place makes this a
/// debounce: the next call's timer replaces it, and only the last one fires.
pub fn after<I: 'static>(delay: Duration, intents: &Intents<I>, make: impl Fn() -> I + 'static) -> Timer {
    let timer = Timer::default();
    let intents = intents.clone();
    timer.start(TimerMode::SingleShot, delay, move || intents.send(make()));
    timer
}

/// Shows the app in the user's language when `catalogs` has it, else in the language its sources are written in: the views
/// through Slint's bundled translation, the phrases through `catalogs`, and numbers in that language's symbols, chosen once for
/// all. Call it as soon as the first window exists, before any page or message is built.
pub fn select_language(catalogs: &'static [Catalog]) {
    let Ok(translations) = Translations::new(catalogs) else {
        return;
    };
    let language = sys_locale::get_locale().and_then(|locale| translations.choose(&locale));
    translations.select(language);
    spiderland_phrases::locale::select(language);
    // Fails only when no translation is bundled, and the sources' language needs none.
    let _ = slint::select_bundled_translation(language.unwrap_or(""));
}
