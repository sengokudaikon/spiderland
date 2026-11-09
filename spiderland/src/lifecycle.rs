//! A small lifecycle a parent drives: showing and hiding its children, and asking whether they can close.

/// What a parent tells a child about its place on screen.
///
/// A child is shown, hidden, or asked whether it can close. A child opts in by mapping it to an intent. The child answers with a
/// [`CloseAnswer`] of its own, whenever it is ready, and the parent's [`Closing`] counts the answers. When the window stays after
/// all, the parent tells its children with [`Lifecycle::CloseCalledOff`].
///
/// A parent whose children are stores it keeps in [`Children`](crate::Children) says in its state where each one stands, as a
/// [`Standing`](crate::Standing), and its store sends each change. A parent can also emit [`Lifecycle`] as an output and forward
/// it, as here.
///
/// A window that closes once its editor has saved:
///
/// ```
/// use spiderland::{CloseAnswer, Closing, Effects, Feature, Host, Intents, Lifecycle, Never, Store};
///
/// struct Window;
/// /// The children the window asks, by a name of its own.
/// #[derive(PartialEq)]
/// enum Child {
///     Editor,
/// }
/// enum WindowIntent {
///     CloseClicked,
///     Answered(Child, CloseAnswer),
/// }
/// impl Feature for Window {
///     type State = Option<Closing<Child>>;
///     type Intent = WindowIntent;
///     type Effect = &'static str;
///     type Output = Lifecycle;
///     type Subscription = Never;
///
///     fn reduce(closing: &mut Option<Closing<Child>>, intent: WindowIntent, effects: &mut Effects<&'static str, Lifecycle>) {
///         match intent {
///             WindowIntent::CloseClicked => {
///                 *closing = Some(Closing::asking([Child::Editor]));
///                 effects.emit(Lifecycle::CloseRequested);
///             }
///             WindowIntent::Answered(child, answer) => {
///                 let answer = closing.as_mut().and_then(|closing| closing.hear(child, answer));
///                 if answer.is_some() {
///                     *closing = None;
///                 }
///                 match answer {
///                     Some(CloseAnswer::Closed) => effects.push("close the window"),
///                     Some(CloseAnswer::StayingOpen) => effects.emit(Lifecycle::CloseCalledOff),
///                     None => {}
///                 }
///             }
///         }
///     }
/// }
///
/// struct Editor;
/// enum EditorIntent {
///     Life(Lifecycle),
///     Saved,
/// }
/// impl Feature for Editor {
///     type State = ();
///     type Intent = EditorIntent;
///     type Effect = &'static str;
///     type Output = CloseAnswer;
///     type Subscription = Never;
///
///     fn reduce(_: &mut (), intent: EditorIntent, effects: &mut Effects<&'static str, CloseAnswer>) {
///         match intent {
///             EditorIntent::Life(Lifecycle::CloseRequested) => effects.push("save"),
///             EditorIntent::Life(Lifecycle::Shown | Lifecycle::Hidden | Lifecycle::CloseCalledOff) => {}
///             EditorIntent::Saved => effects.emit(CloseAnswer::Closed),
///         }
///     }
/// }
///
/// # use std::{cell::RefCell, rc::Rc};
/// # struct Log(Rc<RefCell<Vec<&'static str>>>);
/// # impl Host<Window> for Log {
/// #     fn render(&mut self, _: &Option<Closing<Child>>, _: &Intents<WindowIntent>) {}
/// #     fn perform(&mut self, effect: &'static str, _: &Intents<WindowIntent>) {
/// #         self.0.borrow_mut().push(effect);
/// #     }
/// # }
/// # impl Host<Editor> for Log {
/// #     fn render(&mut self, _: &(), _: &Intents<EditorIntent>) {}
/// #     fn perform(&mut self, effect: &'static str, intents: &Intents<EditorIntent>) {
/// #         self.0.borrow_mut().push(effect);
/// #         intents.send(EditorIntent::Saved);
/// #     }
/// # }
/// # let log = Rc::new(RefCell::new(Vec::new()));
/// # let window_host = Log(log.clone());
/// # let editor_host = Log(log.clone());
/// let window = Store::<Window>::new(None, window_host);
/// let editor = Store::<Editor>::new((), editor_host);
/// window.forward(&editor.intents().map(EditorIntent::Life), |life| *life);
/// editor.forward(&window.intents(), |answer| WindowIntent::Answered(Child::Editor, *answer));
///
/// window.send(WindowIntent::CloseClicked);
/// # assert_eq!(*log.borrow(), ["save", "close the window"]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    /// The child is on screen.
    Shown,
    /// The child is off screen. Its store goes on running, and what to pause is up to the child.
    Hidden,
    /// The window wants to close. A child answers with a [`CloseAnswer`], after finishing whatever has to finish first.
    CloseRequested,
    /// The window stays after all, though it asked to close: a child kept it open, or the parent did. A child that stopped
    /// something to close, such as its work, starts it again.
    CloseCalledOff,
}

/// A child's answer to [`Lifecycle::CloseRequested`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAnswer {
    /// Nothing here holds the window open.
    Closed,
    /// This child wants the window to stay.
    StayingOpen,
}

/// A parent's wait for its children's answers, kept in its state. The parent names each child by a key of its own, such as an
/// index or a page, and each child gets one vote. One child staying open keeps the window open. [`Lifecycle`] shows a window
/// that closes once its editor has saved.
///
/// It waits for every answer even so. By the time the window stays open, a child that answered closed may have done its closing
/// work, such as a save. Told [`Lifecycle::CloseCalledOff`] before it answers, a child would start that work again while it is
/// still closing.
///
/// Each child is heard once. A second answer from one child, or an answer from a child not asked, such as a late one from a close
/// already over, changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closing<K = usize> {
    /// Each child asked, with its answer once it has one.
    children: Vec<(K, Option<CloseAnswer>)>,
}

impl<K: PartialEq> Closing<K> {
    /// Starts waiting for an answer from each of `children`. A key given twice is asked once.
    pub fn asking(children: impl IntoIterator<Item = K>) -> Self {
        let mut asked: Vec<(K, Option<CloseAnswer>)> = Vec::new();
        for child in children {
            if !asked.iter().any(|(key, _)| *key == child) {
                asked.push((child, None));
            }
        }
        Closing { children: asked }
    }

    /// Takes `child`'s answer. Returns the result when that answer is the last one waited for, and `None` otherwise, so a parent
    /// acts on the result once.
    pub fn hear(&mut self, child: K, answer: CloseAnswer) -> Option<CloseAnswer> {
        let (_, heard) = self.children.iter_mut().find(|(key, _)| *key == child)?;
        if heard.is_some() {
            return None;
        }
        *heard = Some(answer);
        self.answer()
    }

    /// The result, once every child has answered. With no children to ask, that is at once.
    pub fn answer(&self) -> Option<CloseAnswer> {
        let mut answer = CloseAnswer::Closed;
        for (_, heard) in &self.children {
            if (*heard)? == CloseAnswer::StayingOpen {
                answer = CloseAnswer::StayingOpen;
            }
        }
        Some(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_answers_once_every_child_has_answered() {
        let mut closing = Closing::asking(0..2);
        assert_eq!(closing.answer(), None);
        assert_eq!(closing.hear(1, CloseAnswer::Closed), None);
        assert_eq!(closing.hear(0, CloseAnswer::Closed), Some(CloseAnswer::Closed));
    }

    #[test]
    fn one_child_staying_open_keeps_the_window_but_the_rest_are_still_heard() {
        let mut closing = Closing::asking(0..2);
        assert_eq!(closing.hear(0, CloseAnswer::StayingOpen), None);
        assert_eq!(closing.hear(1, CloseAnswer::Closed), Some(CloseAnswer::StayingOpen));
    }

    #[test]
    fn with_no_children_there_is_an_answer_at_once() {
        assert_eq!(Closing::<usize>::asking([]).answer(), Some(CloseAnswer::Closed));
    }

    #[test]
    fn a_child_that_answers_twice_is_heard_once() {
        let mut closing = Closing::asking(0..2);
        assert_eq!(closing.hear(0, CloseAnswer::Closed), None);
        assert_eq!(closing.hear(0, CloseAnswer::StayingOpen), None, "the second answer doesn't stand in for the other child");
        assert_eq!(closing.answer(), None);
        assert_eq!(closing.hear(1, CloseAnswer::Closed), Some(CloseAnswer::Closed), "nor does it change the first");
    }

    #[test]
    fn the_result_comes_once() {
        let mut closing = Closing::asking([0]);
        assert_eq!(closing.hear(0, CloseAnswer::Closed), Some(CloseAnswer::Closed));
        assert_eq!(closing.hear(0, CloseAnswer::Closed), None, "a repeat doesn't close the window twice");
        assert_eq!(closing.answer(), Some(CloseAnswer::Closed));
    }

    #[test]
    fn a_child_not_asked_changes_nothing() {
        let mut closing = Closing::asking(["editor"]);
        assert_eq!(closing.hear("viewer", CloseAnswer::StayingOpen), None);
        assert_eq!(closing.hear("editor", CloseAnswer::Closed), Some(CloseAnswer::Closed));
    }

    #[test]
    fn a_child_named_twice_is_asked_once() {
        let mut closing = Closing::asking([0, 0]);
        assert_eq!(closing.hear(0, CloseAnswer::Closed), Some(CloseAnswer::Closed));
    }
}
