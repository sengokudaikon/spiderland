//! The reducer's contract and the store that runs it.

use crate::{Child, Children, Saved, Standing, Subscriptions};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::rc::{Rc, Weak};

/// A feature's pure half. `reduce` touches nothing but its arguments, so it can be tested without a window or a worker.
pub trait Feature: 'static {
    /// Plain data: no handles and no live connections.
    type State: 'static;
    /// What happened: the user asked for something, or work came back.
    type Intent: 'static;
    /// Work to do outside the reducer, as data a test can compare.
    type Effect: 'static;
    /// What this feature tells whoever listens, through [`Store::forward`]. A feature that tells no one says [`Never`].
    type Output: 'static;
    /// Something that runs for as long as the state wants it, such as a timer or a watch on a service, as a key the host
    /// starts in [`Host::subscribe`]. A feature that wants none says [`Never`].
    type Subscription: PartialEq + 'static;

    /// Handles one intent: changes `state`, and lists in `effects` the work to do and what to tell others. It touches nothing
    /// else, so [`step`] can test it with no host.
    fn reduce(state: &mut Self::State, intent: Self::Intent, effects: &mut Effects<Self::Effect, Self::Output>);

    /// Runs once as the store is built, before the first render. The effects it asks for are performed after that render. It can't
    /// emit, because nothing can be listening yet. Does nothing unless a feature says otherwise.
    fn start(state: &mut Self::State, effects: &mut Effects<Self::Effect>) {
        let _ = (state, effects);
    }

    /// The subscriptions `state` wants, each key once. After each batch's render, the store starts the new ones and drops the
    /// ones no longer wanted, which stops them, so what runs follows the state alone. Wants none unless a feature says otherwise.
    ///
    /// A clock that ticks while it shows, and the host that starts it:
    ///
    /// ```
    /// use spiderland::{Effects, Feature, Host, Intents, Never, Store};
    /// use std::any::Any;
    /// use std::cell::Cell;
    /// use std::rc::Rc;
    ///
    /// struct Clock;
    /// #[derive(Debug, PartialEq)]
    /// struct Tick;
    ///
    /// impl Feature for Clock {
    ///     // Whether the clock shows.
    ///     type State = bool;
    ///     type Intent = bool;
    ///     type Effect = Never;
    ///     type Output = Never;
    ///     type Subscription = Tick;
    ///
    ///     fn reduce(showing: &mut bool, show: bool, _: &mut Effects<Never>) {
    ///         *showing = show;
    ///     }
    ///
    ///     fn subscriptions(showing: &bool) -> impl IntoIterator<Item = Tick> {
    ///         showing.then_some(Tick)
    ///     }
    /// }
    ///
    /// // Tested with no host.
    /// assert_eq!(spiderland::subscriptions::<Clock>(&true), [Tick]);
    ///
    /// // Stands in for a timer: it runs while its guard lives.
    /// struct Running(Rc<Cell<bool>>);
    /// impl Drop for Running {
    ///     fn drop(&mut self) {
    ///         self.0.set(false);
    ///     }
    /// }
    ///
    /// struct ClockHost(Rc<Cell<bool>>);
    /// impl Host<Clock> for ClockHost {
    ///     fn render(&mut self, _: &bool, _: &Intents<bool>) {}
    ///     fn perform(&mut self, effect: Never, _: &Intents<bool>) {
    ///         match effect {}
    ///     }
    ///     // A Slint host returns the timer from `spiderland_slint::every` here.
    ///     fn subscribe(&mut self, _: &Tick, _: &Intents<bool>) -> Box<dyn Any> {
    ///         self.0.set(true);
    ///         Box::new(Running(Rc::clone(&self.0)))
    ///     }
    /// }
    ///
    /// let ticking = Rc::new(Cell::new(false));
    /// let store = Store::new(false, ClockHost(Rc::clone(&ticking)));
    /// store.send(true);
    /// assert!(ticking.get());
    /// store.send(false);
    /// assert!(!ticking.get(), "the store dropped the guard once the clock was hidden");
    /// ```
    fn subscriptions(state: &Self::State) -> impl IntoIterator<Item = Self::Subscription> {
        let _ = state;
        []
    }
}

/// An output that can't happen, for a feature that tells no one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Never {}

/// Names a value that can't happen, for a trail that names its steps by `Into<&'static str>`, as [`Store::observe`] says. There is
/// never one to name, so one trail covers a feature with no effects or no outputs.
impl From<&Never> for &'static str {
    fn from(never: &Never) -> Self {
        match *never {}
    }
}

/// What one `reduce` asks for. After the next render its effects are performed in order, and then its outputs go out in order.
#[derive(Debug)]
pub struct Effects<E, O = Never> {
    effects: Vec<E>,
    outputs: Vec<O>,
}

impl<E, O> Effects<E, O> {
    /// A list no store performs. Code shared with the reducer can take it to run outside `reduce`, as a view does to see what that
    /// code decides, and what it asks for there is dropped.
    pub fn detached() -> Self {
        Effects {
            effects: Vec::new(),
            outputs: Vec::new(),
        }
    }

    /// Asks for `effect`, which the host performs after the next render.
    pub fn push(&mut self, effect: E) {
        self.effects.push(effect);
    }

    /// Tells whoever listens through [`Store::forward`].
    pub fn emit(&mut self, output: O) {
        self.outputs.push(output);
    }

    /// Runs a child scope's reducer, which lists its own effect type, and wraps each of them as this feature's. The child emits
    /// nothing: telling the outside is up to the parent. For a child that is a [`Feature`] of its own, use
    /// [`Effects::reduce_child`].
    pub fn scoped<C, R>(&mut self, wrap: impl Fn(C) -> E, child: impl FnOnce(&mut Effects<C>) -> R) -> R {
        let mut effects = Effects::detached();
        let result = child(&mut effects);
        self.effects.extend(effects.effects.into_iter().map(wrap));
        result
    }

    /// Runs a child feature's `reduce` on its part of this feature's state, inside this feature's store. The child's effects
    /// become this feature's through `wrap`. What the child emits comes back here, for the parent to act on: it can change its
    /// state, emit an output of its own, or let it go.
    ///
    /// The rest of a child inside its parent:
    /// - Its intents: the parent has an intent that wraps the child's, and hands it on here.
    /// - Its subscriptions: the parent's [`Feature::subscriptions`] wraps the child's. A child kept in an `Option` stops
    ///   what it runs once it goes back to `None`, with nothing to cancel.
    /// - Its host: the parent's host keeps the child's, and calls it with [`Intents::map`], so what it sends comes back
    ///   wrapped.
    ///
    /// An upload inside a page, which the page drops to stop it:
    ///
    /// ```
    /// use spiderland::{Effects, Feature, Host, Intents, Never, Store};
    /// use std::any::Any;
    /// use std::{cell::RefCell, rc::Rc};
    ///
    /// struct Upload;
    /// enum UploadIntent {
    ///     Sent(u32),
    /// }
    /// #[derive(PartialEq)]
    /// struct Sending;
    /// impl Feature for Upload {
    ///     /// The parts sent so far.
    ///     type State = u32;
    ///     type Intent = UploadIntent;
    ///     type Effect = Never;
    ///     /// Says when the last part is sent.
    ///     type Output = ();
    ///     type Subscription = Sending;
    ///
    ///     fn reduce(sent: &mut u32, UploadIntent::Sent(parts): UploadIntent, effects: &mut Effects<Never, ()>) {
    ///         *sent = parts;
    ///         if parts == 3 {
    ///             effects.emit(());
    ///         }
    ///     }
    ///
    ///     fn subscriptions(_: &u32) -> impl IntoIterator<Item = Sending> {
    ///         [Sending]
    ///     }
    /// }
    ///
    /// struct Page;
    /// #[derive(Default)]
    /// struct PageState {
    ///     upload: Option<u32>,
    ///     done: bool,
    /// }
    /// enum PageIntent {
    ///     Start,
    ///     Stop,
    ///     Upload(UploadIntent),
    /// }
    /// #[derive(PartialEq)]
    /// enum Running {
    ///     Upload(Sending),
    /// }
    /// impl Feature for Page {
    ///     type State = PageState;
    ///     type Intent = PageIntent;
    ///     type Effect = Never;
    ///     type Output = Never;
    ///     type Subscription = Running;
    ///
    ///     fn reduce(state: &mut PageState, intent: PageIntent, effects: &mut Effects<Never>) {
    ///         match intent {
    ///             PageIntent::Start => state.upload = Some(0),
    ///             PageIntent::Stop => state.upload = None,
    ///             PageIntent::Upload(intent) => {
    ///                 // An intent for an upload that is gone is let go.
    ///                 let Some(upload) = state.upload.as_mut() else { return };
    ///                 let ended = effects.reduce_child::<Upload>(upload, intent, |never| match never {});
    ///                 if !ended.is_empty() {
    ///                     state.upload = None;
    ///                     state.done = true;
    ///                 }
    ///             }
    ///         }
    ///     }
    ///
    ///     fn subscriptions(state: &PageState) -> impl IntoIterator<Item = Running> {
    ///         state.upload.iter().flat_map(Upload::subscriptions).map(Running::Upload)
    ///     }
    /// }
    ///
    /// /// The upload's host. It keeps the sender it is given, as a timer or a worker would.
    /// struct UploadHost(Rc<RefCell<Option<Intents<UploadIntent>>>>);
    /// impl Host<Upload> for UploadHost {
    ///     fn perform(&mut self, effect: Never, _: &Intents<UploadIntent>) {
    ///         match effect {}
    ///     }
    ///     fn subscribe(&mut self, Sending: &Sending, intents: &Intents<UploadIntent>) -> Box<dyn Any> {
    ///         *self.0.borrow_mut() = Some(intents.clone());
    ///         Box::new(())
    ///     }
    /// }
    ///
    /// /// The page's host runs the upload's.
    /// struct PageHost(UploadHost);
    /// impl Host<Page> for PageHost {
    ///     fn perform(&mut self, effect: Never, _: &Intents<PageIntent>) {
    ///         match effect {}
    ///     }
    ///     fn subscribe(&mut self, key: &Running, intents: &Intents<PageIntent>) -> Box<dyn Any> {
    ///         match key {
    ///             Running::Upload(key) => self.0.subscribe(key, &intents.map(PageIntent::Upload)),
    ///         }
    ///     }
    /// }
    ///
    /// let worker = Rc::new(RefCell::new(None));
    /// let page = Store::new(PageState::default(), PageHost(UploadHost(Rc::clone(&worker))));
    /// page.send(PageIntent::Start);
    /// let sent = worker.borrow().clone().expect("the upload runs");
    /// sent.send(UploadIntent::Sent(3));
    /// assert_eq!(page.try_read(|state| (state.upload, state.done)), Some((None, true)));
    /// ```
    pub fn reduce_child<C: Feature>(&mut self, state: &mut C::State, intent: C::Intent, wrap: impl Fn(C::Effect) -> E) -> Vec<C::Output> {
        let mut effects = Effects::detached();
        C::reduce(state, intent, &mut effects);
        self.effects.extend(effects.effects.into_iter().map(wrap));
        effects.outputs
    }

    /// Runs a child feature's [`Feature::start`] on its part of this feature's state, as the parent makes the child. The
    /// child's effects become this feature's through `wrap`, as for [`Effects::reduce_child`].
    pub fn start_child<C: Feature>(&mut self, state: &mut C::State, wrap: impl Fn(C::Effect) -> E) {
        let mut effects = Effects::detached();
        C::start(state, &mut effects);
        self.effects.extend(effects.effects.into_iter().map(wrap));
    }
}

/// What one reducer step asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step<E, O = Never> {
    /// The effects it asked for, in order.
    pub effects: Vec<E>,
    /// What it emitted, in order.
    pub outputs: Vec<O>,
}

/// One step of what a store runs, as [`Store::observe`] shows it. More kinds of step may come, so a match over it ends with `_`.
#[non_exhaustive]
pub enum Seen<'a, F: Feature> {
    /// An intent, just before it is reduced.
    Intent(&'a F::Intent),
    /// The state just after an intent is reduced, and as the store starts, once [`Feature::start`] has run. The last one before
    /// a batch's effects is the state its render showed.
    State(&'a F::State),
    /// An effect, just before it is performed.
    Effect(&'a F::Effect),
    /// An output, just before the store's listeners hear it.
    Output(&'a F::Output),
}

impl<F: Feature> Clone for Seen<'_, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F: Feature> Copy for Seen<'_, F> {}

impl<F: Feature> fmt::Debug for Seen<'_, F>
where
    F::Intent: fmt::Debug,
    F::State: fmt::Debug,
    F::Effect: fmt::Debug,
    F::Output: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Seen::Intent(intent) => f.debug_tuple("Intent").field(intent).finish(),
            Seen::State(state) => f.debug_tuple("State").field(state).finish(),
            Seen::Effect(effect) => f.debug_tuple("Effect").field(effect).finish(),
            Seen::Output(output) => f.debug_tuple("Output").field(output).finish(),
        }
    }
}

/// One reducer step, for tests: changes `state` and returns what `intent` asked for.
pub fn step<F: Feature>(state: &mut F::State, intent: F::Intent) -> Step<F::Effect, F::Output> {
    let mut effects = Effects::detached();
    F::reduce(state, intent, &mut effects);
    Step {
        effects: effects.effects,
        outputs: effects.outputs,
    }
}

/// [`Feature::start`], for tests: changes `state` and returns the effects it asked for.
pub fn start<F: Feature>(state: &mut F::State) -> Vec<F::Effect> {
    let mut effects = Effects::detached();
    F::start(state, &mut effects);
    effects.effects
}

/// [`Feature::subscriptions`], for tests: what `state` wants running, in order.
pub fn subscriptions<F: Feature>(state: &F::State) -> Vec<F::Subscription> {
    F::subscriptions(state).into_iter().collect()
}

/// A feature's impure half: the view, the timers and the services.
pub trait Host<F: Feature>: 'static {
    /// Called once after each batch of intents. Should only do what the changed parts of the state need. A host whose views
    /// attach with [`Store::attach`], and so has no view of its own, leaves this out.
    fn render(&mut self, state: &F::State, intents: &Intents<F::Intent>) {
        let _ = (state, intents);
    }

    /// Starts one effect. Work that finishes later sends its result through `intents`.
    fn perform(&mut self, effect: F::Effect, intents: &Intents<F::Intent>);

    /// Starts one subscription the state wants, and returns its guard: dropping the guard must stop it. The store drops it once
    /// the state no longer wants `key`, or when the store goes.
    ///
    /// A host whose feature wants no subscriptions leaves this out. One whose feature wants some must write it. Nothing checks
    /// that as it builds: the first key the state wants panics instead.
    fn subscribe(&mut self, key: &F::Subscription, intents: &Intents<F::Intent>) -> Box<dyn Any> {
        let _ = (key, intents);
        panic!("{} wants a subscription, and its host doesn't write `Host::subscribe`", std::any::type_name::<F>())
    }
}

/// A view that attaches to a store with [`Store::attach`], and can leave it while the store goes on, as a window on a document
/// does. A store has one [`Host`], which runs its effects, and any number of views, which only show its state. A view that
/// attaches late shows the state at once, so it needs no render of its own to catch up.
///
/// A view that shows a [`Pulse`](crate::Pulse) keeps a [`Fired`](crate::Fired) for it, so it doesn't act on a fire from before
/// it attached.
pub trait View<F: Feature>: 'static {
    /// Shows `state`. Called as the view attaches, with the state as it is then, and after each batch while it stays attached,
    /// after the host's own render. Should only do what the changed parts of the state need.
    fn render(&mut self, state: &F::State, intents: &Intents<F::Intent>);

    /// True once there is nothing left to show the state in, such as a window that has closed. The store then lets the view
    /// go, as if its [`Attached`] were dropped. A view is never gone unless it says so.
    fn is_gone(&self) -> bool {
        false
    }
}

/// One view attached to a store.
struct Viewing<F: Feature> {
    view: RefCell<Box<dyn View<F>>>,
    /// Cleared when it detaches or is gone, so a render already under way skips it.
    attached: Cell<bool>,
}

trait Sink<I> {
    fn send(&self, intent: I);
    fn send_all(&self, intents: Vec<I>);
    fn is_alive(&self) -> bool;
}

/// A store's own intents, for as long as the store lives.
impl<F: Feature> Sink<F::Intent> for Weak<Inner<F>> {
    fn send(&self, intent: F::Intent) {
        if let Some(inner) = self.upgrade() {
            inner.accept(intent);
        }
    }

    fn send_all(&self, intents: Vec<F::Intent>) {
        if let Some(inner) = self.upgrade() {
            inner.accept_all(intents);
        }
    }

    fn is_alive(&self) -> bool {
        self.strong_count() > 0
    }
}

/// A child's intents, turned into its parent's.
struct Mapped<I, M> {
    to: Intents<I>,
    map: M,
}

impl<C, I, M: Fn(C) -> I> Sink<C> for Mapped<I, M> {
    fn send(&self, intent: C) {
        self.to.send((self.map)(intent));
    }

    fn send_all(&self, intents: Vec<C>) {
        self.to.0.send_all(intents.into_iter().map(&self.map).collect());
    }

    fn is_alive(&self) -> bool {
        self.to.is_alive()
    }
}

/// Where intents go. It doesn't keep the store alive, so a timer or a thread holding one can't keep a feature going: once
/// the store is gone, sends do nothing.
pub struct Intents<I>(Rc<dyn Sink<I>>);

impl<I> Clone for Intents<I> {
    fn clone(&self) -> Self {
        Intents(self.0.clone())
    }
}

impl<I> fmt::Debug for Intents<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Intents").field("alive", &self.is_alive()).finish()
    }
}

impl<I> Intents<I> {
    /// Sends `intent` to the store, which runs it as [`Store::send`] does. Once the store is gone, it does nothing.
    pub fn send(&self, intent: I) {
        self.0.send(intent);
    }

    /// Sends `intents` as one batch: all of them are reduced, in order, before the render and the effects that follow. It suits
    /// work that reports two things at once, such as a fresh snapshot and the answer that came with it: sent one by one, the
    /// host would render and perform for a state with the snapshot and not yet the answer.
    pub fn send_all(&self, intents: impl IntoIterator<Item = I>) {
        self.0.send_all(intents.into_iter().collect());
    }

    /// A sender of another type, whose intents `map` turns into these. It suits a child's host inside its parent's store. Like this
    /// sender, it doesn't keep the store alive.
    pub fn map<C: 'static>(&self, map: impl Fn(C) -> I + 'static) -> Intents<C>
    where
        I: 'static,
    {
        Intents(Rc::new(Mapped { to: self.clone(), map }))
    }

    fn is_alive(&self) -> bool {
        self.0.is_alive()
    }
}

/// Hears one output, and says false once no one is left to tell.
type Listener<O> = Rc<Heard<O>>;

/// What a [`Listener`] runs.
type Heard<O> = dyn Fn(&O) -> bool;

type Observer<F> = Rc<dyn Fn(Seen<'_, F>)>;

/// Takes an output once every listener has heard it.
type Keeper<O> = Box<dyn FnMut(O)>;

/// Looks at the state after a batch and says what to send, if anything.
type Watcher<S> = Rc<dyn Fn(&S) -> Watched>;

/// Looks at the state after a batch and says which children it wants, then keeps them once no borrow of the state is held.
type Family<S> = Rc<dyn Fn(&S) -> Box<dyn FnOnce()>>;

/// What a [`Watcher`] makes of the state.
enum Watched {
    /// What it shares hasn't changed.
    Same,
    /// What it shares changed: this sends it, once no borrow of the state is held.
    Send(Box<dyn FnOnce()>),
    /// Its destination is gone, so it can go too.
    Gone,
}

struct Inner<F: Feature> {
    state: RefCell<F::State>,
    /// Declared before the host, so what runs stops before the host goes.
    subscriptions: RefCell<Subscriptions<F::Subscription>>,
    host: RefCell<Box<dyn Host<F>>>,
    queue: RefCell<VecDeque<F::Intent>>,
    /// Effects of reduced intents, waiting for the render that goes before them.
    effects: RefCell<VecDeque<F::Effect>>,
    /// Outputs of reduced intents, waiting for those effects.
    outputs: RefCell<VecDeque<F::Output>>,
    /// Replaced whole when one is added or ends, so telling them costs no copy of the list. Observers likewise.
    listeners: RefCell<Rc<[Listener<F::Output>]>>,
    observers: RefCell<Rc<[Observer<F>]>>,
    watchers: RefCell<Rc<[Watcher<F::State>]>>,
    families: RefCell<Rc<[Family<F::State>]>>,
    views: RefCell<Rc<[Rc<Viewing<F>>]>>,
    /// Set only by a [`TestStore`](crate::TestStore), to keep the outputs. Any other store lets each go once it is told.
    keeper: RefCell<Option<Keeper<F::Output>>>,
    /// Also set while the store is built and not yet started, so what it is sent waits for the start.
    draining: Cell<bool>,
    /// Holds only a weak reference to this store.
    intents: Intents<F::Intent>,
}

/// Runs a feature on the UI thread. Clones share one store; whoever holds a clone keeps it alive (usually the view's
/// callbacks).
pub struct Store<F: Feature>(Rc<Inner<F>>);

impl<F: Feature> Clone for Store<F> {
    fn clone(&self) -> Self {
        Store(self.0.clone())
    }
}

impl<F: Feature> fmt::Debug for Store<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Store").field(&format_args!("{}", std::any::type_name::<F>())).finish()
    }
}

/// A store held without keeping it alive, from [`Store::downgrade`]. It suits a window's handler that needs the store while the
/// store lasts, where a [`Store`] would keep it going for as long as the window does.
pub struct WeakStore<F: Feature>(Weak<Inner<F>>);

impl<F: Feature> Clone for WeakStore<F> {
    fn clone(&self) -> Self {
        WeakStore(self.0.clone())
    }
}

impl<F: Feature> fmt::Debug for WeakStore<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakStore")
            .field("feature", &format_args!("{}", std::any::type_name::<F>()))
            .field("alive", &(self.0.strong_count() > 0))
            .finish()
    }
}

impl<F: Feature> WeakStore<F> {
    /// The store, while something else keeps it alive.
    pub fn upgrade(&self) -> Option<Store<F>> {
        self.0.upgrade().map(Store)
    }
}

impl<F: Feature> Store<F> {
    /// Builds the store and starts it, as [`Store::build`] and [`Wiring::start`] do. What the store does as it starts reaches
    /// no listener and no observer, since none can be there yet. Wire a store that has to be heard from the start on a [`Wiring`].
    pub fn new(state: F::State, host: impl Host<F>) -> Self {
        Self::build(state, host).start()
    }

    /// Builds the store without starting it, so its listeners and observers can be wired first. Nothing runs until
    /// [`Wiring::start`]. Intents sent to it before then wait, and run right after the start.
    pub fn build(state: F::State, host: impl Host<F>) -> Wiring<F> {
        let inner = Rc::new_cyclic(|this: &Weak<Inner<F>>| Inner {
            state: RefCell::new(state),
            subscriptions: RefCell::default(),
            host: RefCell::new(Box::new(host) as Box<dyn Host<F>>),
            queue: RefCell::new(VecDeque::new()),
            effects: RefCell::new(VecDeque::new()),
            outputs: RefCell::new(VecDeque::new()),
            listeners: RefCell::new(Rc::from(Vec::new())),
            observers: RefCell::new(Rc::from(Vec::new())),
            watchers: RefCell::new(Rc::from(Vec::new())),
            families: RefCell::new(Rc::from(Vec::new())),
            views: RefCell::new(Rc::from(Vec::new())),
            keeper: RefCell::new(None),
            draining: Cell::new(true),
            intents: Intents(Rc::new(this.clone())),
        });
        Wiring(Store(inner))
    }

    /// Handles `intent` now, unless the store is already at work (a render or an effect called back into the view, an effect
    /// finished at once): then it runs next, after the current batch. State is never borrowed twice.
    pub fn send(&self, intent: F::Intent) {
        self.0.accept(intent);
    }

    /// Sends `intents` as one batch, as [`Intents::send_all`] does.
    pub fn send_all(&self, intents: impl IntoIterator<Item = F::Intent>) {
        self.0.accept_all(intents.into_iter().collect());
    }

    /// Sends `intent` and reads what it did, for a view callback that returns a value. None while the store is at work: the
    /// intent is then only queued and will still run, and the state doesn't show its result yet.
    pub fn ask<R>(&self, intent: F::Intent, read: impl FnOnce(&F::State) -> R) -> Option<R> {
        let queued = self.0.draining.get();
        self.send(intent);
        if queued { None } else { self.try_read(read) }
    }

    /// A sender of this store's intents, for a view's callbacks, a timer or a worker. It doesn't keep the store alive.
    pub fn intents(&self) -> Intents<F::Intent> {
        self.0.intents.clone()
    }

    /// This store, held without keeping it alive.
    pub fn downgrade(&self) -> WeakStore<F> {
        WeakStore(Rc::downgrade(&self.0))
    }

    /// Sends `map(output)` to `to` for each output this store emits, in the order they were forwarded. `map` returns an intent,
    /// or `None` to ignore that output. It keeps neither store alive, and ends with whichever store goes first. Once `to` is
    /// gone, what `map` holds is let go of the next time this store emits.
    ///
    /// An output emitted before there is a listener reaches no one, so a store whose start has to be heard is wired on a
    /// [`Wiring`]. If `map` needs to reach a store, it should hold [`Intents`], because a [`Store`] it holds stays alive for as
    /// long as this one does. To end a connection while both stores go on, use [`Store::connect`].
    pub fn forward<I, R>(&self, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static)
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        self.0.listen(to, map, Rc::new(Cell::new(true)));
    }

    /// Forwards as [`Store::forward`] does, until the returned [`Connection`] is dropped. Dropping it lets go of `map` at once,
    /// and nothing goes through it after that, not even an output from the batch under way.
    pub fn connect<I, R>(&self, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static) -> Connection
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        Inner::connect(&self.0, to, map)
    }

    /// Attaches `view`, which shows the state at once and then after each batch, until the returned [`Attached`] is dropped or
    /// the view says it [is gone](View::is_gone). The store doesn't keep its views alive: what a view holds is let go of as it
    /// detaches.
    ///
    /// ```
    /// use spiderland::{Effects, Feature, Host, Intents, Never, Store, View};
    /// use std::{cell::Cell, rc::Rc};
    ///
    /// struct Counter;
    /// impl Feature for Counter {
    ///     type State = i32;
    ///     type Intent = i32;
    ///     type Effect = Never;
    ///     type Output = Never;
    ///     type Subscription = Never;
    ///
    ///     fn reduce(count: &mut i32, by: i32, _: &mut Effects<Never>) {
    ///         *count += by;
    ///     }
    /// }
    ///
    /// /// Runs the effects, and shows nothing itself.
    /// struct Runner;
    /// impl Host<Counter> for Runner {
    ///     fn perform(&mut self, effect: Never, _: &Intents<i32>) {
    ///         match effect {}
    ///     }
    /// }
    ///
    /// struct Shown(Rc<Cell<i32>>);
    /// impl View<Counter> for Shown {
    ///     fn render(&mut self, count: &i32, _: &Intents<i32>) {
    ///         self.0.set(*count);
    ///     }
    /// }
    ///
    /// let store = Store::new(0, Runner);
    /// store.send(2);
    /// let late = Rc::new(Cell::new(0));
    /// let attached = store.attach(Shown(Rc::clone(&late)));
    /// assert_eq!(late.get(), 2, "a view that attaches late shows the state at once");
    /// store.send(3);
    /// assert_eq!(late.get(), 5);
    /// drop(attached);
    /// store.send(1);
    /// assert_eq!(late.get(), 5, "a view that has left shows nothing more");
    /// ```
    pub fn attach(&self, view: impl View<F>) -> Attached {
        let (viewing, attached) = Inner::add_view(&self.0, view);
        self.0.render_attached(&viewing);
        attached
    }

    /// How many views are attached and not gone. A view counts until its [`Attached`] is dropped or it says it is gone, such as
    /// when a window closes, so one of several windows can tell whether it is the last.
    pub fn views(&self) -> usize {
        let views = Rc::clone(&self.0.views.borrow());
        // A view that is rendering can't be asked, and is there.
        views
            .iter()
            .filter(|viewing| viewing.attached.get() && viewing.view.try_borrow().map_or(true, |view| !view.is_gone()))
            .count()
    }

    /// Sends `map(&project(state))` to `to` now, and again after each batch in which the projection changed. It suits state that
    /// another store keeps a copy of, such as settings: an output is heard only by those listening when it goes out, but a store
    /// connected later starts on the current value. It ends with whichever store goes first.
    ///
    /// A store being built hears the first value as an intent once it has started, after its [`Feature::start`]. One that loads
    /// on start would load once on its own defaults first. Build its state on the current value instead, read with
    /// [`Store::try_read`], and the first value shared changes nothing.
    ///
    /// After each batch, the projection is made again and compared with the last one sent, so keep it cheap: a few fields, not a
    /// whole list.
    pub fn share<T, I>(&self, to: &Intents<I>, project: impl Fn(&F::State) -> T + 'static, map: impl Fn(&T) -> I + 'static)
    where
        T: PartialEq + 'static,
        I: 'static,
    {
        let watcher = self.0.watch(to, project, map);
        self.0.tell_watchers(&[watcher]);
    }

    /// Keeps `children` in step with the state: after each batch, `wanted` names the children the state wants and where each
    /// stands. A new key's child is made with `build`, which is handed this store's intents so it can forward the child's
    /// outputs here. A child no longer named is dropped, and each one is told its [`Standing`] as it is built and whenever it
    /// changes. [`Children`] shows a stack of pages.
    ///
    /// It runs after the batch's render and subscriptions and before its effects, so an effect can reach a child its batch
    /// named. On a started store it keeps the children at once, as they stand now. Wire a store's first children on a
    /// [`Wiring`].
    ///
    /// `build` lives as long as this store, so it shouldn't hold this store: forward to the intents it is handed.
    pub fn children<K, T>(&self, children: &Children<K, T>, wanted: impl Fn(&F::State) -> Vec<(K, Standing)> + 'static, build: impl FnMut(&K, &Intents<F::Intent>) -> T + 'static)
    where
        K: PartialEq + 'static,
        T: Child,
    {
        let family = self.0.keep(children, wanted, build);
        // As for a view that attaches: what a child sends here as it is built waits until they are all kept.
        let idle = !self.0.draining.get();
        {
            let _draining = idle.then(|| Draining::start(&self.0.draining));
            let keep = family(&self.0.state.borrow());
            keep();
        }
        if idle && !self.0.queue.borrow().is_empty() {
            self.0.drain();
        }
    }

    /// Calls `observer` with each step the store runs, in order, as a [`Seen`]: each intent just before it is reduced and the
    /// state just after, each effect just before it is performed, and each output just before it is heard. A batch shows as its
    /// intents, each with the state after it, then its effects, then its outputs.
    ///
    /// It suits a trail of what led to a fault, and what changed on the way. An intent with no state after it is the one whose
    /// reducer panicked.
    ///
    /// It is handed references, so nothing is copied unless it copies, and nothing needs `Debug`. Keep a name, not the intent
    /// itself, because an intent can carry a whole snapshot. From the state, keep what the trail needs, such as a status line, and
    /// record it when it differs from the one kept before.
    ///
    /// Rust has no name for an enum's variant on its own. A trail that names its steps can ask for
    /// `&F::Intent: Into<&'static str>`, which a match or a derive provides. [`Never`] meets it, so one trail covers a feature with
    /// no effects or no outputs.
    ///
    /// No borrow is held while it is shown an intent, an effect or an output, so it can add another observer, or send an intent,
    /// which waits its turn. While it is shown the state, the state is borrowed: it can read it, as [`Store::try_read`] does,
    /// and nothing can change it.
    ///
    /// It sees only what runs after it is added. To see the store's start too, add it on a [`Wiring`], which shows it the state
    /// once [`Feature::start`] has run. It lives as long as the store, so it shouldn't hold the store. If it panics when shown an
    /// intent, an effect or an output, that step is lost, as with a reducer that panics.
    ///
    /// ```
    /// use spiderland::{Effects, Feature, Host, Intents, Never, Seen, Store};
    /// use std::{cell::Cell, cell::RefCell, collections::VecDeque, rc::Rc};
    ///
    /// struct Counter;
    /// enum Intent {
    ///     Add(i32),
    ///     Save,
    /// }
    /// enum Effect {
    ///     Write(i32),
    /// }
    ///
    /// impl Feature for Counter {
    ///     type State = i32;
    ///     type Intent = Intent;
    ///     type Effect = Effect;
    ///     type Output = Never;
    ///     type Subscription = Never;
    ///
    ///     fn reduce(count: &mut i32, intent: Intent, effects: &mut Effects<Effect>) {
    ///         match intent {
    ///             Intent::Add(by) => *count += by,
    ///             Intent::Save => effects.push(Effect::Write(*count)),
    ///         }
    ///     }
    /// }
    ///
    /// struct Quiet;
    /// impl Host<Counter> for Quiet {
    ///     fn render(&mut self, _: &i32, _: &Intents<Intent>) {}
    ///     fn perform(&mut self, _: Effect, _: &Intents<Intent>) {}
    /// }
    ///
    /// // The last four things the store ran, by name, and each count it reached.
    /// let trail = Rc::new(RefCell::new(VecDeque::new()));
    /// let store = Store::new(0, Quiet);
    /// let (keeping, last) = (Rc::clone(&trail), Cell::new(0));
    /// store.observe(move |seen| {
    ///     let line = match seen {
    ///         Seen::Intent(Intent::Add(_)) => "Add".to_owned(),
    ///         Seen::Intent(Intent::Save) => "Save".to_owned(),
    ///         Seen::State(count) => {
    ///             // Only a count that changed.
    ///             if last.replace(*count) == *count {
    ///                 return;
    ///             }
    ///             format!("now {count}")
    ///         }
    ///         Seen::Effect(Effect::Write(_)) => "Write".to_owned(),
    ///         _ => return,
    ///     };
    ///     let mut trail = keeping.borrow_mut();
    ///     if trail.len() == 4 {
    ///         trail.pop_front();
    ///     }
    ///     trail.push_back(line);
    /// });
    /// store.send(Intent::Add(1));
    /// store.send(Intent::Add(2));
    /// store.send(Intent::Save);
    /// assert_eq!(*trail.borrow(), ["Add", "now 3", "Save", "Write"]);
    /// ```
    pub fn observe(&self, observer: impl Fn(Seen<'_, F>) + 'static) {
        let mut observers = self.0.observers.borrow_mut();
        let added: Observer<F> = Rc::new(observer);
        *observers = observers.iter().cloned().chain([added]).collect();
    }

    /// Saves this store's state in `saved` under `key`, as `encode` makes it, each time the app calls [`Saved::save`], for as long
    /// as the store is alive. It doesn't keep the store alive. On the next start the store is built on what [`Saved::take`] gives
    /// back for `key`, decoded by the app. A key added again is the later store's.
    pub fn save_in<V: 'static>(&self, saved: &Saved<V>, key: impl Into<String>, encode: impl Fn(&F::State) -> V + 'static) {
        saved.add(key.into(), crate::saved::saver(self, encode));
    }

    /// Runs `change` on the state directly, for a test or a capture to set up or look at a state. Nothing renders until the
    /// next intent. Only under the `testing` feature: a real feature's changes go through its reducer, and to read the state,
    /// use [`Store::try_read`].
    #[cfg(feature = "testing")]
    pub fn with_state<R>(&self, change: impl FnOnce(&mut F::State) -> R) -> R {
        change(&mut self.0.state.borrow_mut())
    }

    /// Answers a view's question from the state, for a pure callback that a binding calls. None while a reducer is running,
    /// which a binding can only meet if a reducer made the toolkit evaluate one. To read what an intent did, use [`Store::ask`].
    pub fn try_read<R>(&self, read: impl FnOnce(&F::State) -> R) -> Option<R> {
        self.0.state.try_borrow().ok().map(|state| read(&state))
    }

    /// Hands each output to `keep` once every listener has heard it, for a [`TestStore`](crate::TestStore) to keep them.
    pub(crate) fn keep_outputs(&self, keep: impl FnMut(F::Output) + 'static) {
        *self.0.keeper.borrow_mut() = Some(Box::new(keep));
    }

    /// Whether the store has started `key` for the state and not yet stopped it.
    pub(crate) fn is_running(&self, key: &F::Subscription) -> bool {
        self.0.subscriptions.borrow().contains(key)
    }
}

/// A store that is built and not yet started, from [`Store::build`]. It is for wiring what listens to the store before
/// anything runs, so that listeners and observers hear the start too. [`Wiring::start`] starts it.
///
/// ```
/// use spiderland::{Effects, Feature, Host, Intents, Never, Store};
/// use std::{cell::RefCell, rc::Rc};
///
/// /// Says hello as soon as its first effect comes back.
/// struct Greeter;
/// impl Feature for Greeter {
///     type State = ();
///     type Intent = ();
///     type Effect = ();
///     type Output = &'static str;
///     type Subscription = Never;
///
///     fn reduce(_: &mut (), (): (), effects: &mut Effects<(), &'static str>) {
///         effects.emit("hello");
///     }
///
///     fn start(_: &mut (), effects: &mut Effects<()>) {
///         effects.push(());
///     }
/// }
///
/// /// Its work finishes at once.
/// struct AtOnce;
/// impl Host<Greeter> for AtOnce {
///     fn render(&mut self, _: &(), _: &Intents<()>) {}
///     fn perform(&mut self, (): (), intents: &Intents<()>) {
///         intents.send(());
///     }
/// }
///
/// let heard = Rc::new(RefCell::new(Vec::new()));
/// let wiring = Store::<Greeter>::build((), AtOnce);
/// let hearing = Rc::clone(&heard);
/// wiring.forward(&wiring.intents(), move |said| {
///     hearing.borrow_mut().push(*said);
///     None
/// });
/// let _store = wiring.start();
/// assert_eq!(*heard.borrow(), ["hello"], "built with Store::new, no one would have heard it");
/// ```
#[must_use = "a store that is never started does nothing"]
pub struct Wiring<F: Feature>(Store<F>);

impl<F: Feature> fmt::Debug for Wiring<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Wiring").field(&format_args!("{}", std::any::type_name::<F>())).finish()
    }
}

impl<F: Feature> Wiring<F> {
    /// A sender of this store's intents, as [`Store::intents`] gives. What it sends before the start waits for it.
    pub fn intents(&self) -> Intents<F::Intent> {
        self.0.intents()
    }

    /// [`Store::forward`], heard from the start.
    pub fn forward<I, R>(&self, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static)
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        self.0.forward(to, map);
    }

    /// [`Store::connect`], heard from the start.
    pub fn connect<I, R>(&self, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static) -> Connection
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        self.0.connect(to, map)
    }

    /// [`Store::share`]. The first value goes out after the first render, once [`Feature::start`] has run, so build the state on
    /// the current value for the start to see it.
    pub fn share<T, I>(&self, to: &Intents<I>, project: impl Fn(&F::State) -> T + 'static, map: impl Fn(&T) -> I + 'static)
    where
        T: PartialEq + 'static,
        I: 'static,
    {
        (self.0).0.watch(to, project, map);
    }

    /// [`Store::children`], from the start: the first children are built as the store starts, after its first render and
    /// before the effects [`Feature::start`] asked for.
    pub fn children<K, T>(&self, children: &Children<K, T>, wanted: impl Fn(&F::State) -> Vec<(K, Standing)> + 'static, build: impl FnMut(&K, &Intents<F::Intent>) -> T + 'static)
    where
        K: PartialEq + 'static,
        T: Child,
    {
        (self.0).0.keep(children, wanted, build);
    }

    /// [`Store::attach`]. The view's first render is the store's first, at the start.
    pub fn attach(&self, view: impl View<F>) -> Attached {
        Inner::add_view(&(self.0).0, view).1
    }

    /// [`Store::observe`], from the start, so it sees the state [`Feature::start`] left and the effects it asked for as well.
    pub fn observe(&self, observer: impl Fn(Seen<'_, F>) + 'static) {
        self.0.observe(observer);
    }

    /// [`Store::save_in`], wired before the start.
    pub fn save_in<V: 'static>(&self, saved: &Saved<V>, key: impl Into<String>, encode: impl Fn(&F::State) -> V + 'static) {
        self.0.save_in(saved, key, encode);
    }

    /// Starts the store: runs [`Feature::start`], renders the first state, and performs the effects `start` asked for. Then it
    /// runs, in the order they were sent, the intents sent before the start and those that the start's effects sent.
    pub fn start(self) -> Store<F> {
        let inner = &(self.0).0;
        let mut effects = Effects::detached();
        F::start(&mut inner.state.borrow_mut(), &mut effects);
        inner.effects.borrow_mut().extend(effects.effects);
        // Still marked at work, so what the observer sends waits for the first render.
        inner.observe(Seen::State(&*inner.state.borrow()));
        inner.draining.set(false);
        inner.render_first();
        inner.drain();
        self.0
    }
}

/// Runs what it holds when dropped.
struct OnDrop(Option<Box<dyn FnOnce()>>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            stop();
        }
    }
}

/// Ends a connection made with [`Store::connect`] when dropped.
#[must_use = "dropping it disconnects at once"]
pub struct Connection {
    _stop: OnDrop,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection").finish_non_exhaustive()
    }
}

/// Detaches a view attached with [`Store::attach`] when dropped. It holds the store weakly, so it can outlive it.
#[must_use = "dropping it detaches the view at once"]
pub struct Attached {
    _stop: OnDrop,
}

impl fmt::Debug for Attached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attached").finish_non_exhaustive()
    }
}

impl<F: Feature> Inner<F> {
    /// Adds a listener that sends to `to` while `connected` holds and `to` lives.
    fn listen<I, R>(&self, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static, connected: Rc<Cell<bool>>) -> Weak<Heard<F::Output>>
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        let to = to.clone();
        let added: Listener<F::Output> = Rc::new(move |output| {
            if !connected.get() || !to.is_alive() {
                return false;
            }
            if let Some(intent) = map(output).into() {
                to.send(intent);
            }
            true
        });
        let weak = Rc::downgrade(&added);
        let mut listeners = self.listeners.borrow_mut();
        *listeners = listeners.iter().cloned().chain([added]).collect();
        weak
    }

    fn connect<I, R>(this: &Rc<Self>, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static) -> Connection
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        let connected = Rc::new(Cell::new(true));
        let listener = this.listen(to, map, Rc::clone(&connected));
        let source = Rc::downgrade(this);
        Connection {
            _stop: OnDrop(Some(Box::new(move || {
                connected.set(false);
                if let Some(source) = source.upgrade() {
                    let mut kept = source.listeners.borrow_mut();
                    *kept = kept.iter().filter(|kept| !Weak::ptr_eq(&Rc::downgrade(kept), &listener)).cloned().collect();
                }
            }))),
        }
    }

    /// Adds `view` to those rendered after each batch, without rendering it.
    fn add_view(this: &Rc<Self>, view: impl View<F>) -> (Rc<Viewing<F>>, Attached) {
        let viewing = Rc::new(Viewing {
            view: RefCell::new(Box::new(view) as Box<dyn View<F>>),
            attached: Cell::new(true),
        });
        {
            let mut views = this.views.borrow_mut();
            *views = views.iter().cloned().chain([Rc::clone(&viewing)]).collect();
        }
        let (store, weak) = (Rc::downgrade(this), Rc::downgrade(&viewing));
        let attached = Attached {
            _stop: OnDrop(Some(Box::new(move || {
                if let Some(viewing) = weak.upgrade() {
                    viewing.attached.set(false);
                }
                if let Some(store) = store.upgrade() {
                    store.prune_views();
                }
            }))),
        };
        (viewing, attached)
    }

    /// Renders a view that has just attached. If the store is at work, what the render sends waits for its turn. If not, the
    /// render is a batch of its own: what it sends runs right after it.
    fn render_attached(&self, viewing: &Viewing<F>) {
        let idle = !self.draining.get();
        {
            let _draining = idle.then(|| Draining::start(&self.draining));
            if let Ok(state) = self.state.try_borrow() {
                self.render_view(viewing, &state);
            }
        }
        if idle && !self.queue.borrow().is_empty() {
            self.drain();
        }
        self.prune_views();
    }

    /// Renders one view, unless it has left, or is gone.
    fn render_view(&self, viewing: &Viewing<F>, state: &F::State) {
        if !viewing.attached.get() {
            return;
        }
        let Ok(mut view) = viewing.view.try_borrow_mut() else {
            return;
        };
        if !view.is_gone() {
            view.render(state, &self.intents);
        }
        if view.is_gone() {
            viewing.attached.set(false);
        }
    }

    /// Renders every attached view, after the host's own render.
    fn render_views(&self) {
        let views = Rc::clone(&self.views.borrow());
        if views.is_empty() {
            return;
        }
        {
            let state = self.state.borrow();
            for viewing in views.iter() {
                self.render_view(viewing, &state);
            }
        }
        drop(views);
        self.prune_views();
    }

    /// Lets go of views that have left or are gone.
    fn prune_views(&self) {
        let Ok(mut views) = self.views.try_borrow_mut() else {
            return;
        };
        if views.iter().any(|viewing| !viewing.attached.get()) {
            *views = views.iter().filter(|viewing| viewing.attached.get()).cloned().collect();
        }
    }

    /// Adds a watcher that sends `map(&project(state))` to `to` whenever the projection differs from the last one it sent.
    fn watch<T, I>(&self, to: &Intents<I>, project: impl Fn(&F::State) -> T + 'static, map: impl Fn(&T) -> I + 'static) -> Watcher<F::State>
    where
        T: PartialEq + 'static,
        I: 'static,
    {
        let to = to.clone();
        let last = RefCell::new(None::<T>);
        let added: Watcher<F::State> = Rc::new(move |state| {
            if !to.is_alive() {
                return Watched::Gone;
            }
            let now = project(state);
            if last.borrow().as_ref() == Some(&now) {
                return Watched::Same;
            }
            let intent = map(&now);
            *last.borrow_mut() = Some(now);
            let to = to.clone();
            Watched::Send(Box::new(move || to.send(intent)))
        });
        let mut watchers = self.watchers.borrow_mut();
        *watchers = watchers.iter().cloned().chain([Rc::clone(&added)]).collect();
        added
    }

    /// Runs `watchers` on the state, then sends what they made once the state's borrow is over, since a store they reach can
    /// share back. A watcher whose destination is gone goes.
    fn tell_watchers(&self, watchers: &[Watcher<F::State>]) {
        let mut sends = Vec::new();
        let mut gone = Vec::new();
        {
            let state = self.state.borrow();
            for watcher in watchers {
                match watcher(&state) {
                    Watched::Same => {}
                    Watched::Send(send) => sends.push(send),
                    Watched::Gone => gone.push(Rc::clone(watcher)),
                }
            }
        }
        if !gone.is_empty() {
            let mut kept = self.watchers.borrow_mut();
            *kept = kept.iter().filter(|kept| !gone.iter().any(|gone| Rc::ptr_eq(kept, gone))).cloned().collect();
        }
        for send in sends {
            send();
        }
    }

    /// Adds a family that keeps `children` in step with what `wanted` finds in the state.
    fn keep<K, T>(
        &self,
        children: &Children<K, T>,
        wanted: impl Fn(&F::State) -> Vec<(K, Standing)> + 'static,
        build: impl FnMut(&K, &Intents<F::Intent>) -> T + 'static,
    ) -> Family<F::State>
    where
        K: PartialEq + 'static,
        T: Child,
    {
        let (children, intents) = (children.clone(), self.intents.clone());
        let build = Rc::new(RefCell::new(build));
        let added: Family<F::State> = Rc::new(move |state| {
            let wanted = wanted(state);
            let (children, intents, build) = (children.clone(), intents.clone(), Rc::clone(&build));
            Box::new(move || children.sync(wanted, &mut |key| (build.borrow_mut())(key, &intents)))
        });
        let mut families = self.families.borrow_mut();
        *families = families.iter().cloned().chain([Rc::clone(&added)]).collect();
        added
    }

    /// Keeps every family's children in step with the state. No borrow of the state or the host is held while a child is
    /// built, told or dropped, and what a child sends here waits for the next batch.
    fn keep_children(&self) {
        let families = Rc::clone(&self.families.borrow());
        if families.is_empty() {
            return;
        }
        let keeps: Vec<Box<dyn FnOnce()>> = {
            let state = self.state.borrow();
            families.iter().map(|family| family(&state)).collect()
        };
        for keep in keeps {
            keep();
        }
    }

    fn tell_all_watchers(&self) {
        let watchers = Rc::clone(&self.watchers.borrow());
        self.tell_watchers(&watchers);
    }

    fn accept(&self, intent: F::Intent) {
        self.queue.borrow_mut().push_back(intent);
        if !self.draining.get() {
            self.drain();
        }
    }

    /// Queued at once, so a drain already under way reduces them in one pass too.
    fn accept_all(&self, intents: Vec<F::Intent>) {
        self.queue.borrow_mut().extend(intents);
        if !self.draining.get() {
            self.drain();
        }
    }

    fn render_first(&self) {
        let _draining = Draining::start(&self.draining);
        self.render_and_perform();
        self.tell_all_watchers();
    }

    /// One batch's work after its intents are reduced: the render, the views, the subscriptions, the children, then the effects.
    fn render_and_perform(&self) {
        {
            let mut host = self.host.borrow_mut();
            host.render(&self.state.borrow(), &self.intents);
            self.render_views();
            self.subscribe(&mut **host);
        }
        self.keep_children();
        self.perform_effects(&mut **self.host.borrow_mut());
    }

    /// Starts what the state wants and stops what it no longer wants. The state's borrow ends before the host runs, and what a
    /// subscription sends as it starts waits for the next batch.
    fn subscribe(&self, host: &mut dyn Host<F>) {
        let wanted: Vec<F::Subscription> = F::subscriptions(&self.state.borrow()).into_iter().collect();
        self.subscriptions.borrow_mut().sync_boxed(wanted, |key| host.subscribe(key, &self.intents));
    }

    fn perform_effects(&self, host: &mut dyn Host<F>) {
        loop {
            // The effects' borrow ends before the host runs.
            let next = self.effects.borrow_mut().pop_front();
            let Some(effect) = next else {
                break;
            };
            self.observe(Seen::Effect(&effect));
            host.perform(effect, &self.intents);
        }
    }

    /// A panic leaves what it didn't reach for the next drain: intents still queued, and the effects and outputs of intents already
    /// reduced, whose state changes stand. The effects and outputs of the intent whose reducer panicked go with it.
    fn drain(&self) {
        let _draining = Draining::start(&self.draining);
        loop {
            let mut any = false;
            loop {
                // The queue's borrow ends before the reducer runs.
                let next = self.queue.borrow_mut().pop_front();
                let Some(intent) = next else {
                    break;
                };
                self.observe(Seen::Intent(&intent));
                let mut effects = Effects::detached();
                F::reduce(&mut self.state.borrow_mut(), intent, &mut effects);
                self.effects.borrow_mut().extend(effects.effects);
                self.outputs.borrow_mut().extend(effects.outputs);
                any = true;
                self.observe(Seen::State(&*self.state.borrow()));
            }
            if !any && self.effects.borrow().is_empty() && self.outputs.borrow().is_empty() {
                return;
            }
            self.render_and_perform();
            self.tell_listeners();
            self.tell_all_watchers();
        }
    }

    /// No borrow of the observers is held while one runs, so one can add another, or send an intent, which waits its turn.
    fn observe(&self, seen: Seen<'_, F>) {
        let observers = Rc::clone(&self.observers.borrow());
        for observer in observers.iter() {
            observer(seen);
        }
    }

    /// A listener can run another store, which can forward from this one, so no borrow is held while one runs. An intent it sends
    /// back here waits for the next batch. If a listener panics, the listeners after it miss that one output.
    fn tell_listeners(&self) {
        loop {
            let next = self.outputs.borrow_mut().pop_front();
            let Some(output) = next else {
                break;
            };
            self.observe(Seen::Output(&output));
            let listeners = Rc::clone(&self.listeners.borrow());
            for listener in listeners.iter() {
                if !listener(&output) {
                    let mut kept = self.listeners.borrow_mut();
                    *kept = kept.iter().filter(|kept| !Rc::ptr_eq(kept, listener)).cloned().collect();
                }
            }
            if let Some(keep) = self.keeper.borrow_mut().as_mut() {
                keep(output);
            }
        }
    }
}

/// Marks a store as at work. Resets on a panic too, so a reducer that panics under a test harness doesn't wedge the store.
struct Draining<'a>(&'a Cell<bool>);

impl<'a> Draining<'a> {
    fn start(draining: &'a Cell<bool>) -> Self {
        draining.set(true);
        Draining(draining)
    }
}

impl Drop for Draining<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestStore;

    struct Counter;

    #[derive(Debug, PartialEq)]
    enum Effect {
        Echo(i32),
    }

    enum Intent {
        Add(i32),
        AddLater(i32),
    }

    impl Feature for Counter {
        type State = i32;
        type Intent = Intent;
        type Effect = Effect;
        type Output = Never;
        type Subscription = Never;

        fn reduce(state: &mut i32, intent: Intent, effects: &mut Effects<Effect>) {
            match intent {
                Intent::Add(by) => *state += by,
                Intent::AddLater(by) => effects.push(Effect::Echo(by)),
            }
        }
    }

    #[derive(Default)]
    struct Log {
        renders: Vec<i32>,
        /// Sent from inside the next render, as a call into the view that comes back at once would.
        send_from_render: Option<i32>,
    }

    struct Recording(Rc<RefCell<Log>>);

    impl Host<Counter> for Recording {
        fn render(&mut self, state: &i32, intents: &Intents<Intent>) {
            let again = {
                let mut log = self.0.borrow_mut();
                log.renders.push(*state);
                log.send_from_render.take()
            };
            if let Some(by) = again {
                intents.send(Intent::Add(by));
            }
        }

        fn perform(&mut self, effect: Effect, intents: &Intents<Intent>) {
            let Effect::Echo(by) = effect;
            intents.send(Intent::Add(by));
        }
    }

    fn store() -> (Store<Counter>, Rc<RefCell<Log>>) {
        let log = Rc::new(RefCell::new(Log::default()));
        (Store::new(0, Recording(log.clone())), log)
    }

    #[test]
    fn it_renders_the_first_state_and_after_each_intent() {
        let (store, log) = store();
        store.send(Intent::Add(2));
        store.send(Intent::Add(3));
        assert_eq!(log.borrow().renders, [0, 2, 5]);
    }

    #[test]
    fn an_intent_sent_during_a_render_runs_next_without_a_second_borrow() {
        let (store, log) = store();
        log.borrow_mut().send_from_render = Some(10);
        store.send(Intent::Add(1));
        assert_eq!(log.borrow().renders, [0, 1, 11]);
    }

    #[test]
    fn an_effect_that_finishes_at_once_comes_back_as_an_intent() {
        let (store, log) = store();
        store.send(Intent::AddLater(7));
        assert_eq!(log.borrow().renders, [0, 0, 7]);
    }

    #[test]
    fn intents_sent_as_one_batch_render_once_after_all_of_them() {
        let (store, log) = store();
        store.send_all([Intent::Add(1), Intent::Add(2)]);
        store.intents().send_all([Intent::Add(3), Intent::Add(4)]);
        store.intents().map(Intent::Add).send_all([5, 6]);
        assert_eq!(log.borrow().renders, [0, 3, 10, 21], "no render shows a batch half done");
    }

    #[test]
    fn a_batchs_effects_follow_its_one_render_and_what_they_send_is_the_next_batch() {
        let (store, log) = store();
        let seen = observed(&store);
        store.send_all([Intent::AddLater(1), Intent::Add(10), Intent::AddLater(2)]);
        assert_eq!(
            *seen.borrow(),
            [
                "add later 1",
                "= 0",
                "add 10",
                "= 10",
                "add later 2",
                "= 10",
                "echo 1",
                "echo 2",
                "add 1",
                "= 11",
                "add 2",
                "= 13"
            ]
        );
        assert_eq!(log.borrow().renders, [0, 10, 13]);
    }

    #[test]
    fn a_weak_store_reaches_the_store_without_keeping_it() {
        let (store, log) = store();
        let weak = store.downgrade();
        weak.upgrade().expect("the store is there").send(Intent::Add(1));
        assert_eq!(log.borrow().renders, [0, 1]);
        drop(store);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn intents_do_nothing_once_the_store_is_gone() {
        let (store, log) = store();
        let intents = store.intents();
        drop(store);
        intents.send(Intent::Add(1));
        assert_eq!(log.borrow().renders, [0]);
    }

    #[test]
    fn a_panicking_reducer_does_not_wedge_the_store() {
        struct Fragile;
        impl Feature for Fragile {
            type State = i32;
            type Intent = bool;
            type Effect = ();
            type Output = Never;
            type Subscription = Never;
            fn reduce(state: &mut i32, panic_now: bool, _: &mut Effects<()>) {
                assert!(!panic_now);
                *state += 1;
            }
        }
        let store = TestStore::<Fragile, _>::rendering(0, |state| *state);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.send(true)));
        assert!(panicked.is_err());
        store.send(false);
        assert_eq!(store.take_renders().last(), Some(&1));
    }

    #[test]
    fn an_intent_sent_from_the_first_render_runs_right_after_it() {
        let log = Rc::new(RefCell::new(Log {
            send_from_render: Some(4),
            ..Log::default()
        }));
        let _store = Store::new(0, Recording(log.clone()));
        assert_eq!(log.borrow().renders, [0, 4]);
    }

    /// Asks the store from inside an effect, as a view callback reached from `perform` would.
    struct Asking {
        store: Rc<RefCell<Option<Store<Counter>>>>,
        heard: Rc<Cell<Option<Option<i32>>>>,
    }

    impl Host<Counter> for Asking {
        fn render(&mut self, _: &i32, _: &Intents<Intent>) {}

        fn perform(&mut self, _: Effect, _: &Intents<Intent>) {
            let store = self.store.borrow().clone().unwrap();
            self.heard.set(Some(store.ask(Intent::Add(10), |state| *state)));
        }
    }

    #[test]
    fn asking_answers_with_the_state_the_intent_left() {
        let (store, _log) = store();
        assert_eq!(store.ask(Intent::Add(2), |state| *state), Some(2));
    }

    #[test]
    fn asking_while_the_store_is_at_work_answers_nothing_and_the_intent_still_runs() {
        let slot: Rc<RefCell<Option<Store<Counter>>>> = Rc::default();
        let heard: Rc<Cell<Option<Option<i32>>>> = Rc::default();
        let store = Store::new(
            1,
            Asking {
                store: Rc::clone(&slot),
                heard: Rc::clone(&heard),
            },
        );
        *slot.borrow_mut() = Some(store.clone());
        store.send(Intent::AddLater(0));
        assert_eq!(heard.get(), Some(None), "the old state is no answer to this intent");
        assert_eq!(store.try_read(|state| *state), Some(11));
        *slot.borrow_mut() = None;
    }

    #[test]
    fn effects_before_a_panic_in_the_batch_still_go_out() {
        struct Fragile;
        impl Feature for Fragile {
            type State = i32;
            type Intent = Option<i32>;
            type Effect = i32;
            type Output = Never;
            type Subscription = Never;
            fn reduce(state: &mut i32, intent: Option<i32>, effects: &mut Effects<i32>) {
                effects.push(-1);
                let by = intent.expect("this intent panics");
                *state += by;
                effects.push(by);
            }
        }
        #[derive(Default)]
        struct Seen {
            performed: Vec<i32>,
            batch: bool,
        }
        struct Batching(Rc<RefCell<Seen>>);
        impl Host<Fragile> for Batching {
            fn render(&mut self, _: &i32, intents: &Intents<Option<i32>>) {
                if std::mem::take(&mut self.0.borrow_mut().batch) {
                    intents.send(Some(5));
                    intents.send(None);
                }
            }
            fn perform(&mut self, effect: i32, _: &Intents<Option<i32>>) {
                self.0.borrow_mut().performed.push(effect);
            }
        }
        let seen = Rc::new(RefCell::new(Seen::default()));
        let store = Store::new(0, Batching(Rc::clone(&seen)));
        seen.borrow_mut().batch = true;
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.send(Some(0))));
        assert!(panicked.is_err());
        seen.borrow_mut().performed.clear();
        store.send(Some(1));
        assert_eq!(
            seen.borrow().performed,
            [-1, 5, -1, 1],
            "the intent before the panic keeps its effects, and the one that panicked loses its own"
        );
    }

    /// A scope is a state inside a state: the parent forwards the intent and maps the child's effects.
    #[test]
    fn a_child_scope_nests_by_forwarding_and_mapping() {
        struct Parent;
        #[derive(Debug, PartialEq)]
        enum ParentEffect {
            Child(Effect),
        }
        enum ParentIntent {
            Child(Intent),
            Close,
        }
        impl Feature for Parent {
            type State = Option<i32>;
            type Intent = ParentIntent;
            type Effect = ParentEffect;
            type Output = Never;
            type Subscription = Never;
            fn reduce(state: &mut Option<i32>, intent: ParentIntent, effects: &mut Effects<ParentEffect>) {
                match (intent, state.as_mut()) {
                    (ParentIntent::Child(intent), Some(child)) => effects.scoped(ParentEffect::Child, |effects| Counter::reduce(child, intent, effects)),
                    (ParentIntent::Child(_), None) => {}
                    (ParentIntent::Close, _) => *state = None,
                }
            }
        }
        let mut state = Some(1);
        assert_eq!(
            step::<Parent>(&mut state, ParentIntent::Child(Intent::AddLater(2))).effects,
            [ParentEffect::Child(Effect::Echo(2))]
        );
        assert!(step::<Parent>(&mut state, ParentIntent::Child(Intent::Add(2))).effects.is_empty());
        assert_eq!(state, Some(3));
        assert!(step::<Parent>(&mut state, ParentIntent::Close).effects.is_empty());
        assert!(
            step::<Parent>(&mut state, ParentIntent::Child(Intent::AddLater(2))).effects.is_empty(),
            "a closed scope takes no intents"
        );
    }

    /// Emits what it is told to. Its host logs each render and effect, so a test can read the order across stores.
    struct Talker;

    #[derive(Debug, PartialEq)]
    struct Said(&'static str);

    enum TalkerIntent {
        Say(&'static str),
        SayAndWork(&'static str),
        Heard,
        Explode(&'static str),
    }

    impl Feature for Talker {
        type State = ();
        type Intent = TalkerIntent;
        type Effect = &'static str;
        type Output = Said;
        type Subscription = Never;

        fn reduce(_: &mut (), intent: TalkerIntent, effects: &mut Effects<&'static str, Said>) {
            match intent {
                TalkerIntent::Say(words) => effects.emit(Said(words)),
                TalkerIntent::SayAndWork(words) => {
                    effects.emit(Said(words));
                    effects.push(words);
                }
                TalkerIntent::Heard => {}
                TalkerIntent::Explode(words) => {
                    effects.emit(Said(words));
                    panic!("a fault in the reducer, expected by this test");
                }
            }
        }
    }

    type Events = Rc<RefCell<Vec<String>>>;

    struct TalkerHost(&'static str, Events);

    impl Host<Talker> for TalkerHost {
        fn render(&mut self, _: &(), _: &Intents<TalkerIntent>) {
            self.1.borrow_mut().push(format!("{} render", self.0));
        }

        fn perform(&mut self, effect: &'static str, _: &Intents<TalkerIntent>) {
            self.1.borrow_mut().push(format!("{} perform {effect}", self.0));
        }
    }

    fn talker(name: &'static str, events: &Events) -> Store<Talker> {
        Store::new((), TalkerHost(name, Rc::clone(events)))
    }

    #[test]
    fn step_returns_what_the_reducer_emitted() {
        let step = step::<Talker>(&mut (), TalkerIntent::SayAndWork("hello"));
        assert_eq!(step.effects, ["hello"]);
        assert_eq!(step.outputs, [Said("hello")]);
    }

    #[test]
    fn an_output_goes_out_after_the_render_and_the_effects_of_its_batch() {
        let events = Events::default();
        let (from, to) = (talker("from", &events), talker("to", &events));
        from.forward(&to.intents(), |_| TalkerIntent::Heard);
        events.borrow_mut().clear();
        from.send(TalkerIntent::SayAndWork("hello"));
        assert_eq!(*events.borrow(), ["from render", "from perform hello", "to render"]);
    }

    #[test]
    fn each_listener_hears_each_output_and_map_can_ignore_one() {
        let events = Events::default();
        let (from, first, second) = (talker("from", &events), talker("first", &events), talker("second", &events));
        from.forward(&first.intents(), |_| TalkerIntent::Heard);
        from.forward(&second.intents(), |Said(words)| (*words == "for all").then_some(TalkerIntent::Heard));
        events.borrow_mut().clear();
        from.send(TalkerIntent::Say("for one"));
        from.send(TalkerIntent::Say("for all"));
        assert_eq!(*events.borrow(), ["from render", "first render", "from render", "first render", "second render"]);
    }

    #[test]
    fn stores_can_forward_to_each_other_without_a_second_borrow() {
        let events = Events::default();
        let (a, b) = (talker("a", &events), talker("b", &events));
        a.forward(&b.intents(), |Said(words)| TalkerIntent::Say(words));
        // `b` answers only this word, or the two would go back and forth forever.
        b.forward(&a.intents(), |Said(words)| (*words == "ping").then_some(TalkerIntent::Heard));
        events.borrow_mut().clear();
        a.send(TalkerIntent::Say("ping"));
        assert_eq!(*events.borrow(), ["a render", "b render", "a render"], "the answer waits for a's next batch");
    }

    #[test]
    fn a_listener_can_forward_from_the_store_that_is_telling_it() {
        let events = Events::default();
        let (from, to) = (talker("from", &events), talker("to", &events));
        let added = Rc::new(Cell::new(false));
        let (adding, source, target) = (Rc::clone(&added), from.clone(), to.intents());
        from.forward(&to.intents(), move |_| {
            if !adding.replace(true) {
                source.forward(&target, |_| TalkerIntent::Heard);
            }
            None
        });
        from.send(TalkerIntent::Say("first"));
        assert!(added.get());
        events.borrow_mut().clear();
        from.send(TalkerIntent::Say("second"));
        assert_eq!(*events.borrow(), ["from render", "to render"], "the listener it added hears the next output");
        // The first listener holds a clone of `from`: let go of it, or the two keep each other alive.
        *from.0.listeners.borrow_mut() = Rc::from(Vec::new());
    }

    #[test]
    fn a_forward_ends_with_the_store_it_sends_to() {
        let events = Events::default();
        let (from, to) = (talker("from", &events), talker("to", &events));
        let held = Rc::new(());
        let holding = Rc::clone(&held);
        from.forward(&to.intents(), move |_| {
            let _ = &holding;
            TalkerIntent::Heard
        });
        drop(to);
        events.borrow_mut().clear();
        from.send(TalkerIntent::Say("hello"));
        assert_eq!(*events.borrow(), ["from render"], "nothing reaches a store that is gone");
        assert_eq!(Rc::strong_count(&held), 1, "and the forward lets go of what it holds");
    }

    #[test]
    fn outputs_of_a_panicking_intent_go_with_it() {
        let events = Events::default();
        let (from, to) = (talker("from", &events), talker("to", &events));
        from.forward(&to.intents(), |_| TalkerIntent::Heard);
        events.borrow_mut().clear();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| from.send(TalkerIntent::Explode("lost"))));
        assert!(panicked.is_err());
        assert!(events.borrow().is_empty());
        from.send(TalkerIntent::Say("after"));
        assert_eq!(*events.borrow(), ["from render", "to render"], "only the later output goes out, and the store goes on");
    }

    /// Asks for two effects as it starts: one a host finishes at once, and one it only logs.
    struct Starter;

    impl Feature for Starter {
        type State = i32;
        type Intent = i32;
        type Effect = i32;
        type Output = Never;
        type Subscription = Never;

        fn reduce(state: &mut i32, by: i32, _: &mut Effects<i32>) {
            *state += by;
        }

        fn start(state: &mut i32, effects: &mut Effects<i32>) {
            *state = 10;
            effects.push(1);
            effects.push(2);
        }
    }

    struct StarterHost(Events);

    impl Host<Starter> for StarterHost {
        fn render(&mut self, state: &i32, _: &Intents<i32>) {
            self.0.borrow_mut().push(format!("render {state}"));
        }

        fn perform(&mut self, effect: i32, intents: &Intents<i32>) {
            self.0.borrow_mut().push(format!("perform {effect}"));
            if effect == 1 {
                intents.send(5);
            }
        }
    }

    #[test]
    fn start_changes_the_first_state_and_its_effects_follow_one_render() {
        let events = Events::default();
        let _store = Store::new(0, StarterHost(Rc::clone(&events)));
        assert_eq!(*events.borrow(), ["render 10", "perform 1", "perform 2", "render 15"]);
    }

    #[test]
    fn start_is_tested_with_no_host() {
        let mut state = 0;
        assert_eq!(start::<Starter>(&mut state), [1, 2]);
        assert_eq!(state, 10);
        assert!(start::<Counter>(&mut state).is_empty(), "a feature that doesn't say otherwise asks for nothing");
    }

    /// A parent whose store runs a child's feature, and whose host runs the child's host.
    struct Nesting;

    enum NestingIntent {
        Child(Intent),
    }

    impl Feature for Nesting {
        type State = i32;
        type Intent = NestingIntent;
        type Effect = Effect;
        type Output = Never;
        type Subscription = Never;

        fn reduce(state: &mut i32, NestingIntent::Child(intent): NestingIntent, effects: &mut Effects<Effect>) {
            Counter::reduce(state, intent, effects);
        }
    }

    struct NestingHost(Recording);

    impl Host<Nesting> for NestingHost {
        fn render(&mut self, state: &i32, intents: &Intents<NestingIntent>) {
            self.0.render(state, &intents.map(NestingIntent::Child));
        }

        fn perform(&mut self, effect: Effect, intents: &Intents<NestingIntent>) {
            self.0.perform(effect, &intents.map(NestingIntent::Child));
        }
    }

    #[test]
    fn a_child_host_runs_inside_its_parents_store_through_a_mapped_sender() {
        let log = Rc::new(RefCell::new(Log::default()));
        let store = Store::new(0, NestingHost(Recording(log.clone())));
        log.borrow_mut().send_from_render = Some(1);
        store.send(NestingIntent::Child(Intent::AddLater(2)));
        assert_eq!(
            log.borrow().renders,
            [0, 0, 3],
            "what the child sends from render and perform reaches the parent's store, as one batch"
        );
    }

    #[test]
    fn a_mapped_sender_ends_with_its_store_and_a_map_can_be_mapped() {
        let (store, log) = store();
        let child: Intents<i32> = store.intents().map(Intent::Add);
        let grandchild: Intents<&str> = child.map(|words: &str| words.len() as i32);
        grandchild.send("four");
        assert_eq!(log.borrow().renders, [0, 4]);
        drop(store);
        grandchild.send("gone");
        child.send(1);
        assert_eq!(log.borrow().renders, [0, 4], "dropping the store ends every mapped sender");
    }

    /// What an observer of a counter saw, by name, with each state as `= count`. The counter's intents aren't `Debug`.
    fn observed(store: &Store<Counter>) -> Rc<RefCell<Vec<String>>> {
        let seen = Rc::<RefCell<Vec<String>>>::default();
        let keeping = Rc::clone(&seen);
        store.observe(move |seen| {
            keeping.borrow_mut().push(match seen {
                Seen::Intent(Intent::Add(by)) => format!("add {by}"),
                Seen::Intent(Intent::AddLater(by)) => format!("add later {by}"),
                Seen::State(count) => format!("= {count}"),
                Seen::Effect(Effect::Echo(by)) => format!("echo {by}"),
                Seen::Output(never) => match *never {},
            });
        });
        seen
    }

    #[test]
    fn an_observer_sees_intents_states_and_effects_in_the_order_they_run() {
        let (store, log) = store();
        let seen = observed(&store);
        log.borrow_mut().send_from_render = Some(1);
        store.send(Intent::AddLater(2));
        assert_eq!(*seen.borrow(), ["add later 2", "= 0", "echo 2", "add 1", "= 1", "add 2", "= 3"]);
        assert_eq!(log.borrow().renders, [0, 0, 3]);
    }

    #[test]
    fn an_observer_sees_the_state_after_each_intent_of_a_batch() {
        let (store, log) = store();
        let seen = observed(&store);
        store.send_all([Intent::Add(1), Intent::Add(2)]);
        assert_eq!(*seen.borrow(), ["add 1", "= 1", "add 2", "= 3"], "so each change is laid to the intent that made it");
        assert_eq!(log.borrow().renders, [0, 3], "while the render shows only the last");
    }

    #[test]
    fn an_observer_sees_each_output_before_it_is_heard() {
        struct Teller;
        impl Feature for Teller {
            type State = ();
            type Intent = &'static str;
            type Effect = Never;
            type Output = &'static str;
            type Subscription = Never;
            fn reduce(_: &mut (), intent: &'static str, effects: &mut Effects<Never, &'static str>) {
                effects.emit(intent);
            }
        }
        struct Quiet;
        impl Host<Teller> for Quiet {
            fn perform(&mut self, effect: Never, _: &Intents<&'static str>) {
                match effect {}
            }
        }
        let store = Store::<Teller>::new((), Quiet);
        let order = Rc::<RefCell<Vec<String>>>::default();
        let (seeing, hearing) = (Rc::clone(&order), Rc::clone(&order));
        store.observe(move |seen| {
            if let Seen::Output(said) = seen {
                seeing.borrow_mut().push(format!("seen {said}"));
            }
        });
        store.forward(&store.intents(), move |said| {
            hearing.borrow_mut().push(format!("heard {said}"));
            None::<&'static str>
        });
        store.send("hello");
        assert_eq!(*order.borrow(), ["seen hello", "heard hello"]);
    }

    #[test]
    fn the_intent_seen_with_no_state_after_it_is_the_one_whose_reducer_panicked() {
        struct Fragile;
        impl Feature for Fragile {
            type State = ();
            type Intent = &'static str;
            type Effect = ();
            type Output = Never;
            type Subscription = Never;
            fn reduce(_: &mut (), intent: &'static str, _: &mut Effects<()>) {
                assert_ne!(intent, "bad", "a fault in the reducer, expected by this test");
            }
        }
        let store = TestStore::<Fragile>::new(());
        let trail = Rc::<RefCell<Vec<&'static str>>>::default();
        let keeping = Rc::clone(&trail);
        store.store().observe(move |seen| match seen {
            Seen::Intent(intent) => keeping.borrow_mut().push(intent),
            Seen::State(()) => keeping.borrow_mut().push("reduced"),
            _ => {}
        });
        store.send("good");
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.send("bad")));
        assert!(panicked.is_err());
        assert_eq!(trail.borrow().last(), Some(&"bad"));
        store.send("after");
        assert_eq!(*trail.borrow(), ["good", "reduced", "bad", "after", "reduced"], "and the store goes on being observed");
    }

    #[test]
    fn an_observer_added_from_inside_another_sees_what_runs_next() {
        let (store, _log) = store();
        let first = observed(&store);
        let second = Rc::<RefCell<Vec<String>>>::default();
        let (source, filling, added) = (store.clone(), Rc::clone(&second), Cell::new(false));
        store.observe(move |_| {
            if !added.replace(true) {
                let filling = Rc::clone(&filling);
                source.observe(move |seen| {
                    if let Seen::Intent(Intent::Add(by)) = seen {
                        filling.borrow_mut().push(format!("add {by}"));
                    }
                });
            }
        });
        store.send(Intent::Add(1));
        store.send(Intent::Add(2));
        assert_eq!(*first.borrow(), ["add 1", "= 1", "add 2", "= 3"]);
        assert_eq!(*second.borrow(), ["add 2"]);
        // The adding observer holds a clone of `store`: let go of it, or the two keep each other alive.
        *store.0.observers.borrow_mut() = Rc::from(Vec::new());
    }

    /// Wants ticks and saves watched as its state says. It sets the state it is sent, and asks for one effect each time.
    struct Watcher;

    #[derive(Debug, PartialEq)]
    enum Watch {
        Ticks,
        Saves,
    }

    impl Feature for Watcher {
        type State = (bool, bool);
        type Intent = (bool, bool);
        type Effect = &'static str;
        type Output = Never;
        type Subscription = Watch;

        fn reduce(state: &mut (bool, bool), wanted: (bool, bool), effects: &mut Effects<&'static str>) {
            *state = wanted;
            effects.push("after");
        }

        fn subscriptions(&(ticks, saves): &(bool, bool)) -> impl IntoIterator<Item = Watch> {
            [ticks.then_some(Watch::Ticks), saves.then_some(Watch::Saves)].into_iter().flatten()
        }
    }

    /// Logs when what it started stops.
    struct Stops(Events, &'static str);

    impl Drop for Stops {
        fn drop(&mut self) {
            self.0.borrow_mut().push(format!("stop {}", self.1));
        }
    }

    /// Shows nothing, as a host whose window is gone, and logs what runs.
    struct Windowless(Events);

    impl Host<Watcher> for Windowless {
        fn render(&mut self, _: &(bool, bool), _: &Intents<(bool, bool)>) {
            self.0.borrow_mut().push("render".to_owned());
        }

        fn perform(&mut self, effect: &'static str, _: &Intents<(bool, bool)>) {
            self.0.borrow_mut().push(format!("perform {effect}"));
        }

        fn subscribe(&mut self, key: &Watch, _: &Intents<(bool, bool)>) -> Box<dyn Any> {
            let name = match key {
                Watch::Ticks => "ticks",
                Watch::Saves => "saves",
            };
            self.0.borrow_mut().push(format!("start {name}"));
            Box::new(Stops(Rc::clone(&self.0), name))
        }
    }

    #[test]
    fn what_runs_follows_the_state_alone_whatever_the_host_renders() {
        let events = Events::default();
        let take = || std::mem::take(&mut *events.borrow_mut());
        let store = Store::new((true, false), Windowless(Rc::clone(&events)));
        assert_eq!(take(), ["render", "start ticks"]);
        store.send((true, true));
        assert_eq!(take(), ["render", "start saves", "perform after"], "after the render, before the effects");
        store.send((false, true));
        assert_eq!(take(), ["render", "stop ticks", "perform after"]);
        store.send((false, true));
        assert_eq!(take(), ["render", "perform after"], "a key still wanted keeps running");
        drop(store);
        assert_eq!(take(), ["stop saves"], "and what runs stops with the store");
    }

    #[test]
    fn what_a_state_wants_is_tested_with_no_host() {
        assert_eq!(subscriptions::<Watcher>(&(true, true)), [Watch::Ticks, Watch::Saves]);
        assert_eq!(subscriptions::<Watcher>(&(false, true)), [Watch::Saves]);
        assert!(subscriptions::<Counter>(&0).is_empty(), "a feature that doesn't say otherwise wants nothing");
    }

    #[test]
    #[should_panic(expected = "wants a subscription, and its host doesn't write `Host::subscribe`")]
    fn a_host_that_starts_nothing_for_a_feature_that_wants_something_panics() {
        struct Forgetful;
        impl Host<Watcher> for Forgetful {
            fn render(&mut self, _: &(bool, bool), _: &Intents<(bool, bool)>) {}
            fn perform(&mut self, _: &'static str, _: &Intents<(bool, bool)>) {}
        }
        let _store = Store::new((true, false), Forgetful);
    }

    #[test]
    fn a_forward_through_a_mapped_sender_ends_with_the_store_it_reaches() {
        let events = Events::default();
        let from = talker("from", &events);
        let (to, log) = store();
        let held = Rc::new(());
        let holding = Rc::clone(&held);
        from.forward(&to.intents().map(Intent::Add), move |Said(words)| {
            let _ = &holding;
            words.len() as i32
        });
        from.send(TalkerIntent::Say("four"));
        assert_eq!(log.borrow().renders, [0, 4]);
        drop(to);
        from.send(TalkerIntent::Say("gone"));
        assert_eq!(Rc::strong_count(&held), 1, "the forward ended with the store behind the mapped sender");
    }
}
