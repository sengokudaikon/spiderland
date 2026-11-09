//! Several windows on one store, each a view attached through `Windowed`: each shows the state, one of two closes on its own, and
//! the last one asks the store. One test: Slint's platform is set once per process.

#![allow(missing_docs, reason = "`slint!` makes the view's types public, with no docs")]

use slint::ComponentHandle;
use spiderland::{Effects, Feature, Fired, Host, Intents, Never, Pulse, Store};
use spiderland_slint::testing::{Headless, settle};
use spiderland_slint::{WindowView, Windowed, ask_before_closing};

slint::slint! {
    export component Pane inherits Window {
        in property <int> count;
        in property <int> saved;
        out property <int> saves-shown;
        changed saved => {
            saves-shown += 1;
        }
    }
}

/// Counts, and fires a pulse on each save. Asked to close, it remembers that it was.
struct Counter;

#[derive(Default)]
struct State {
    count: i32,
    saved: Pulse,
    asked_to_close: bool,
}

enum Intent {
    Add(i32),
    Save,
    CloseRequested,
}

impl Feature for Counter {
    type State = State;
    type Intent = Intent;
    type Effect = Never;
    type Output = Never;
    type Subscription = Never;

    fn reduce(state: &mut State, intent: Intent, _: &mut Effects<Never>) {
        match intent {
            Intent::Add(by) => state.count += by,
            Intent::Save => state.saved.fire(()),
            Intent::CloseRequested => state.asked_to_close = true,
        }
    }
}

struct Runner;

impl Host<Counter> for Runner {
    fn perform(&mut self, effect: Never, _: &Intents<Intent>) {
        match effect {}
    }
}

#[derive(Default)]
struct PaneView {
    saved: Fired,
}

impl WindowView<Counter> for PaneView {
    type Window = Pane;

    fn render(&mut self, window: &Pane, state: &State, _: &Intents<Intent>) {
        window.set_count(state.count);
        window.set_saved(self.saved.serial(&state.saved));
    }
}

/// A pane, shown, with its view attached and its close asking the store when it is the last.
fn open(store: &Store<Counter>) -> (Pane, spiderland::Attached) {
    let pane = Pane::new().unwrap();
    pane.show().unwrap();
    let attached = store.attach(Windowed::new(&pane, PaneView::default()));
    let asking = store.clone();
    ask_before_closing(&pane, &store.intents(), move || (asking.views() == 1).then_some(Intent::CloseRequested));
    (pane, attached)
}

#[test]
fn several_windows_show_one_store_and_the_last_one_asks_before_closing() {
    let headless = Headless::install();
    headless.separate_windows();
    let store = Store::new(State::default(), Runner);
    store.send(Intent::Save);

    let (first, _first) = open(&store);
    store.send(Intent::Add(2));
    let (second, _second) = open(&store);
    assert_eq!((first.get_count(), second.get_count()), (2, 2), "a window opened later shows the state at once");
    assert_eq!(store.views(), 2);

    store.send(Intent::Save);
    settle();
    assert_eq!(
        (first.get_saves_shown(), second.get_saves_shown()),
        (1, 1),
        "each window acts on the save after it opened, and not on the one before"
    );

    headless.on(second.window()).close();
    assert!(!second.window().is_visible(), "one of two windows closes on its own");
    assert_eq!(store.views(), 1, "a hidden window's view is gone");
    store.send(Intent::Add(1));
    assert_eq!((first.get_count(), second.get_count()), (3, 2), "and isn't rendered again");

    headless.on(first.window()).close();
    assert!(first.window().is_visible(), "the last window asks the store, and stays");
    assert!(store.try_read(|state| state.asked_to_close).unwrap());
    first.hide().unwrap();
    settle();
}
