use crate::{Feature, Host, Intents, Store};
use std::any::{Any, type_name};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

/// How many rounds [`TestStore::answer`] goes before it gives up on effects that keep asking again.
const ROUNDS: usize = 100;

/// A real [`Store`] for tests, on a host that only keeps what the store asks of it. It runs the whole loop, batches and all, where
/// [`step`](crate::step) runs one reducer step.
///
/// - **Effects** wait for the test, which takes them with [`take_effects`](TestStore::take_effects) or answers them with
///   [`answer`](TestStore::answer). One still waiting when the store is dropped fails the test, so no effect goes unchecked:
///   those [`Feature::start`] asked for too.
/// - **Renders** are kept as what `show` makes of each state, the first render included, for
///   [`take_renders`](TestStore::take_renders). [`TestStore::new`] keeps `()`, which counts them.
/// - **Subscriptions** start nothing. [`is_running`](TestStore::is_running) says whether the store has started a key, and
///   [`subscribed`](TestStore::subscribed) hands the test that start, to answer through as its work would. What it sends once
///   the store has stopped it goes nowhere, and a key stopped and started again is a start of its own.
/// - **Outputs** are kept for [`take_outputs`](TestStore::take_outputs), once whatever the test forwards them to through
///   [`store`](TestStore::store) has heard them. Unlike an effect, one left untaken doesn't fail the test.
///
/// An effect's answer is the intent its work would send. Sent from the test, it runs as the next batch, as an answer from the
/// pool would. No bound is put on the feature's types, so an effect is checked by matching on it.
///
/// ```
/// use spiderland::{Effects, Feature, Never, TestStore};
///
/// struct Counter;
/// enum Intent {
///     Add(i32),
///     Save,
///     Saved(i32),
/// }
/// #[derive(Debug, PartialEq)]
/// enum Effect {
///     Write(i32),
/// }
/// #[derive(Default)]
/// struct State {
///     count: i32,
///     saved: Option<i32>,
/// }
///
/// impl Feature for Counter {
///     type State = State;
///     type Intent = Intent;
///     type Effect = Effect;
///     type Output = Never;
///     type Subscription = Never;
///
///     fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
///         match intent {
///             Intent::Add(by) => state.count += by,
///             Intent::Save => effects.push(Effect::Write(state.count)),
///             Intent::Saved(count) => state.saved = Some(count),
///         }
///     }
/// }
///
/// let store = TestStore::<Counter, _>::rendering(State::default(), |state| (state.count, state.saved));
/// store.send(Intent::Add(2));
/// store.send(Intent::Save);
/// // The write waits for the test, which answers it as the work would.
/// store.answer(|Effect::Write(count)| [Intent::Saved(count)]);
/// assert_eq!(store.take_renders(), [(0, None), (2, None), (2, None), (2, Some(2))]);
/// ```
pub struct TestStore<F: Feature, R = ()> {
    store: Store<F>,
    kept: Rc<RefCell<Kept<F, R>>>,
}

struct Kept<F: Feature, R> {
    effects: Vec<F::Effect>,
    outputs: Vec<F::Output>,
    renders: Vec<R>,
    /// How many subscriptions have started.
    started: u64,
}

/// What a render keeps of the state.
type Show<S, R> = Box<dyn FnMut(&S) -> R>;

/// The host under a [`TestStore`].
struct Keeping<F: Feature, R> {
    kept: Rc<RefCell<Kept<F, R>>>,
    show: Show<F::State, R>,
}

impl<F: Feature, R: 'static> Host<F> for Keeping<F, R> {
    fn render(&mut self, state: &F::State, _: &Intents<F::Intent>) {
        let shown = (self.show)(state);
        self.kept.borrow_mut().renders.push(shown);
    }

    fn perform(&mut self, effect: F::Effect, _: &Intents<F::Intent>) {
        self.kept.borrow_mut().effects.push(effect);
    }

    fn subscribe(&mut self, _: &F::Subscription, intents: &Intents<F::Intent>) -> Box<dyn Any> {
        let mut kept = self.kept.borrow_mut();
        kept.started += 1;
        Box::new(Started(Subscribed {
            start: kept.started,
            intents: intents.clone(),
            running: Rc::new(Cell::new(true)),
        }))
    }
}

/// One start of a subscription under a [`TestStore`], from [`TestStore::subscribed`]. It sends as the work it stands for would,
/// through the sender the host was given, until the store stops it. From then on, what it sends goes nowhere, as from work that
/// was stopped.
///
/// Two are equal when they are the same start. So a test can tell a subscription that went on, such as one whose key stayed
/// through a batch, from one that was stopped and started again.
pub struct Subscribed<I> {
    start: u64,
    intents: Intents<I>,
    running: Rc<Cell<bool>>,
}

impl<I> Subscribed<I> {
    /// Sends `intent` to the store, as the work would, while this start runs. Once the store has stopped it, it does nothing.
    pub fn send(&self, intent: I) {
        if self.running.get() {
            self.intents.send(intent);
        }
    }

    /// Sends `intents` as one batch, as [`Intents::send_all`] does, while this start runs.
    pub fn send_all(&self, intents: impl IntoIterator<Item = I>) {
        if self.running.get() {
            self.intents.send_all(intents);
        }
    }

    /// True until the store stops this start.
    pub fn is_running(&self) -> bool {
        self.running.get()
    }
}

impl<I> Clone for Subscribed<I> {
    fn clone(&self) -> Self {
        Subscribed {
            start: self.start,
            intents: self.intents.clone(),
            running: Rc::clone(&self.running),
        }
    }
}

impl<I> PartialEq for Subscribed<I> {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
    }
}

impl<I> Eq for Subscribed<I> {}

impl<I> fmt::Debug for Subscribed<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscribed").field("start", &self.start).field("running", &self.running.get()).finish()
    }
}

/// The guard the test's host returns. The store drops it to stop the subscription.
struct Started<I>(Subscribed<I>);

impl<I> Drop for Started<I> {
    fn drop(&mut self) {
        self.0.running.set(false);
    }
}

impl<F: Feature> TestStore<F> {
    /// Starts the feature on `state`, as [`Store::new`] does, keeping a `()` for each render.
    pub fn new(state: F::State) -> Self {
        Self::rendering(state, |_| ())
    }
}

impl<F: Feature, R: 'static> TestStore<F, R> {
    /// Starts the feature on `state`, as [`Store::new`] does, keeping `show(state)` for each render. A page's `present` suits
    /// it, so a test reads what each render would have shown.
    pub fn rendering(state: F::State, show: impl FnMut(&F::State) -> R + 'static) -> Self {
        let kept = Rc::new(RefCell::new(Kept {
            effects: Vec::new(),
            outputs: Vec::new(),
            renders: Vec::new(),
            started: 0,
        }));
        let host = Keeping::<F, R> {
            kept: Rc::clone(&kept),
            show: Box::new(show),
        };
        // `start` can't emit, and the host answers nothing as the store is built, so no output goes out before this.
        let store = Store::new(state, host);
        let keeping = Rc::clone(&kept);
        store.keep_outputs(move |output| keeping.borrow_mut().outputs.push(output));
        TestStore { store, kept }
    }

    /// Sends `intent`, as [`Store::send`] does.
    pub fn send(&self, intent: F::Intent) {
        self.store.send(intent);
    }

    /// Sends `intents` as one batch, as [`Store::send_all`] does.
    pub fn send_all(&self, intents: impl IntoIterator<Item = F::Intent>) {
        self.store.send_all(intents);
    }

    /// Reads the state. Panics from inside the store's work, such as from a forwarded output's `map`.
    pub fn read<T>(&self, read: impl FnOnce(&F::State) -> T) -> T {
        self.store.try_read(read).expect("the store is at work: read it between sends")
    }

    /// The effects the store asked for since they were last taken or answered, in order. Taking them is what checks them.
    pub fn take_effects(&self) -> Vec<F::Effect> {
        std::mem::take(&mut self.kept.borrow_mut().effects)
    }

    /// Answers each waiting effect with the intents `answer` returns for it, then each effect those lead to, until none waits.
    /// A round's answers go in as one batch, as a host's do when its work finishes at once. An answer of nothing lets an effect
    /// go.
    ///
    /// Panics if effects still wait after 100 rounds, when each answer asks again.
    pub fn answer<A: IntoIterator<Item = F::Intent>>(&self, mut answer: impl FnMut(F::Effect) -> A) {
        for _ in 0..ROUNDS {
            let effects = self.take_effects();
            if effects.is_empty() {
                return;
            }
            let answers: Vec<F::Intent> = effects.into_iter().flat_map(&mut answer).collect();
            self.send_all(answers);
        }
        if !self.kept.borrow().effects.is_empty() {
            panic!("{} still asks after {ROUNDS} rounds of answers: each answer asks again", type_name::<F>());
        }
    }

    /// What the store told others since the outputs were last taken, in order. Each goes out after its batch's render and
    /// effects, as in an app.
    pub fn take_outputs(&self) -> Vec<F::Output> {
        std::mem::take(&mut self.kept.borrow_mut().outputs)
    }

    /// What `show` made of each state rendered since the renders were last taken, in order.
    pub fn take_renders(&self) -> Vec<R> {
        std::mem::take(&mut self.kept.borrow_mut().renders)
    }

    /// Whether the store has started `key` and not stopped it. The host starts nothing, so this is what
    /// [`Feature::subscriptions`] wanted after the last batch.
    pub fn is_running(&self, key: &F::Subscription) -> bool {
        self.store.is_running(key)
    }

    /// The start of `key` that runs now, if the store has started it, to answer through as its work would.
    pub fn subscribed(&self, key: &F::Subscription) -> Option<Subscribed<F::Intent>> {
        self.store
            .read_guard(key, |guard| guard?.downcast_ref::<Started<F::Intent>>().map(|started| started.0.clone()))
    }

    /// The store itself: to forward its outputs, observe it or ask it, or to hand its [`Intents`] to a view's callbacks.
    pub fn store(&self) -> &Store<F> {
        &self.store
    }
}

/// Fails the test when effects still wait, unless it is already failing.
impl<F: Feature, R> Drop for TestStore<F, R> {
    fn drop(&mut self) {
        let waiting = self.kept.borrow().effects.len();
        if waiting > 0 && !std::thread::panicking() {
            let effects = if waiting == 1 { "effect" } else { "effects" };
            panic!(
                "{waiting} {effects} of {} still waiting as the test ends: take them with `take_effects` to see them, or `answer` them",
                type_name::<F>()
            );
        }
    }
}

impl<F: Feature, R> fmt::Debug for TestStore<F, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kept = self.kept.borrow();
        f.debug_struct("TestStore")
            .field("feature", &type_name::<F>())
            .field("waiting", &kept.effects.len())
            .field("outputs", &kept.outputs.len())
            .field("renders", &kept.renders.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Effects, Never};

    /// Saves its count, and tries again when a write fails.
    struct Saver;

    #[derive(Debug, PartialEq)]
    enum Effect {
        Write(i32),
    }

    enum Intent {
        Add(i32),
        Save,
        Written(Result<(), ()>),
        Watch(bool),
    }

    #[derive(Default)]
    struct State {
        count: i32,
        saved: Option<i32>,
        watching: bool,
    }

    impl Feature for Saver {
        type State = State;
        type Intent = Intent;
        type Effect = Effect;
        type Output = Never;
        type Subscription = ();

        fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
            match intent {
                Intent::Add(by) => state.count += by,
                Intent::Save | Intent::Written(Err(())) => effects.push(Effect::Write(state.count)),
                Intent::Written(Ok(())) => state.saved = Some(state.count),
                Intent::Watch(on) => state.watching = on,
            }
        }

        /// A count it starts with is saved straight away.
        fn start(state: &mut State, effects: &mut Effects<Effect>) {
            if state.count > 0 {
                effects.push(Effect::Write(state.count));
            }
        }

        fn subscriptions(state: &State) -> impl IntoIterator<Item = ()> {
            state.watching.then_some(())
        }
    }

    /// Writes each number it is sent, and tells others of it.
    struct Teller;

    impl Feature for Teller {
        type State = ();
        type Intent = i32;
        type Effect = Effect;
        type Output = i32;
        type Subscription = Never;

        fn reduce(_: &mut (), number: i32, effects: &mut Effects<Effect, i32>) {
            effects.push(Effect::Write(number));
            effects.emit(number);
        }
    }

    #[test]
    fn effects_wait_for_the_test_and_its_answers_run_as_the_next_batch() {
        let store = TestStore::<Saver, _>::rendering(State::default(), |state| (state.count, state.saved));
        store.send(Intent::Add(2));
        store.send(Intent::Save);
        assert_eq!(store.take_effects(), [Effect::Write(2)]);
        assert!(store.take_effects().is_empty(), "taken once");
        store.send_all([Intent::Add(1), Intent::Written(Ok(()))]);
        assert_eq!(store.take_renders(), [(0, None), (2, None), (2, None), (3, Some(3))]);
    }

    #[test]
    fn answers_go_on_until_no_effect_waits() {
        let store = TestStore::<Saver>::new(State::default());
        store.send(Intent::Save);
        let mut failures = 1;
        store.answer(|Effect::Write(_)| {
            let failed = failures > 0;
            failures -= 1;
            [Intent::Written(if failed { Err(()) } else { Ok(()) })]
        });
        assert_eq!(store.read(|state| state.saved), Some(0));
        assert_eq!(store.take_renders().len(), 4, "the first render, the save, the failed write, and the write that took");
    }

    #[test]
    fn a_rounds_answers_are_one_batch() {
        let store = TestStore::<Saver>::new(State::default());
        store.send_all([Intent::Save, Intent::Save]);
        store.take_renders();
        store.answer(|_| [Intent::Written(Ok(()))]);
        assert_eq!(store.take_renders().len(), 1, "two answers, one render, as from a host whose work finishes at once");
    }

    #[test]
    fn what_start_asks_for_and_the_first_render_are_kept_too() {
        let store = TestStore::<Saver>::new(State { count: 3, ..State::default() });
        assert_eq!(store.take_effects(), [Effect::Write(3)]);
        assert_eq!(store.take_renders().len(), 1);
    }

    #[test]
    fn outputs_are_kept_in_order_and_one_left_untaken_fails_nothing() {
        let store = TestStore::<Teller>::new(());
        store.send_all([1, 2]);
        assert_eq!(store.take_outputs(), [1, 2]);
        assert!(store.take_outputs().is_empty(), "taken once");
        store.send(3);
        store.answer(|_| []);
    }

    #[test]
    fn an_output_forwarded_to_another_store_is_kept_too() {
        let teller = TestStore::<Teller>::new(());
        let saver = TestStore::<Saver>::new(State::default());
        teller.store().forward(&saver.store().intents(), |number| Intent::Add(*number));
        teller.send(2);
        assert_eq!(saver.read(|state| state.count), 2);
        assert_eq!(teller.take_outputs(), [2]);
        assert_eq!(teller.take_effects(), [Effect::Write(2)]);
    }

    #[test]
    fn a_subscription_runs_while_the_state_wants_it() {
        let store = TestStore::<Saver>::new(State::default());
        assert!(!store.is_running(&()));
        store.send(Intent::Watch(true));
        assert!(store.is_running(&()));
        store.send(Intent::Watch(false));
        assert!(!store.is_running(&()));
    }

    /// Loads what the query names, while it is on.
    struct Loader;

    #[derive(Debug, PartialEq)]
    struct Load(&'static str);

    enum Find {
        Type(&'static str),
        On(bool),
        Found(&'static str),
    }

    #[derive(Default)]
    struct Loading {
        query: &'static str,
        on: bool,
        found: Vec<&'static str>,
    }

    impl Feature for Loader {
        type State = Loading;
        type Intent = Find;
        type Effect = Never;
        type Output = Never;
        type Subscription = Load;

        fn reduce(state: &mut Loading, intent: Find, _: &mut Effects<Never>) {
            match intent {
                Find::Type(query) => state.query = query,
                Find::On(on) => state.on = on,
                Find::Found(found) => state.found.push(found),
            }
        }

        fn subscriptions(state: &Loading) -> impl IntoIterator<Item = Load> {
            state.on.then_some(Load(state.query))
        }
    }

    #[test]
    fn a_subscription_answers_through_its_start_until_it_is_stopped() {
        let store = TestStore::<Loader>::new(Loading::default());
        assert!(store.subscribed(&Load("")).is_none(), "nothing runs yet");
        store.send_all([Find::Type("a"), Find::On(true)]);
        let first = store.subscribed(&Load("a")).expect("the load runs");
        first.send(Find::Found("a1"));
        store.send(Find::Type("ab"));
        assert!(!first.is_running(), "a new query stopped the old load");
        assert!(store.subscribed(&Load("a")).is_none());
        first.send(Find::Found("late"));
        let second = store.subscribed(&Load("ab")).expect("the new load runs");
        second.send_all([Find::Found("ab1"), Find::Found("ab2")]);
        assert_eq!(store.read(|state| state.found.clone()), ["a1", "ab1", "ab2"], "what the old load sent late went nowhere");
    }

    #[test]
    fn a_key_stopped_and_started_again_is_a_start_of_its_own() {
        let store = TestStore::<Loader>::new(Loading::default());
        store.send(Find::On(true));
        let first = store.subscribed(&Load("")).expect("the load runs");
        store.send_all([Find::On(false), Find::On(true)]);
        assert_eq!(store.subscribed(&Load("")), Some(first.clone()), "a key wanted again in the same batch went on");
        store.send(Find::On(false));
        store.send(Find::On(true));
        let again = store.subscribed(&Load("")).expect("the load runs again");
        assert_ne!(again, first, "stopped in one batch and started in the next, it is a start of its own");
        assert!(!first.is_running() && again.is_running());
    }

    #[test]
    #[should_panic(expected = "1 effect of spiderland::test_store::tests::Saver still waiting as the test ends")]
    fn a_store_dropped_with_an_effect_waiting_fails_the_test() {
        let store = TestStore::<Saver>::new(State::default());
        store.send(Intent::Save);
    }

    #[test]
    #[should_panic(expected = "still asks after 100 rounds of answers")]
    fn answers_that_always_ask_again_fail_rather_than_hang() {
        let store = TestStore::<Saver>::new(State::default());
        store.send(Intent::Save);
        store.answer(|_| [Intent::Written(Err(()))]);
    }
}
