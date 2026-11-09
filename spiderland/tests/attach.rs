//! Views that attach to a store and leave it while the store goes on.

use spiderland::{Attached, Effects, Feature, Host, Intents, Never, Store, View};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Counts, and on `Echo` asks the host for an effect that attaches a view from inside `perform`.
struct Counter;

enum Intent {
    Add(i32),
    Echo,
}

impl Feature for Counter {
    type State = i32;
    type Intent = Intent;
    type Effect = ();
    type Output = Never;
    type Subscription = Never;

    fn reduce(count: &mut i32, intent: Intent, effects: &mut Effects<()>) {
        match intent {
            Intent::Add(by) => *count += by,
            Intent::Echo => effects.push(()),
        }
    }
}

/// Each count a view showed, and whether it has said it is gone.
#[derive(Default)]
struct Seen {
    counts: RefCell<Vec<i32>>,
    gone: Cell<bool>,
}

struct Shown(Rc<Seen>);

impl View<Counter> for Shown {
    fn render(&mut self, count: &i32, _: &Intents<Intent>) {
        self.0.counts.borrow_mut().push(*count);
    }

    fn is_gone(&self) -> bool {
        self.0.gone.get()
    }
}

fn shown() -> (Shown, Rc<Seen>) {
    let seen = Rc::new(Seen::default());
    (Shown(Rc::clone(&seen)), seen)
}

/// Runs no effects but the one that attaches a view, kept in `attached`.
#[derive(Default)]
struct Attaching {
    store: Rc<RefCell<Option<Store<Counter>>>>,
    attached: Rc<RefCell<Vec<Attached>>>,
    seen: Rc<RefCell<Option<Rc<Seen>>>>,
}

impl Host<Counter> for Attaching {
    fn perform(&mut self, (): (), _: &Intents<Intent>) {
        let (view, seen) = shown();
        if let Some(store) = self.store.borrow().as_ref() {
            self.attached.borrow_mut().push(store.attach(view));
        }
        *self.seen.borrow_mut() = Some(seen);
    }
}

#[test]
fn a_view_shows_the_state_at_once_and_after_each_batch_until_it_leaves() {
    let store = Store::new(0, Attaching::default());
    store.send(Intent::Add(2));
    let (view, seen) = shown();
    let attached = store.attach(view);
    assert_eq!(*seen.counts.borrow(), [2]);
    store.send(Intent::Add(1));
    assert_eq!(*seen.counts.borrow(), [2, 3]);
    assert_eq!(store.views(), 1);
    drop(attached);
    store.send(Intent::Add(1));
    assert_eq!(*seen.counts.borrow(), [2, 3], "a view that left shows nothing more");
    assert_eq!(store.views(), 0);
}

#[test]
fn a_view_attached_while_the_store_works_shows_that_batch_once() {
    let host = Attaching::default();
    let (store_slot, attached, seen) = (Rc::clone(&host.store), Rc::clone(&host.attached), Rc::clone(&host.seen));
    let store = Store::new(0, host);
    *store_slot.borrow_mut() = Some(store.clone());
    store.send_all([Intent::Add(4), Intent::Echo]);
    let seen = seen.borrow().clone().expect("the effect attached a view");
    assert_eq!(*seen.counts.borrow(), [4], "rendered as it attached, after the batch's own renders");
    store.send(Intent::Add(1));
    assert_eq!(*seen.counts.borrow(), [4, 5]);
    // The store holds itself through its host's slot, so let go of that first.
    store_slot.borrow_mut().take();
    attached.borrow_mut().clear();
}

#[test]
fn a_view_attached_before_the_start_first_shows_the_start() {
    struct Starting;
    impl Feature for Starting {
        type State = i32;
        type Intent = i32;
        type Effect = Never;
        type Output = Never;
        type Subscription = Never;

        fn reduce(count: &mut i32, by: i32, _: &mut Effects<Never>) {
            *count += by;
        }

        fn start(count: &mut i32, _: &mut Effects<Never>) {
            *count = 10;
        }
    }
    struct Quiet;
    impl Host<Starting> for Quiet {
        fn perform(&mut self, effect: Never, _: &Intents<i32>) {
            match effect {}
        }
    }
    struct Kept(Rc<RefCell<Vec<i32>>>);
    impl View<Starting> for Kept {
        fn render(&mut self, count: &i32, _: &Intents<i32>) {
            self.0.borrow_mut().push(*count);
        }
    }

    let kept = Rc::default();
    let wiring = Store::<Starting>::build(0, Quiet);
    let _attached = wiring.attach(Kept(Rc::clone(&kept)));
    assert!(kept.borrow().is_empty(), "nothing renders before the start");
    let store = wiring.start();
    assert_eq!(*kept.borrow(), [10]);
    store.send(1);
    assert_eq!(*kept.borrow(), [10, 11]);
}

#[test]
fn a_view_that_is_gone_is_let_go_of_without_its_guard() {
    let store = Store::new(0, Attaching::default());
    let (view, seen) = shown();
    let _attached = store.attach(view);
    let (other, _) = shown();
    let _other = store.attach(other);
    assert_eq!(store.views(), 2);
    seen.gone.set(true);
    assert_eq!(store.views(), 1, "a view that is gone drops out of the count at once");
    store.send(Intent::Add(1));
    assert_eq!(*seen.counts.borrow(), [0], "and isn't rendered again");
    assert_eq!(Rc::strong_count(&seen), 1, "the store let go of it");
}

#[test]
fn a_view_that_sends_as_it_attaches_shows_what_that_did() {
    struct Asking(Rc<RefCell<Vec<i32>>>);
    impl View<Counter> for Asking {
        fn render(&mut self, count: &i32, intents: &Intents<Intent>) {
            self.0.borrow_mut().push(*count);
            if *count == 0 {
                intents.send(Intent::Add(7));
            }
        }
    }
    let store = Store::new(0, Attaching::default());
    let kept = Rc::default();
    let _attached = store.attach(Asking(Rc::clone(&kept)));
    assert_eq!(*kept.borrow(), [0, 7], "what its first render sent ran right after, and it showed that too");
}

#[test]
fn a_guard_dropped_during_a_render_ends_the_view_it_guards() {
    struct Dropping(Rc<RefCell<Option<Attached>>>);
    impl View<Counter> for Dropping {
        fn render(&mut self, count: &i32, _: &Intents<Intent>) {
            if *count > 0 {
                self.0.borrow_mut().take();
            }
        }
    }
    let store = Store::new(0, Attaching::default());
    let slot: Rc<RefCell<Option<Attached>>> = Rc::default();
    // Attached first, so it renders first in each batch and drops the other's guard before the other's turn.
    let _dropping = store.attach(Dropping(Rc::clone(&slot)));
    let (view, seen) = shown();
    *slot.borrow_mut() = Some(store.attach(view));
    store.send(Intent::Add(1));
    assert_eq!(*seen.counts.borrow(), [0], "the batch that dropped it didn't render it");
}

#[test]
fn a_guard_outlives_its_store() {
    let store = Store::new(0, Attaching::default());
    let (view, seen) = shown();
    let attached = store.attach(view);
    drop(store);
    assert_eq!(Rc::strong_count(&seen), 1, "the view went with the store");
    drop(attached);
}
