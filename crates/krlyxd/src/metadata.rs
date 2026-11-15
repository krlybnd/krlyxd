//! Small, transport-neutral package and coordinate metadata. Schemas, operation
//! semantics and source provenance remain owned by their domains.
use crate::{ApiVersion, Diagnostic, Id, LengthUnit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handedness {
    Right,
    Left,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// Convention metadata, not a transformation or an instruction to convert data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coordinates {
    pub handedness: Handedness,
    pub up: Axis,
}
impl Coordinates {
    pub const RIGHT_HANDED_Z_UP: Self = Self { handedness: Handedness::Right, up: Axis::Z };
}

/// Producer identity can be cited in provenance without inventing a portable
/// content hash. Cargo validates package SemVer; this neutral record validates
/// nonempty, whitespace-free identifiers only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageMetadata<'a> {
    pub name: &'a str,
    pub version: &'a str,
}
impl PackageMetadata<'_> {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(invalid("package name must be a nonempty lowercase ASCII identifier"));
        }
        token(self.version, "package version")
    }
}

/// Common structural portion of a public domain descriptor. No tool schema,
/// scheduler flags or geometry types belong in the raw package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractMetadata<'a> {
    pub package: PackageMetadata<'a>,
    pub protocol: ApiVersion,
    pub api: ApiVersion,
    pub engine_revision: &'a str,
    pub coordinates: Coordinates,
    pub length_unit: LengthUnit,
}
impl ContractMetadata<'_> {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        self.package.validate()?;
        token(self.engine_revision, "engine revision")
    }
}
fn token(value: &str, name: &str) -> Result<(), Diagnostic> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Err(invalid(format!("{name} must be a nonempty token")))
    } else {
        Ok(())
    }
}
fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new("metadata"), message: message.into() }
}
