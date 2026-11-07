use crate::{Feature, Host, Intents, Store};
use std::any::{Any, type_name};
use std::cell::RefCell;
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
/// - **Subscriptions** start nothing. [`is_running`](TestStore::is_running) says whether the store has started a key.
/// - **Outputs** go only where the test forwards them through [`store`](TestStore::store), as in an app: to another
///   `TestStore`, say. What a reducer emits is tested with [`step`](crate::step).
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
    kept: Rc<RefCell<Kept<F::Effect, R>>>,
}

struct Kept<E, R> {
    effects: Vec<E>,
    renders: Vec<R>,
}

/// What a render keeps of the state.
type Show<S, R> = Box<dyn FnMut(&S) -> R>;

/// The host under a [`TestStore`].
struct Keeping<F: Feature, R> {
    kept: Rc<RefCell<Kept<F::Effect, R>>>,
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

    fn subscribe(&mut self, _: &F::Subscription, _: &Intents<F::Intent>) -> Box<dyn Any> {
        Box::new(())
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
            renders: Vec::new(),
        }));
        let host = Keeping::<F, R> {
            kept: Rc::clone(&kept),
            show: Box::new(show),
        };
        TestStore {
            store: Store::new(state, host),
            kept,
        }
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

    /// What `show` made of each state rendered since the renders were last taken, in order.
    pub fn take_renders(&self) -> Vec<R> {
        std::mem::take(&mut self.kept.borrow_mut().renders)
    }

    /// Whether the store has started `key` and not stopped it. The host starts nothing, so this is what
    /// [`Feature::subscriptions`] wanted after the last batch.
    pub fn is_running(&self, key: &F::Subscription) -> bool {
        self.store.is_running(key)
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
    fn a_subscription_runs_while_the_state_wants_it() {
        let store = TestStore::<Saver>::new(State::default());
        assert!(!store.is_running(&()));
        store.send(Intent::Watch(true));
        assert!(store.is_running(&()));
        store.send(Intent::Watch(false));
        assert!(!store.is_running(&()));
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
