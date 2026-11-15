//! Opt-in staged output. Each event is flushed before the next native stage starts.
//!
//! A terminal event is attempted once. An I/O error stops both writers; recovery
//! never appends `failed` after an attempted `completed`, even if only the journal
//! received it. Failed delivery retracts the manifest best effort, retaining all
//! other artifacts. Journal/stdout delivery and file publication are not atomic.
use super::output::{self, diag, save, ManifestMode};
use krly3d_reconstruction::{Host, Settings};
use krlyvis_api::DrawingCall;
use krlyxd::CallControl;
use serde_json::{json, Value};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(PartialEq, Eq)]
pub(super) enum Delivery {
    Open,
    TerminalAttempted,
    IoFailed,
}

pub(super) struct Journal<J, O> {
    file: J,
    stdout: O,
    flow: String,
    protocol: &'static str,
    identity_key: &'static str,
    sequence: u64,
    started: Instant,
    previous: Instant,
    pub(super) delivery: Delivery,
}

impl<J: Write, O: Write> Journal<J, O> {
    #[cfg(test)]
    pub(super) fn into_writers(self) -> (J, O) {
        (self.file, self.stdout)
    }

    fn new(file: J, stdout: O, flow: String, started: Instant) -> Self {
        Self {
            file,
            stdout,
            flow,
            protocol: output::STREAM_PROTOCOL,
            identity_key: "flow_id",
            sequence: 0,
            started,
            previous: started,
            delivery: Delivery::Open,
        }
    }

    pub(super) fn sheet(file: J, stdout: O, sheet_id: String, started: Instant) -> Self {
        Self {
            protocol: output::SHEET_PROTOCOL,
            identity_key: "sheet_id",
            ..Self::new(file, stdout, sheet_id, started)
        }
    }

    pub(super) fn emit(&mut self, kind: &str, mut event: Value) -> Result<(), String> {
        if self.delivery != Delivery::Open {
            return Err("event output is closed".into());
        }
        // Commit to one terminal attempt before touching either writer. A flush
        // error can mean the complete record is already visible to a reader.
        if matches!(kind, "completed" | "failed" | "cancelled") {
            self.delivery = Delivery::TerminalAttempted;
        }
        let now = Instant::now();
        event["protocol"] = json!(self.protocol);
        event[self.identity_key] = json!(self.flow);
        event["seq"] = json!(self.sequence);
        event["revision"] = json!(1);
        event["kind"] = json!(kind);
        event["elapsed_ms"] = json!(now.duration_since(self.started).as_secs_f64() * 1000.0);
        event["stage_ms"] = json!(now.duration_since(self.previous).as_secs_f64() * 1000.0);
        let line = event.to_string() + "\n";
        let delivered = self
            .file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.flush())
            .and_then(|()| self.stdout.write_all(line.as_bytes()))
            .and_then(|()| self.stdout.flush());
        if let Err(error) = delivered {
            self.delivery = Delivery::IoFailed;
            return Err(error.to_string());
        }
        self.sequence += 1;
        self.previous = now;
        Ok(())
    }
}

pub(super) fn run(
    call: DrawingCall,
    settings: Settings,
    output: &Path,
    started: Instant,
    control: Arc<CallControl>,
    stdout: impl Write,
) -> Result<(), String> {
    // Reject invalid scalar inputs before reserving any output directory.
    call.validate().map_err(diag)?;
    settings.validate().map_err(diag)?;
    super::lifecycle::check(&control)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    std::fs::create_dir(output).map_err(|e| e.to_string())?;
    let mut journal = Journal::new(
        File::create(output.join(output::EVENTS)).map_err(|e| e.to_string())?,
        stdout,
        format!("local-{}-{stamp}", std::process::id()),
        started,
    );
    run_stages_controlled(call, settings, output, &mut journal, control)
}

#[cfg(test)]
fn run_stages<J: Write, O: Write>(
    call: DrawingCall,
    settings: Settings,
    output: &Path,
    journal: &mut Journal<J, O>,
) -> Result<(), String> {
    run_stages_controlled(call, settings, output, journal, Arc::new(CallControl::new()))
}

fn run_stages_controlled<J: Write, O: Write>(
    call: DrawingCall,
    settings: Settings,
    output: &Path,
    journal: &mut Journal<J, O>,
    control: Arc<CallControl>,
) -> Result<(), String> {
    let outcome = stages(call, settings, output, journal, Arc::clone(&control));
    outcome.map_err(|message| {
        let message = output::discard_manifest(output, message);
        // Computation/artifact errors can still be reported. A known output
        // failure must not cause further writes to either stream.
        if journal.delivery == Delivery::Open {
            let kind =
                if control.check() == Err(krlyxd::CallStop::Cancelled) { "cancelled" } else { "failed" };
            let _ = journal.emit(kind, json!({"message":message}));
        }
        message
    })
}

fn stages<J: Write, O: Write>(
    call: DrawingCall,
    settings: Settings,
    output: &Path,
    journal: &mut Journal<J, O>,
    control: Arc<CallControl>,
) -> Result<(), String> {
    emit(journal, &control, "started", json!({"execution":"in_process"}))?;
    let mut host = Host::default();
    let job = host.replace_drawing_controlled(call, settings, Arc::clone(&control)).map_err(diag)?;
    let evidence = job.evidence().to_result("local-drawing").map_err(diag)?;
    output::save_evidence(output, &evidence)?;
    emit(
        journal,
        &control,
        "evidence",
        json!({"artifact":output::EVIDENCE,"image_sha256":evidence.provenance.image_sha256}),
    )?;
    let ready = job.coarse().map_err(diag)?;
    let coarse = host.publish_coarse(&ready).map_err(diag)?;
    save(output, output::COARSE_MESH, krly3d_mesh::codec::encode_result(coarse.mesh()).map_err(diag)?)?;
    let artifact = format!("{}:model", journal.flow);
    emit(
        journal,
        &control,
        "coarse_preview",
        json!({"artifact":output::COARSE_MESH,"artifact_id":artifact,
        "artifact_revision":1,"approximation":"bounding_prism","verified":false}),
    )?;
    // This call cannot start until the coarse event has been flushed above.
    let completed = ready.finish().map_err(diag)?;
    save(output, output::MODEL, completed.result().model().to_k3d().map_err(diag)?)?;
    save(output, output::MESH, krly3d_mesh::codec::encode_result(completed.result().mesh()).map_err(diag)?)?;
    let model = completed.result().model();
    let frame = model.frame();
    let loops = model.region().polygons().map_err(diag)?;
    let sketch_loops = loops
        .iter()
        .enumerate()
        .map(|(index, polygon)| {
            json!({
                "points_xy": polygon.iter().flat_map(|point| point.iter().copied()).collect::<Vec<_>>(),
                "hole": index > 0,
            })
        })
        .collect::<Vec<_>>();
    let construction = json!({
        "version": 1,
        "coordinate_system": "right_handed_z_up",
        "units": "model",
        "source_center": frame.source_center,
        "source_bounds": [frame.source_bounds[0][0], frame.source_bounds[0][1], frame.source_bounds[1][0], frame.source_bounds[1][1]],
        "sketch_plane": "XY",
        "sketch_offset": 0.0,
        "sketch_loops": sketch_loops,
        "extrusion_direction": [0.0, 0.0, 1.0],
        "extrusion_distance": settings.height,
    });
    emit(
        journal,
        &control,
        "editable_proposal",
        json!({"artifact":output::MODEL,"mesh":output::MESH,
        "artifact_id":artifact,"artifact_revision":2,"replaces_revision":1,"verified":false,
        "construction":construction}),
    )?;
    let completed = completed.render().map_err(diag)?;
    let preview = completed.result().review().preview().ok_or("missing preview")?;
    save(output, output::PREVIEW, &preview.png)?;
    emit(journal, &control, "preview", output::preview_event(preview, output::PREVIEW))?;
    let completed = completed.compare().map_err(diag)?;
    let likeness = completed.result().review().likeness().ok_or("missing likeness")?;
    save(output, output::LIKENESS, krly3d_view_api::likeness::encode_result(likeness).map_err(diag)?)?;
    emit(journal, &control, "validation", output::validation_event(likeness, output::LIKENESS))?;
    let result = host.publish(completed).map_err(diag)?;
    let manifest =
        output::manifest(&result, journal.started, ManifestMode::Stream { flow_id: &journal.flow });
    output::save_manifest(output, &manifest)?;
    emit(journal, &control, "completed", json!({"manifest":manifest}))
}

fn emit<J: Write, O: Write>(
    journal: &mut Journal<J, O>,
    control: &CallControl,
    kind: &str,
    event: Value,
) -> Result<(), String> {
    super::lifecycle::check(control)?;
    journal.emit(kind, event)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{run_stages, Journal};
    use crate::output;
    use krly3d_reconstruction::Settings;
    use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
    use serde_json::Value;
    use std::io::{self, ErrorKind, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    const STAGES: [&str; 6] =
        ["started", "evidence", "coarse_preview", "editable_proposal", "preview", "validation"];

    #[derive(Clone, Copy)]
    enum Fault {
        Write,
        Flush,
    }

    #[derive(Default)]
    struct Probe {
        bytes: Vec<u8>,
        fault: Option<(&'static str, Fault)>,
        pending_write_error: bool,
        pending_flush_error: bool,
        failed: bool,
        calls_after_failure: usize,
        manifest: PathBuf,
        saw_manifest: bool,
    }

    impl Probe {
        fn check_failure(&mut self, pending: bool) -> io::Result<()> {
            if self.failed {
                self.calls_after_failure += 1;
            }
            if self.failed || pending {
                self.failed = true;
                return Err(io::Error::new(ErrorKind::BrokenPipe, "injected output failure"));
            }
            Ok(())
        }

        fn events(&self) -> Vec<Value> {
            // A write error can leave an incomplete final NDJSON record.
            self.bytes
                .split_inclusive(|byte| *byte == b'\n')
                .filter(|line| line.ends_with(b"\n"))
                .map(|line| serde_json::from_slice(line).unwrap())
                .collect()
        }
    }

    impl Write for Probe {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.check_failure(self.pending_write_error)?;
            if let Some((kind, fault)) = self.fault {
                if serde_json::from_slice::<Value>(bytes).is_ok_and(|event| event["kind"] == kind) {
                    self.saw_manifest = self.manifest.is_file();
                    self.fault = None;
                    match fault {
                        Fault::Write => {
                            // Accept part of the terminal record before failing
                            // write_all's next write, as a pipe or disk can do.
                            let accepted = bytes.len() / 2;
                            self.bytes.extend_from_slice(&bytes[..accepted]);
                            self.pending_write_error = true;
                            return Ok(accepted);
                        }
                        Fault::Flush => self.pending_flush_error = true,
                    }
                }
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.check_failure(self.pending_flush_error)
        }
    }

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!(
                "krly3d-host-cli-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn drawing() -> DrawingCall {
        let mut data = b"P5 16 16 255\n".to_vec();
        for y in 0..16 {
            for x in 0..16 {
                let outer = (2..14).contains(&x) && (2..14).contains(&y);
                let hole = (6..10).contains(&x) && (6..10).contains(&y);
                data.push(if outer && !hole { 0 } else { 255 });
            }
        }
        DrawingCall {
            protocol: PROTOCOL.into(),
            request_id: "local-drawing".into(),
            input: DrawingInput {
                image: DrawingImage { format: "pnm".into(), data },
                pixel_unit: 0.5,
                simplify: 0.0,
                seed: None,
                source: "ring".into(),
            },
        }
    }

    fn settings() -> Settings {
        Settings { height: 3.0, tolerance: 0.05 }
    }

    fn completion_failure(fault: Fault, fail_journal: bool) {
        let directory = Directory::new();
        let broken = Probe {
            fault: Some(("completed", fault)),
            manifest: directory.0.join(output::MANIFEST),
            ..Probe::default()
        };
        let (file, stdout) =
            if fail_journal { (broken, Probe::default()) } else { (Probe::default(), broken) };
        let mut journal = Journal::new(file, stdout, "terminal-test".into(), Instant::now());
        let error = run_stages(drawing(), settings(), &directory.0, &mut journal).unwrap_err();
        assert!(error.contains("injected output failure"), "{error}");
        assert!(!directory.0.join(output::MANIFEST).exists());
        for name in [
            output::EVIDENCE,
            output::COARSE_MESH,
            output::MODEL,
            output::MESH,
            output::PREVIEW,
            output::LIKENESS,
        ] {
            assert!(directory.0.join(name).is_file(), "diagnostic artifact lost: {name}");
        }
        let broken = if fail_journal { &journal.file } else { &journal.stdout };
        assert!(broken.failed && broken.saw_manifest, "failure must follow saving the success manifest");
        for writer in [&journal.file, &journal.stdout] {
            assert_eq!(writer.calls_after_failure, 0);
            let events = writer.events();
            let kinds: Vec<_> = events.iter().map(|event| event["kind"].as_str().unwrap()).collect();
            assert_eq!(&kinds[..STAGES.len()], &STAGES);
            assert!(kinds[STAGES.len()..].iter().all(|kind| *kind == "completed"));
            // Also catch a partial contradictory `failed` attempt.
            assert!(!String::from_utf8_lossy(&writer.bytes).contains("\"kind\":\"failed\""));
        }
        // If the journal failed, stdout never receives the terminal attempt.
        let expected_stdout = if fail_journal || matches!(fault, Fault::Write) { 6 } else { 7 };
        assert_eq!(journal.stdout.events().len(), expected_stdout);
        // A failed stdout write/flush must not even try a recovery journal write.
        let expected_journal = if fail_journal && matches!(fault, Fault::Write) { 6 } else { 7 };
        assert_eq!(journal.file.events().len(), expected_journal);
    }

    #[test]
    fn completed_stdout_write_failure_retracts_manifest_without_failed_terminal() {
        completion_failure(Fault::Write, false);
    }

    #[test]
    fn completed_stdout_flush_failure_retracts_manifest_without_failed_terminal() {
        completion_failure(Fault::Flush, false);
    }

    #[test]
    fn completed_journal_write_failure_retracts_manifest_without_failed_terminal() {
        completion_failure(Fault::Write, true);
    }

    #[test]
    fn completed_journal_flush_failure_retracts_manifest_without_failed_terminal() {
        completion_failure(Fault::Flush, true);
    }

    #[test]
    fn computation_failure_emits_failed_once_when_output_is_healthy() {
        let directory = Directory::new();
        let mut call = drawing();
        call.input.image.data = b"not a PNM".to_vec();
        let mut journal =
            Journal::new(Probe::default(), Probe::default(), "failed-test".into(), Instant::now());
        assert!(run_stages(call, settings(), &directory.0, &mut journal).is_err());
        assert!(!directory.0.join(output::MANIFEST).exists());
        assert_eq!(journal.file.bytes, journal.stdout.bytes);
        let events = journal.stdout.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["kind"], "started");
        assert_eq!(events[1]["kind"], "failed");
        assert_eq!(events[1]["seq"], 1);
    }

    #[test]
    fn earlier_output_failure_stops_work_and_does_not_attempt_failed() {
        let directory = Directory::new();
        let stdout = Probe { fault: Some(("evidence", Fault::Flush)), ..Probe::default() };
        let mut journal = Journal::new(Probe::default(), stdout, "early-test".into(), Instant::now());
        assert!(run_stages(drawing(), settings(), &directory.0, &mut journal).is_err());
        assert!(directory.0.join(output::EVIDENCE).is_file());
        assert!(!directory.0.join(output::COARSE_MESH).exists());
        assert!(!directory.0.join(output::MANIFEST).exists());
        assert_eq!(journal.file.bytes, journal.stdout.bytes);
        assert_eq!(journal.file.events().len(), 2);
        assert_eq!(journal.stdout.calls_after_failure, 0);
    }
}
