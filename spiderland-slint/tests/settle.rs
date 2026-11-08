//! `settle`, `until` and `idle` come back only where nothing more is due. One test: Slint's platform is set once per process.

use slint::Timer;
use spiderland::{Effects, Feature, Host, Intents, Never, Store};
use spiderland_slint::testing::{Headless, idle, settle};
use spiderland_slint::{Background, after};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

struct Chain;

enum Intent {
    Start(u32),
    Finished(u32),
    Again(u32),
}

enum Effect {
    Work(u32),
    /// Starts a timer due at once, which asks for more work.
    Soon(u32),
}

#[derive(Default)]
struct State {
    finished: Vec<u32>,
}

impl Feature for Chain {
    type State = State;
    type Intent = Intent;
    type Effect = Effect;
    type Output = Never;
    type Subscription = Never;

    fn reduce(state: &mut State, intent: Intent, effects: &mut Effects<Effect>) {
        match intent {
            Intent::Start(n) | Intent::Again(n) => effects.push(Effect::Work(n)),
            Intent::Finished(n) => {
                state.finished.push(n);
                if n == 1 {
                    effects.push(Effect::Soon(2));
                }
            }
        }
    }
}

struct Runner {
    background: Background,
    timer: Option<Timer>,
}

impl Host<Chain> for Runner {
    fn render(&mut self, _: &State, _: &Intents<Intent>) {}

    fn perform(&mut self, effect: Effect, intents: &Intents<Intent>) {
        match effect {
            Effect::Work(n) => self.background.run(
                intents,
                move || {
                    // Long enough that the work can't finish within the settle that starts it.
                    std::thread::sleep(Duration::from_millis(20));
                    n
                },
                move |result| Intent::Finished(result.unwrap()),
            ),
            Effect::Soon(n) => self.timer = Some(after(Duration::ZERO, intents, move || Intent::Again(n))),
        }
    }
}

#[test]
fn idle_and_settle_come_back_only_once_nothing_more_is_due() {
    let _headless = Headless::frozen();
    let background = Background::new(|job| {
        std::thread::spawn(job);
        true
    });
    let store = Store::new(
        State::default(),
        Runner {
            background: background.clone(),
            timer: None,
        },
    );

    // The first work's result sets a timer due at once, and that timer asks for more work.
    store.send(Intent::Start(1));
    idle(&background);
    assert_eq!(background.pending(), 0, "idle came back with work still out");
    assert_eq!(store.try_read(|state| state.finished.clone()), Some(vec![1, 2]));

    // A timer due at once, set by an event, fires in the same settle.
    let fired = Rc::new(Cell::new(false));
    let firing = fired.clone();
    slint::spawn_local(async move {
        Timer::single_shot(Duration::ZERO, move || firing.set(true));
    })
    .unwrap();
    settle();
    assert!(fired.get(), "a timer due now, set by an event, waited for another settle");
}
