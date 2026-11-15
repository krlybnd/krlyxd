//! Domain-neutral primitives shared by domain APIs and their engines.
//!
//! This crate owns no execution threads, transport, serialization or document policy.

pub mod clock;
pub mod control;
pub mod exact;
pub mod id;
pub mod math;
pub mod metadata;
pub mod quality;
pub mod runner_limits;

pub use clock::{Clock, FakeClock, SystemClock};
pub use control::{CallControl, CallStop};
pub use exact::Exactness;
pub use id::Id;
pub use metadata::{Axis, ContractMetadata, Coordinates, Handedness, PackageMetadata};
pub use quality::{Confidence, PositionalUncertainty};

/// Version of a public API or common call envelope, independent of package SemVer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

macro_rules! text_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(text: &str) -> Self {
                Self(text.to_owned())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

text_id!(RequestId, "Caller-supplied correlation identifier for a submission.");
text_id!(OperationId, "Identifier of an accepted operation.");
text_id!(ArtifactId, "Identifier retained across revisions of an artifact.");

/// A neutral machine-readable code and human-readable explanation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Id,
    pub message: String,
}

/// An explicit length quantity; validation and conversion belong to its consumer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Length {
    pub value: f64,
    pub unit: LengthUnit,
}

/// Model coordinates are unitless until a domain supplies their physical scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    Model,
    Metre,
}
