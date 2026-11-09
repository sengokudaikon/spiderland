//! `Background` and the listeners on a loop polled by hand, with no toolkit: `EventLoop` is all they need from one.

use spiderland::{Background, Effects, Error, EventLoop, Feature, Host, Intents, Never, Store, on_wake, subscriptions};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};
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
    Finished(Result<i32, Error>),
    Woken,
}

#[derive(Clone, Default)]
struct State {
    results: Vec<Result<i32, Error>>,
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
    assert_eq!(shown.borrow().results, [Ok(7), Err(Error::Panicked)]);

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

#[test]
fn work_the_pool_refuses_or_lets_go_of_still_comes_back() {
    let shown = Rc::default();
    let store = Store::new(State::default(), Shown(Rc::clone(&shown)));
    let intents = store.intents();

    let refusing = Background::<ByHand>::new(|_| false);
    refusing.run(&intents, || 7, Intent::Finished);
    assert_eq!(shown.borrow().results, [Err(Error::Refused)], "a refusal comes back at once");
    assert_eq!(refusing.pending(), 0);

    let letting_go = Background::<ByHand>::new(|job| {
        drop(job);
        true
    });
    letting_go.run(&intents, || 7, Intent::Finished);
    until("the work let go of", || letting_go.pending() == 0);
    assert_eq!(shown.borrow().results, [Err(Error::Refused), Err(Error::Dropped)]);
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

/// An export that runs while the state wants it, and stops itself once it has heard three reports.
struct Export;

#[derive(Clone, Default)]
struct Exporting {
    running: bool,
    reports: Vec<usize>,
    result: Option<Result<usize, Error>>,
}

enum ExportIntent {
    Reported(usize),
    Exported(Result<usize, Error>),
}

#[derive(Debug, PartialEq)]
struct Running;

impl Feature for Export {
    type State = Exporting;
    type Intent = ExportIntent;
    type Effect = Never;
    type Output = Never;
    type Subscription = Running;

    fn reduce(state: &mut Exporting, intent: ExportIntent, _: &mut Effects<Never>) {
        match intent {
            ExportIntent::Reported(done) => {
                state.reports.push(done);
                state.running = done < 3;
            }
            ExportIntent::Exported(result) => {
                state.result = Some(result);
                state.running = false;
            }
        }
    }

    fn subscriptions(state: &Exporting) -> impl IntoIterator<Item = Running> {
        state.running.then_some(Running)
    }
}

struct Exporter {
    background: Background<ByHand>,
    shown: Rc<RefCell<Exporting>>,
    /// Set by the work as it returns.
    returned: Arc<AtomicBool>,
}

impl Host<Export> for Exporter {
    fn render(&mut self, state: &Exporting, _: &Intents<ExportIntent>) {
        self.shown.borrow_mut().clone_from(state);
    }

    fn perform(&mut self, effect: Never, _: &Intents<ExportIntent>) {
        match effect {}
    }

    fn subscribe(&mut self, Running: &Running, intents: &Intents<ExportIntent>) -> Box<dyn Any> {
        let returned = Arc::clone(&self.returned);
        let stopping = self.background.stoppable(
            intents,
            move |stop, progress| {
                let mut done = 0;
                while !stop.is_stopped() {
                    done += 1;
                    progress.report(done);
                    std::thread::sleep(Duration::from_millis(2));
                }
                returned.store(true, Ordering::SeqCst);
                done
            },
            ExportIntent::Reported,
            ExportIntent::Exported,
        );
        Box::new(stopping)
    }
}

fn pool() -> Background<ByHand> {
    Background::new(|job| {
        std::thread::spawn(job);
        true
    })
}

#[test]
fn stoppable_work_reports_and_then_finishes() {
    let shown = Rc::default();
    let store = Store::new(State::default(), Shown(Rc::clone(&shown)));
    let intents = store.intents();
    let background = pool();
    let (go, gate) = std::sync::mpsc::channel::<()>();
    let stopping = background.stoppable(
        &intents,
        move |_, progress| {
            progress.report(1);
            gate.recv().unwrap();
            progress.report(2);
            gate.recv().unwrap();
            7
        },
        |_: i32| Intent::Woken,
        Intent::Finished,
    );
    until("the first report", || shown.borrow().wakes == 1);
    go.send(()).unwrap();
    until("the second report", || shown.borrow().wakes == 2);
    assert!(!stopping.is_finished());
    go.send(()).unwrap();
    until("the result", || background.pending() == 0);
    assert_eq!(shown.borrow().results, [Ok(7)]);
    assert!(stopping.is_finished());
}

#[test]
fn stoppable_work_stops_with_its_subscription_even_while_the_store_hears_it() {
    let shown = Rc::new(RefCell::new(Exporting::default()));
    let returned = Arc::new(AtomicBool::new(false));
    let background = pool();
    let host = Exporter {
        background: background.clone(),
        shown: Rc::clone(&shown),
        returned: Arc::clone(&returned),
    };
    let _store = Store::new(
        Exporting {
            running: true,
            ..Exporting::default()
        },
        host,
    );
    until("the work to stop and return", || returned.load(Ordering::SeqCst));
    until("nothing left to arrive", || background.pending() == 0);
    for _ in 0..5 {
        turn();
    }
    let state = shown.borrow();
    // Reports can merge, so the one that stopped the work may be past 3. It is the last one heard.
    assert_eq!(
        state.reports.iter().filter(|&&done| done >= 3).count(),
        1,
        "nothing arrived after the report that stopped it"
    );
    assert!(state.reports.windows(2).all(|pair| pair[0] < pair[1]), "reports arrive in order, merged or not");
    assert!(state.result.is_none(), "what stopped work returns arrives nowhere");
}

#[test]
fn stoppable_work_the_pool_refuses_comes_back_at_once() {
    let shown = Rc::default();
    let store = Store::new(State::default(), Shown(Rc::clone(&shown)));
    let refusing = Background::<ByHand>::new(|_| false);
    let stopping = refusing.stoppable(&store.intents(), |_, _: &spiderland::Progress<i32>| 1, |_| Intent::Woken, Intent::Finished);
    assert_eq!(shown.borrow().results, [Err(Error::Refused)]);
    assert_eq!(refusing.pending(), 0);
    drop(stopping);
}

/// Runs `future` on this thread until it is done, as work on the pool that waits on futures does.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park();
    }
}

#[test]
fn stoppable_work_waiting_on_something_else_wakes_once_it_is_stopped() {
    let shown = Rc::default();
    let store = Store::new(State::default(), Shown(Rc::clone(&shown)));
    let background = pool();
    // Stands for a child process the work waits on, which never exits on its own.
    let (_child, exited) = oneshot::channel::<()>();
    let (told, heard) = std::sync::mpsc::channel();
    let stopping = background.stoppable(
        &store.intents(),
        move |stop, _: &spiderland::Progress<i32>| {
            let (mut exited, mut stopped) = (exited, std::pin::pin!(stop.stopped()));
            told.send("waiting").unwrap();
            let woke = block_on(std::future::poll_fn(|context| {
                if Pin::new(&mut exited).poll(context).is_ready() {
                    return Poll::Ready("exited");
                }
                stopped.as_mut().poll(context).map(|()| "stopped")
            }));
            told.send(woke).unwrap();
            assert!(stop.is_stopped());
            block_on(stop.stopped());
            told.send("a stop already over completes at once").unwrap();
            1
        },
        |_| Intent::Woken,
        Intent::Finished,
    );
    let next = || heard.recv_timeout(Duration::from_secs(10)).expect("the work to say");
    assert_eq!(next(), "waiting");
    drop(stopping);
    assert_eq!(next(), "stopped");
    assert_eq!(next(), "a stop already over completes at once");
    until("nothing left to arrive", || background.pending() == 0);
    assert!(shown.borrow().results.is_empty(), "what stopped work returns arrives nowhere");
}
