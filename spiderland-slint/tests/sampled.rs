//! `Sampled` on a real Slint window: a burst of moves reaches the store as its last one. One test: Slint's platform is set once
//! per process.

#![allow(missing_docs, reason = "`slint!` makes the view's types public, with no docs")]

use slint::ComponentHandle;
use spiderland::{Effects, Feature, Host, Intents, Never, Store};
use spiderland_slint::Sampled;
use spiderland_slint::testing::{Headless, advance, settle};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

slint::slint! {
    export component Pad inherits Window {
        in property <int> shown;
        callback moved(int);
        callback dropped();
    }
}

struct Drag;

enum Intent {
    Moved(i32),
    Dropped,
}

#[derive(Default)]
struct State {
    at: i32,
    /// Where each drop was.
    drops: Vec<i32>,
}

impl Feature for Drag {
    type State = State;
    type Intent = Intent;
    type Effect = Never;
    type Output = Never;
    type Subscription = Never;

    fn reduce(state: &mut State, intent: Intent, _: &mut Effects<Never>) {
        match intent {
            Intent::Moved(at) => state.at = at,
            Intent::Dropped => state.drops.push(state.at),
        }
    }
}

struct Shown {
    pad: Pad,
    renders: Rc<Cell<u32>>,
}

impl Host<Drag> for Shown {
    fn render(&mut self, state: &State, _: &Intents<Intent>) {
        self.pad.set_shown(state.at);
        self.renders.set(self.renders.get() + 1);
    }

    fn perform(&mut self, effect: Never, _: &Intents<Intent>) {
        match effect {}
    }
}

#[test]
fn a_burst_of_samples_reaches_the_store_as_its_last_one() {
    let _headless = Headless::install();
    let pad = Pad::new().expect("the pad");
    let renders = Rc::new(Cell::new(0));
    let store = Store::new(
        State::default(),
        Shown {
            pad: pad.clone_strong(),
            renders: Rc::clone(&renders),
        },
    );
    let sampled = Sampled::new(&store.intents());
    pad.on_moved({
        let sampled = sampled.clone();
        move |at| sampled.sample(Intent::Moved(at))
    });
    pad.on_dropped({
        let sampled = sampled.clone();
        move || sampled.send(Intent::Dropped)
    });

    let before = renders.get();
    for at in 1..=1000 {
        pad.invoke_moved(at);
    }
    assert_eq!(renders.get(), before, "nothing reached the store before the next turn");
    assert!(sampled.is_waiting());
    settle();
    assert_eq!(renders.get(), before + 1, "a thousand moves rendered once");
    assert_eq!(pad.get_shown(), 1000, "and the last one shows");
    assert!(!sampled.is_waiting());

    // A command goes after the move sampled before it, at once.
    pad.invoke_moved(5);
    pad.invoke_dropped();
    assert_eq!(store.try_read(|state| state.drops.clone()), Some(vec![5]));

    // A flush sends what waits at once, and the turn after finds nothing left.
    sampled.sample(Intent::Moved(7));
    sampled.flush();
    assert_eq!(pad.get_shown(), 7);
    let flushed = renders.get();
    settle();
    assert_eq!(renders.get(), flushed, "the turn after had nothing to send");

    // At most once an interval: the first after a quiet spell goes at once, the rest wait it out, and the last goes too.
    let frame = Duration::from_millis(16);
    let paced = Sampled::every(frame, &store.intents());
    paced.sample(Intent::Moved(20));
    settle();
    assert_eq!(pad.get_shown(), 20, "the first goes on the next turn");
    let first = renders.get();
    for at in 21..=30 {
        paced.sample(Intent::Moved(at));
        settle();
    }
    assert_eq!((renders.get(), pad.get_shown()), (first, 20), "the rest wait out the interval");
    advance(frame);
    assert_eq!((renders.get(), pad.get_shown()), (first + 1, 30), "then the latest goes, once");
    advance(frame);
    assert_eq!(renders.get(), first + 1, "nothing waited, so nothing went");
    paced.sample(Intent::Moved(31));
    settle();
    assert_eq!(pad.get_shown(), 31, "after a quiet interval, a sample goes on the next turn again");
    drop(paced);

    // Dropped, a sampler still sends what waits.
    let other = Sampled::new(&store.intents());
    other.sample(Intent::Moved(9));
    drop(other);
    assert_eq!(pad.get_shown(), 9, "the last sample always goes out");
}
