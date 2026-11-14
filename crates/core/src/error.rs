//! The one error type of the kernel.

use std::fmt;

/// What went wrong, coarse enough for a caller to act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request itself is wrong: a missing or out-of-range parameter.
    Input,
    /// Nothing registered can do what was asked.
    Unsupported,
    /// A node, face, edge or plugin named in the request does not exist.
    NotFound,
    /// The mathematics has no answer for these numbers.
    Geometry,
    /// The result would not be a valid body.
    Topology,
    /// Text or bytes could not be read as the expected format.
    Format,
}

impl ErrorKind {
    /// The kind as a word, for messages and for clients that branch on it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Unsupported => "unsupported",
            Self::NotFound => "not found",
            Self::Geometry => "geometry",
            Self::Topology => "topology",
            Self::Format => "format",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub fn input(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Input, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub fn geometry(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Geometry, message)
    }

    pub fn topology(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Topology, message)
    }

    pub fn format(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Format, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.label(), self.message)
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn display_leads_with_the_kind() {
        let error = Error::not_found("no face named `front`");

        assert_eq!(error.to_string(), "not found: no face named `front`");
    }
}
