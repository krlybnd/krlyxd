#![allow(clippy::unwrap_used)]
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "krly3d-field-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn input() -> Value {
    json!({"version":1,"units":"mm","bounds":{"min":[-2,-2,-2],"max":[2,2,2]},"cell_size":0.2,
        "nodes":[{"type":"ellipsoid","center":[0,0,0],"radii":[1,1,1]}],"root":0})
}
#[test]
fn cli_emits_bounded_ordered_records_and_tuple_mesh_file() {
    let dir = Directory::new();
    let path = dir.0.join("recipe.json");
    fs::write(&path, input().to_string()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_krly3d-field"))
        .arg("--recipe")
        .arg(path)
        .arg("--output")
        .arg(dir.0.join("output"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).unwrap();
    assert!(stdout.lines().all(|line| line.len() < 1024));
    let records: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["stage"], "sampling");
    assert_eq!(records[1]["stage"], "mesh");
    assert_eq!(records[2]["stage"], "completed");
    assert_eq!(records[2]["units"], "mm");
    assert_eq!(records[2]["exactness"], "approximate");
    let mesh: Value = serde_json::from_slice(&fs::read(dir.0.join("output/mesh.json")).unwrap()).unwrap();
    assert_eq!(mesh.as_object().unwrap().len(), 2);
    assert_eq!(mesh["points"].as_array().unwrap().len() as u64, records[2]["points"].as_u64().unwrap());
    assert_eq!(mesh["triangles"].as_array().unwrap().len() as u64, records[2]["triangles"].as_u64().unwrap());
    assert!(mesh["points"].as_array().unwrap().iter().all(|v| v.as_array().unwrap().len() == 3));
    assert!(mesh["triangles"].as_array().unwrap().iter().all(|v| v.as_array().unwrap().len() == 3));
}
#[test]
fn cropped_recipe_exits_nonzero_without_completed_or_partial_file() {
    let dir = Directory::new();
    let path = dir.0.join("recipe.json");
    let mut value = input();
    value["nodes"][0]["radii"] = json!([3, 3, 3]);
    fs::write(&path, value.to_string()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_krly3d-field"))
        .arg("--recipe")
        .arg(path)
        .arg("--output")
        .arg(dir.0.join("output"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!String::from_utf8(result.stdout).unwrap().contains("completed"));
    let error: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(error["code"], "boundary");
    assert!(!dir.0.join("output/mesh.json").exists());
}
#[test]
fn oversized_input_and_duplicate_arguments_are_rejected() {
    let dir = Directory::new();
    let path = dir.0.join("recipe.json");
    fs::write(&path, vec![b' '; 262145]).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_krly3d-field"))
        .arg("--recipe")
        .arg(&path)
        .arg("--output")
        .arg(&dir.0)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let error: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(error["code"], "limit");
    let result = Command::new(env!("CARGO_BIN_EXE_krly3d-field"))
        .args(["--recipe", "one", "--recipe", "two", "--output", "out"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}
