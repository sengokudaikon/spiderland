//! State an app saves of its stores, each under a key, and gives back to the stores it builds the next time.

use crate::{Feature, Store};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// What an app saves of its stores, each under a key of its own, in a form it picks, such as text or a JSON value. The frame
/// encodes nothing: each store says how to turn its state into a `V`, and the app turns a `V` back into a state.
///
/// Each store that saves is added with [`Store::save_in`]. When the app says, such as when its window is asked to close,
/// [`Saved::save`] reads every store still alive at that moment and returns what each says. The app keeps that where it likes. On
/// the next start it makes a `Saved` of it, and each store [`take`](Saved::take)s its own part as it is built.
///
/// So each store saves its own part, and a parent that rebuilds its [`Children`](crate::Children) by key needs to know nothing of
/// what each child saved. A child's build takes the child's part by its key.
///
/// A store saves for as long as it is alive, so a page closed or a child dropped has nothing to restore, which suits most apps.
/// What was given back and never taken is not saved again.
///
/// ```
/// use spiderland::{Effects, Feature, Host, Intents, Never, Saved, Store};
///
/// /// A search box, which the next start shows again.
/// struct Search;
/// impl Feature for Search {
///     type State = String;
///     type Intent = String;
///     type Effect = Never;
///     type Output = Never;
///     type Subscription = Never;
///
///     fn reduce(query: &mut String, typed: String, _: &mut Effects<Never>) {
///         *query = typed;
///     }
/// }
/// # struct Quiet;
/// # impl Host<Search> for Quiet {
/// #     fn perform(&mut self, effect: Never, _: &Intents<String>) {
/// #         match effect {}
/// #     }
/// # }
///
/// // The first run: nothing saved yet.
/// let saved = Saved::default();
/// let search = Store::<Search>::new(saved.take("search").unwrap_or_default(), Quiet);
/// search.save_in(&saved, "search", String::clone);
/// search.send("notes".to_owned());
/// let kept = saved.save();
/// assert_eq!(kept, [("search".to_owned(), "notes".to_owned())]);
///
/// // The next run, from what was kept.
/// let saved = Saved::new(kept);
/// let search = Store::<Search>::new(saved.take("search").unwrap_or_default(), Quiet);
/// assert_eq!(search.try_read(String::clone).unwrap(), "notes");
/// ```
pub struct Saved<V>(Rc<RefCell<Kept<V>>>);

struct Kept<V> {
    /// What the app gave back, for each store to take once.
    given: Vec<(String, V)>,
    /// Each store that saves, by key, in the order they were added.
    savers: Vec<(String, Saver<V>)>,
}

/// Says what its store saves, or `None` once the store is gone.
type Saver<V> = Box<dyn Fn() -> Option<V>>;

impl<V> Default for Saved<V> {
    fn default() -> Self {
        Saved::new([])
    }
}

impl<V> Clone for Saved<V> {
    fn clone(&self) -> Self {
        Saved(Rc::clone(&self.0))
    }
}

impl<V> fmt::Debug for Saved<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kept = self.0.borrow();
        f.debug_struct("Saved")
            .field("given", &kept.given.iter().map(|(key, _)| key).collect::<Vec<_>>())
            .field("saving", &kept.savers.iter().map(|(key, _)| key).collect::<Vec<_>>())
            .finish()
    }
}

impl<V> Saved<V> {
    /// What an earlier run saved, such as the result of its [`Saved::save`], for the stores built now to take.
    pub fn new(given: impl IntoIterator<Item = (String, V)>) -> Self {
        Saved(Rc::new(RefCell::new(Kept {
            given: given.into_iter().collect(),
            savers: Vec::new(),
        })))
    }

    /// Takes what was given back under `key`, once: a second call for the same key gets `None`. A store takes its part as it is
    /// built, as the state it starts on.
    pub fn take(&self, key: &str) -> Option<V> {
        let mut kept = self.0.borrow_mut();
        let at = kept.given.iter().position(|(given, _)| given == key)?;
        Some(kept.given.remove(at).1)
    }

    /// Reads every store that saves and is still alive, and returns what each says under its key, in the order they were added.
    /// Call it from outside a reducer, such as from a host or a window's callback.
    ///
    /// # Panics
    ///
    /// If a store that saves is reducing at that moment, as its state can't be read then.
    pub fn save(&self) -> Vec<(String, V)> {
        // Each store is read with no borrow held here, so what it runs as it is read can add a store that saves.
        let savers = std::mem::take(&mut self.0.borrow_mut().savers);
        let mut saved = Vec::new();
        let mut alive = Vec::new();
        for (key, saver) in savers {
            if let Some(value) = saver() {
                saved.push((key.clone(), value));
                alive.push((key, saver));
            }
        }
        let mut kept = self.0.borrow_mut();
        let added = std::mem::replace(&mut kept.savers, alive);
        for (key, saver) in added {
            kept.add(key, saver);
        }
        saved
    }

    pub(crate) fn add(&self, key: String, saver: Saver<V>) {
        self.0.borrow_mut().add(key, saver);
    }
}

impl<V> Kept<V> {
    /// A key added again is the later store's from then on.
    fn add(&mut self, key: String, saver: Saver<V>) {
        self.savers.retain(|(kept, _)| *kept != key);
        self.savers.push((key, saver));
    }
}

/// What [`Store::save_in`] adds.
pub(crate) fn saver<F: Feature, V>(store: &Store<F>, encode: impl Fn(&F::State) -> V + 'static) -> Saver<V> {
    let store = store.downgrade();
    Box::new(move || {
        let store = store.upgrade()?;
        Some(store.try_read(&encode).expect("a store's state is saved while it reduces: save from outside a reducer"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Effects, Host, Intents, Never};

    struct Count;
    impl Feature for Count {
        type State = i32;
        type Intent = i32;
        type Effect = Never;
        type Output = Never;
        type Subscription = Never;

        fn reduce(count: &mut i32, by: i32, _: &mut Effects<Never>) {
            *count += by;
        }
    }

    struct Quiet;
    impl Host<Count> for Quiet {
        fn perform(&mut self, effect: Never, _: &Intents<i32>) {
            match effect {}
        }
    }

    fn count(saved: &Saved<i32>, key: &str) -> Store<Count> {
        let store = Store::new(saved.take(key).unwrap_or_default(), Quiet);
        store.save_in(saved, key, |count| *count);
        store
    }

    #[test]
    fn each_store_alive_says_what_it_saves_under_its_key() {
        let saved = Saved::default();
        let (first, second) = (count(&saved, "first"), count(&saved, "second"));
        first.send(1);
        second.send(2);
        assert_eq!(saved.save(), [("first".to_owned(), 1), ("second".to_owned(), 2)]);
        drop(first);
        assert_eq!(saved.save(), [("second".to_owned(), 2)], "a store dropped has nothing to save");
    }

    #[test]
    fn what_was_given_is_taken_once() {
        let saved = Saved::new([("count".to_owned(), 5)]);
        assert_eq!(saved.take("count"), Some(5));
        assert_eq!(saved.take("count"), None);
        assert_eq!(saved.take("other"), None);
    }

    #[test]
    fn what_was_given_and_never_taken_is_not_saved_again() {
        let saved = Saved::new([("closed".to_owned(), 3)]);
        let open = count(&saved, "open");
        open.send(1);
        assert_eq!(saved.save(), [("open".to_owned(), 1)]);
    }

    #[test]
    fn a_key_added_again_is_the_later_stores() {
        let saved = Saved::default();
        let earlier = count(&saved, "count");
        earlier.send(1);
        let later = count(&saved, "count");
        later.send(2);
        assert_eq!(saved.save(), [("count".to_owned(), 2)]);
        drop(later);
        assert!(saved.save().is_empty(), "the earlier store doesn't take the key back");
    }

    #[test]
    fn a_store_added_while_another_is_read_saves_from_then_on() {
        let saved = Saved::<i32>::default();
        let first = count(&saved, "first");
        let (adding, added) = (saved.clone(), RefCell::new(None));
        let store = first.clone();
        saved.add(
            "adder".to_owned(),
            Box::new(move || {
                added.borrow_mut().get_or_insert_with(|| count(&adding, "late"));
                store.try_read(|count| *count)
            }),
        );
        assert_eq!(saved.save().len(), 2);
        let keys: Vec<String> = saved.save().into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["first", "adder", "late"]);
    }
}
