//! A one-shot command to a view.

/// A value and a serial that moves on each fire, even for the same value. It suits a view with properties and no events, such as
/// "focus this" or "play the entrance". It is plain data, so a feature can keep it in its state and fire it from `reduce`.
///
/// A host sets the value first and the serial after it, and the view acts when the serial changes. Slint's side:
///
/// ```slint
/// in property <string> focus-target;
/// in property <int> focus-pulse;
/// changed focus-pulse => { /* act on focus-target */ }
/// ```
///
/// A view hears the latest fire, not each one: two fires with no turn of the event loop between them can arrive as one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pulse<T = ()> {
    value: T,
    serial: i32,
}

impl<T> Pulse<T> {
    /// A pulse that hasn't fired, holding `value` until it does.
    pub fn new(value: T) -> Pulse<T> {
        Pulse { value, serial: 0 }
    }

    /// Sets the value and moves the serial.
    pub fn fire(&mut self, value: T) {
        self.value = value;
        self.serial = self.serial.wrapping_add(1);
    }

    /// The value of the latest fire, or the one given to [`Pulse::new`] before any.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// For a view's `int` property. It starts at 0, so a view's `changed` doesn't run before the first fire.
    pub fn serial(&self) -> i32 {
        self.serial
    }
}

/// What one view has shown of a [`Pulse`]. [`Fired::serial`] gives the view a serial of its own, which starts at 0 the first time
/// the view shows the pulse and moves each time the pulse fires after that. So a view that attaches to a store after a pulse fired,
/// such as a second window on a document that showed "saved", doesn't act on that fire.
///
/// A view keeps one for each pulse it shows, and sets the view's property from it in place of [`Pulse::serial`]:
///
/// ```
/// use spiderland::{Fired, Pulse};
///
/// let mut saved = Pulse::new("saved");
/// saved.fire("saved");
///
/// // A window that attaches now hasn't seen anything fire.
/// let mut shown = Fired::default();
/// assert_eq!(shown.serial(&saved), 0);
/// saved.fire("saved");
/// assert_eq!(shown.serial(&saved), 1);
/// assert_eq!(shown.serial(&saved), 1, "nothing fired since");
/// ```
///
/// A view built with its store, from the first state, can use either: it sees every fire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fired {
    /// The pulse's serial when the view last showed it.
    last: Option<i32>,
    /// The serial the view was given.
    shown: i32,
}

impl Fired {
    /// The serial for this view: 0 the first time, then one more each time the pulse has moved since the last call. Fires with no
    /// call between them count as one, as they do for a view.
    pub fn serial<T>(&mut self, pulse: &Pulse<T>) -> i32 {
        match self.last {
            None => self.last = Some(pulse.serial()),
            Some(last) if last != pulse.serial() => {
                self.last = Some(pulse.serial());
                self.shown = self.shown.wrapping_add(1);
            }
            Some(_) => {}
        }
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_fire_moves_the_serial_even_for_the_same_value() {
        let mut pulse = Pulse::new("a");
        assert_eq!((*pulse.value(), pulse.serial()), ("a", 0));
        pulse.fire("a");
        assert_eq!((*pulse.value(), pulse.serial()), ("a", 1));
        pulse.fire("b");
        assert_eq!((*pulse.value(), pulse.serial()), ("b", 2));
    }

    #[test]
    fn a_view_counts_from_when_it_first_shows_the_pulse_even_if_the_state_is_rebuilt() {
        let mut pulse = Pulse::new(());
        pulse.fire(());
        pulse.fire(());
        let mut fired = Fired::default();
        assert_eq!(fired.serial(&pulse), 0);
        pulse.fire(());
        pulse.fire(());
        assert_eq!(fired.serial(&pulse), 1, "two fires between renders are one");
        let rebuilt = Pulse::new(());
        assert_eq!(fired.serial(&rebuilt), 2, "a pulse whose serial went back still counts as a move, not a step back");
    }

    #[test]
    fn the_serial_still_moves_when_it_wraps() {
        let mut pulse = Pulse::<()> { value: (), serial: i32::MAX };
        pulse.fire(());
        assert_eq!(pulse.serial(), i32::MIN);
    }
}
