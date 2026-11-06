//! A small lifecycle a parent drives: a child is shown, hidden, or asked whether it can close. It has no machinery of its own. A
//! parent emits [`Lifecycle`] as an output, and a child opts in by mapping it to an intent. The child answers with a
//! [`CloseAnswer`] of its own, whenever it is ready, and the parent's [`Closing`] counts the answers.
//!
//! A window that closes once its editor has saved:
//!
//! ```
//! use spiderland::{CloseAnswer, Closing, Effects, Feature, Host, Intents, Lifecycle, Never, Store};
//!
//! struct Window;
//! enum WindowIntent {
//!     CloseClicked,
//!     Answered(CloseAnswer),
//! }
//! impl Feature for Window {
//!     type State = Option<Closing>;
//!     type Intent = WindowIntent;
//!     type Effect = &'static str;
//!     type Output = Lifecycle;
//!     type Subscription = Never;
//!
//!     fn reduce(closing: &mut Option<Closing>, intent: WindowIntent, effects: &mut Effects<&'static str, Lifecycle>) {
//!         match intent {
//!             WindowIntent::CloseClicked => {
//!                 *closing = Some(Closing::asking(1));
//!                 effects.emit(Lifecycle::CloseRequested);
//!             }
//!             WindowIntent::Answered(answer) => {
//!                 let answer = closing.as_mut().and_then(|closing| closing.hear(answer));
//!                 if answer.is_some() {
//!                     *closing = None;
//!                 }
//!                 if answer == Some(CloseAnswer::Closed) {
//!                     effects.push("close the window");
//!                 }
//!             }
//!         }
//!     }
//! }
//!
//! struct Editor;
//! enum EditorIntent {
//!     Life(Lifecycle),
//!     Saved,
//! }
//! impl Feature for Editor {
//!     type State = ();
//!     type Intent = EditorIntent;
//!     type Effect = &'static str;
//!     type Output = CloseAnswer;
//!     type Subscription = Never;
//!
//!     fn reduce(_: &mut (), intent: EditorIntent, effects: &mut Effects<&'static str, CloseAnswer>) {
//!         match intent {
//!             EditorIntent::Life(Lifecycle::CloseRequested) => effects.push("save"),
//!             EditorIntent::Life(Lifecycle::Shown | Lifecycle::Hidden) => {}
//!             EditorIntent::Saved => effects.emit(CloseAnswer::Closed),
//!         }
//!     }
//! }
//!
//! # use std::{cell::RefCell, rc::Rc};
//! # struct Log(Rc<RefCell<Vec<&'static str>>>);
//! # impl Host<Window> for Log {
//! #     fn render(&mut self, _: &Option<Closing>, _: &Intents<WindowIntent>) {}
//! #     fn perform(&mut self, effect: &'static str, _: &Intents<WindowIntent>) {
//! #         self.0.borrow_mut().push(effect);
//! #     }
//! # }
//! # impl Host<Editor> for Log {
//! #     fn render(&mut self, _: &(), _: &Intents<EditorIntent>) {}
//! #     fn perform(&mut self, effect: &'static str, intents: &Intents<EditorIntent>) {
//! #         self.0.borrow_mut().push(effect);
//! #         intents.send(EditorIntent::Saved);
//! #     }
//! # }
//! # let log = Rc::new(RefCell::new(Vec::new()));
//! # let window_host = Log(log.clone());
//! # let editor_host = Log(log.clone());
//! let window = Store::<Window>::new(None, window_host);
//! let editor = Store::<Editor>::new((), editor_host);
//! window.forward(&editor.intents().map(EditorIntent::Life), |life| *life);
//! editor.forward(&window.intents().map(WindowIntent::Answered), |answer| *answer);
//!
//! window.send(WindowIntent::CloseClicked);
//! # assert_eq!(*log.borrow(), ["save", "close the window"]);
//! ```

/// What a parent tells a child about its place on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// The child is on screen.
    Shown,
    /// The child is off screen. Its store goes on running, and what to pause is up to the child.
    Hidden,
    /// The window wants to close. A child answers with a [`CloseAnswer`], after finishing whatever has to finish first.
    CloseRequested,
}

/// A child's answer to [`Lifecycle::CloseRequested`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAnswer {
    /// Nothing here holds the window open.
    Closed,
    /// This child wants the window to stay.
    StayingOpen,
}

/// A parent's wait for its children's answers, kept in its state. Each child gets one vote, and one child staying open keeps the
/// window open. Answers still come in from the rest, so a child that said closed may have finished its work by then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closing {
    waiting: usize,
    staying: bool,
}

impl Closing {
    /// Starts waiting for `children` answers.
    pub fn asking(children: usize) -> Closing {
        Closing {
            waiting: children,
            staying: false,
        }
    }

    /// Counts one answer, and returns the result once every child has answered.
    pub fn hear(&mut self, answer: CloseAnswer) -> Option<CloseAnswer> {
        self.waiting = self.waiting.saturating_sub(1);
        self.staying |= answer == CloseAnswer::StayingOpen;
        self.answer()
    }

    /// The result, once every child has answered. With no children to ask, that is at once.
    pub fn answer(&self) -> Option<CloseAnswer> {
        match (self.waiting, self.staying) {
            (0, false) => Some(CloseAnswer::Closed),
            (0, true) => Some(CloseAnswer::StayingOpen),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_answers_once_every_child_has_answered() {
        let mut closing = Closing::asking(2);
        assert_eq!(closing.answer(), None);
        assert_eq!(closing.hear(CloseAnswer::Closed), None);
        assert_eq!(closing.hear(CloseAnswer::Closed), Some(CloseAnswer::Closed));
    }

    #[test]
    fn one_child_staying_open_keeps_the_window_but_the_rest_are_still_heard() {
        let mut closing = Closing::asking(2);
        assert_eq!(closing.hear(CloseAnswer::StayingOpen), None);
        assert_eq!(closing.hear(CloseAnswer::Closed), Some(CloseAnswer::StayingOpen));
    }

    #[test]
    fn with_no_children_there_is_an_answer_at_once() {
        assert_eq!(Closing::asking(0).answer(), Some(CloseAnswer::Closed));
    }
}
