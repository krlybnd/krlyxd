#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "krly-print-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_krly3d-print"));
        command
            .arg("--mesh")
            .arg(self.0.join("mesh.json"))
            .arg("--output")
            .arg(self.0.join("out.stl"))
            .arg("--report")
            .arg(self.0.join("report.json"));
        command
    }
    fn report(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.0.join("report.json")).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const TETRA: &str =
    r#"{"points":[[0,0,0],[1,0,0],[0,1,0],[0,0,1]],"triangles":[[0,2,1],[0,1,3],[0,3,2],[1,2,3]]}"#;

#[test]
fn produces_stl_and_explicit_unknown_report() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("mesh.json"), TETRA).unwrap();
    let result = fixture.command().args(["--units", "mm"]).output().unwrap();
    assert!(result.status.success());
    assert!(result.stdout.is_empty());
    assert_eq!(fs::metadata(fixture.0.join("out.stl")).unwrap().len(), 284);
    let report = fixture.report();
    assert_eq!(report["units"], "mm");
    assert_eq!(report["exported"], true);
    assert_eq!(report["print_ready"], false);
    assert_eq!(report["float32_roundtrip"]["status"], "passed");
    assert_eq!(report["self_intersections"]["status"], "passed");
    assert_eq!(report["self_intersections"]["exact"], false);
    assert_eq!(report["float32_roundtrip"]["self_intersections"]["status"], "passed");
    assert_eq!(report["error"], serde_json::Value::Null);
}

#[test]
fn rejects_missing_units_without_artifacts() {
    let fixture = Fixture::new();
    assert_eq!(fixture.command().output().unwrap().status.code(), Some(2));
    assert!(!fixture.0.join("out.stl").exists());
    assert!(!fixture.0.join("report.json").exists());
}

#[test]
fn invalid_topology_and_parse_errors_leave_reports_without_stl() {
    for input in [
        r#"{"points":[[0,0,0],[1,0,0],[0,1,0]],"triangles":[[0,1,2]]}"#,
        "not json",
        r#"{"points":[[1e400,0,0]],"triangles":[]}"#,
        r#"{"points":[[0,0,0]],"triangles":[[1,0,0]]}"#,
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("mesh.json"), input).unwrap();
        assert_eq!(fixture.command().args(["--units", "mm"]).output().unwrap().status.code(), Some(1));
        assert!(!fixture.0.join("out.stl").exists());
        let report = fixture.report();
        assert_eq!(report["exported"], false);
        assert_eq!(report["print_ready"], false);
        assert_eq!(report["units"], "mm");
        assert!(report["error"]["code"].is_string());
    }
}

#[test]
fn rounding_rejection_is_reported_separately_from_topology() {
    let fixture = Fixture::new();
    let mut mesh: serde_json::Value = serde_json::from_str(TETRA).unwrap();
    for point in mesh["points"].as_array_mut().unwrap() {
        point[0] = (point[0].as_f64().unwrap() + 1e8).into();
    }
    fs::write(fixture.0.join("mesh.json"), serde_json::to_vec(&mesh).unwrap()).unwrap();
    assert_eq!(fixture.command().args(["--units", "mm"]).output().unwrap().status.code(), Some(1));
    let report = fixture.report();
    assert_eq!(report["topology_valid"], true);
    assert_eq!(report["exported"], false);
    assert_eq!(report["float32_roundtrip"]["status"], "failed");
    assert_eq!(report["error"]["code"], "float32_roundtrip");
    assert!(!fixture.0.join("out.stl").exists());
}

#[test]
fn never_overwrites_existing_output() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("mesh.json"), TETRA).unwrap();
    fs::write(fixture.0.join("out.stl"), b"keep me").unwrap();
    assert_eq!(fixture.command().args(["--units", "mm"]).output().unwrap().status.code(), Some(1));
    assert_eq!(fs::read(fixture.0.join("out.stl")).unwrap(), b"keep me");
    assert_eq!(fixture.report()["exported"], false);
}

#[test]
fn closed_manifold_self_intersection_rejects_export_with_witness() {
    let fixture = Fixture::new();
    let mesh = serde_json::json!({
        "points": [[0.,0.,0.],[1.,0.,0.],[1.,1.,0.],[0.,1.,0.],[0.,0.,1.],[1.,0.,1.],[0.5,0.5,-0.5],[0.,1.,1.]],
        "triangles": [[0,2,1],[0,3,2],[4,5,6],[4,6,7],[0,1,5],[0,5,4],[1,2,6],[1,6,5],[2,3,7],[2,7,6],[3,0,4],[3,4,7]],
    });
    fs::write(fixture.0.join("mesh.json"), serde_json::to_vec(&mesh).unwrap()).unwrap();
    assert_eq!(fixture.command().args(["--units", "mm"]).output().unwrap().status.code(), Some(1));
    let report = fixture.report();
    assert_eq!(report["topology_valid"], true);
    assert_eq!(report["connected_components"], 1);
    assert_eq!(report["self_intersections"]["status"], "failed");
    assert!(report["self_intersections"]["first_pair"].is_array());
    assert_eq!(report["exported"], false);
    assert!(!fixture.0.join("out.stl").exists());
}
