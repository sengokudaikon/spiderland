//! A service on another thread telling a feature that something changed.

use crate::{EventLoop, Intents};
use std::future::poll_fn;
use std::pin::{Pin, pin};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::Poll;
use tokio::sync::{Notify, oneshot, watch};

/// Fires from any thread. A burst of fires makes at most two intents, the one under way and one more, however long it is.
/// Each intent should read the latest state, so nothing is lost and nothing piles up.
#[derive(Clone)]
pub struct Waker(Arc<Notify>);

impl Waker {
    pub fn wake(&self) {
        self.0.notify_one();
    }
}

/// Drops the work given to [`Background::local`](crate::Background::local) when dropped, if it hasn't finished.
pub struct Dropping(oneshot::Sender<()>);

impl Dropping {
    pub fn is_finished(&self) -> bool {
        self.0.is_closed()
    }
}

/// Waits for `work` on the UI thread, then hands its result to `then`. Dropping the returned guard drops the work and `then`.
pub(crate) fn droppable<L: EventLoop, T: 'static>(work: impl Future<Output = T> + 'static, then: impl FnOnce(T) + 'static) -> Dropping {
    let (guard, mut dropped) = oneshot::channel::<()>();
    L::spawn_local(async move {
        let result = {
            let mut work = pin!(work);
            poll_fn(|context| {
                if Pin::new(&mut dropped).poll(context).is_ready() {
                    return Poll::Ready(None);
                }
                work.as_mut().poll(context).map(Some)
            })
            .await
        };
        drop(dropped);
        if let Some(result) = result {
            then(result);
        }
    });
    Dropping(guard)
}

/// Stops listening when dropped, so it can be a guard for [`Subscriptions`](crate::Subscriptions). That is safe from inside
/// the intent the listener itself sent: nothing is sent after that, and what the listener holds is let go of on the next turn
/// of the event loop, with no need for anything further to be published.
pub struct Listening {
    _work: Dropping,
    _wanted: Rc<()>,
}

impl Listening {
    fn start<L: EventLoop, F: Future<Output = ()> + 'static>(body: impl FnOnce(Weak<()>) -> F) -> Listening {
        let wanted = Rc::new(());
        Listening {
            // Not counted as a Background's work: a listener has no end for a test to wait for.
            _work: droppable::<L, _>(body(Rc::downgrade(&wanted)), |()| {}),
            _wanted: wanted,
        }
    }
}

/// Sends `make()` on the UI thread run by `L` each time a worker publishes on `updates`, and once more when it ends, so a
/// feature can tell. Stops when the returned guard is dropped.
///
/// It hears what is published from now on. To hear a value already published too, call `mark_changed` on `updates` first.
pub fn on_watch<L, T, I>(mut updates: watch::Receiver<T>, intents: &Intents<I>, make: impl Fn() -> I + 'static) -> Listening
where
    L: EventLoop,
    T: 'static,
    I: 'static,
{
    let intents = intents.clone();
    Listening::start::<L, _>(move |wanted| async move {
        loop {
            let ended = updates.changed().await.is_err();
            // The last intent may have let go of this: a value published since doesn't call it back.
            if wanted.strong_count() == 0 {
                break;
            }
            intents.send(make());
            if ended {
                break;
            }
        }
    })
}

/// Sends `make()` on the UI thread run by `L` each time the returned [`Waker`] fires.
pub fn on_wake<L: EventLoop, I: 'static>(intents: &Intents<I>, make: impl Fn() -> I + 'static) -> (Waker, Listening) {
    let notify = Arc::new(Notify::new());
    let intents = intents.clone();
    let waiting = notify.clone();
    let listening = Listening::start::<L, _>(move |wanted| async move {
        loop {
            waiting.notified().await;
            if wanted.strong_count() == 0 {
                break;
            }
            intents.send(make());
        }
    });
    (Waker(notify), listening)
}
