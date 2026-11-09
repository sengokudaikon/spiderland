//! One-way UI state, with no UI toolkit in it.
//!
//! A [`Feature`] is pure: `reduce` takes an intent, changes the state and lists effects as plain data. A [`Host`] is the
//! impure half: it renders the state and performs the effects. A [`Store`] runs one against the other, on one thread.
//!
//! ```text
//! view callback ─┐
//! finished work ─┼─ Intent ─► reduce ─► State ─► Host::render ─► view
//! timer tick ────┘               └────► Effect ─► Host::perform ─► finished work
//! ```
//!
//! A store works in batches. A batch is the intents waiting when the store gets to work. It reduces them all, renders once,
//! starts and stops subscriptions, performs their effects in order, and then sends their outputs. An intent sent from any of
//! that, such as by an effect that finishes at once, waits for the next batch, so the state is never borrowed twice.
//!
//! A whole feature, with the host a test or a window would give it:
//!
//! ```
//! use spiderland::{Effects, Feature, Host, Intents, Never, Store, callbacks};
//!
//! struct Counter;
//! enum Intent {
//!     Add(i32),
//!     Save,
//!     Saved,
//! }
//! enum Effect {
//!     Write(i32),
//! }
//! #[derive(Default)]
//! struct State {
//!     count: i32,
//!     saves: i32,
//! }
//!
//! impl Feature for Counter {
//!     type State = State;
//!     type Intent = Intent;
//!     type Effect = Effect;
//!     type Output = Never;
//!     type Subscription = Never;
//!
//!     fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
//!         match intent {
//!             Intent::Add(by) => state.count += by,
//!             Intent::Save => effects.push(Effect::Write(state.count)),
//!             Intent::Saved => state.saves += 1,
//!         }
//!     }
//! }
//!
//! // Rules are tested with no host at all.
//! let mut state = State::default();
//! spiderland::step::<Counter>(&mut state, Intent::Add(2));
//! assert!(matches!(spiderland::step::<Counter>(&mut state, Intent::Save).effects[..], [Effect::Write(2)]));
//!
//! // A real host sets a view's properties in `render` and starts background work in `perform`.
//! struct Shown(std::rc::Rc<std::cell::Cell<(i32, i32)>>);
//! impl Host<Counter> for Shown {
//!     fn render(&mut self, state: &State, _: &Intents<Intent>) {
//!         self.0.set((state.count, state.saves));
//!     }
//!     fn perform(&mut self, Effect::Write(_): Effect, intents: &Intents<Intent>) {
//!         intents.send(Intent::Saved);
//!     }
//! }
//! let shown = std::rc::Rc::default();
//! let store = Store::new(State::default(), Shown(std::rc::Rc::clone(&shown)));
//! store.send(Intent::Add(2));
//! store.send(Intent::Save);
//! assert_eq!(shown.get(), (2, 1));
//! ```
//!
//! A flow across effects is tested with [`TestStore`]: the whole loop, on a host that keeps what the store asks of it. The test
//! takes each effect, or answers it with the intent its work would send, and an effect left waiting fails the test.
//!
//! Stores talk through outputs. A feature names an [`Output`](Feature::Output), `reduce` calls [`Effects::emit`], and
//! [`Store::forward`] turns each output into an intent of another store. [`Lifecycle`] is built on them: a parent showing and
//! hiding its children, and asking whether they can close. A store built with [`Store::build`] is wired on a [`Wiring`] before
//! it starts, so its listeners hear the start too. [`Store::share`] keeps another store current on part of the state, however
//! late that store is built.
//!
//! A parent can own child stores. Its state names which children exist, by key, and where each stands, and
//! [`Store::children`] keeps a [`Children`] in step: it builds each child as its key appears, drops it once the key goes, and
//! tells it its [`Standing`] as a [`Lifecycle`] event. A stack of pages, a slot and a set of documents are each a shape of
//! keys in the state. A child that is dropped stops what it runs, and an answer for it goes nowhere.
//!
//! Each store can save its part of the session in a [`Saved`], under a key, in a form the app picks: the frame encodes nothing.
//! The app says when to save, such as when the user asks to close, and on the next start each store takes its part back as it is
//! built.
//!
//! A view with properties and no events takes one-shot commands, such as "focus this", through a [`Pulse`].
//!
//! A store has one host, which runs its effects, and any number of [`View`]s, which only show its state. A view attaches with
//! [`Store::attach`] and can leave while the store goes on, as a window on a document does. It shows the state at once, and a
//! [`Fired`] keeps it from acting on a pulse that fired before it attached.
//!
//! What runs for as long as the state wants it, such as a timer while a page shows, is a
//! [`Subscription`](Feature::Subscription). The feature says which ones its state wants, the host starts each one, and the
//! store stops the ones no longer wanted, whether or not a view is there to render.
//!
//! A host's work off the UI thread goes through [`Background`], and comes back as an intent. [`on_watch`] and [`on_wake`] let a
//! service on another thread tell a feature that something changed. All of them run on an [`EventLoop`], the one thing they
//! need from a toolkit, which a host crate such as `spiderland-slint` names once. Work that runs while the state wants it, and
//! says how far it has got, goes through [`Background::stoppable`]. [`Latest`] and [`OneAtATime`] are plain data for a reducer
//! with work in flight: which answer still counts, and what waits its turn.
//!
//! [`Store::observe`] shows each step as a store runs it: each intent and the state after it, each effect and each output. An app
//! can keep a trail of what led to a fault, and what changed on the way.
//!
//! What this leaves out on purpose:
//! - **Outside state as a separate input.** A worker's snapshot reaches a feature as an intent and is kept in its state, so
//!   a render reads one consistent snapshot rather than asking a live handle row by row.
//! - **Whole-view recomputes.** [`Tracked`] and [`Slice`] let a render redo only the parts whose inputs changed.
//! - **Scope machinery.** A child is a state inside a state. A parent's `reduce` hands a child feature its state and intent with
//!   [`Effects::reduce_child`], which wraps its effects and hands back what it emits, or a scope its [`Effects::scoped`]. A
//!   child is kept in a [`Slot`], which numbers each start, so what is sent for a child that was stopped is let go. Stopped,
//!   it stops what it runs.

mod background;
mod children;
mod error;
mod lifecycle;
mod macros;
mod pulse;
mod requests;
mod saved;
mod slot;
mod store;
mod subscriptions;
mod test_store;
mod tracked;
mod wake;

pub use background::{Background, EventLoop, Job, Progress, Stop, Stopping};
pub use children::{Child, Children, Standing};
pub use error::Error;
pub use lifecycle::{CloseAnswer, Closing, Lifecycle};
pub use pulse::{Fired, Pulse};
pub use requests::{Asked, Latest, OneAtATime};
pub use saved::Saved;
pub use slot::{Instance, Slot};
pub use store::{Attached, Connection, Effects, Feature, Host, Intents, Never, Seen, Step, Store, View, WeakStore, Wiring, start, step, subscriptions};
pub use subscriptions::Subscriptions;
pub use test_store::{Subscribed, TestStore};
pub use tracked::{CheckedSlice, Input, Inputs, Slice, Tracked, Version};
pub use wake::{Dropping, Listening, Waker, on_wake, on_watch};
