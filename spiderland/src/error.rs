//! Why background work came back without a result of its own.

use std::fmt;

/// Why work given to [`Background::run`](crate::Background::run) has no result of its own. The feature hears it as `Err`, and
/// decides what it means there: a fault in one feature's work is not automatically a fault in the app. It is a
/// [`std::error::Error`], so `?` carries it into an app's own error.
///
/// More reasons may come, so a `match` on it needs a `_` arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The work panicked. The panic goes on to the pool once the feature has been told, so the pool can keep a record of it.
    Panicked,
    /// The pool refused the work, so it never ran.
    Refused,
    /// The pool let go of the work before it ran.
    Dropped,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Panicked => "background work panicked",
            Error::Refused => "the pool refused background work",
            Error::Dropped => "the pool let go of background work before it ran",
        })
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_goes_into_an_apps_error_with_question_mark() {
        fn finish(result: Result<i32, Error>) -> Result<i32, Box<dyn std::error::Error>> {
            Ok(result?)
        }
        assert_eq!(finish(Err(Error::Panicked)).unwrap_err().to_string(), "background work panicked");
    }
}
