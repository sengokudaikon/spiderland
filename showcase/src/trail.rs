//! The trail: the last steps the app's stores ran, each by name, and each change to what a store shows. A fault report would carry
//! it, to say what the app was doing. Each store adds to it as it runs, through an observer.

use spiderland::{Feature, Seen};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// The last [`Trail::LENGTH`] lines, oldest first. Its clones are one trail.
#[derive(Clone, Debug, Default)]
pub struct Trail(Rc<RefCell<VecDeque<String>>>);

impl Trail {
    /// How many lines it keeps.
    pub const LENGTH: usize = 64;

    /// An observer for the store called `name`. It adds each intent, effect and output by the name of its variant, and the line
    /// `shows` makes of the state each time that line changes. Add it on the store's wiring, so it sees the start too.
    ///
    /// It keeps a name, not the intent, because an intent can carry a whole document.
    pub fn follow<F: Feature>(&self, name: impl Into<String>, shows: fn(&F::State) -> String) -> impl Fn(Seen<'_, F>) + 'static
    where
        for<'a> &'a F::Intent: Into<&'static str>,
        for<'a> &'a F::Effect: Into<&'static str>,
        for<'a> &'a F::Output: Into<&'static str>,
    {
        let (trail, name, shown) = (self.clone(), name.into(), RefCell::new(None));
        move |seen: Seen<'_, F>| {
            let line = match seen {
                Seen::Intent(intent) => format!("{name} {}", Into::<&str>::into(intent)),
                Seen::State(state) => {
                    let now = shows(state);
                    if shown.borrow().as_ref() == Some(&now) {
                        return;
                    }
                    let line = format!("{name}: {now}");
                    shown.replace(Some(now));
                    line
                }
                Seen::Effect(effect) => format!("{name} does {}", Into::<&str>::into(effect)),
                Seen::Output(output) => format!("{name} says {}", Into::<&str>::into(output)),
                _ => return,
            };
            trail.add(line);
        }
    }

    /// The lines kept, oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.0.borrow().iter().cloned().collect()
    }

    fn add(&self, line: String) {
        let mut lines = self.0.borrow_mut();
        if lines.len() == Self::LENGTH {
            lines.pop_front();
        }
        lines.push_back(line);
    }
}
