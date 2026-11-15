//! Measured quality primitives. Unknown evidence stays unknown; requested
//! tolerances are not constructors for achieved uncertainty.
use crate::{Diagnostic, Id, Length};

/// A finite confidence in [0, 1]. The domain must document what it measures;
/// a number alone does not establish calibration or a statistical probability.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Confidence(f64);
impl Confidence {
    pub fn new(value: f64) -> Result<Self, Diagnostic> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(invalid("confidence must be finite and between zero and one"))
        }
    }
    pub fn value(self) -> f64 {
        self.0
    }
}

/// Unknown is distinct from measured zero. The unit stays explicit; no model
/// scale or conversion is inferred by this primitive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PositionalUncertainty(Option<Length>);
impl PositionalUncertainty {
    pub fn unknown() -> Self {
        Self(None)
    }
    pub fn measured(value: Length) -> Result<Self, Diagnostic> {
        if value.value.is_finite() && value.value >= 0.0 {
            Ok(Self(Some(value)))
        } else {
            Err(invalid("positional uncertainty must be finite and nonnegative"))
        }
    }
    pub fn value(self) -> Option<Length> {
        self.0
    }
}
fn invalid(message: &str) -> Diagnostic {
    Diagnostic { code: Id::new("quality"), message: message.into() }
}
