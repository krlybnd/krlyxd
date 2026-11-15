//! Snapshot bridge and controlled execution over the existing geometry session and sole tessellator.

pub mod preview;
pub mod runner;

use krly3d_brep_engine::{BrepEngine, Snapshot};
use krly3d_core::{CallControl, Error};
use krly3d_kernel::model::{Delta, Document, Statement};
use krly3d_kernel::{Checkpoint, Kernel};
use krly3d_mesh::Provenance;
use krly3d_mesh::{descriptor, diagnostic, ArtifactId, Diagnostic, MeshCall, MeshInput, MeshResult};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Owns the B-rep session so its identity cannot be rebound to another kernel by a caller.
/// Edits use the existing kernel APIs; there is no operation runner or session lock here.
pub struct MeshEngine {
    brep: BrepEngine,
    next_artifact: Arc<AtomicU64>,
}

/// Immutable snapshot prepared by this engine. Fields are private: callers cannot forge
/// source identities, replace the resolved model, or change the normalized input.
pub struct PreparedMesh {
    snapshot: Snapshot,
    input: MeshInput,
}

impl MeshEngine {
    /// Compose the pilot from an existing native B-rep document. Both transport roots
    /// use the same parser, standard plugins and document validation; no file I/O or
    /// transport configuration belongs in this helper.
    pub fn from_k3d(text: &str) -> Result<Self, Diagnostic> {
        Ok(Self::from_brep(BrepEngine::from_k3d(text)?))
    }

    pub fn new(kernel: Kernel) -> Result<Self, Diagnostic> {
        Ok(Self::from_brep(BrepEngine::new(kernel)?))
    }

    /// Compose mesh conversion over the product's existing B-rep owner.
    pub fn from_brep(brep: BrepEngine) -> Self {
        Self { brep, next_artifact: Arc::new(AtomicU64::new(1)) }
    }

    /// Shared B-rep identity and immutable snapshot boundary.
    pub fn brep(&self) -> &BrepEngine {
        &self.brep
    }

    pub fn kernel(&self) -> &Kernel {
        self.brep.kernel()
    }

    pub fn apply(&mut self, statement: Statement) -> Result<Delta, Error> {
        self.brep.apply(statement)
    }

    pub fn open(&mut self, document: Document) -> Result<Delta, Error> {
        self.brep.open(document)
    }

    pub fn checkpoint(&self) -> Checkpoint {
        self.brep.checkpoint()
    }

    pub fn rollback(&mut self, checkpoint: Checkpoint) {
        self.brep.rollback(checkpoint);
    }

    /// Validate without tessellation, then resolve a version exactly once on a snapshot.
    pub fn prepare(&self, call: MeshCall) -> Result<PreparedMesh, Diagnostic> {
        call.validate()?;
        let snapshot = self.brep.snapshot(&call.input.node)?;
        let mut input = call.input;
        input.node = snapshot.identity().node.clone();
        // Match the legacy mesh tolerance error, including its message.
        if input.tolerance.value <= self.brep.kernel().context().accuracy().length() {
            return Err(legacy_diagnostic(Error::input(format!(
                "a mesh tolerance must be above the length tolerance, got {}",
                input.tolerance.value
            ))));
        }
        Ok(PreparedMesh { snapshot, input })
    }

    /// Reads only the prepared snapshot, even if edits or rollback happened since prepare.
    pub fn execute(&self, prepared: PreparedMesh) -> Result<MeshResult, Diagnostic> {
        let context = prepared.snapshot.legacy_context().clone();
        self.execute_in(prepared, &context)
    }

    /// Cooperatively stop local Rust execution without changing the synchronous wire
    /// contract. The caller owns the control and its clock/deadline. This is not an
    /// operation handle or event stream; `MeshCall::timeout_ms` remains unsupported.
    pub fn execute_controlled(
        &self,
        prepared: PreparedMesh,
        control: Arc<CallControl>,
    ) -> Result<MeshResult, Diagnostic> {
        let context = prepared.snapshot.legacy_context().with_call_control(control);
        self.execute_in(prepared, &context)
    }

    fn execute_in(
        &self,
        prepared: PreparedMesh,
        context: &krly3d_kernel::Context,
    ) -> Result<MeshResult, Diagnostic> {
        if !self.brep.owns(&prepared.snapshot) {
            return Err(diagnostic("input", "a prepared mesh belongs to a different session"));
        }
        execute_prepared(prepared, context, &self.next_artifact)
    }

    /// Detach owned execution services so a worker never borrows the mutable session.
    pub(crate) fn task(
        &self,
        prepared: PreparedMesh,
        control: Arc<CallControl>,
    ) -> Result<MeshTask, Diagnostic> {
        if !self.brep.owns(&prepared.snapshot) {
            return Err(diagnostic("input", "a prepared mesh belongs to a different session"));
        }
        let context = prepared.snapshot.legacy_context().with_call_control(control);
        Ok(MeshTask { prepared, context, next_artifact: Arc::clone(&self.next_artifact) })
    }

    pub fn call(&self, call: MeshCall) -> Result<MeshResult, Diagnostic> {
        self.execute(self.prepare(call)?)
    }
}

/// Owned computation inputs; constructing a task neither meshes nor mutates the document.
pub(crate) struct MeshTask {
    prepared: PreparedMesh,
    context: krly3d_kernel::Context,
    next_artifact: Arc<AtomicU64>,
}

impl MeshTask {
    pub(crate) fn provenance(&self) -> Provenance {
        provenance(&self.prepared)
    }

    pub(crate) fn execute(self) -> Result<MeshResult, Diagnostic> {
        execute_prepared(self.prepared, &self.context, &self.next_artifact)
    }
}

fn provenance(prepared: &PreparedMesh) -> Provenance {
    let metadata = descriptor();
    Provenance {
        package_version: metadata.package_version.to_string(),
        engine_revision: metadata.engine_revision.clone(),
        source: prepared.snapshot.identity().source.clone(),
        input: prepared.input.clone(),
    }
}

fn execute_prepared(
    prepared: PreparedMesh,
    context: &krly3d_kernel::Context,
    next_artifact: &AtomicU64,
) -> Result<MeshResult, Diagnostic> {
    context.check_control().map_err(legacy_diagnostic)?;
    let reading = krly3d_std::query::mesh::read(context, prepared.snapshot.legacy_shape(), &prepared.input)
        .map_err(legacy_diagnostic)?;
    let provenance = provenance(&prepared);
    context
        .publish_control(|| {
            let serial = next(next_artifact)?;
            let artifact =
                ArtifactId::new(&format!("{}:artifact:{serial:x}", prepared.snapshot.identity().session));
            Ok(reading.into_result(provenance, artifact))
        })
        .map_err(legacy_diagnostic)?
}

fn next(counter: &AtomicU64) -> Result<u64, Diagnostic> {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1))
        .map_err(|_| diagnostic("resource_limit", "mesh session identity space exhausted"))
}

fn legacy_diagnostic(error: Error) -> Diagnostic {
    // Keep the existing diagnostic labels/messages, including `not found`.
    diagnostic(error.kind.label(), error.message)
}

#[cfg(test)]
mod tests {
    use super::MeshEngine;
    use krly3d_core::CallControl;
    use krly3d_kernel::model::Statement;
    use krly3d_kernel::schema::Params;
    use krly3d_mesh::{MeshCall, MeshInput, RequestId};
    use std::sync::Arc;

    #[test]
    fn detached_task_keeps_services_and_identity_after_engine_is_dropped() {
        let mut kernel = krly3d_std::standard().build();
        kernel
            .apply(
                Statement::new(
                    "part",
                    "box",
                    Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0),
                    &[],
                )
                .unwrap(),
            )
            .unwrap();
        let engine = MeshEngine::new(kernel).unwrap();
        let mut input = MeshInput::new("part");
        input.triangles = true;
        let request = MeshCall::new(RequestId::new("detached"), input);
        let before = engine.call(request.clone()).unwrap();
        let prepared = engine.prepare(request).unwrap();
        let task = engine.task(prepared, Arc::new(CallControl::new())).unwrap();
        let provenance = task.provenance();
        drop(engine);
        let result = std::thread::spawn(move || task.execute()).join().unwrap().unwrap();
        assert_eq!(result.provenance, provenance);
        assert_eq!((result.volume, result.area), (48.0, 88.0));
        assert_ne!(result.mesh.as_ref().unwrap().id, before.mesh.as_ref().unwrap().id);
        assert_eq!(result.mesh.unwrap().data, before.mesh.unwrap().data);
    }
}
