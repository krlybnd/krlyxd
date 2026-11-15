//! File artifacts and the common manifest metadata for both wire versions.

use krly3d_reconstruction::{Event, EventKind, Reconstruction};
use serde_json::{json, Value};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::Instant;

pub(super) const STREAM_PROTOCOL: &str = "krly3d.shared-reconstruction/2";
pub(super) const SHEET_PROTOCOL: &str = "krly3d.shared-sheet/1";
pub(super) const EVIDENCE: &str = "evidence.json";
pub(super) const MODEL: &str = "proposal.k3d";
pub(super) const MESH: &str = "mesh.json";
pub(super) const EVENTS: &str = "events.jsonl";
pub(super) const MANIFEST: &str = "manifest.json";
pub(super) const COARSE_MESH: &str = "coarse-mesh.json";
pub(super) const PREVIEW: &str = "preview.png";
pub(super) const LIKENESS: &str = "likeness.json";

pub(super) enum ManifestMode<'a> {
    Single { computation_ms: f64 },
    Stream { flow_id: &'a str },
}

/// Domain metadata shared by all output envelopes, including each sheet part.
pub(super) fn reconstruction_metadata(result: &Reconstruction) -> Value {
    let evidence = result.evidence().provenance();
    let settings = result.settings();
    json!({
        "revision":result.revision(),
        "height":settings.height,"tolerance":settings.tolerance,"pixel_unit":evidence.pixel_unit,"seed":evidence.seed,
        "image_sha256":evidence.image_sha256,"source":evidence.source,
        "brep_source":result.model().snapshot().identity().source.as_str(),
        "volume":result.mesh().volume,"open":result.mesh().open,
        "expected_volume":result.evidence().material_area()*settings.height,
        "confidence":null,"positional_uncertainty":null
    })
}

pub(super) fn assumptions(review: bool) -> Vec<&'static str> {
    let mut assumptions = vec![
        "Orthographic top view",
        "Caller-supplied scale and uniform height",
        "Visible holes are through holes",
        "Pixel contour is not verified physical geometry",
    ];
    if review {
        assumptions.push("Likeness compares sampled outer extents; it does not certify holes or topology");
    }
    assumptions
}

pub(super) fn preview_event(preview: &krly3d_view_api::RenderResult, artifact: &str) -> Value {
    let identity = &preview.provenance.source;
    json!({"artifact":artifact,
        "provenance":{"source":{"source":identity.source.as_str(),"session":identity.session.as_str(),
            "node":identity.node,"build_key":identity.build_key},"engine_revision":preview.provenance.engine_revision.as_str()},
        "width":preview.width,"height":preview.height})
}

pub(super) fn validation_event(
    likeness: &krly3d_view_api::likeness::LikenessResult,
    artifact: &str,
) -> Value {
    json!({"artifact":artifact,"matches":likeness.matches,
        "scope":"sampled_outer_extents","physical_accuracy_verified":false})
}

pub(super) fn manifest(result: &Reconstruction, started: Instant, mode: ManifestMode<'_>) -> Value {
    let mut value = reconstruction_metadata(result);
    let mut artifacts = json!({"evidence":EVIDENCE,"model":MODEL,"mesh":MESH,"events":EVENTS});
    let protocol = match mode {
        ManifestMode::Single { .. } => "krly3d.shared-reconstruction/1",
        ManifestMode::Stream { .. } => {
            artifacts["coarse_mesh"] = json!(COARSE_MESH);
            artifacts["preview"] = json!(PREVIEW);
            artifacts["likeness"] = json!(LIKENESS);
            STREAM_PROTOCOL
        }
    };
    value["protocol"] = json!(protocol);
    value["execution"] = json!("in_process");
    value["assumptions"] = json!(assumptions(matches!(mode, ManifestMode::Stream { .. })));
    value["artifacts"] = artifacts;
    value["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    match mode {
        ManifestMode::Single { computation_ms } => value["computation_ms"] = json!(computation_ms),
        ManifestMode::Stream { flow_id } => value["flow_id"] = json!(flow_id),
    }
    value
}

/// Legacy mode serializes only after native composition and emits one manifest.
pub(super) fn export(
    result: &Reconstruction,
    events: &[Event],
    output: &Path,
    started: Instant,
    computation_ms: f64,
    stdout: &mut impl Write,
    control: &krlyxd::CallControl,
) -> Result<(), String> {
    super::lifecycle::check(control)?;
    let evidence = result.evidence().to_result("local-drawing").map_err(diag)?;
    let native = result.model().to_k3d().map_err(diag)?;
    let mesh = krly3d_mesh::codec::encode_result(result.mesh()).map_err(diag)?;
    let events = events.iter().map(|event| {
        let kind = match event.kind {
            EventKind::Invalidated(_) => "invalidated",
            EventKind::Committed => "committed",
            EventKind::CoarsePreview => "coarse_preview",
        };
        json!({"protocol":"krly3d.shared-host-events/1", "seq":event.sequence,"revision":event.revision,"kind":kind,
            "height":event.settings.height,"tolerance":event.settings.tolerance}).to_string()
    }).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::create_dir(output).map_err(|e| e.to_string())?;
    super::lifecycle::check(control)?;
    save_evidence(output, &evidence)?;
    super::lifecycle::check(control)?;
    save(output, MODEL, native)?;
    super::lifecycle::check(control)?;
    save(output, MESH, mesh)?;
    super::lifecycle::check(control)?;
    save(output, EVENTS, events)?;
    super::lifecycle::check(control)?;
    let manifest = manifest(result, started, ManifestMode::Single { computation_ms });
    let outcome = save_manifest(output, &manifest).and_then(|()| {
        stdout.write_all((manifest.to_string() + "\n").as_bytes()).map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())
    });
    outcome.map_err(|message| discard_manifest(output, message))
}

pub(super) fn save(directory: &Path, name: &str, value: impl AsRef<[u8]>) -> Result<(), String> {
    std::fs::write(directory.join(name), value).map_err(|e| e.to_string())
}

pub(super) fn save_evidence(directory: &Path, evidence: &krlyvis_api::DrawingResult) -> Result<(), String> {
    save(directory, EVIDENCE, krlyvis_api::encode_result(evidence).map_err(diag)?)
}

pub(super) fn save_manifest(directory: &Path, manifest: &Value) -> Result<(), String> {
    save(directory, MANIFEST, manifest.to_string() + "\n")
}

/// Replace a retained sheet manifest only after its complete replacement has
/// reached the same filesystem. The old manifest survives a failed write/rename.
pub(super) fn replace_manifest(directory: &Path, manifest: &Value) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = directory.join(format!(
        ".manifest-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let outcome = (|| {
        file.write_all((manifest.to_string() + "\n").as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, directory.join(MANIFEST))
    })();
    if let Err(error) = outcome {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(())
}

/// Keep diagnostic artifacts, but retract a partial or undelivered success marker.
/// Removal is best effort; report its failure without hiding the original error.
pub(super) fn discard_manifest(directory: &Path, message: String) -> String {
    match std::fs::remove_file(directory.join(MANIFEST)) {
        Ok(()) => message,
        Err(error) if error.kind() == ErrorKind::NotFound => message,
        Err(error) => format!("{message}; could not remove {MANIFEST}: {error}"),
    }
}

pub(super) fn diag(error: krlyvis_api::Diagnostic) -> String {
    format!("{}: {}", error.code, error.message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{replace_manifest, MANIFEST};
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!(
                "krly3d-atomic-manifest-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn entries(&self) -> Vec<String> {
            std::fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomic_manifest_replaces_previous_status_without_temporary_files() {
        let directory = Directory::new();
        for status in ["running", "completed", "failed"] {
            let manifest = json!({"status":status,"parts":[{"id":"A","status":"completed"}]});
            replace_manifest(&directory.0, &manifest).unwrap();
            let actual: Value =
                serde_json::from_slice(&std::fs::read(directory.0.join(MANIFEST)).unwrap()).unwrap();
            assert_eq!(actual, manifest);
            assert_eq!(directory.entries(), [MANIFEST]);
        }
    }

    #[test]
    fn failed_rename_preserves_target_and_cleans_temporary_file() {
        let directory = Directory::new();
        let target = directory.0.join(MANIFEST);
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"preserved").unwrap();
        assert!(replace_manifest(&directory.0, &json!({"status":"failed"})).is_err());
        assert_eq!(std::fs::read(target.join("keep")).unwrap(), b"preserved");
        assert_eq!(directory.entries(), [MANIFEST]);
    }
}
