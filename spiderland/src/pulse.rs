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

    pub fn value(&self) -> &T {
        &self.value
    }

    /// For a view's `int` property. It starts at 0, so a view's `changed` doesn't run before the first fire.
    pub fn serial(&self) -> i32 {
        self.serial
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
    fn the_serial_still_moves_when_it_wraps() {
        let mut pulse = Pulse::<()> { value: (), serial: i32::MAX };
        pulse.fire(());
        assert_eq!(pulse.serial(), i32::MIN);
    }
}
