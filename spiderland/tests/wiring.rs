//! Wiring a store before it starts, connections that end, and state shared with a store built later.

use spiderland::{Connection, Effects, Feature, Host, Intents, Never, Seen, Store};
use std::cell::RefCell;
use std::rc::Rc;

/// Settings another page keeps a copy of. Each intent sets one field and says what it set. Its start asks for a write.
struct Settings;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Set {
    Theme(u8),
    Size(u8),
    /// The answer to the write its start asks for.
    Saved,
}

#[derive(Default)]
struct Values {
    theme: u8,
    size: u8,
}

impl Feature for Settings {
    type State = Values;
    type Intent = Set;
    type Effect = ();
    type Output = Set;
    type Subscription = Never;

    fn reduce(values: &mut Values, set: Set, effects: &mut Effects<(), Set>) {
        match set {
            Set::Theme(theme) => values.theme = theme,
            Set::Size(size) => values.size = size,
            Set::Saved => {}
        }
        effects.emit(set);
    }

    fn start(_: &mut Values, effects: &mut Effects<()>) {
        effects.push(());
    }
}

/// Its write answers at once, as work that finishes in `perform` does.
struct AtOnce;

impl Host<Settings> for AtOnce {
    fn render(&mut self, _: &Values, _: &Intents<Set>) {}

    fn perform(&mut self, (): (), intents: &Intents<Set>) {
        intents.send(Set::Saved);
    }
}

/// Keeps what it is sent.
struct Page;

impl Feature for Page {
    type State = Vec<Set>;
    type Intent = Set;
    type Effect = Never;
    type Output = Never;
    type Subscription = Never;

    fn reduce(heard: &mut Vec<Set>, set: Set, _: &mut Effects<Never>) {
        heard.push(set);
    }
}

struct Shown(Rc<RefCell<Vec<Set>>>);

impl Host<Page> for Shown {
    fn render(&mut self, heard: &Vec<Set>, _: &Intents<Set>) {
        self.0.borrow_mut().clone_from(heard);
    }

    fn perform(&mut self, effect: Never, _: &Intents<Set>) {
        match effect {}
    }
}

fn new_page() -> (Store<Page>, Rc<RefCell<Vec<Set>>>) {
    let heard = Rc::default();
    (Store::new(Vec::new(), Shown(Rc::clone(&heard))), heard)
}

#[test]
fn what_a_store_does_as_it_starts_is_heard_when_it_is_wired_first() {
    let (page, heard) = new_page();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let wiring = Store::<Settings>::build(Values::default(), AtOnce);
    wiring.forward(&page.intents(), |set| *set);
    let seeing = Rc::clone(&seen);
    wiring.observe(move |seen| {
        seeing.borrow_mut().push(match seen {
            Seen::Intent(set) => format!("{set:?}"),
            Seen::State(values) => format!("size {}", values.size),
            Seen::Effect(()) => "write".to_owned(),
            Seen::Output(set) => format!("said {set:?}"),
            _ => return,
        })
    });
    let _settings = wiring.start();
    assert_eq!(*heard.borrow(), [Set::Saved], "the answer to the write `start` asked for");
    assert_eq!(
        *seen.borrow(),
        ["size 0", "write", "Saved", "size 0", "said Saved"],
        "the observer saw the start's state and effect too"
    );

    let (late, late_heard) = new_page();
    let settings = Store::<Settings>::new(Values::default(), AtOnce);
    settings.forward(&late.intents(), |set| *set);
    assert!(late_heard.borrow().is_empty(), "built with Store::new, the start's answer reached no one");
}

#[test]
fn what_a_wiring_is_sent_waits_for_the_start() {
    let wiring = Store::<Settings>::build(Values::default(), AtOnce);
    let (page, heard) = new_page();
    wiring.forward(&page.intents(), |set| *set);
    wiring.intents().send(Set::Theme(2));
    assert!(heard.borrow().is_empty(), "nothing runs before the start");
    let _settings = wiring.start();
    assert_eq!(
        *heard.borrow(),
        [Set::Theme(2), Set::Saved],
        "in the order sent: before the start, then by the start's write"
    );
}

#[test]
fn a_connection_delivers_until_it_is_dropped_and_lets_go_at_once() {
    let settings = Store::<Settings>::new(Values::default(), AtOnce);
    let (page, heard) = new_page();
    let held = Rc::new(());
    let holding = Rc::clone(&held);
    let connection = settings.connect(&page.intents(), move |set| {
        let _ = &holding;
        *set
    });
    settings.send(Set::Theme(1));
    drop(connection);
    assert_eq!(Rc::strong_count(&held), 1, "what map held went with the connection");
    settings.send(Set::Theme(2));
    assert_eq!(*heard.borrow(), [Set::Theme(1)]);
}

#[test]
fn a_connection_dropped_while_an_output_goes_out_hears_no_more_of_it() {
    let settings = Store::<Settings>::new(Values::default(), AtOnce);
    let (page, heard) = new_page();
    let slot: Rc<RefCell<Option<Connection>>> = Rc::default();
    let dropping = Rc::clone(&slot);
    // Heard first, so it drops the connection before the connection's turn.
    settings.forward(&page.intents(), move |_| {
        dropping.borrow_mut().take();
        None
    });
    *slot.borrow_mut() = Some(settings.connect(&page.intents(), |set| *set));
    settings.send_all([Set::Theme(1), Set::Size(2)]);
    assert!(heard.borrow().is_empty());
}

#[test]
fn shared_state_goes_out_now_and_again_only_when_it_changes() {
    let settings = Store::<Settings>::new(Values { theme: 3, size: 0 }, AtOnce);
    let (page, heard) = new_page();
    settings.share(&page.intents(), |values| values.theme, |theme| Set::Theme(*theme));
    assert_eq!(*heard.borrow(), [Set::Theme(3)], "a store connected later starts on the current value");
    settings.send(Set::Size(9));
    settings.send(Set::Theme(3));
    assert_eq!(*heard.borrow(), [Set::Theme(3)], "nothing it shares changed");
    settings.send(Set::Theme(4));
    assert_eq!(*heard.borrow(), [Set::Theme(3), Set::Theme(4)]);
}

#[test]
fn shared_state_from_a_wiring_goes_out_once_it_starts() {
    let wiring = Store::<Settings>::build(Values { theme: 5, size: 0 }, AtOnce);
    let (page, heard) = new_page();
    wiring.share(&page.intents(), |values| values.theme, |theme| Set::Theme(*theme));
    assert!(heard.borrow().is_empty());
    let _settings = wiring.start();
    assert_eq!(*heard.borrow(), [Set::Theme(5)]);
}

#[test]
fn sharing_ends_with_the_store_it_reaches() {
    let settings = Store::<Settings>::new(Values::default(), AtOnce);
    let (page, _) = new_page();
    let held = Rc::new(());
    let holding = Rc::clone(&held);
    settings.share(
        &page.intents(),
        |values| values.theme,
        move |theme| {
            let _ = &holding;
            Set::Theme(*theme)
        },
    );
    drop(page);
    settings.send(Set::Theme(1));
    assert_eq!(Rc::strong_count(&held), 1, "the next batch let go of what it held");
}

#[test]
fn a_scoped_share_goes_out_until_it_is_dropped_and_lets_go_at_once() {
    let settings = Store::<Settings>::new(Values { theme: 1, size: 0 }, AtOnce);
    let (page, heard) = new_page();
    let held = Rc::new(());
    let holding = Rc::clone(&held);
    let sharing = settings.share_scoped(
        &page.intents(),
        |values| values.theme,
        move |theme| {
            let _ = &holding;
            Set::Theme(*theme)
        },
    );
    assert_eq!(*heard.borrow(), [Set::Theme(1)], "a store connected later starts on the current value");
    settings.send(Set::Theme(2));
    drop(sharing);
    assert_eq!(Rc::strong_count(&held), 1, "what map held went with the connection");
    settings.send(Set::Theme(3));
    assert_eq!(*heard.borrow(), [Set::Theme(1), Set::Theme(2)]);
}

#[test]
fn a_scoped_share_ended_during_a_batch_sends_nothing_of_it() {
    let settings = Store::<Settings>::new(Values::default(), AtOnce);
    let (page, heard) = new_page();
    let slot: Rc<RefCell<Option<Connection>>> = Rc::default();
    let dropping = Rc::clone(&slot);
    // Outputs are heard before what is shared goes out, so this ends the share before the batch's value.
    settings.forward(&page.intents(), move |_| {
        dropping.borrow_mut().take();
        None
    });
    *slot.borrow_mut() = Some(settings.share_scoped(&page.intents(), |values| values.theme, |theme| Set::Theme(*theme)));
    settings.send(Set::Theme(4));
    assert_eq!(*heard.borrow(), [Set::Theme(0)], "only the value sent as it connected");
}

#[test]
fn a_scoped_share_wired_first_goes_out_once_the_store_starts() {
    let wiring = Store::<Settings>::build(Values { theme: 5, size: 0 }, AtOnce);
    let (page, heard) = new_page();
    let sharing = wiring.share_scoped(&page.intents(), |values| values.theme, |theme| Set::Theme(*theme));
    assert!(heard.borrow().is_empty());
    let settings = wiring.start();
    assert_eq!(*heard.borrow(), [Set::Theme(5)]);
    drop(sharing);
    settings.send(Set::Theme(6));
    assert_eq!(*heard.borrow(), [Set::Theme(5)]);
}

#[test]
fn a_scoped_observer_sees_until_it_is_dropped_even_within_a_batch() {
    let settings = Store::<Settings>::new(Values::default(), AtOnce);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let held = Rc::new(());
    let (seeing, holding) = (Rc::clone(&seen), Rc::clone(&held));
    let slot: Rc<RefCell<Option<Connection>>> = Rc::default();
    let dropping = Rc::clone(&slot);
    // Added first, so it sees each step before the scoped one, and ends it once it sees a size.
    settings.observe(move |seen| {
        if let Seen::Intent(Set::Size(_)) = seen {
            dropping.borrow_mut().take();
        }
    });
    *slot.borrow_mut() = Some(settings.observe_scoped(move |seen| {
        let _ = &holding;
        if let Seen::Intent(set) = seen {
            seeing.borrow_mut().push(*set);
        }
    }));
    settings.send_all([Set::Theme(1), Set::Size(2), Set::Theme(3)]);
    assert_eq!(*seen.borrow(), [Set::Theme(1)], "it saw nothing from the moment it was dropped");
    assert_eq!(Rc::strong_count(&held), 1, "what the observer held went with it");
}

#[test]
fn a_scoped_observer_wired_first_sees_the_start() {
    let wiring = Store::<Settings>::build(Values::default(), AtOnce);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let seeing = Rc::clone(&seen);
    let observing = wiring.observe_scoped(move |seen| {
        if let Seen::Effect(()) = seen {
            seeing.borrow_mut().push("write");
        }
    });
    let settings = wiring.start();
    assert_eq!(*seen.borrow(), ["write"], "the write the start asked for");
    drop(observing);
    settings.send(Set::Theme(1));
    assert_eq!(*seen.borrow(), ["write"]);
}
