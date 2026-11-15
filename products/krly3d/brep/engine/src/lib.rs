//! B-rep session ownership, immutable snapshots and planar construction.
//!
//! Snapshots pin a resolved shape and its originating services without borrowing the
//! mutable session. Conversion belongs to consumers; this crate does not tessellate.
//! The legacy accessors are a temporary bridge, not a completed B-rep API migration
//! or an advertised wire capability. [`build_planar_retained`] retains a fresh engine
//! and snapshot; [`build_planar`] also exports an editable native document for the wire.

use krly3d_brep::{Diagnostic, Id, SnapshotIdentity};
use krly3d_core::Error;
use krly3d_kernel::model::{Delta, Document, NodeRef, Statement};
use krly3d_kernel::ports::shape::Shape;
use krly3d_kernel::ports::write::write;
use krly3d_kernel::{Checkpoint, Context, Kernel};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod planar;
pub use planar::{
    build_planar, build_planar_retained, build_planar_retained_controlled, build_region_retained,
    build_region_retained_controlled, RetainedPlanar,
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// Owns a native B-rep session and the unique identity used by its snapshots.
pub struct BrepEngine {
    kernel: Kernel,
    session: Id,
}

/// An immutable, engine-created handle that survives edits, rollback and owner drop.
///
/// Private fields prevent callers from forging handles or replacing their services.
/// The public metadata alone is not a handle.
///
/// ```compile_fail
/// use krly3d_brep_engine::{BrepEngine, Snapshot};
/// use std::sync::Arc;
/// let engine = BrepEngine::from_k3d("k3d 1\nlayer geo\npart = box w=6 d=4 h=2").unwrap();
/// let original = engine.snapshot("part").unwrap();
/// let forged = Snapshot {
///     identity: original.identity().clone(),
///     shape: Arc::clone(original.legacy_shape()),
///     context: original.legacy_context().clone(),
/// };
/// ```
#[derive(Clone)]
pub struct Snapshot {
    identity: SnapshotIdentity,
    shape: Arc<dyn Shape>,
    context: Context,
}

impl BrepEngine {
    pub fn new(kernel: Kernel) -> Result<Self, Diagnostic> {
        let serial = NEXT_SESSION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1))
            .map_err(|_| diagnostic("resource_limit", "mesh session identity space exhausted"))?;
        let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let session = Id::new(&format!("brep-session:{}:{epoch:x}:{serial:x}", std::process::id()));
        Ok(Self { kernel, session })
    }

    /// Parse and open a native document with the existing standard plugins and
    /// validation. Refused expectations fail before exposing an engine.
    pub fn from_k3d(text: &str) -> Result<Self, Diagnostic> {
        let document = krly3d_core::document::text::parse(text, &[krly3d_kernel::model::GEOMETRY])
            .map_err(legacy_diagnostic)?;
        let mut engine = Self::new(krly3d_std::standard().build())?;
        let delta = engine.open(document).map_err(legacy_diagnostic)?;
        if delta.refused() {
            return Err(diagnostic("input", delta.text()));
        }
        Ok(engine)
    }

    /// Explicitly export the current document with the native k3d writer.
    pub fn to_k3d(&self) -> Result<String, Diagnostic> {
        write(self.kernel.context(), "k3d", self.kernel.document()).into_result().map_err(legacy_diagnostic)
    }

    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    pub fn apply(&mut self, statement: Statement) -> Result<Delta, Error> {
        self.kernel.apply(statement)
    }

    pub fn open(&mut self, document: Document) -> Result<Delta, Error> {
        self.kernel.open(document)
    }

    pub fn checkpoint(&self) -> Checkpoint {
        self.kernel.checkpoint()
    }

    pub fn rollback(&mut self, checkpoint: Checkpoint) {
        self.kernel.rollback(checkpoint);
    }

    /// Resolve a name/version once against an immutable model, without conversion.
    pub fn snapshot(&self, node: &str) -> Result<Snapshot, Diagnostic> {
        let (_, model) = self.kernel.snapshot();
        let mut reference = NodeRef::parse(node).map_err(legacy_diagnostic)?;
        let built = model
            .built(&reference)
            .ok_or_else(|| legacy_diagnostic(Error::not_found(format!("there is no node `{reference}`"))))?;
        if reference.version.is_none() {
            let versions = model.nodes().find(|(name, _)| *name == reference.name).map(|(_, n)| n);
            reference.version =
                Some(versions.and_then(|n| u32::try_from(n).ok()).ok_or_else(|| {
                    diagnostic("resource_limit", "mesh node version exceeds the u32 range")
                })?);
        }
        let node = reference.to_string();
        let build_key = format!("{:032x}", built.key);
        let source = Id::new(&format!("{}:{node}:{build_key}", self.session));
        Ok(Snapshot {
            identity: SnapshotIdentity { source, session: self.session.clone(), node, build_key },
            shape: Arc::clone(&built.shape),
            context: self.kernel.context().clone(),
        })
    }

    /// Whether this engine created the snapshot, irrespective of its current document.
    pub fn owns(&self, snapshot: &Snapshot) -> bool {
        snapshot.identity.session == self.session
    }

    pub fn session_id(&self) -> &Id {
        &self.session
    }
}

impl Snapshot {
    pub fn identity(&self) -> &SnapshotIdentity {
        &self.identity
    }

    /// Temporary legacy conversion bridge to the pinned shape. This does not complete
    /// the public B-rep API migration; consumers must use the originating context.
    pub fn legacy_shape(&self) -> &Arc<dyn Shape> {
        &self.shape
    }

    /// Temporary legacy conversion bridge to cloned originating services and caches,
    /// retained independently of the owner. This is not a portable context contract.
    pub fn legacy_context(&self) -> &Context {
        &self.context
    }
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot").field("identity", &self.identity).finish_non_exhaustive()
    }
}

fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}

fn legacy_diagnostic(error: Error) -> Diagnostic {
    // Preserve the existing labels/messages, including the space in `not found`.
    diagnostic(error.kind.label(), error.message)
}
