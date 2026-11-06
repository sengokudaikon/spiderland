//! `Background` and the listeners on a loop polled by hand, with no toolkit: `EventLoop` is all they need from one.

use spiderland::{Background, Effects, EventLoop, Feature, Host, Intents, Never, Panicked, Store, on_wake, subscriptions};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

thread_local! {
    static SPAWNED: RefCell<Vec<Pin<Box<dyn Future<Output = ()>>>>> = RefCell::default();
}

/// Keeps what it is given, and runs it only when [`turn`] polls it.
struct ByHand;

impl EventLoop for ByHand {
    fn spawn_local(future: impl Future<Output = ()> + 'static) {
        SPAWNED.with_borrow_mut(|spawned| spawned.push(Box::pin(future)));
    }
}

/// Polls each future once and keeps the ones not done. Those spawned meanwhile wait for the next turn.
fn turn() {
    let mut context = Context::from_waker(std::task::Waker::noop());
    let polled: Vec<_> = SPAWNED
        .take()
        .into_iter()
        .filter_map(|mut future| future.as_mut().poll(&mut context).is_pending().then_some(future))
        .collect();
    SPAWNED.with_borrow_mut(|spawned| drop(spawned.splice(0..0, polled)));
}

fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        turn();
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct Probe;

enum Intent {
    Finished(Result<i32, Panicked>),
    Woken,
}

#[derive(Clone, Default)]
struct State {
    results: Vec<Result<i32, Panicked>>,
    wakes: i32,
}

impl Feature for Probe {
    type State = State;
    type Intent = Intent;
    type Effect = Never;
    type Output = Never;
    type Subscription = Never;

    fn reduce(state: &mut State, intent: Intent, _: &mut Effects<Never>) {
        match intent {
            Intent::Finished(result) => state.results.push(result),
            Intent::Woken => state.wakes += 1,
        }
    }
}

struct Shown(Rc<RefCell<State>>);

impl Host<Probe> for Shown {
    fn render(&mut self, state: &State, _: &Intents<Intent>) {
        self.0.borrow_mut().clone_from(state);
    }

    fn perform(&mut self, effect: Never, _: &Intents<Intent>) {
        match effect {}
    }
}

#[test]
fn work_and_wakes_come_back_on_any_event_loop() {
    let shown = Rc::default();
    let store = Store::new(State::default(), Shown(Rc::clone(&shown)));
    let intents = store.intents();
    let background = Background::<ByHand>::new(|job| {
        std::thread::spawn(job);
        true
    });

    background.run(&intents, || 7, Intent::Finished);
    assert_eq!(background.pending(), 1, "the work's result has yet to come back");
    until("background work", || background.pending() == 0);
    background.run(&intents, || panic!("a fault in the work, expected by this test"), Intent::Finished);
    until("background work", || background.pending() == 0);
    assert_eq!(shown.borrow().results, [Ok(7), Err(Panicked)]);

    // Work let go of before it finishes never hands over its result.
    let (answer, answered) = oneshot::channel::<i32>();
    let heard = Rc::new(Cell::new(false));
    let hearing = Rc::clone(&heard);
    let dropped = background.local(answered, move |_| hearing.set(true));
    turn();
    drop(dropped);
    until("letting go", || answer.is_closed());
    assert!(!heard.get());
    assert_eq!(background.pending(), 0);

    // A wake from another thread arrives as an intent.
    let (waker, _listening) = on_wake::<ByHand, _>(&intents, || Intent::Woken);
    turn();
    std::thread::spawn(move || waker.wake()).join().unwrap();
    until("the wake", || shown.borrow().wakes == 1);
}

/// A search whose load is a subscription keyed on its query, so a new query drops the load for the old one.
struct Search;

enum SearchIntent {
    Typed(&'static str),
    Found(&'static str, usize),
}

#[derive(Clone, Copy, Default)]
struct Searching {
    query: &'static str,
    /// The query a count is for, and the count.
    found: Option<(&'static str, usize)>,
}

#[derive(Debug, PartialEq)]
struct Load(&'static str);

impl Feature for Search {
    type State = Searching;
    type Intent = SearchIntent;
    type Effect = Never;
    type Output = Never;
    type Subscription = Load;

    fn reduce(state: &mut Searching, intent: SearchIntent, _: &mut Effects<Never>) {
        match intent {
            SearchIntent::Typed(query) => state.query = query,
            // Work on the pool can't be stopped, so its answer is checked against the query. A load dropped with its key can't
            // answer, so this only matters for `Background::run`.
            SearchIntent::Found(query, count) if query == state.query => state.found = Some((query, count)),
            SearchIntent::Found(..) => {}
        }
    }

    fn subscriptions(state: &Searching) -> impl IntoIterator<Item = Load> {
        let answered = state.found.is_some_and(|(query, _)| query == state.query);
        (!state.query.is_empty() && !answered).then_some(Load(state.query))
    }
}

/// Each load's query, and the sender the test answers it through.
type Loads = Rc<RefCell<Vec<(&'static str, oneshot::Sender<usize>)>>>;

/// Each load waits for an answer the test gives through the sender it keeps.
struct Loader {
    background: Background<ByHand>,
    loads: Loads,
    shown: Rc<Cell<Searching>>,
}

impl Host<Search> for Loader {
    fn render(&mut self, state: &Searching, _: &Intents<SearchIntent>) {
        self.shown.set(*state);
    }

    fn perform(&mut self, effect: Never, _: &Intents<SearchIntent>) {
        match effect {}
    }

    fn subscribe(&mut self, &Load(query): &Load, intents: &Intents<SearchIntent>) -> Box<dyn Any> {
        let (answer, answered) = oneshot::channel();
        self.loads.borrow_mut().push((query, answer));
        let intents = intents.clone();
        Box::new(self.background.local(answered, move |count| intents.send(SearchIntent::Found(query, count.unwrap()))))
    }
}

#[test]
fn a_load_keyed_on_its_query_is_dropped_when_the_query_changes() {
    let typed = |query| Searching { query, found: None };
    assert_eq!(subscriptions::<Search>(&typed("a")), [Load("a")]);
    assert!(
        subscriptions::<Search>(&Searching {
            query: "a",
            found: Some(("a", 1))
        })
        .is_empty(),
        "an answered query loads nothing"
    );
    assert_eq!(
        subscriptions::<Search>(&Searching {
            query: "ab",
            found: Some(("a", 1))
        }),
        [Load("ab")]
    );

    let loads = Rc::default();
    let shown = Rc::default();
    let loader = Loader {
        background: Background::new(|_| false),
        loads: Rc::clone(&loads),
        shown: Rc::clone(&shown),
    };
    let store = Store::new(Searching::default(), loader);
    store.send(SearchIntent::Typed("a"));
    store.send(SearchIntent::Typed("ab"));
    let (stale, latest) = {
        let mut loads = loads.borrow_mut();
        assert_eq!(loads.iter().map(|(query, _)| *query).collect::<Vec<_>>(), ["a", "ab"]);
        let latest = loads.pop().unwrap().1;
        (loads.pop().unwrap().1, latest)
    };
    until("the stale load let go of", || stale.is_closed());
    assert!(stale.send(1).is_err(), "the load for a query no longer typed was dropped");
    latest.send(2).unwrap();
    until("the answer", || shown.get().found.is_some());
    assert_eq!(shown.get().found, Some(("ab", 2)));
    assert!(loads.borrow().is_empty(), "an answered query starts no load");
}
