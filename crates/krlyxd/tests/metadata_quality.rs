#![allow(clippy::unwrap_used)]
use krlyxd::{
    ApiVersion, Axis, Confidence, ContractMetadata, Coordinates, Handedness, Length, LengthUnit,
    PackageMetadata, PositionalUncertainty,
};

#[test]
fn unknown_and_measured_zero_are_distinct_without_implicit_unit_conversion() {
    let unknown = PositionalUncertainty::unknown();
    assert_eq!(unknown, PositionalUncertainty::default());
    assert_eq!(unknown.value(), None);
    for unit in [LengthUnit::Model, LengthUnit::Metre] {
        let length = Length { value: 0., unit };
        let measured = PositionalUncertainty::measured(length).unwrap();
        assert_eq!(measured.value(), Some(length));
        assert_ne!(unknown, measured);
    }
    assert!(PositionalUncertainty::measured(Length { value: f64::MAX, unit: LengthUnit::Model }).is_ok());
}

#[test]
fn quality_rejects_nonfinite_negative_and_out_of_range_evidence() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01] {
        assert!(Confidence::new(value).is_err());
        assert!(PositionalUncertainty::measured(Length { value, unit: LengthUnit::Metre }).is_err());
    }
    assert!(Confidence::new(1.000001).is_err());
    for value in [0., 0.5, 1.] {
        assert_eq!(Confidence::new(value).unwrap().value(), value);
    }
}

#[test]
fn metadata_validation_is_structural_not_a_domain_or_semver_registry() {
    let valid = ContractMetadata {
        package: PackageMetadata { name: "example-domain", version: "1.2.3-rc.1+build.2" },
        protocol: ApiVersion { major: 2, minor: 0 },
        api: ApiVersion { major: 1, minor: 3 },
        engine_revision: "example/4",
        coordinates: Coordinates { handedness: Handedness::Left, up: Axis::Y },
        length_unit: LengthUnit::Metre,
    };
    assert!(valid.validate().is_ok());
    for name in ["", "bad name", "../package", "MixedCase", "árvíz"] {
        assert!(ContractMetadata { package: PackageMetadata { name, ..valid.package }, ..valid }
            .validate()
            .is_err());
    }
    for version in ["", "1.0\n", "1.0 0", "\0"] {
        assert!(PackageMetadata { version, ..valid.package }.validate().is_err());
    }
    for engine_revision in ["", "engine revision", "engine\n"] {
        assert!(ContractMetadata { engine_revision, ..valid }.validate().is_err());
    }
    assert_eq!(Coordinates::RIGHT_HANDED_Z_UP, Coordinates { handedness: Handedness::Right, up: Axis::Z });
}
