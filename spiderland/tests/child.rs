//! A child feature inside its parent's store: its effects wrapped, its outputs handed back, its subscriptions stopped when it goes.

use spiderland::{Effects, Feature, Host, Intents, Store, TestStore};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Counts up to a limit. It asks to save each count, says when it reaches the limit, and ticks while it runs.
struct Count;

#[derive(Debug, PartialEq)]
enum CountEffect {
    Save(u32),
    Hello,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tick;

impl Feature for Count {
    type State = u32;
    type Intent = ();
    type Effect = CountEffect;
    type Output = &'static str;
    type Subscription = Tick;

    fn start(_: &mut u32, effects: &mut Effects<CountEffect>) {
        effects.push(CountEffect::Hello);
    }

    fn reduce(count: &mut u32, (): (), effects: &mut Effects<CountEffect, &'static str>) {
        *count += 1;
        effects.push(CountEffect::Save(*count));
        if *count == 2 {
            effects.emit("reached");
        }
    }

    fn subscriptions(_: &u32) -> impl IntoIterator<Item = Tick> {
        [Tick]
    }
}

/// Keeps one count while it runs, and drops it once the count says it is done.
struct Parent;

#[derive(Debug, Default)]
struct State {
    count: Option<u32>,
    reached: bool,
}

#[derive(Debug)]
enum Intent {
    Start,
    Stop,
    Count(()),
}

#[derive(Debug, PartialEq)]
enum Effect {
    Count(CountEffect),
}

#[derive(Debug, PartialEq)]
enum Running {
    Count(Tick),
}

impl Feature for Parent {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = &'static str;
    type Subscription = Running;

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect, &'static str>) {
        match intent {
            Intent::Start => {
                let count = state.count.insert(0);
                effects.start_child::<Count>(count, Effect::Count);
            }
            Intent::Stop => state.count = None,
            Intent::Count(intent) => {
                let Some(count) = state.count.as_mut() else {
                    return;
                };
                for said in effects.reduce_child::<Count>(count, intent, Effect::Count) {
                    state.reached = true;
                    state.count = None;
                    effects.emit(said);
                }
            }
        }
    }

    fn subscriptions(state: &State) -> impl IntoIterator<Item = Running> {
        state.count.iter().flat_map(Count::subscriptions).map(Running::Count)
    }
}

#[test]
fn a_childs_effects_are_wrapped_and_what_it_says_comes_back_to_the_parent() {
    let parent = TestStore::<Parent>::new(State::default());
    parent.send(Intent::Start);
    assert_eq!(parent.take_effects(), [Effect::Count(CountEffect::Hello)], "the child's start ran as the parent made it");
    parent.send(Intent::Count(()));
    assert_eq!(parent.take_effects(), [Effect::Count(CountEffect::Save(1))]);
    assert!(parent.is_running(&Running::Count(Tick)));

    parent.send(Intent::Count(()));
    assert_eq!(parent.take_effects(), [Effect::Count(CountEffect::Save(2))]);
    assert_eq!(parent.take_outputs(), ["reached"], "the parent heard the child, and told the outside");
    assert!(parent.read(|state| state.reached && state.count.is_none()));
    assert!(!parent.is_running(&Running::Count(Tick)), "the child is gone, so what it ran stopped");

    parent.send(Intent::Count(()));
    assert!(parent.take_effects().is_empty(), "an intent for a child that is gone is let go");
}

/// The child's host: it keeps what its tick would send to, and says when the tick stops.
struct CountHost {
    ticking: Rc<RefCell<Option<Intents<()>>>>,
    stopped: Rc<Cell<bool>>,
}

struct Stopped(Rc<Cell<bool>>);

impl Drop for Stopped {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

impl Host<Count> for CountHost {
    fn perform(&mut self, _: CountEffect, _: &Intents<()>) {}

    fn subscribe(&mut self, Tick: &Tick, intents: &Intents<()>) -> Box<dyn Any> {
        *self.ticking.borrow_mut() = Some(intents.clone());
        Box::new(Stopped(Rc::clone(&self.stopped)))
    }
}

/// The parent's host runs the child's.
struct ParentHost(CountHost);

impl Host<Parent> for ParentHost {
    fn perform(&mut self, Effect::Count(effect): Effect, intents: &Intents<Intent>) {
        self.0.perform(effect, &intents.map(Intent::Count));
    }

    fn subscribe(&mut self, Running::Count(key): &Running, intents: &Intents<Intent>) -> Box<dyn Any> {
        self.0.subscribe(key, &intents.map(Intent::Count))
    }
}

#[test]
fn a_childs_host_runs_inside_its_parents_and_its_work_stops_with_the_child() {
    let (ticking, stopped) = (Rc::default(), Rc::default());
    let host = ParentHost(CountHost {
        ticking: Rc::clone(&ticking),
        stopped: Rc::clone(&stopped),
    });
    let parent = Store::new(State::default(), host);
    parent.send(Intent::Start);
    let tick = ticking.borrow().clone().expect("the child's tick runs");
    tick.send(());
    assert_eq!(parent.try_read(|state| state.count), Some(Some(1)), "what the child's host sent came back wrapped");
    parent.send(Intent::Stop);
    assert!(stopped.get(), "dropping the child stopped its tick");
}
