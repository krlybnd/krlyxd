//! Single-shot bounded field adapter. Geometry is written to a file, never stdout.
use krly3d_field::{Error, ENGINE_REVISION, MAX_RECIPE_BYTES};
use krly3d_field_engine::Progress;
use krlyxd::CallControl;
use serde::Serialize;
use serde_json::json;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        let message: String = error.message.chars().take(512).collect();
        eprintln!("{}", json!({"stage":"failed","code":error.code,"message":message}));
        std::process::exit(1);
    }
}
fn run() -> Result<(), Error> {
    let (recipe_path, output) = arguments()?;
    let mut bytes = Vec::new();
    File::open(recipe_path)
        .map_err(io_error)?
        .take(MAX_RECIPE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let recipe = krly3d_field::decode(&bytes)?;
    let grid = recipe.validate()?;
    let stop = Signals::new()?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mesh = krly3d_field_engine::mesh_controlled(&recipe, &stop.control, &mut |event| {
        let value = match event {
            Progress::Sampling { grid_nodes } => json!({"stage":"sampling","grid_nodes":grid_nodes}),
            Progress::Mesh { grid_nodes } => json!({"stage":"mesh","grid_nodes":grid_nodes}),
        };
        emit(&mut stdout, &value)
    })?;
    stop.check()?;
    fs::create_dir_all(&output).map_err(io_error)?;
    // A private temporary name avoids publishing partial JSON on a failed write.
    let temporary = output.join(format!(".mesh-{}.json.tmp", std::process::id()));
    let file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(io_error)?;
    let result = (|| {
        let mut writer = BufWriter::new(CheckedWriter { file, stop: &stop });
        #[derive(Serialize)]
        struct MeshFile<'a> {
            points: &'a [[f64; 3]],
            triangles: &'a [[u32; 3]],
        }
        serde_json::to_writer(&mut writer, &MeshFile { points: &mesh.points, triangles: &mesh.triangles })
            .map_err(|e| stop.check().err().unwrap_or_else(|| Error::new("io", e.to_string())))?;
        writer.flush().map_err(io_error)?;
        stop.check()?;
        fs::rename(&temporary, output.join("mesh.json")).map_err(io_error)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    stop.check()?;
    emit(
        &mut stdout,
        &json!({
            "stage":"completed", "points":mesh.points.len(), "triangles":mesh.triangles.len(),
            "units":"mm", "recipe_version":1, "engine_revision":ENGINE_REVISION,
            "exactness":"approximate", "cell_size":recipe.cell_size, "grid_spacing":grid.spacing,
        }),
    )
}
fn arguments() -> Result<(PathBuf, PathBuf), Error> {
    let mut args = std::env::args_os().skip(1);
    let (mut recipe, mut output) = (None, None);
    while let Some(arg) = args.next() {
        let target = if arg == "--recipe" {
            &mut recipe
        } else if arg == "--output" {
            &mut output
        } else {
            return Err(usage());
        };
        if target.is_some() {
            return Err(usage());
        }
        *target = Some(PathBuf::from(args.next().ok_or_else(usage)?));
    }
    Ok((recipe.ok_or_else(usage)?, output.ok_or_else(usage)?))
}
fn usage() -> Error {
    Error::new("input", "usage: krly3d-field --recipe <path> --output <directory>")
}
fn io_error(error: io::Error) -> Error {
    Error::new("io", error.to_string())
}
fn emit(writer: &mut impl Write, event: &serde_json::Value) -> Result<(), Error> {
    serde_json::to_writer(&mut *writer, event).map_err(|e| Error::new("io", e.to_string()))?;
    writer.write_all(b"\n").map_err(io_error)?;
    writer.flush().map_err(io_error)
}
struct CheckedWriter<'a> {
    file: File,
    stop: &'a Signals,
}
impl Write for CheckedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stop.check().map_err(io::Error::other)?;
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stop.check().map_err(io::Error::other)?;
        self.file.flush()
    }
}
struct Signals {
    control: Arc<CallControl>,
    done: Arc<AtomicBool>,
    interrupted: Arc<AtomicBool>,
    ids: Vec<signal_hook::SigId>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Signals {
    fn new() -> Result<Self, Error> {
        let control = Arc::new(CallControl::new());
        let done = Arc::new(AtomicBool::new(false));
        let interrupted = Arc::new(AtomicBool::new(false));
        let mut result = Self {
            control: Arc::clone(&control),
            done: Arc::clone(&done),
            interrupted: Arc::clone(&interrupted),
            ids: Vec::new(),
            worker: None,
        };
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            result.ids.push(signal_hook::flag::register(signal, Arc::clone(&interrupted)).map_err(io_error)?);
        }
        result.worker = Some(std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if interrupted.load(Ordering::Relaxed) {
                    control.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }));
        Ok(result)
    }
    fn check(&self) -> Result<(), Error> {
        if self.interrupted.load(Ordering::Relaxed) {
            self.control.cancel();
        }
        self.control.check().map_err(|e| Error::new("stopped", e.to_string()))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
