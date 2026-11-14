//! The outcome of asking one plugin to do something.

use crate::error::Error;

/// Three-way outcome that lets several plugins share one id.
///
/// `Declined` is not an error: it hands the request to the next plugin in the chain,
/// which is how a closed form can sit in front of a general method.
#[derive(Clone, Debug, PartialEq)]
pub enum Attempt<T> {
    Done(T),
    /// This plugin does not handle the request; the reason says why.
    Declined(String),
    /// This plugin handles such requests and this one went wrong; the chain stops.
    Failed(Error),
}

impl<T> Attempt<T> {
    pub fn declined(reason: impl Into<String>) -> Self {
        Self::Declined(reason.into())
    }

    pub fn map<U>(self, change: impl FnOnce(T) -> U) -> Attempt<U> {
        match self {
            Self::Done(value) => Attempt::Done(change(value)),
            Self::Declined(reason) => Attempt::Declined(reason),
            Self::Failed(error) => Attempt::Failed(error),
        }
    }

    /// Collapses to a `Result` at the point where no further plugin can be asked:
    /// a request nobody took is reported as unsupported.
    pub fn into_result(self) -> Result<T, Error> {
        match self {
            Self::Done(value) => Ok(value),
            Self::Declined(reason) => Err(Error::unsupported(reason)),
            Self::Failed(error) => Err(error),
        }
    }
}

impl<T> From<Result<T, Error>> for Attempt<T> {
    fn from(result: Result<T, Error>) -> Self {
        match result {
            Ok(value) => Self::Done(value),
            Err(error) => Self::Failed(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Attempt;
    use crate::error::{Error, ErrorKind};

    #[test]
    fn a_declined_attempt_becomes_an_unsupported_error() {
        let attempt: Attempt<u8> = Attempt::declined("tangent contact");

        let error = attempt.into_result().unwrap_err();

        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert_eq!(error.message, "tangent contact");
    }

    #[test]
    fn a_failed_attempt_keeps_its_error() {
        let attempt: Attempt<u8> = Attempt::Failed(Error::geometry("zero radius"));

        assert_eq!(attempt.into_result(), Err(Error::geometry("zero radius")));
    }

    #[test]
    fn map_changes_only_a_done_value() {
        let done = Attempt::Done(2).map(|n| n * 10);
        let declined: Attempt<i32> = Attempt::<i32>::declined("not mine").map(|n| n * 10);

        assert_eq!(done, Attempt::Done(20));
        assert_eq!(declined, Attempt::declined("not mine"));
    }
}
