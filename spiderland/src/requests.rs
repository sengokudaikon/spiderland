//! Plain data for a reducer that has work in flight: which answer still counts, and what waits its turn.

/// Numbers requests so that only the answer to the latest one counts. It is plain data, kept in the state, such as for a search
/// whose answers can come back out of order. The reducer asks, puts the [`Asked`] in the effect, and checks the answer that
/// comes back with it.
///
/// A load kept as a subscription, keyed on its query, needs none of this: the store drops the old load as the query changes.
/// This is for work that can't be dropped, such as [`Background::run`](crate::Background::run).
///
/// ```
/// use spiderland::Latest;
///
/// let mut search = Latest::default();
/// let first = search.ask();
/// let second = search.ask();
/// assert!(search.is_waiting());
/// assert!(!search.answer(first), "an older answer that comes back late doesn't count");
/// assert!(search.answer(second));
/// assert!(!search.answer(second), "nor does a second answer to the same request");
/// assert!(!search.is_waiting());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Latest {
    asked: u64,
    answered: u64,
}

/// One request numbered by [`Latest::ask`], to send with the work and back with its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Asked(u64);

impl Latest {
    /// Numbers a new request. Every request asked before it no longer counts.
    pub fn ask(&mut self) -> Asked {
        self.asked += 1;
        Asked(self.asked)
    }

    /// Takes the answer to `asked`. True when it counts: it is the latest request and the first answer to it.
    pub fn answer(&mut self, asked: Asked) -> bool {
        let counts = asked.0 == self.asked && self.answered != self.asked;
        if counts {
            self.answered = asked.0;
        }
        counts
    }

    /// True while the latest request has no answer, such as to show that a search is under way.
    pub fn is_waiting(&self) -> bool {
        self.answered != self.asked
    }
}

/// Work that runs one at a time, such as the saves of one document. While one runs, the latest one asked for waits, in place of
/// any that waited before it. It is plain data, kept in the state.
///
/// ```
/// use spiderland::OneAtATime;
///
/// let mut saves = OneAtATime::default();
/// assert_eq!(saves.ask("first"), Some("first"), "nothing runs, so it starts now");
/// assert_eq!(saves.ask("second"), None, "it waits");
/// assert_eq!(saves.ask("third"), None, "and takes the place of what waited");
/// assert_eq!(saves.done(), Some("third"), "the latest starts once the first is done");
/// assert_eq!(saves.done(), None);
/// assert!(!saves.is_running());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneAtATime<T> {
    running: bool,
    waiting: Option<T>,
}

impl<T> Default for OneAtATime<T> {
    fn default() -> Self {
        OneAtATime { running: false, waiting: None }
    }
}

impl<T> OneAtATime<T> {
    /// Asks for `work`. Returns it, to start now, if nothing runs. Otherwise it waits, and whatever waited before is dropped.
    pub fn ask(&mut self, work: T) -> Option<T> {
        if self.running {
            self.waiting = Some(work);
            None
        } else {
            self.running = true;
            Some(work)
        }
    }

    /// Says the running work is done, whether or not it went well. Returns what waited, to start now.
    pub fn done(&mut self) -> Option<T> {
        let next = self.waiting.take();
        self.running = next.is_some();
        next
    }

    /// True while work runs.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// What waits for the running work to be done.
    pub fn waiting(&self) -> Option<&T> {
        self.waiting.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_to_an_old_request_does_not_count_even_before_the_latest_comes() {
        let mut latest = Latest::default();
        let old = latest.ask();
        let new = latest.ask();
        assert!(!latest.answer(old));
        assert!(latest.is_waiting(), "the latest still waits");
        assert!(latest.answer(new));
    }

    #[test]
    fn nothing_is_waited_for_before_anything_is_asked() {
        assert!(!Latest::default().is_waiting());
    }

    #[test]
    fn work_asked_for_while_nothing_runs_starts_at_once_each_time() {
        let mut work = OneAtATime::default();
        assert_eq!(work.ask(1), Some(1));
        assert_eq!(work.done(), None);
        assert_eq!(work.ask(2), Some(2));
        assert_eq!(work.waiting(), None);
        assert!(work.is_running());
    }
}
