//! One central metadata gate: Cargo is the package/version source of truth;
//! raw validates common identity and domain APIs own schemas/capabilities.
#![allow(clippy::unwrap_used)]
use krlyxd::{ContractMetadata, Coordinates, LengthUnit, PackageMetadata};
use serde_json::Value;
use std::process::Command;

#[test]
fn workspace_packages_and_exported_contracts_share_validated_producer_metadata() {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    for package in packages {
        let identity = PackageMetadata {
            name: package["name"].as_str().unwrap(),
            version: package["version"].as_str().unwrap(),
        };
        assert!(identity.validate().is_ok(), "{identity:?}");
    }
    let contracts: [ContractMetadata<'_>; 7] = [
        krly2d_api::metadata(),
        krlyvis_api::metadata(),
        krly3d_brep::planar::metadata(),
        krly3d_mesh::descriptor().metadata(),
        krly3d_mesh::operation::descriptor().metadata(),
        krly3d_view_api::wire::metadata(),
        krly3d_view_api::operation::metadata(),
    ];
    for contract in contracts {
        assert!(contract.validate().is_ok(), "{contract:?}");
        let package = packages.iter().find(|p| p["name"] == contract.package.name).unwrap();
        assert_eq!(package["version"], contract.package.version);
        assert_eq!(contract.coordinates, Coordinates::RIGHT_HANDED_Z_UP);
        assert_eq!(contract.length_unit, LengthUnit::Model);
    }
    let view = krly3d_view_api::wire::metadata();
    let descriptor = krly3d_view_api::wire::descriptor();
    assert_eq!(descriptor["package"], view.package.name);
    assert_eq!(descriptor["package_version"], view.package.version);
    assert_eq!(descriptor["engine_revision"], view.engine_revision);
    assert_eq!(descriptor["api"]["major"], view.api.major);
    assert_eq!(descriptor["protocol"]["minor"], view.protocol.minor);
}

#[test]
fn view_operation_descriptor_preserves_shared_metadata_and_has_its_own_version() {
    let metadata = krly3d_view_api::operation::metadata();
    let descriptor = krly3d_view_api::operation::descriptor();
    assert_eq!(descriptor["package"], metadata.package.name);
    assert_eq!(descriptor["package_version"], metadata.package.version);
    assert_eq!(descriptor["engine_revision"], metadata.engine_revision);
    assert_eq!(descriptor["api"]["major"], metadata.api.major);
    assert_eq!(descriptor["protocol"]["major"], metadata.protocol.major);
    assert_eq!(metadata.api.major, 2);
    assert_eq!(krly3d_view_api::wire::metadata().api.major, 1);
}
