//! Work that must not hold the UI thread. It runs on the app's pool and comes back on the event loop as an intent.

use crate::wake::{Dropping, droppable};
use crate::{Error, Intents};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::oneshot;

/// The loop that runs a UI thread's events, and futures between them. It is the one thing [`Background`] and the listeners
/// need from a toolkit, and a host crate implements it once: `spiderland-slint`'s `Slint` hands each future to
/// `slint::spawn_local`.
pub trait EventLoop {
    /// Runs `future` on the UI thread, between events. It is called from the UI thread. Where no event loop can ever run, it may
    /// drop the future, since nothing could show the result anyway. A future still waiting when the loop stops is never polled
    /// again, so its result is lost too.
    fn spawn_local(future: impl Future<Output = ()> + 'static);
}

/// One piece of work for the app's pool to start.
pub type Job = Box<dyn FnOnce() + Send>;

/// Tells the feature that its work panicked: only an unwinding drops this with the reply unsent.
struct Reply<T>(Option<oneshot::Sender<Result<T, Error>>>);

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(reply) = self.0.take() {
            let _ = reply.send(Err(Error::Panicked));
        }
    }
}

/// Counts a piece of accepted work off when its result has gone, or can't go.
struct Delivered(Arc<AtomicUsize>);

impl Drop for Delivered {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Starts work on the app's pool, and waits on the UI thread run by `L`. Clone it into whatever hosts a feature.
///
/// Its four ways in differ in where the work runs and how its result comes back:
///
/// | Method | The work | Its result |
/// |---|---|---|
/// | [`run`](Background::run) | Runs on the app's pool | Goes to the store as an intent, or why there is none as an [`Error`]. |
/// | [`wait`](Background::wait) | Is awaited on the UI thread | Goes to the store as an intent. |
/// | [`finish`](Background::finish) | Is awaited on the UI thread | Goes to a closure, always. |
/// | [`local`](Background::local) | Is awaited on the UI thread | Goes to a closure, unless the returned guard is dropped first. |
///
/// Only `local` can be stopped by its caller. The others run to their end, and a result sent to a store that is gone goes nowhere.
///
/// Every result comes back through the event loop. If that loop has stopped, or its [`EventLoop::spawn_local`] dropped the work, no
/// result arrives, whichever method started it. So, to hear a write's outcome on the way out, keep the event loop running until
/// the feature that wrote says it is done, as [`Lifecycle::CloseRequested`](crate::Lifecycle::CloseRequested) and
/// [`CloseAnswer`](crate::CloseAnswer) let it. Quit the loop after that, and close the pool last.
pub struct Background<L> {
    start: Arc<dyn Fn(Job) -> bool + Send + Sync>,
    /// Accepted work whose result hasn't been delivered yet.
    pending: Arc<AtomicUsize>,
    event_loop: PhantomData<fn() -> L>,
}

impl<L> fmt::Debug for Background<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Background").field("pending", &self.pending.load(Ordering::Acquire)).finish_non_exhaustive()
    }
}

impl<L> Clone for Background<L> {
    fn clone(&self) -> Self {
        Background {
            start: self.start.clone(),
            pending: self.pending.clone(),
            event_loop: PhantomData,
        }
    }
}

impl<L: EventLoop> Background<L> {
    /// `start` starts a job on the app's pool and says whether the pool took it. A job it refuses, or lets go of before it runs,
    /// comes back to its feature as an [`Error`]. The app closes that pool on the way out, after cancelling anything long. A panic in a job goes on to the pool once the feature has been told, so the pool can keep a record.
    pub fn new(start: impl Fn(Job) -> bool + Send + Sync + 'static) -> Background<L> {
        Background {
            start: Arc::new(start),
            pending: Arc::default(),
            event_loop: PhantomData,
        }
    }

    /// Runs `work` off the UI thread, then sends `then(result)` to `intents` on the UI thread. Call it from the UI thread.
    ///
    /// Each call sends one result. It is the work's own, or an [`Error`] saying why there is none: the work panicked, the pool
    /// refused it, or the pool let go of it before it ran. A refusal is sent at once, and runs after the batch under way. Each
    /// feature decides what each of them means, so a page doesn't stay loading for an answer that will never come.
    ///
    /// Accepted work runs to its end on the pool. If the store is gone by then, the result goes nowhere, so anything whose outcome
    /// must be recorded belongs to a store that lives as long as that record matters.
    pub fn run<T, I>(&self, intents: &Intents<I>, work: impl FnOnce() -> T + Send + 'static, then: impl FnOnce(Result<T, Error>) -> I + 'static)
    where
        T: Send + 'static,
        I: 'static,
    {
        let (reply, replied) = oneshot::channel();
        let job = Box::new(move || {
            let mut reply = Reply(Some(reply));
            let result = work();
            if let Some(reply) = reply.0.take() {
                let _ = reply.send(Ok(result));
            }
        });
        if !(self.start)(job) {
            intents.send(then(Err(Error::Refused)));
            return;
        }
        let delivered = self.accepted();
        let intents = intents.clone();
        L::spawn_local(async move {
            let _delivered = delivered;
            // No reply: the pool let go of the job before it began.
            let result = replied.await.unwrap_or(Err(Error::Dropped));
            intents.send(then(result));
        });
    }

    /// Waits for `work` on the UI thread, then sends `then(result)` to `intents`: for an answer that arrives by itself, such as
    /// a reply from a worker, with no pool thread held while it does. Nothing the caller holds can stop it, and the pool closing
    /// doesn't either. It ends with the event loop, though, as all of these do. Call it from the UI thread. A panic here is a
    /// panic on the UI thread, so `work` should only wait.
    pub fn wait<T, I>(&self, intents: &Intents<I>, work: impl Future<Output = T> + 'static, then: impl FnOnce(T) -> I + 'static)
    where
        T: 'static,
        I: 'static,
    {
        let delivered = self.accepted();
        let intents = intents.clone();
        L::spawn_local(async move {
            let _delivered = delivered;
            let result = work.await;
            intents.send(then(result));
        });
    }

    /// Waits for `work` on the UI thread and hands its result to `then`. It returns no guard, so nothing the caller holds can stop
    /// it, which suits a result that must be heard, such as whether a write went through. The event loop still has to be running
    /// when the result comes: see [`Background`] for how to stop the app without losing it. Work that may be let go of goes through
    /// [`Background::local`]. Call it from the UI thread.
    pub fn finish<T: 'static>(&self, work: impl Future<Output = T> + 'static, then: impl FnOnce(T) + 'static) {
        let delivered = self.accepted();
        L::spawn_local(async move {
            let _delivered = delivered;
            then(work.await);
        });
    }

    /// Waits for `work` on the UI thread, then hands its result to `then`. Dropping the returned guard drops the work instead.
    /// Unlike aborting a task, that is safe at any moment, including from inside `then`: a store's reducer runs there, and what it
    /// asks for may be to let go of this very work. Call it from the UI thread.
    pub fn local<T: 'static>(&self, work: impl Future<Output = T> + 'static, then: impl FnOnce(T) + 'static) -> Dropping {
        let delivered = self.accepted();
        droppable::<L, _>(work, move |result| {
            let _delivered = delivered;
            then(result);
        })
    }

    fn accepted(&self) -> Delivered {
        self.pending.fetch_add(1, Ordering::AcqRel);
        Delivered(self.pending.clone())
    }

    /// Accepted work whose result hasn't been delivered yet, for a test to wait on. Work the event loop drops counts off too,
    /// though nothing was delivered.
    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }
}
