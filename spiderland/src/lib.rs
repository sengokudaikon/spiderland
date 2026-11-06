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
//! Stores talk through outputs. A feature names an [`Output`](Feature::Output), `reduce` calls [`Effects::emit`], and
//! [`Store::forward`] turns each output into an intent of another store. [`Lifecycle`] is built on them: a parent showing and
//! hiding its children, and asking whether they can close.
//!
//! A view with properties and no events takes one-shot commands, such as "focus this", through a [`Pulse`].
//!
//! A host's work off the UI thread goes through [`Background`], and comes back as an intent. [`on_watch`] and [`on_wake`] let a
//! service on another thread tell a feature that something changed. All of them run on an [`EventLoop`], the one thing they
//! need from a toolkit, which a host crate such as `spiderland-slint` names once.
//!
//! [`Store::observe`] shows each intent and effect as a store runs it, so an app can keep a trail of what led to a fault.
//!
//! What this leaves out on purpose:
//! - **Outside state as a separate input.** A worker's snapshot reaches a feature as an intent and is kept in its state, so
//!   a render reads one consistent snapshot rather than asking a live handle row by row.
//! - **Whole-view recomputes.** [`Tracked`] and [`Slice`] let a render redo only the parts whose inputs changed.
//! - **Scope machinery.** A scope is a state inside a state: a parent's `reduce` hands the child its state, intent and
//!   [`Effects::scoped`]. A scope that ends is an `Option` going back to `None`.

mod background;
mod lifecycle;
mod macros;
mod pulse;
mod store;
mod subscriptions;
mod tracked;
mod wake;

pub use background::{Background, EventLoop, Job};
pub use lifecycle::{CloseAnswer, Closing, Lifecycle};
pub use pulse::Pulse;
pub use store::{Effects, Feature, Host, Intents, Never, Panicked, Seen, Step, Store, start, step};
pub use subscriptions::Subscriptions;
pub use tracked::{CheckedSlice, Input, Inputs, Slice, Tracked, Version};
pub use wake::{Dropping, Listening, Waker, on_wake, on_watch};
