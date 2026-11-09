//! A child feature kept inside its parent's state, numbered each time it starts, so an answer meant for an earlier one is let go.

use crate::Feature;

/// A child feature's state inside its parent's, numbered each time it starts. It is plain data, kept in the parent's state, such as
/// for an export that the user can stop and start again.
///
/// An intent for the child carries the [`Instance`] it was sent for, and [`Slot::get_mut`] gives the child's state only while that
/// one is still there. So an answer from work that couldn't be stopped, or from a sender the child's host kept, doesn't reach a
/// child started after it. Key the child's subscriptions on the instance too, with [`Slot::subscriptions`]: stopped and started
/// again in one batch, the child then gets its work afresh, where a key that stayed the same would keep the old work running.
///
/// [`Latest`](crate::Latest) doesn't do this: it counts only the first answer to a request, and a child hears many.
///
/// ```
/// use spiderland::Slot;
///
/// let mut export = Slot::default();
/// let (first, _) = export.start(0);
/// *export.get_mut(first).unwrap() += 1;
/// export.stop();
/// let (second, _) = export.start(0);
/// assert!(export.get_mut(first).is_none(), "an answer for the first doesn't reach the second");
/// assert_eq!(export.get_mut(second).copied(), Some(0));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot<S> {
    child: Option<(Instance, S)>,
    /// How many children have started here.
    started: u64,
}

/// Which start of a [`Slot`]'s child an intent, an effect or a subscription is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Instance(u64);

impl<S> Default for Slot<S> {
    fn default() -> Self {
        Slot { child: None, started: 0 }
    }
}

impl<S> Slot<S> {
    /// Starts a child on `state`, in place of any there, and numbers it. Returns its number and its state, such as to run
    /// [`Effects::start_child`](crate::Effects::start_child) on, with the number in each effect it wraps.
    pub fn start(&mut self, state: S) -> (Instance, &mut S) {
        self.started += 1;
        let (instance, state) = self.child.insert((Instance(self.started), state));
        (*instance, state)
    }

    /// Stops the child, if there is one, and returns its state. What it runs stops with it, and what is sent for it is let go.
    pub fn stop(&mut self) -> Option<S> {
        self.child.take().map(|(_, state)| state)
    }

    /// The child's state, while `instance` is the child here. An intent for a child stopped, or one before it, gets `None`.
    pub fn get_mut(&mut self, instance: Instance) -> Option<&mut S> {
        self.child.as_mut().filter(|(now, _)| *now == instance).map(|(_, state)| state)
    }

    /// The child's state, such as for `present` to show.
    pub fn get(&self) -> Option<&S> {
        self.child.as_ref().map(|(_, state)| state)
    }

    /// The number of the child here, if any.
    pub fn instance(&self) -> Option<Instance> {
        self.child.as_ref().map(|(instance, _)| *instance)
    }

    /// True while a child is here.
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// What the child's state wants to run, each with its number, for the parent's [`Feature::subscriptions`] to wrap. A child
    /// started again gets keys of its own, so the store stops the old work and starts the new.
    pub fn subscriptions<C: Feature<State = S>>(&self) -> impl Iterator<Item = (Instance, C::Subscription)> + '_ {
        self.child.iter().flat_map(|(instance, state)| {
            let instance = *instance;
            C::subscriptions(state).into_iter().map(move |key| (instance, key))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Effects, Never};

    struct Ticking;
    impl Feature for Ticking {
        type State = u32;
        type Intent = u32;
        type Effect = Never;
        type Output = Never;
        type Subscription = &'static str;

        fn reduce(ticks: &mut u32, by: u32, _: &mut Effects<Never>) {
            *ticks += by;
        }

        fn subscriptions(_: &u32) -> impl IntoIterator<Item = &'static str> {
            ["tick"]
        }
    }

    #[test]
    fn each_start_has_a_number_of_its_own() {
        let mut slot = Slot::default();
        assert_eq!((slot.instance(), slot.is_running()), (None, false));
        let (first, _) = slot.start(1);
        assert_eq!(slot.stop(), Some(1));
        assert!(slot.get_mut(first).is_none(), "nothing is here");
        let (second, _) = slot.start(2);
        assert_ne!(first, second);
        assert!(slot.get_mut(first).is_none(), "the first is gone");
        assert_eq!(slot.get_mut(second).copied(), Some(2));
        let (third, _) = slot.start(3);
        assert!(slot.get_mut(second).is_none(), "a start in place of one there replaces it");
        assert_eq!((slot.get().copied(), slot.instance()), (Some(3), Some(third)));
    }

    #[test]
    fn a_child_started_again_wants_its_work_under_a_key_of_its_own() {
        let mut slot = Slot::default();
        let (first, _) = slot.start(0);
        let before: Vec<_> = slot.subscriptions::<Ticking>().collect();
        assert_eq!(before, [(first, "tick")]);
        slot.stop();
        assert_eq!(slot.subscriptions::<Ticking>().count(), 0);
        let (second, _) = slot.start(0);
        let after: Vec<_> = slot.subscriptions::<Ticking>().collect();
        assert_eq!(after, [(second, "tick")]);
        assert_ne!(before, after);
    }
}
