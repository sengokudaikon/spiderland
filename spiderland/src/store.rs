//! The reducer's contract and the store that runs it.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
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

    fn reduce(state: &mut Self::State, intent: Self::Intent, effects: &mut Effects<Self::Effect, Self::Output>);

    /// Runs once as the store is built, before the first render. The effects it asks for are performed after that render. It can't
    /// emit, because nothing can be listening yet. Does nothing unless a feature says otherwise.
    fn start(state: &mut Self::State, effects: &mut Effects<Self::Effect>) {
        let _ = (state, effects);
    }
}

/// An output that can't happen, for a feature that tells no one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Never {}

/// What one `reduce` asks for. After the next render its effects are performed in order, and then its outputs go out in order.
pub struct Effects<E, O = Never> {
    effects: Vec<E>,
    outputs: Vec<O>,
}

impl<E, O> Effects<E, O> {
    /// A list no one performs, for code shared with the reducer that only reads when it runs outside it, as a view does.
    pub fn detached() -> Self {
        Effects {
            effects: Vec::new(),
            outputs: Vec::new(),
        }
    }

    pub fn push(&mut self, effect: E) {
        self.effects.push(effect);
    }

    /// Tells whoever listens through [`Store::forward`].
    pub fn emit(&mut self, output: O) {
        self.outputs.push(output);
    }

    /// Runs a child scope's reducer, which lists its own effect type, and wraps each of them as this feature's. The child emits
    /// nothing: telling the outside is up to the parent.
    pub fn scoped<C, R>(&mut self, wrap: impl Fn(C) -> E, child: impl FnOnce(&mut Effects<C>) -> R) -> R {
        let mut effects = Effects::detached();
        let result = child(&mut effects);
        self.effects.extend(effects.effects.into_iter().map(wrap));
        result
    }
}

/// What one reducer step asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step<E, O = Never> {
    pub effects: Vec<E>,
    pub outputs: Vec<O>,
}

/// What a store is about to run, as [`Store::observe`] shows it.
pub enum Seen<'a, F: Feature> {
    /// An intent, just before it is reduced.
    Intent(&'a F::Intent),
    /// An effect, just before it is performed.
    Effect(&'a F::Effect),
}

impl<F: Feature> Clone for Seen<'_, F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F: Feature> Copy for Seen<'_, F> {}

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

/// Background work panicked. Finished work hands this to its feature, which decides what the failure means there: a
/// fault in one feature's work is not automatically a fault in the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panicked;

/// A feature's impure half: the view, the timers and the services.
pub trait Host<F: Feature>: 'static {
    /// Called once after each batch of intents. Should only do what the changed parts of the state need.
    fn render(&mut self, state: &F::State, intents: &Intents<F::Intent>);

    /// Starts one effect. Work that finishes later sends its result through `intents`.
    fn perform(&mut self, effect: F::Effect, intents: &Intents<F::Intent>);
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

impl<I> Intents<I> {
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
type Listener<O> = Rc<dyn Fn(&O) -> bool>;

type Observer<F> = Rc<dyn Fn(Seen<'_, F>)>;

struct Inner<F: Feature> {
    state: RefCell<F::State>,
    host: RefCell<Box<dyn Host<F>>>,
    queue: RefCell<VecDeque<F::Intent>>,
    /// Effects of reduced intents, waiting for the render that goes before them.
    effects: RefCell<VecDeque<F::Effect>>,
    /// Outputs of reduced intents, waiting for those effects.
    outputs: RefCell<VecDeque<F::Output>>,
    listeners: RefCell<Vec<Listener<F::Output>>>,
    /// Replaced whole when one is added, so telling them costs no copy of the list.
    observers: RefCell<Rc<[Observer<F>]>>,
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

impl<F: Feature> Store<F> {
    /// Starts the store: runs [`Feature::start`], renders the first state and performs the effects `start` asked for. An intent
    /// sent from any of that runs right after it.
    pub fn new(mut state: F::State, host: impl Host<F>) -> Self {
        let mut effects = Effects::detached();
        F::start(&mut state, &mut effects);
        let inner = Rc::new_cyclic(|this: &Weak<Inner<F>>| Inner {
            state: RefCell::new(state),
            host: RefCell::new(Box::new(host) as Box<dyn Host<F>>),
            queue: RefCell::new(VecDeque::new()),
            effects: RefCell::new(effects.effects.into()),
            outputs: RefCell::new(VecDeque::new()),
            listeners: RefCell::new(Vec::new()),
            observers: RefCell::new(Rc::from(Vec::new())),
            draining: Cell::new(false),
            intents: Intents(Rc::new(this.clone())),
        });
        inner.render_first();
        inner.drain();
        Store(inner)
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

    pub fn intents(&self) -> Intents<F::Intent> {
        self.0.intents.clone()
    }

    /// Sends `map(output)` to `to` for each output this store emits, in the order they were forwarded. `map` returns an intent,
    /// or `None` to ignore that output. It keeps neither store alive, and ends with whichever store goes first.
    ///
    /// Forward straight after building the store: an output emitted before there is a listener reaches no one. If `map` needs to
    /// reach a store, it should hold [`Intents`], because a [`Store`] it holds stays alive for as long as this one does.
    pub fn forward<I, R>(&self, to: &Intents<I>, map: impl Fn(&F::Output) -> R + 'static)
    where
        I: 'static,
        R: Into<Option<I>>,
    {
        let to = to.clone();
        self.0.listeners.borrow_mut().push(Rc::new(move |output| {
            if !to.is_alive() {
                return false;
            }
            if let Some(intent) = map(output).into() {
                to.send(intent);
            }
            true
        }));
    }

    /// Calls `observer` with each intent just before it is reduced and each effect just before it is performed, in the order the
    /// store runs them. It suits a trail of what led to a fault: the last intent seen before a reducer panics is the one that
    /// panicked.
    ///
    /// It is handed a reference, so nothing is copied unless it copies, and nothing needs `Debug`. Keep a name, not the intent
    /// itself, because an intent can carry a whole snapshot.
    ///
    /// It sees only what runs after it is added, so it misses what [`Store::new`] ran: the effects `start` asked for, and what
    /// the first render and those effects sent. It lives as long as the store, so it shouldn't hold the store. If it panics, the
    /// intent or effect it was shown is lost, as with a reducer that panics.
    ///
    /// ```
    /// use spiderland::{Effects, Feature, Host, Intents, Never, Seen, Store};
    /// use std::{cell::RefCell, collections::VecDeque, rc::Rc};
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
    /// // The last three things the store ran, by name.
    /// let trail = Rc::new(RefCell::new(VecDeque::new()));
    /// let store = Store::new(0, Quiet);
    /// let keeping = Rc::clone(&trail);
    /// store.observe(move |seen| {
    ///     let name = match seen {
    ///         Seen::Intent(Intent::Add(_)) => "Add",
    ///         Seen::Intent(Intent::Save) => "Save",
    ///         Seen::Effect(Effect::Write(_)) => "Write",
    ///     };
    ///     let mut trail = keeping.borrow_mut();
    ///     if trail.len() == 3 {
    ///         trail.pop_front();
    ///     }
    ///     trail.push_back(name);
    /// });
    /// store.send(Intent::Add(1));
    /// store.send(Intent::Add(2));
    /// store.send(Intent::Save);
    /// assert_eq!(*trail.borrow(), ["Add", "Save", "Write"]);
    /// ```
    pub fn observe(&self, observer: impl Fn(Seen<'_, F>) + 'static) {
        let mut observers = self.0.observers.borrow_mut();
        let added: Observer<F> = Rc::new(observer);
        *observers = observers.iter().cloned().chain([added]).collect();
    }

    /// Runs `change` on the state directly, for a test or a capture to set up or look at a state. Nothing renders until the
    /// next intent. A real feature's changes go through its reducer.
    pub fn with_state<R>(&self, change: impl FnOnce(&mut F::State) -> R) -> R {
        change(&mut self.0.state.borrow_mut())
    }

    /// Answers a view's question from the state, for a pure callback that a binding calls. None while a reducer is running,
    /// which a binding can only meet if a reducer made the toolkit evaluate one. To read what an intent did, use [`Store::ask`].
    pub fn try_read<R>(&self, read: impl FnOnce(&F::State) -> R) -> Option<R> {
        self.0.state.try_borrow().ok().map(|state| read(&state))
    }
}

impl<F: Feature> Inner<F> {
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
        let mut host = self.host.borrow_mut();
        host.render(&self.state.borrow(), &self.intents);
        self.perform_effects(&mut **host);
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
            }
            if !any && self.effects.borrow().is_empty() && self.outputs.borrow().is_empty() {
                return;
            }
            {
                let mut host = self.host.borrow_mut();
                host.render(&self.state.borrow(), &self.intents);
                self.perform_effects(&mut **host);
            }
            self.tell_listeners();
        }
    }

    /// No borrow is held while an observer runs, so one can add another or send an intent, which waits its turn.
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
            let listeners = self.listeners.borrow().clone();
            for listener in &listeners {
                if !listener(&output) {
                    self.listeners.borrow_mut().retain(|kept| !Rc::ptr_eq(kept, listener));
                }
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
        assert_eq!(*seen.borrow(), ["add later 1", "add 10", "add later 2", "echo 1", "echo 2", "add 1", "add 2"]);
        assert_eq!(log.borrow().renders, [0, 10, 13]);
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
            fn reduce(state: &mut i32, panic_now: bool, _: &mut Effects<()>) {
                assert!(!panic_now);
                *state += 1;
            }
        }
        struct Quiet(Rc<Cell<i32>>);
        impl Host<Fragile> for Quiet {
            fn render(&mut self, state: &i32, _: &Intents<bool>) {
                self.0.set(*state);
            }
            fn perform(&mut self, _: (), _: &Intents<bool>) {}
        }
        let shown = Rc::new(Cell::new(-1));
        let store = Store::new(0, Quiet(shown.clone()));
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.send(true)));
        assert!(panicked.is_err());
        store.send(false);
        assert_eq!(shown.get(), 1);
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
        from.0.listeners.borrow_mut().clear();
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

    /// What an observer of a counter saw, by name. The counter's intents aren't `Debug`.
    fn observed(store: &Store<Counter>) -> Rc<RefCell<Vec<String>>> {
        let seen = Rc::<RefCell<Vec<String>>>::default();
        let keeping = Rc::clone(&seen);
        store.observe(move |seen| {
            keeping.borrow_mut().push(match seen {
                Seen::Intent(Intent::Add(by)) => format!("add {by}"),
                Seen::Intent(Intent::AddLater(by)) => format!("add later {by}"),
                Seen::Effect(Effect::Echo(by)) => format!("echo {by}"),
            });
        });
        seen
    }

    #[test]
    fn an_observer_sees_intents_and_effects_in_the_order_they_run() {
        let (store, log) = store();
        let seen = observed(&store);
        log.borrow_mut().send_from_render = Some(1);
        store.send(Intent::AddLater(2));
        assert_eq!(*seen.borrow(), ["add later 2", "echo 2", "add 1", "add 2"]);
        assert_eq!(log.borrow().renders, [0, 0, 3]);
    }

    #[test]
    fn the_last_intent_seen_before_a_reducer_panics_is_the_one_that_panicked() {
        struct Fragile;
        impl Feature for Fragile {
            type State = ();
            type Intent = &'static str;
            type Effect = ();
            type Output = Never;
            fn reduce(_: &mut (), intent: &'static str, _: &mut Effects<()>) {
                assert_ne!(intent, "bad", "a fault in the reducer, expected by this test");
            }
        }
        struct Quiet;
        impl Host<Fragile> for Quiet {
            fn render(&mut self, _: &(), _: &Intents<&'static str>) {}
            fn perform(&mut self, (): (), _: &Intents<&'static str>) {}
        }
        let store = Store::new((), Quiet);
        let trail = Rc::<RefCell<Vec<&'static str>>>::default();
        let keeping = Rc::clone(&trail);
        store.observe(move |seen| {
            if let Seen::Intent(intent) = seen {
                keeping.borrow_mut().push(intent);
            }
        });
        store.send("good");
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.send("bad")));
        assert!(panicked.is_err());
        assert_eq!(trail.borrow().last(), Some(&"bad"));
        store.send("after");
        assert_eq!(*trail.borrow(), ["good", "bad", "after"], "and the store goes on being observed");
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
        assert_eq!(*first.borrow(), ["add 1", "add 2"]);
        assert_eq!(*second.borrow(), ["add 2"]);
        // The adding observer holds a clone of `store`: let go of it, or the two keep each other alive.
        *store.0.observers.borrow_mut() = Rc::from(Vec::new());
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
