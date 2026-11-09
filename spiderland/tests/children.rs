//! Child stores a parent keeps by key: built and dropped as its state names them, told where they stand, and heard when they answer.

use spiderland::{Children, CloseAnswer, Closing, Effects, Feature, Host, Intents, Lifecycle, Never, Standing, Store};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// A tab: it keeps what it hears, and answers a close as it was told to.
struct Tab;

#[derive(Debug, Default)]
struct TabState {
    heard: Vec<Lifecycle>,
    pokes: u32,
    /// What it answers when asked to close. With `None`, it doesn't answer yet.
    answer: Option<CloseAnswer>,
}

#[derive(Debug)]
enum TabIntent {
    Life(Lifecycle),
    Answer(CloseAnswer),
    Poke,
}

impl From<Lifecycle> for TabIntent {
    fn from(life: Lifecycle) -> Self {
        TabIntent::Life(life)
    }
}

impl Feature for Tab {
    type State = TabState;
    type Intent = TabIntent;
    type Effect = Never;
    type Output = CloseAnswer;
    type Subscription = Never;

    fn reduce(state: &mut TabState, intent: TabIntent, effects: &mut Effects<Never, CloseAnswer>) {
        match intent {
            TabIntent::Life(life) => {
                state.heard.push(life);
                if let (Lifecycle::CloseRequested, Some(answer)) = (life, state.answer) {
                    effects.emit(answer);
                }
            }
            TabIntent::Answer(answer) => effects.emit(answer),
            TabIntent::Poke => state.pokes += 1,
        }
    }
}

/// Says when its tab's store is dropped.
struct TabHost(Rc<Cell<bool>>);

impl Host<Tab> for TabHost {
    fn perform(&mut self, effect: Never, _: &Intents<TabIntent>) {
        match effect {}
    }
}

impl Drop for TabHost {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

/// A window of tabs, the last one shown. Asked to close, it asks each tab, and closes once all of them have closed.
struct Window;

#[derive(Debug, Default)]
struct State {
    tabs: Vec<u32>,
    closing: Option<Closing<u32>>,
    closed: Option<CloseAnswer>,
}

#[derive(Debug)]
enum Intent {
    Open(u32),
    Close(u32),
    CloseClicked,
    Answered(u32, CloseAnswer),
}

#[derive(Debug, PartialEq)]
enum Effect {
    Poke(u32),
}

impl Feature for Window {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = Never;

    fn start(state: &mut State, effects: &mut Effects<Effect>) {
        if let Some(first) = state.tabs.first() {
            effects.push(Effect::Poke(*first));
        }
    }

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
        match intent {
            Intent::Open(tab) => {
                state.tabs.push(tab);
                effects.push(Effect::Poke(tab));
            }
            Intent::Close(tab) => state.tabs.retain(|open| *open != tab),
            Intent::CloseClicked => state.closing = Some(Closing::asking(state.tabs.clone())),
            Intent::Answered(tab, answer) => {
                if answer == CloseAnswer::Closed {
                    state.tabs.retain(|open| *open != tab);
                }
                let done = state.closing.as_mut().and_then(|closing| closing.hear(tab, answer));
                if let Some(done) = done {
                    state.closing = None;
                    state.closed = Some(done);
                }
            }
        }
    }
}

/// Where each tab stands: all of them asked while the window closes, else the last shown.
fn standings(state: &State) -> Vec<(u32, Standing)> {
    let top = state.tabs.len().saturating_sub(1);
    state
        .tabs
        .iter()
        .enumerate()
        .map(|(at, tab)| {
            let standing = match () {
                () if state.closing.is_some() => Standing::Closing,
                () if at == top => Standing::Shown,
                () => Standing::Hidden,
            };
            (*tab, standing)
        })
        .collect()
}

/// Pokes a tab, which it can only do once the tab is built.
struct WindowHost(Children<u32, Store<Tab>>);

impl Host<Window> for WindowHost {
    fn perform(&mut self, Effect::Poke(tab): Effect, _: &Intents<Intent>) {
        self.0.get(&tab).expect("an effect reaches a tab its batch named").send(TabIntent::Poke);
    }
}

/// A window on `state`, with its tabs, which answer a close as `answers` says, and record their drops in `dropped`.
struct Scene {
    window: Store<Window>,
    tabs: Children<u32, Store<Tab>>,
    dropped: Rc<RefCell<HashMap<u32, Rc<Cell<bool>>>>>,
    built: Rc<Cell<u32>>,
}

impl Scene {
    fn new(state: State, answers: HashMap<u32, CloseAnswer>) -> Scene {
        let tabs = Children::default();
        let dropped: Rc<RefCell<HashMap<u32, Rc<Cell<bool>>>>> = Rc::default();
        let built = Rc::new(Cell::new(0));
        let wiring = Store::<Window>::build(state, WindowHost(tabs.clone()));
        let (keeping, counting) = (Rc::clone(&dropped), Rc::clone(&built));
        wiring.children(&tabs, standings, move |&tab, window| {
            counting.set(counting.get() + 1);
            let gone = Rc::new(Cell::new(false));
            keeping.borrow_mut().insert(tab, Rc::clone(&gone));
            let state = TabState {
                answer: answers.get(&tab).copied(),
                ..TabState::default()
            };
            let store = Store::<Tab>::build(state, TabHost(gone));
            store.forward(window, move |answer| Intent::Answered(tab, *answer));
            store.start()
        });
        Scene {
            window: wiring.start(),
            tabs,
            dropped,
            built,
        }
    }

    fn heard(&self, tab: u32) -> Vec<Lifecycle> {
        self.tabs.get(&tab).expect("the tab is kept").try_read(|state| state.heard.clone()).unwrap()
    }

    fn is_dropped(&self, tab: u32) -> bool {
        self.dropped.borrow()[&tab].get()
    }
}

#[test]
fn children_follow_the_state_and_one_dropped_hears_nothing_more() {
    let scene = Scene::new(State::default(), HashMap::new());
    scene.window.send(Intent::Open(1));
    scene.window.send(Intent::Open(2));
    assert_eq!(scene.tabs.keys(), [1, 2]);
    let late = scene.tabs.get(&1).unwrap().intents();

    scene.window.send(Intent::Close(1));
    assert_eq!(scene.tabs.keys(), [2]);
    assert!(scene.is_dropped(1), "a child no longer named is dropped");
    // What its work sends after that goes nowhere.
    late.send(TabIntent::Poke);
    assert_eq!(scene.built.get(), 2, "a child kept isn't built again");
}

#[test]
fn an_effect_reaches_a_child_its_batch_named() {
    let scene = Scene::new(State::default(), HashMap::new());
    scene.window.send(Intent::Open(1));
    assert_eq!(scene.tabs.get(&1).unwrap().try_read(|state| state.pokes), Some(1));
}

#[test]
fn children_wired_before_the_start_are_built_before_the_starts_effects() {
    let scene = Scene::new(
        State {
            tabs: vec![7],
            ..State::default()
        },
        HashMap::new(),
    );
    assert_eq!(scene.tabs.get(&7).unwrap().try_read(|state| state.pokes), Some(1));
    assert_eq!(scene.heard(7), [Lifecycle::Shown], "a child hears where it stands as it is built");
}

#[test]
fn each_child_hears_where_it_stands_as_it_changes() {
    let scene = Scene::new(State::default(), HashMap::new());
    scene.window.send(Intent::Open(1));
    scene.window.send(Intent::Open(2));
    assert_eq!(scene.heard(1), [Lifecycle::Shown, Lifecycle::Hidden], "a page covered by another is hidden");
    assert_eq!(scene.heard(2), [Lifecycle::Shown]);

    scene.window.send(Intent::CloseClicked);
    scene.window.send(Intent::Answered(1, CloseAnswer::StayingOpen));
    scene.window.send(Intent::Answered(2, CloseAnswer::StayingOpen));
    assert_eq!(scene.window.try_read(|state| state.closed), Some(Some(CloseAnswer::StayingOpen)));
    assert_eq!(
        scene.heard(1),
        [Lifecycle::Shown, Lifecycle::Hidden, Lifecycle::CloseRequested, Lifecycle::CloseCalledOff, Lifecycle::Hidden],
        "leaving the close, a child hears it was called off, and where it stands again"
    );
    assert_eq!(scene.heard(2).last(), Some(&Lifecycle::Shown));
}

#[test]
fn a_parent_asks_its_children_to_close_and_closes_once_they_have() {
    let answers = HashMap::from([(1, CloseAnswer::Closed), (2, CloseAnswer::Closed)]);
    let scene = Scene::new(State::default(), answers);
    scene.window.send_all([Intent::Open(1), Intent::Open(2)]);
    scene.window.send(Intent::CloseClicked);
    assert_eq!(scene.window.try_read(|state| state.closed), Some(Some(CloseAnswer::Closed)));
    assert!(scene.tabs.is_empty(), "each child that closed was dropped");
    assert!(scene.is_dropped(1) && scene.is_dropped(2));
}

#[test]
fn a_child_whose_own_answer_removes_it_is_dropped_once_its_batch_is_over() {
    let scene = Scene::new(State::default(), HashMap::new());
    scene.window.send(Intent::Open(1));
    // The tab works on its own, with its parent idle: its answer runs the parent at once, inside the tab's own batch.
    let tab = scene.tabs.get(&1).unwrap().intents();
    tab.send(TabIntent::Answer(CloseAnswer::Closed));
    assert!(scene.tabs.is_empty());
    assert!(scene.is_dropped(1), "the tab went once its batch was over");
}

#[test]
fn a_key_named_twice_is_one_child() {
    let scene = Scene::new(
        State {
            tabs: vec![3, 3],
            ..State::default()
        },
        HashMap::new(),
    );
    assert_eq!((scene.tabs.keys(), scene.built.get()), (vec![3], 1));
}

#[test]
fn children_kept_on_a_started_store_are_built_at_once() {
    struct Quiet;
    impl Host<Window> for Quiet {
        fn perform(&mut self, _: Effect, _: &Intents<Intent>) {}
    }
    let window = Store::<Window>::new(
        State {
            tabs: vec![4],
            ..State::default()
        },
        Quiet,
    );
    let tabs = Children::default();
    window.children(&tabs, standings, |_, _| Store::<Tab>::new(TabState::default(), TabHost(Rc::default())));
    assert_eq!(tabs.keys(), [4]);
    window.send(Intent::Open(5));
    assert_eq!(tabs.keys(), [4, 5]);
}

#[test]
fn a_child_built_on_a_started_store_can_send_to_its_parent_at_once() {
    struct Quiet;
    impl Host<Window> for Quiet {
        fn perform(&mut self, _: Effect, _: &Intents<Intent>) {}
    }
    let window = Store::<Window>::new(
        State {
            tabs: vec![4],
            ..State::default()
        },
        Quiet,
    );
    let tabs = Children::default();
    window.children(&tabs, standings, |&tab, window| {
        if tab == 4 {
            window.send(Intent::Open(9));
        }
        Store::<Tab>::new(TabState::default(), TabHost(Rc::default()))
    });
    assert_eq!(tabs.keys(), [4, 9], "what it sent ran once the first children were kept");
}
