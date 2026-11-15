use krly3d_mesh::{diagnostic, Diagnostic};
use krly3d_mesh_exchange::{binary_stl, decode_mesh, report, Units};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const USAGE: &str = "krly3d-print --mesh PATH --output file.stl --report file.json --units mm";
const INPUT_LIMIT: u64 = 128 * 1024 * 1024;

struct Args {
    mesh: PathBuf,
    output: PathBuf,
    report: PathBuf,
}

fn main() {
    let args = match arguments() {
        Ok(Some(args)) => args,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(message) => {
            eprintln!("{message}\nUsage: {USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(error) = run(&args) {
        eprintln!("{}: {}", error.code.as_str(), error.message);
        std::process::exit(1);
    }
}

fn arguments() -> Result<Option<Args>, String> {
    let values: Vec<_> = std::env::args_os().skip(1).collect();
    if values.len() == 1 && values[0] == "--help" {
        return Ok(None);
    }
    let mut mesh = None;
    let mut output = None;
    let mut report = None;
    let mut units = None;
    let mut iter = values.into_iter();
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or_else(|| "each flag needs a value".to_string())?;
        let target = match flag.to_str() {
            Some("--mesh") => &mut mesh,
            Some("--output") => &mut output,
            Some("--report") => &mut report,
            Some("--units") => &mut units,
            _ => return Err(format!("unknown flag {}", flag.to_string_lossy())),
        };
        if target.replace(value).is_some() {
            return Err("duplicate flag".into());
        }
    }
    if units.as_deref() != Some(std::ffi::OsStr::new("mm")) {
        return Err("--units mm is required; coordinates must already be millimetres".into());
    }
    Ok(Some(Args {
        mesh: mesh.ok_or("--mesh is required")?.into(),
        output: output.ok_or("--output is required")?.into(),
        report: report.ok_or("--report is required")?.into(),
    }))
}

fn io_error(error: impl std::fmt::Display) -> Diagnostic {
    diagnostic("io", error.to_string())
}

fn run(args: &Args) -> Result<(), Diagnostic> {
    // Never overwrite source, an earlier artifact, a symlink, or a report. Reserve
    // the report first, so no STL is published if its report cannot be created.
    let mut report_file =
        OpenOptions::new().write(true).create_new(true).open(&args.report).map_err(io_error)?;
    let mut json = serde_json::json!({
        "format": krly3d_mesh::print::REPORT_FORMAT,
        "units": "mm", "exported": false, "print_ready": false,
        "self_intersections": {"status": "unknown", "reason": "not implemented"},
        "wall_thickness": {"status": "unknown", "reason": "not implemented"},
        "float32_roundtrip": {"status": "not_run"}, "error": null,
    });
    let result = execute(args, &mut json);
    if let Err(error) = &result {
        json["error"] = serde_json::json!({"code": error.code.as_str(), "message": error.message});
    }
    let bytes = serde_json::to_vec_pretty(&json).map_err(io_error)?;
    if let Err(error) = report_file.write_all(&bytes).and_then(|()| report_file.sync_all()) {
        if result.is_ok() {
            let _ = fs::remove_file(&args.output);
        }
        return Err(io_error(error));
    }
    result
}

fn execute(args: &Args, json: &mut serde_json::Value) -> Result<(), Diagnostic> {
    let input = read_bounded(&args.mesh)?;
    let mesh = decode_mesh(&input)?;
    let preflight = report(&mesh)?;
    *json = preflight.to_json();
    json["units"] = "mm".into();
    json["volume_units"] = "mm_cubed".into();
    json["exported"] = false.into();
    json["float32_roundtrip"] = serde_json::json!({"status": "not_run"});
    json["error"] = serde_json::Value::Null;
    preflight.require_exportable()?;
    let export = binary_stl(&mesh, Units::Millimetres).inspect_err(|_| {
        json["float32_roundtrip"] = serde_json::json!({"status": "failed"});
    })?;
    json["float32_roundtrip"] = serde_json::json!({
        "status": "passed",
        "checks": ["finite_coordinates", "no_vertex_collapses", "closed_manifold_topology", "positive_volume", "preserved_face_orientation", "numerical_self_intersections"],
        "self_intersections": export.roundtrip.self_intersections.to_json(),
    });
    let mut output = OpenOptions::new().write(true).create_new(true).open(&args.output).map_err(io_error)?;
    if let Err(error) = output.write_all(&export.bytes).and_then(|()| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(&args.output);
        return Err(io_error(error));
    }
    json["exported"] = true.into();
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, Diagnostic> {
    let file = fs::File::open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.len() > INPUT_LIMIT {
        return Err(diagnostic("resource_limit", "mesh input must be a regular file of at most 128 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(INPUT_LIMIT + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err(diagnostic("resource_limit", "mesh input exceeded 128 MiB"));
    }
    Ok(bytes)
}
