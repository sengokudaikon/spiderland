//! Telling what changed without comparing it: Rust has no reactive properties, so state says when it was touched and a
//! render skips what depends only on untouched state.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

/// Names one state of a [`Tracked`] value. Versions come from one counter for the whole process, so a value put in another's
/// place never takes on a version a render has seen. Only a clone shares one, until either is written to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version(u64);

impl Version {
    fn next() -> Self {
        static LAST: AtomicU64 = AtomicU64::new(0);
        Version(LAST.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// A value whose version moves whenever it could have changed. Any `&mut` access counts, so a reducer can't change it and
/// forget to say so. Access that changes nothing still counts: at worst that redoes a render, and it never misses one.
#[derive(Clone, Debug)]
pub struct Tracked<T> {
    value: T,
    version: Version,
}

impl<T: Default> Default for Tracked<T> {
    fn default() -> Self {
        Tracked::new(T::default())
    }
}

impl<T> Tracked<T> {
    /// `value`, with a version no other value has had.
    pub fn new(value: T) -> Self {
        Tracked { value, version: Version::next() }
    }

    /// The version of what it holds now, for a [`Slice`] key.
    pub fn version(&self) -> Version {
        self.version
    }
}

impl<T: PartialEq> Tracked<T> {
    /// Replaces the value, moving the version only if it differs: for a snapshot that often arrives unchanged.
    pub fn set(&mut self, value: T) {
        if self.value != value {
            **self = value;
        }
    }
}

impl<T> Deref for Tracked<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Tracked<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.version = Version::next();
        &mut self.value
    }
}

/// One part of a render, redone only when its key changes. The key is usually the versions of what that part reads:
/// `(files.version(), filter.version())`. A key missing an input shows stale values, so keep it beside the function it
/// guards, which should take only those inputs. [`Slice::show`] makes the key from that function's inputs, so it can't miss one.
#[derive(Debug)]
pub struct Slice<K> {
    key: Option<K>,
}

impl<K> Default for Slice<K> {
    fn default() -> Self {
        Slice { key: None }
    }
}

impl<K: PartialEq> Slice<K> {
    /// Runs `apply` on the first call and whenever `key` differs from the last one. True when it ran.
    pub fn update(&mut self, key: K, apply: impl FnOnce()) -> bool {
        if self.key.as_ref() == Some(&key) {
            return false;
        }
        self.key = Some(key);
        apply();
        true
    }

    /// Makes a value from `inputs` and hands it to `apply`, on the first call and whenever their key differs from the last one.
    /// True when it ran.
    ///
    /// `make` is a plain `fn`, so it reads only what it is given, and the key is made from the same inputs. An input left out
    /// can't be read, so it can't be missing from the key. It can still read statics, such as the language shown, which is set
    /// once per process.
    ///
    /// ```
    /// use spiderland::{Slice, Tracked};
    ///
    /// fn summary(names: &Tracked<Vec<&str>>, limit: usize) -> String {
    ///     names.iter().take(limit).copied().collect::<Vec<_>>().join(", ")
    /// }
    ///
    /// let mut names = Tracked::new(vec!["a", "b", "c"]);
    /// let mut slice = Slice::default();
    /// let mut shown = String::new();
    /// slice.show((&names, 2), summary, |text| shown = text);
    /// assert_eq!(shown, "a, b");
    /// names.push("d");
    /// slice.show((&names, 3), summary, |text| shown = text);
    /// assert_eq!(shown, "a, b, c");
    /// ```
    ///
    /// A `make` that reads something it isn't given doesn't build, because a closure that captures can't be a `fn`:
    ///
    /// ```compile_fail,E0308
    /// # use spiderland::{Slice, Tracked};
    /// let names = Tracked::new(vec!["a", "b", "c"]);
    /// let limit = Tracked::new(2);
    /// let mut slice = Slice::default();
    /// slice.show(&names, |names| names.len().min(*limit), |_| {});
    /// ```
    pub fn show<I: Inputs<Key = K>, V>(&mut self, inputs: I, make: I::Make<V>, apply: impl FnOnce(V)) -> bool {
        let key = inputs.key();
        if self.key.as_ref() == Some(&key) {
            return false;
        }
        self.key = Some(key);
        apply(inputs.make(make));
        true
    }
}

/// One thing a [`Slice::show`] reads, and the part of its key that says when it changed. A [`Tracked`] gives its version, and a
/// plain `Copy` value, such as a flag or a count, gives itself.
///
/// A type that keeps its own count of changes can give that count. Implement it on a reference, so the function gets the whole
/// value:
///
/// ```
/// struct Session {
///     generation: u64,
///     ready: bool,
/// }
///
/// impl spiderland::Input for &Session {
///     type Key = u64;
///
///     fn key(&self) -> u64 {
///         self.generation
///     }
/// }
/// ```
///
/// A reference is always `Copy`, so a reference to a `PartialEq` type already gives itself, and this impl would clash with that
/// one. For such a type, implement it on a wrapper of your own.
pub trait Input {
    /// What changes whenever the input could have.
    type Key: PartialEq;

    /// The key of the input as it is now.
    fn key(&self) -> Self::Key;
}

// This and the impl below don't overlap only because `Tracked` isn't `PartialEq`. Don't derive it.
impl<T> Input for &Tracked<T> {
    type Key = Version;

    fn key(&self) -> Version {
        self.version
    }
}

impl<T: Copy + PartialEq> Input for T {
    type Key = T;

    fn key(&self) -> T {
        *self
    }
}

/// What a [`Slice::show`] reads: one [`Tracked`], or a tuple of up to six [`Input`]s. `Make` is the `fn` that takes them.
pub trait Inputs {
    /// The inputs' keys together.
    type Key: PartialEq;
    /// A plain `fn` that takes the inputs and makes a `V`.
    type Make<V>;

    /// The key of the inputs as they are now.
    fn key(&self) -> Self::Key;

    /// Hands the inputs to `make`.
    fn make<V>(self, make: Self::Make<V>) -> V;
}

impl<'a, T> Inputs for &'a Tracked<T> {
    type Key = Version;
    type Make<V> = fn(&'a Tracked<T>) -> V;

    fn key(&self) -> Version {
        self.version
    }

    fn make<V>(self, make: Self::Make<V>) -> V {
        make(self)
    }
}

/// One input of any kind. Its key is that input's own, not a tuple, like a lone [`Tracked`]'s, so a `Slice<Version>` takes
/// either.
impl<A: Input> Inputs for (A,) {
    type Key = A::Key;
    type Make<V> = fn(A) -> V;

    fn key(&self) -> A::Key {
        self.0.key()
    }

    fn make<V>(self, make: Self::Make<V>) -> V {
        make(self.0)
    }
}

macro_rules! inputs {
    ($($input:ident $index:tt),+) => {
        impl<$($input: Input),+> Inputs for ($($input,)+) {
            type Key = ($($input::Key,)+);
            type Make<V> = fn($($input),+) -> V;

            fn key(&self) -> Self::Key {
                ($(self.$index.key(),)+)
            }

            fn make<V>(self, make: Self::Make<V>) -> V {
                make($(self.$index),+)
            }
        }
    };
}

inputs!(A 0, B 1);
inputs!(A 0, B 1, C 2);
inputs!(A 0, B 1, C 2, D 3);
inputs!(A 0, B 1, C 2, D 3, E 4);
inputs!(A 0, B 1, C 2, D 3, E 4, F 5);

/// A [`Slice`] that makes a value before showing it, and so can check itself. Under the `testing` feature, whenever the key
/// says nothing changed it makes the value anyway and panics if it differs from what is showing: a key missing an input fails
/// there rather than showing stale values. Without the feature, the value is made only when the key changes.
///
/// Turn `testing` on from dev-dependencies, so that tests check and a normal build, debug or release, doesn't pay for it. The
/// feature then applies only when the tests of a package that lists it are built, so each package whose tests show such a
/// slice lists it.
#[derive(Debug)]
pub struct CheckedSlice<K, V> {
    key: Option<K>,
    #[cfg(feature = "testing")]
    shown: Option<V>,
    #[cfg(not(feature = "testing"))]
    shown: std::marker::PhantomData<V>,
}

impl<K, V> Default for CheckedSlice<K, V> {
    fn default() -> Self {
        CheckedSlice {
            key: None,
            #[cfg(feature = "testing")]
            shown: None,
            #[cfg(not(feature = "testing"))]
            shown: std::marker::PhantomData,
        }
    }
}

impl<K: PartialEq, V: PartialEq + Clone> CheckedSlice<K, V> {
    /// Makes the value and shows it, on the first call and whenever `key` differs from the last one. `name` says which slice
    /// a failed check is.
    pub fn update(&mut self, name: &str, key: K, make: impl FnOnce() -> V, show: impl FnOnce(V)) {
        if self.key.as_ref() == Some(&key) {
            #[cfg(feature = "testing")]
            assert!(
                self.shown.as_ref() == Some(&make()),
                "the {name} slice's key is missing an input: it says nothing changed, and its value did"
            );
            #[cfg(not(feature = "testing"))]
            let _ = (name, make);
            return;
        }
        self.key = Some(key);
        let value = make();
        #[cfg(feature = "testing")]
        {
            self.shown = Some(value.clone());
        }
        show(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checked_slice_shows_when_its_key_changes() {
        let mut slice = CheckedSlice::default();
        let mut shown = Vec::new();
        slice.update("test", 1, || "a", |value| shown.push(value));
        slice.update("test", 1, || "a", |value| shown.push(value));
        slice.update("test", 2, || "b", |value| shown.push(value));
        assert_eq!(shown, ["a", "b"]);
    }

    #[test]
    #[cfg(feature = "testing")]
    #[should_panic(expected = "key is missing an input")]
    fn a_checked_slice_catches_a_key_that_misses_an_input() {
        let mut slice = CheckedSlice::default();
        slice.update("test", 1, || "a", |_| {});
        slice.update("test", 1, || "b", |_| {});
    }

    #[test]
    fn a_write_moves_the_version_and_a_read_does_not() {
        let mut value = Tracked::new(1);
        let start = value.version();
        assert_eq!(*value, 1);
        assert_eq!(value.version(), start);
        *value = 2;
        assert_ne!(value.version(), start);
    }

    #[test]
    fn a_value_put_in_anothers_place_has_a_version_of_its_own() {
        let mut value = Tracked::new(1);
        let mut slice = Slice::default();
        let mut shown = 0;
        slice.update(value.version(), || shown = *value);
        value = Tracked::new(2);
        slice.update(value.version(), || shown = *value);
        assert_eq!(shown, 2);
        assert_ne!(Tracked::<i32>::default().version(), Tracked::<i32>::default().version());
    }

    #[test]
    fn a_clone_shares_its_version_until_either_is_written_to() {
        let value = Tracked::new(1);
        let mut clone = value.clone();
        assert_eq!(clone.version(), value.version());
        *clone = 2;
        assert_ne!(clone.version(), value.version());
    }

    #[test]
    fn setting_an_equal_value_leaves_the_version() {
        let mut value = Tracked::new(1);
        let start = value.version();
        value.set(1);
        assert_eq!(value.version(), start);
        value.set(2);
        assert_ne!(value.version(), start);
        assert_eq!(*value, 2);
    }

    #[test]
    fn a_slice_shows_again_when_any_input_changes() {
        fn total(values: &Tracked<Vec<i32>>, scale: i32) -> i32 {
            values.iter().sum::<i32>() * scale
        }
        let mut values = Tracked::new(vec![1, 2]);
        let mut slice = Slice::default();
        let mut shown = Vec::new();
        assert!(slice.show((&values, 1), total, |total| shown.push(total)));
        assert!(!slice.show((&values, 1), total, |total| shown.push(total)));
        assert!(slice.show((&values, 10), total, |total| shown.push(total)));
        values.push(3);
        assert!(slice.show((&values, 10), total, |total| shown.push(total)));
        assert_eq!(shown, [3, 30, 60]);
    }

    #[test]
    fn a_slice_of_one_input_keys_on_it_alone() {
        let mut values = Tracked::new(vec![1]);
        let mut slice: Slice<Version> = Slice::default();
        let mut shown = 0;
        slice.show(&values, |values| values.len(), |len| shown = len);
        values.push(2);
        slice.show(&values, |values| values.len(), |len| shown = len);
        assert_eq!(shown, 2);
        let mut count: Slice<u8> = Slice::default();
        assert!(count.show((4u8,), |count| count * 2, |_| {}));
        assert!(!count.show((4u8,), |count| count * 2, |_| {}));
    }

    #[test]
    fn a_slice_runs_first_and_then_only_when_its_key_changes() {
        let mut slice = Slice::default();
        let mut runs = 0;
        assert!(slice.update(1, || runs += 1));
        assert!(!slice.update(1, || runs += 1));
        assert!(slice.update(2, || runs += 1));
        assert_eq!(runs, 2);
    }
}
