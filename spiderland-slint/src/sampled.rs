//! An input that comes fast, such as a drag, sent to a store once per turn of the event loop.

use slint::Timer;
use spiderland::Intents;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

/// Sends the latest of an input that comes fast, such as where a drag has got to, once per turn of Slint's event loop. Samples
/// made between two turns replace each other, so a burst reaches the store as its last sample, in one batch. The store then
/// reduces, renders and runs its subscriptions and effects once for the burst, not once for each event.
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
/// let sampled = spiderland_slint::Sampled::new(&store.intents());
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
}

impl<I: 'static> Sampled<I> {
    /// A sampler that sends to `to`. Make it on the UI thread.
    pub fn new(to: &Intents<I>) -> Sampled<I> {
        Sampled(Rc::new(Sampler {
            to: to.clone(),
            waiting: RefCell::new(None),
            due: Cell::new(false),
        }))
    }

    /// Keeps `intent` to send on the next turn of the event loop, in place of a sample still waiting.
    pub fn sample(&self, intent: I) {
        let replaced = self.0.waiting.borrow_mut().replace(intent);
        drop(replaced);
        if !self.0.due.replace(true) {
            let sampler = Rc::downgrade(&self.0);
            Timer::single_shot(Duration::ZERO, move || {
                if let Some(sampler) = sampler.upgrade() {
                    sampler.flush();
                }
            });
        }
    }

    /// Sends `intent` now, after a sample still waiting, so it never goes ahead of a value sampled before it.
    pub fn send(&self, intent: I) {
        self.0.flush();
        self.0.to.send(intent);
    }

    /// Sends a sample still waiting now, such as as a drag ends.
    pub fn flush(&self) {
        self.0.flush();
    }

    /// True while a sample waits for the next turn of the event loop.
    pub fn is_waiting(&self) -> bool {
        self.0.waiting.borrow().is_some()
    }
}

impl<I> Sampler<I> {
    fn flush(&self) {
        self.due.set(false);
        // Taken first, so what the store runs as it hears it can sample again.
        let waiting = self.waiting.borrow_mut().take();
        if let Some(intent) = waiting {
            self.to.send(intent);
        }
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
        f.debug_struct("Sampled").field("waiting", &self.0.waiting.borrow().is_some()).finish()
    }
}
