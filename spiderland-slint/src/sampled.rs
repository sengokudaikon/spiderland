//! An input that comes fast, such as a drag, sent to a store as its latest value, once per turn of the event loop or per interval.

use slint::{Timer, TimerMode};
use spiderland::Intents;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Sends the latest of an input that comes fast, such as where a drag has got to. Samples made before the next send replace each
/// other, so a burst reaches the store as its last sample, in one batch. The store then reduces, renders and runs its
/// subscriptions and effects once for the burst, not once for each event.
///
/// [`Sampled::new`] sends on the next turn of Slint's event loop, when it runs its timers. That merges what one turn delivers,
/// such as wheel steps or what a view's callbacks send in a row. Slint already merges the pointer's moves within a turn, so for a
/// drag, [`Sampled::every`] with an interval such as a frame's sends at most once an interval.
///
/// It is for an input whose values replace each other, such as a pointer's position, a scroll offset or a size. A command, such
/// as a save, goes through [`Sampled::send`], which sends a sample still waiting first, so the command never goes ahead of a
/// value sampled before it. An intent sent by another way keeps its own order, so send through the sampler everything that has
/// to follow its samples.
///
/// [`Sampled::flush`] sends what waits at once, such as as a drag ends. The last sample always goes out: dropped, the sampler
/// sends what still waits. Clones share what waits.
///
/// ```no_run
/// # use spiderland::{Effects, Feature, Host, Intents, Never, Store};
/// # use std::time::Duration;
/// # struct Drag;
/// # enum Intent { Moved(f32), Drop }
/// # impl Feature for Drag {
/// #     type State = f32;
/// #     type Intent = Intent;
/// #     type Effect = Never;
/// #     type Output = Never;
/// #     type Subscription = Never;
/// #     fn reduce(at: &mut f32, intent: Intent, _: &mut Effects<Never>) {
/// #         if let Intent::Moved(x) = intent { *at = x }
/// #     }
/// # }
/// # struct Quiet;
/// # impl Host<Drag> for Quiet {
/// #     fn perform(&mut self, effect: Never, _: &Intents<Intent>) { match effect {} }
/// # }
/// # let store = Store::new(0.0, Quiet);
/// let sampled = spiderland_slint::Sampled::every(Duration::from_millis(16), &store.intents());
/// // From the view's callbacks: each move replaces the last one not yet sent.
/// sampled.sample(Intent::Moved(10.0));
/// sampled.sample(Intent::Moved(12.0));
/// // The drop goes after the last move.
/// sampled.send(Intent::Drop);
/// ```
pub struct Sampled<I>(Rc<Sampler<I>>);

struct Sampler<I> {
    to: Intents<I>,
    waiting: RefCell<Option<I>>,
    /// Whether a turn of the event loop is due to send what waits.
    due: Cell<bool>,
    /// How long after a send the next one waits. Zero sends on each turn.
    interval: Duration,
    /// Runs for `interval` after each send, and sends what waits once it ends.
    cooling: Timer,
}

impl<I: 'static> Sampled<I> {
    /// A sampler that sends to `to` on the next turn of the event loop. Make it on the UI thread.
    pub fn new(to: &Intents<I>) -> Sampled<I> {
        Sampled::every(Duration::ZERO, to)
    }

    /// A sampler that sends to `to` at most once every `interval`. The first sample after a quiet spell goes on the next turn of
    /// the event loop, and those after it wait out the interval, so a steady stream goes out once an interval and its last
    /// sample goes out too. Make it on the UI thread.
    pub fn every(interval: Duration, to: &Intents<I>) -> Sampled<I> {
        Sampled(Rc::new(Sampler {
            to: to.clone(),
            waiting: RefCell::new(None),
            due: Cell::new(false),
            interval,
            cooling: Timer::default(),
        }))
    }

    /// Keeps `intent` to send next, in place of a sample still waiting.
    pub fn sample(&self, intent: I) {
        let replaced = self.0.waiting.borrow_mut().replace(intent);
        drop(replaced);
        if self.0.due.get() || self.0.cooling.running() {
            return;
        }
        self.0.due.set(true);
        let sampler = Rc::downgrade(&self.0);
        Timer::single_shot(Duration::ZERO, move || Sampler::send_waiting(&sampler));
    }

    /// Sends `intent` now, after a sample still waiting, so it never goes ahead of a value sampled before it.
    pub fn send(&self, intent: I) {
        self.flush();
        self.0.to.send(intent);
    }

    /// Sends a sample still waiting now, such as as a drag ends.
    pub fn flush(&self) {
        Sampler::send_waiting(&Rc::downgrade(&self.0));
    }

    /// True while a sample waits to be sent.
    pub fn is_waiting(&self) -> bool {
        self.0.waiting.borrow().is_some()
    }
}

impl<I: 'static> Sampler<I> {
    /// Sends what waits, if the sampler is still there, and waits out the interval before the next send.
    fn send_waiting(sampler: &Weak<Sampler<I>>) {
        let Some(this) = sampler.upgrade() else {
            return;
        };
        this.due.set(false);
        // Taken first, so what the store runs as it hears it can sample again.
        let waiting = this.waiting.borrow_mut().take();
        let Some(intent) = waiting else {
            return;
        };
        if !this.interval.is_zero() {
            let sampler = sampler.clone();
            this.cooling.start(TimerMode::SingleShot, this.interval, move || Sampler::send_waiting(&sampler));
        }
        this.to.send(intent);
    }
}

impl<I> Drop for Sampler<I> {
    fn drop(&mut self) {
        if let Some(intent) = self.waiting.get_mut().take() {
            self.to.send(intent);
        }
    }
}

impl<I> Clone for Sampled<I> {
    fn clone(&self) -> Self {
        Sampled(Rc::clone(&self.0))
    }
}

impl<I> fmt::Debug for Sampled<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sampled")
            .field("waiting", &self.0.waiting.borrow().is_some())
            .field("interval", &self.0.interval)
            .finish()
    }
}
