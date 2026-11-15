//! Serial native sheet adapter. One deadline covers admission, all parts and
//! callback publication. The CLI owns signals and interruptible pipe output;
//! ordinary filesystem operations may still block inside the kernel.

use super::options::Options;
use super::output::{self, diag, save};
use super::stream::{Delivery, Journal};
use krly3d_reconstruction::sheet::{PartSpec, PartStage, PartStatus, Sheet};
use krlyxd::{CallControl, CallStop, Diagnostic, Id};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(super) fn run(
    options: Options,
    started: Instant,
    control: Arc<CallControl>,
    stdout: impl Write,
) -> Result<(), String> {
    let sheet_options = options.sheet.as_ref().ok_or("missing sheet options")?;
    check(&control).map_err(diag)?;
    let parts = super::parts::read(&sheet_options.parts, options.settings.tolerance, &control)?;
    check(&control).map_err(diag)?;
    let call = options.read_call(&control)?;
    check(&control).map_err(diag)?;
    // The caller's actual bytes are captured once before the call is consumed.
    // Do not reread its path after admission: it may have changed meanwhile.
    let input_name = format!("input.{}", call.input.image.format);
    let input_bytes = call.input.image.data.clone();
    let mut sheet =
        Sheet::new(call, parts, options.settings.tolerance, Arc::clone(&control)).map_err(diag)?;
    check(&control).map_err(diag)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    let sheet_id = format!("local-sheet-{}-{stamp}", std::process::id());
    // create_dir is the admission gate, including existing files and symlinks.
    std::fs::create_dir(&options.output).map_err(|error| error.to_string())?;
    let mut manifest = Manifest::new(&sheet, &sheet_id, &input_name, options.settings.tolerance, started);
    manifest.persist(&options.output).map_err(diag)?;
    let file = match OpenOptions::new().write(true).create_new(true).open(options.output.join(output::EVENTS))
    {
        Ok(file) => file,
        Err(error) => {
            return Err(diag(manifest.fail(&sheet, &options.output, io_error(error.to_string()))));
        }
    };
    let mut journal = Journal::sheet(file, stdout, sheet_id, started);
    let outcome = (|| {
        check(&control)?;
        save(&options.output, &input_name, input_bytes).map_err(io_error)?;
        let specs: Vec<_> = sheet.records().iter().map(|record| spec_json(record.spec())).collect();
        save(&options.output, "parts.json", json!(specs).to_string() + "\n").map_err(io_error)?;
        std::fs::create_dir(options.output.join("parts")).map_err(|error| io_error(error.to_string()))?;
        check(&control)?;
        journal
            .emit("started", json!({"execution":"in_process","part_count":sheet.records().len()}))
            .map_err(io_error)?;
        advance(&mut sheet, &control, &options.output, &mut manifest, &mut journal)
    })();
    // Preserve a stop diagnostic when an interruptible writer observed the token.
    // Other I/O errors (e.g. a disconnected reader) remain output failures.
    let outcome = outcome.map_err(|failure| check(&control).err().unwrap_or(failure));
    finish(outcome, &sheet, &options.output, &mut manifest, &mut journal)
}

fn advance<J: Write, O: Write>(
    sheet: &mut Sheet,
    control: &CallControl,
    directory: &Path,
    manifest: &mut Manifest,
    journal: &mut Journal<J, O>,
) -> Result<(), Diagnostic> {
    loop {
        check(control)?;
        let next = sheet.records().iter().position(|record| record.status() == PartStatus::Pending);
        // Persist running before advance: extraction may fail before Evidence.
        if let Some(index) = next {
            manifest.value["parts"][index]["status"] = json!("running");
            manifest.value["parts"][index]["stage"] = json!("evidence");
            manifest.persist(directory)?;
        }
        let completed = sheet.advance(|id, stage| {
            check(control)?;
            let index = manifest.index(id)?;
            let relative = format!("parts/{id}");
            let part_directory = directory.join(&relative);
            let artifact = |name: &str| format!("{relative}/{name}");
            let artifact_id = format!("{}:{id}:model", manifest.value["sheet_id"].as_str().unwrap_or_default());
            let kind = match &stage {
                PartStage::Evidence(_) => "evidence",
                PartStage::Coarse(_) => "coarse_preview",
                PartStage::Editable(_) => "editable_proposal",
                PartStage::Preview(_) => "preview",
                PartStage::Validation(_) => "validation",
            };
            manifest.value["parts"][index]["status"] = json!("running");
            manifest.value["parts"][index]["stage"] = json!(kind);
            manifest.persist(directory)?;
            let entry = &mut manifest.value["parts"][index];
            let mut event = match stage {
                PartStage::Evidence(evidence) => {
                    std::fs::create_dir(&part_directory).map_err(|error| io_error(error.to_string()))?;
                    let result = evidence.to_result(&format!("sheet:{id}"))?;
                    output::save_evidence(&part_directory, &result).map_err(io_error)?;
                    entry["artifacts"]["evidence"] = json!(artifact(output::EVIDENCE));
                    entry["provenance"] = json!(evidence.provenance());
                    json!({"artifact":artifact(output::EVIDENCE),"image_sha256":evidence.provenance().image_sha256})
                }
                PartStage::Coarse(coarse) => {
                    save(&part_directory, output::COARSE_MESH, krly3d_mesh::codec::encode_result(coarse.mesh())?).map_err(io_error)?;
                    entry["artifacts"]["coarse_mesh"] = json!(artifact(output::COARSE_MESH));
                    json!({"artifact":artifact(output::COARSE_MESH),"artifact_id":artifact_id,
                        "artifact_revision":1,"approximation":"bounding_prism","verified":false})
                }
                PartStage::Editable(result) => {
                    save(&part_directory, output::MODEL, result.model().to_k3d()?).map_err(io_error)?;
                    entry["artifacts"]["model"] = json!(artifact(output::MODEL));
                    save(&part_directory, output::MESH, krly3d_mesh::codec::encode_result(result.mesh())?).map_err(io_error)?;
                    entry["artifacts"]["mesh"] = json!(artifact(output::MESH));
                    entry["result"] = output::reconstruction_metadata(result);
                    entry["volume"] = entry["result"]["volume"].clone();
                    entry["expected_volume"] = entry["result"]["expected_volume"].clone();
                    json!({"artifact":artifact(output::MODEL),"mesh":artifact(output::MESH),"artifact_id":artifact_id,
                        "artifact_revision":2,"replaces_revision":1,"verified":false})
                }
                PartStage::Preview(preview) => {
                    save(&part_directory, output::PREVIEW, &preview.png).map_err(io_error)?;
                    entry["artifacts"]["preview"] = json!(artifact(output::PREVIEW));
                    let event = output::preview_event(preview, &artifact(output::PREVIEW));
                    entry["preview_provenance"] = event["provenance"].clone();
                    event
                }
                PartStage::Validation(likeness) => {
                    save(&part_directory, output::LIKENESS, krly3d_view_api::likeness::encode_result(likeness)?).map_err(io_error)?;
                    entry["artifacts"]["likeness"] = json!(artifact(output::LIKENESS));
                    entry["matches"] = json!(likeness.matches);
                    output::validation_event(likeness, &artifact(output::LIKENESS))
                }
            };
            event["kind"] = json!(kind);
            manifest.persist(directory)?;
            check(control)?;
            journal.emit("part_event", json!({"part_id":id,"event":event})).map_err(io_error)?;
            check(control)
        })?;
        manifest.refresh(sheet);
        manifest.persist(directory)?;
        if !completed {
            break;
        }
        let index = next.ok_or_else(|| error("stage", "runtime completed an unexpected part"))?;
        let entry = &manifest.value["parts"][index];
        if entry["status"] != "completed" {
            return Err(error("stage", "runtime did not retain a completed part"));
        }
        check(control)?;
        journal
            .emit("part_completed", json!({"part_id":entry["id"],"result":entry["result"]}))
            .map_err(io_error)?;
        check(control)?;
    }
    if !sheet.is_complete() {
        return Err(error("stage", "runtime stopped before completing the sheet"));
    }
    check(control)?;
    manifest.value["status"] = json!("completed");
    manifest.persist(directory)?;
    check(control)?;
    // Do not perform a fallible checkpoint after committing a terminal attempt.
    journal.emit("completed", json!({"manifest":manifest.value})).map_err(io_error)
}

/// Delivery failure changes the retained manifest to failed, but never retries
/// either journal writer or appends a conflicting terminal event.
fn finish<J: Write, O: Write>(
    outcome: Result<(), Diagnostic>,
    sheet: &Sheet,
    directory: &Path,
    manifest: &mut Manifest,
    journal: &mut Journal<J, O>,
) -> Result<(), String> {
    outcome.map_err(|failure| {
        let failure = manifest.fail(sheet, directory, failure);
        if journal.delivery == Delivery::Open {
            let kind = if failure.code.as_str() == "cancelled" { "cancelled" } else { "failed" };
            let _ = journal.emit(kind, json!({"message":diag(failure.clone()),"error":diagnostic_json(&failure),"manifest":manifest.value}));
        }
        diag(failure)
    })
}

struct Manifest {
    value: Value,
    started: Instant,
    writable: bool,
}

impl Manifest {
    fn new(sheet: &Sheet, sheet_id: &str, input: &str, tolerance: f64, started: Instant) -> Self {
        let parts: Vec<_> = sheet
            .records()
            .iter()
            .map(|record| {
                let spec = record.spec();
                json!({"id":spec.id,"seed":spec.seed,"height":spec.height,
                "status":status(record.status()),"stage":null,"output":format!("parts/{}",spec.id),
                "artifacts":{},"result":null,"provenance":null,"volume":null,"expected_volume":null,
                "matches":null,"error":null,"assumptions":output::assumptions(true)})
            })
            .collect();
        let mut manifest = Self {
            value: json!({"protocol":output::SHEET_PROTOCOL,"sheet_id":sheet_id,"status":"running",
            "execution":"in_process","provenance":sheet.provenance(),
            "image_sha256":sheet.provenance().image_sha256,"source":sheet.provenance().source,
            "pixel_unit":sheet.provenance().pixel_unit,"tolerance":tolerance,
            "assumptions":output::assumptions(true),"parts":parts,"error":null,
            "artifacts":{"events":output::EVENTS,"input":input,"parts":"parts.json"},
            "replay":{"image":input,"parts":"parts.json","pixel_unit":sheet.provenance().pixel_unit,"tolerance":tolerance},
            "elapsed_ms":0.0}),
            started,
            writable: true,
        };
        // Preserve the legacy exact-mode manifest, but record every explicit
        // positive setting needed to replay this sheet at its original scale.
        if sheet.provenance().simplify > 0.0 {
            manifest.value["replay"]["outline_tolerance"] = json!(sheet.provenance().simplify);
        }
        manifest
    }

    fn index(&self, id: &str) -> Result<usize, Diagnostic> {
        self.value["parts"]
            .as_array()
            .and_then(|parts| parts.iter().position(|part| part["id"] == id))
            .ok_or_else(|| error("stage", "runtime reported an unknown part id"))
    }

    fn refresh(&mut self, sheet: &Sheet) {
        for (index, record) in sheet.records().iter().enumerate() {
            let entry = &mut self.value["parts"][index];
            entry["status"] = json!(status(record.status()));
            if let Some(failure) = record.error() {
                entry["error"] = diagnostic_json(failure);
            }
            if let Some(result) = record.result() {
                entry["result"] = output::reconstruction_metadata(result);
                entry["result"]["artifacts"] = entry["artifacts"].clone();
                entry["result"]["provenance"] = json!(result.evidence().provenance());
                entry["result"]["matches"] =
                    json!(result.review().likeness().and_then(|likeness| likeness.matches));
                entry["volume"] = entry["result"]["volume"].clone();
                entry["expected_volume"] = entry["result"]["expected_volume"].clone();
                entry["matches"] = entry["result"]["matches"].clone();
            }
        }
    }

    fn persist(&mut self, directory: &Path) -> Result<(), Diagnostic> {
        if !self.writable {
            return Err(io_error("manifest output is closed"));
        }
        self.value["elapsed_ms"] = json!(self.started.elapsed().as_secs_f64() * 1000.0);
        output::replace_manifest(directory, &self.value).map_err(|message| {
            self.writable = false;
            io_error(message)
        })
    }

    fn fail(&mut self, sheet: &Sheet, directory: &Path, mut failure: Diagnostic) -> Diagnostic {
        self.refresh(sheet);
        self.value["status"] =
            json!(if failure.code.as_str() == "cancelled" { "cancelled" } else { "failed" });
        self.value["error"] = diagnostic_json(&failure);
        if self.writable {
            if let Err(error) = self.persist(directory) {
                failure.message.push_str(&format!("; could not retain failure manifest: {}", diag(error)));
            }
        }
        failure
    }
}

fn status(status: PartStatus) -> &'static str {
    match status {
        PartStatus::Pending => "pending",
        PartStatus::Running => "running",
        PartStatus::Completed => "completed",
        PartStatus::Failed => "failed",
        PartStatus::Cancelled => "cancelled",
    }
}

fn spec_json(spec: &PartSpec) -> Value {
    json!({"id":spec.id,"seed":spec.seed,"height":spec.height})
}

fn diagnostic_json(error: &Diagnostic) -> Value {
    json!({"code":error.code.as_str(),"message":error.message})
}

fn error(code: &str, message: &str) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}

fn io_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new("io"), message: message.into() }
}

fn check(control: &CallControl) -> Result<(), Diagnostic> {
    control.check().map_err(|reason| match reason {
        CallStop::Cancelled => error("cancelled", "sheet cancelled"),
        CallStop::DeadlineExceeded => error("deadline_exceeded", "sheet deadline exceeded"),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{advance, finish, Manifest};
    use crate::output;
    use crate::stream::Journal;
    use krly3d_reconstruction::sheet::{PartSpec, Sheet};
    use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
    use krlyxd::CallControl;
    use serde_json::Value;
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let directory =
                Self(std::env::temp_dir().join(format!("krly3d-sheet-cli-{}-{stamp}", std::process::id())));
            std::fs::create_dir(&directory.0).unwrap();
            std::fs::create_dir(directory.0.join("parts")).unwrap();
            directory
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sheet(control: Arc<CallControl>, seeds: &[[f64; 2]]) -> Sheet {
        sheet_with_tolerance(control, seeds, 0.0)
    }

    fn sheet_with_tolerance(control: Arc<CallControl>, seeds: &[[f64; 2]], simplify: f64) -> Sheet {
        let mut data = b"P5 16 16 255\n".to_vec();
        for y in 0..16 {
            for x in 0..16 {
                data.push(if (3..13).contains(&x) && (3..13).contains(&y) { 0 } else { 255 });
            }
        }
        let call = DrawingCall {
            protocol: PROTOCOL.into(),
            request_id: "sheet-test".into(),
            input: DrawingInput {
                image: DrawingImage { format: "pnm".into(), data },
                pixel_unit: 0.5,
                simplify,
                seed: None,
                source: "sheet-test".into(),
            },
        };
        let parts = seeds
            .iter()
            .enumerate()
            .map(|(index, seed)| PartSpec { id: format!("P{index}"), seed: *seed, height: 3.0 })
            .collect();
        Sheet::new(call, parts, 0.05, control).unwrap()
    }

    #[test]
    fn sheet_manifest_retains_positive_outline_tolerance_for_replay() {
        for tolerance in [0.0, 1.0] {
            let sheet = sheet_with_tolerance(Arc::new(CallControl::new()), &[[5.0, 5.0]], tolerance);
            let mut manifest = Manifest::new(&sheet, "outline-replay", "input.pnm", 0.05, Instant::now());
            let directory = Directory::new();
            manifest.persist(&directory.0).unwrap();
            let saved: Value =
                serde_json::from_slice(&std::fs::read(directory.0.join(output::MANIFEST)).unwrap()).unwrap();
            assert_eq!(saved["provenance"]["simplify"], tolerance);
            assert_eq!(saved["replay"]["pixel_unit"], 0.5);
            assert_eq!(saved["replay"]["tolerance"], 0.05);
            if tolerance > 0.0 {
                assert_eq!(saved["replay"]["outline_tolerance"], tolerance);
            } else {
                assert!(saved["replay"].get("outline_tolerance").is_none());
            }
        }
    }

    #[derive(Default)]
    struct TerminalFailure {
        bytes: Vec<u8>,
        fail_on_write: bool,
        enabled: bool,
        pending: bool,
        failed: bool,
        calls_after_failure: usize,
    }

    impl Write for TerminalFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.failed {
                self.calls_after_failure += 1;
                return Err(io::Error::other("write after failure"));
            }
            if self.pending && self.fail_on_write {
                self.failed = true;
                return Err(io::Error::other("injected completed write failure"));
            }
            if self.enabled
                && serde_json::from_slice::<Value>(bytes).is_ok_and(|event| event["kind"] == "completed")
            {
                self.pending = true;
                if self.fail_on_write {
                    let accepted = bytes.len() / 2;
                    self.bytes.extend_from_slice(&bytes[..accepted]);
                    return Ok(accepted);
                }
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.failed {
                self.calls_after_failure += 1;
            }
            if self.pending || self.failed {
                self.failed = true;
                return Err(io::Error::other("injected completed flush failure"));
            }
            Ok(())
        }
    }

    #[test]
    fn completed_delivery_failure_retains_failed_manifest_without_conflicting_terminal() {
        for fail_journal in [false, true] {
            for fail_on_write in [false, true] {
                completion_failure(fail_journal, fail_on_write);
            }
        }
    }

    fn completion_failure(fail_journal: bool, fail_on_write: bool) {
        let directory = Directory::new();
        let control = Arc::new(CallControl::new());
        let mut sheet = sheet(Arc::clone(&control), &[[4.0, 4.0]]);
        let started = Instant::now();
        let mut manifest = Manifest::new(&sheet, "test", "input.pnm", 0.05, started);
        manifest.persist(&directory.0).unwrap();
        let broken = TerminalFailure { enabled: true, fail_on_write, ..TerminalFailure::default() };
        let (file, stdout) = if fail_journal {
            (broken, TerminalFailure::default())
        } else {
            (TerminalFailure::default(), broken)
        };
        let mut journal = Journal::sheet(file, stdout, "test".into(), started);
        let outcome = advance(&mut sheet, &control, &directory.0, &mut manifest, &mut journal);
        assert!(finish(outcome, &sheet, &directory.0, &mut manifest, &mut journal)
            .unwrap_err()
            .contains("injected"));
        let saved: Value =
            serde_json::from_slice(&std::fs::read(directory.0.join(output::MANIFEST)).unwrap()).unwrap();
        assert_eq!(saved["status"], "failed");
        assert_eq!(saved["parts"][0]["status"], "completed");
        assert!(directory.0.join("parts/P0/proposal.k3d").is_file());
        let (file, stdout) = journal.into_writers();
        assert!(if fail_journal { file.failed } else { stdout.failed });
        for writer in [&file, &stdout] {
            assert_eq!(writer.calls_after_failure, 0);
            // Search raw bytes too: a contradictory partial terminal is forbidden.
            assert!(!String::from_utf8_lossy(&writer.bytes).contains("\"kind\":\"failed\""));
            let events: Vec<Value> = writer
                .bytes
                .split_inclusive(|byte| *byte == b'\n')
                .filter(|line| line.ends_with(b"\n"))
                .map(|line| serde_json::from_slice(line).unwrap())
                .collect();
            assert_eq!(events.iter().filter(|event| event["kind"] == "part_completed").count(), 1);
            assert!(events.iter().filter(|event| event["kind"] == "completed").count() <= 1);
        }
        if fail_journal {
            assert!(!String::from_utf8_lossy(&stdout.bytes).contains("\"kind\":\"completed\""));
        }
    }

    #[test]
    fn duplicate_component_preserves_completed_and_pending_parts() {
        let directory = Directory::new();
        let control = Arc::new(CallControl::new());
        let mut sheet = sheet(Arc::clone(&control), &[[4.0, 4.0], [5.0, 5.0], [6.0, 6.0]]);
        let started = Instant::now();
        let mut manifest = Manifest::new(&sheet, "test", "input.pnm", 0.05, started);
        manifest.persist(&directory.0).unwrap();
        let mut journal = Journal::sheet(Vec::new(), Vec::new(), "test".into(), started);
        let outcome = advance(&mut sheet, &control, &directory.0, &mut manifest, &mut journal);
        assert!(finish(outcome, &sheet, &directory.0, &mut manifest, &mut journal)
            .unwrap_err()
            .contains("duplicate_component"));
        let saved: Value =
            serde_json::from_slice(&std::fs::read(directory.0.join(output::MANIFEST)).unwrap()).unwrap();
        assert_eq!(saved["status"], "failed");
        assert_eq!(saved["parts"][0]["status"], "completed");
        assert_eq!(saved["parts"][1]["status"], "failed");
        assert_eq!(saved["parts"][1]["error"]["code"], "duplicate_component");
        assert_eq!(saved["parts"][2]["status"], "pending");
        assert!(!directory.0.join("parts/P1").exists());
        assert!(!directory.0.join("parts/P2").exists());
        let (file, stdout) = journal.into_writers();
        assert_eq!(file, stdout);
        let events: Vec<Value> = file
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(events.last().unwrap()["kind"], "failed");
        assert!(events.iter().all(|event| event["kind"] != "completed"));
        assert!(events
            .iter()
            .filter(|event| event["kind"] == "part_event")
            .all(|event| event["part_id"] == "P0"));
    }
}
