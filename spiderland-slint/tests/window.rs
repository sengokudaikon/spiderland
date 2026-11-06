//! `Windowed` on a real Slint window: while the window lasts, and once it is gone. One test: Slint's platform is set once per
//! process.

#![allow(missing_docs, reason = "`slint!` makes the view's types public, with no docs")]

use slint::ComponentHandle;
use spiderland::{Effects, Feature, Intents, Never, Store};
use spiderland_slint::testing::Headless;
use spiderland_slint::{WindowHost, Windowed};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

slint::slint! {
    export component Page inherits Window {
        in property <int> count;
    }
}

struct Counter;

enum Intent {
    Add(i32),
    Save,
    Saved,
    Watch,
}

enum Effect {
    Save,
}

#[derive(Default)]
struct State {
    count: i32,
    saved: u32,
    watching: bool,
}

impl Feature for Counter {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = ();

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
        match intent {
            Intent::Add(by) => state.count += by,
            Intent::Save => effects.push(Effect::Save),
            Intent::Saved => state.saved += 1,
            Intent::Watch => state.watching = true,
        }
    }

    fn subscriptions(state: &State) -> impl IntoIterator<Item = ()> {
        state.watching.then_some(())
    }
}

#[derive(Default)]
struct Seen {
    renders: usize,
    /// Whether each effect had the window.
    windows: Vec<bool>,
    subscribed: usize,
}

struct PageHost(Rc<RefCell<Seen>>);

impl WindowHost<Counter> for PageHost {
    type Window = Page;

    fn render(&mut self, page: &Page, state: &State, _: &Intents<Intent>) {
        page.set_count(state.count);
        self.0.borrow_mut().renders += 1;
    }

    fn perform(&mut self, page: Option<&Page>, Effect::Save: Effect, intents: &Intents<Intent>) {
        self.0.borrow_mut().windows.push(page.is_some());
        intents.send(Intent::Saved);
    }

    fn subscribe(&mut self, (): &(), _: &Intents<Intent>) -> Box<dyn Any> {
        self.0.borrow_mut().subscribed += 1;
        Box::new(())
    }
}

#[test]
fn a_host_renders_while_its_window_lasts_and_performs_after() {
    let _headless = Headless::install();
    let seen = Rc::new(RefCell::new(Seen::default()));
    // Never shown: a window that shows is held by Slint, and couldn't be dropped here.
    let page = Page::new().unwrap();
    let store = Store::<Counter>::new(State::default(), Windowed::new(&page, PageHost(seen.clone())));
    store.send(Intent::Add(2));
    assert_eq!(page.get_count(), 2, "the render reached the window");
    store.send(Intent::Save);
    assert_eq!(seen.borrow().windows, [true]);

    let weak = page.as_weak();
    drop(page);
    assert!(weak.upgrade().is_none(), "nothing else holds the window");
    let renders = seen.borrow().renders;
    store.send(Intent::Add(1));
    store.send(Intent::Save);
    assert_eq!(seen.borrow().renders, renders, "no render once the window is gone");
    assert_eq!(seen.borrow().windows, [true, false], "the effect still ran, with no window");
    assert_eq!(store.try_read(|state| (state.count, state.saved)), Some((3, 2)), "both saves were heard");

    store.send(Intent::Watch);
    assert_eq!(seen.borrow().subscribed, 1, "a subscription follows the state, window or not");
}
