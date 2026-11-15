//! Bounded serial composition of explicitly selected components of one decoded
//! drawing. Each part owns its reconstruction; all parts keep drawing coordinates.
//! This is not an assembly solver, an automatic segmenter or a resumable scheduler.
use crate::{check, error, CoarsePreview, Host, Reconstruction, Settings};
use krly3d_view_api::likeness::LikenessResult;
use krly3d_view_api::RenderResult;
use krlyvis_api::{DrawingCall, DrawingProvenance};
use krlyvis_engine::{DrawingSource, SharedDrawing};
use krlyxd::{CallControl, Diagnostic};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const MAX_PARTS: usize = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct PartSpec {
    pub id: String,
    /// Input image pixels: x right, y down, inside a material component.
    pub seed: [f64; 2],
    pub height: f64,
}

impl PartSpec {
    pub fn validate(&self, tolerance: f64) -> Result<(), Diagnostic> {
        let bytes = self.id.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 64
            || !bytes[0].is_ascii_alphabetic()
            || bytes.iter().any(|b| !b.is_ascii_alphanumeric() && *b != b'_' && *b != b'-')
        {
            return Err(error(
                "input",
                "part id needs an ASCII letter followed by up to 63 letters, digits, underscores or hyphens",
            ));
        }
        if self.seed.iter().any(|value| !value.is_finite() || *value < 0.0) {
            return Err(error("input", "part seed needs two nonnegative finite pixel coordinates"));
        }
        Settings { height: self.height, tolerance }.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

pub struct PartRecord {
    spec: PartSpec,
    status: PartStatus,
    result: Option<Arc<Reconstruction>>,
    error: Option<Diagnostic>,
}

impl PartRecord {
    pub fn spec(&self) -> &PartSpec {
        &self.spec
    }
    pub fn status(&self) -> PartStatus {
        self.status
    }
    pub fn result(&self) -> Option<&Arc<Reconstruction>> {
        self.result.as_ref()
    }
    pub fn error(&self) -> Option<&Diagnostic> {
        self.error.as_ref()
    }
}

/// Borrowed native stage output. Returning an observer error stops the sheet;
/// callbacks execute between computation stages with no host/control mutex held.
/// Backpressure is synchronous. Callers must provide their own interruptible I/O.
pub enum PartStage<'a> {
    Evidence(&'a SharedDrawing),
    Coarse(&'a CoarsePreview),
    Editable(&'a Reconstruction),
    Preview(&'a RenderResult),
    Validation(&'a LikenessResult),
}

type ComponentKey = Vec<Vec<[u64; 2]>>;

pub struct Sheet {
    source: DrawingSource,
    records: Vec<PartRecord>,
    tolerance: f64,
    control: Arc<CallControl>,
    selected: Vec<ComponentKey>,
    next: usize,
    stopped: Option<Diagnostic>,
}

impl Sheet {
    /// Validate the entire selection list before decoding the image once. Part
    /// geometry/seed-in-component failures remain per-part results during advance.
    pub fn new(
        call: DrawingCall,
        parts: Vec<PartSpec>,
        tolerance: f64,
        control: Arc<CallControl>,
    ) -> Result<Self, Diagnostic> {
        check(&control)?;
        call.validate()?;
        if call.input.seed.is_some() {
            return Err(error("input", "sheet seeds belong to the part selection list"));
        }
        if parts.is_empty() || parts.len() > MAX_PARTS {
            return Err(error("input", "sheet requires between 1 and 16 parts"));
        }
        let mut ids = BTreeSet::new();
        for spec in &parts {
            spec.validate(tolerance)?;
            if !ids.insert(&spec.id) {
                return Err(error("input", "duplicate part id"));
            }
        }
        let source = DrawingSource::read_controlled(call, Arc::clone(&control))?;
        check(&control)?;
        let records = parts
            .into_iter()
            .map(|spec| PartRecord { spec, status: PartStatus::Pending, result: None, error: None })
            .collect();
        Ok(Self { source, records, tolerance, control, selected: Vec::new(), next: 0, stopped: None })
    }

    pub fn provenance(&self) -> &DrawingProvenance {
        self.source.provenance()
    }
    pub fn records(&self) -> &[PartRecord] {
        &self.records
    }
    pub fn is_complete(&self) -> bool {
        self.next == self.records.len() && self.stopped.is_none()
    }

    /// Complete one part, preserving earlier results on any later failure. At most
    /// one part computes at a time; at most sixteen results are retained. Calling
    /// after a failure returns that same diagnostic without retry or more events.
    pub fn advance(
        &mut self,
        mut observer: impl FnMut(&str, PartStage<'_>) -> Result<(), Diagnostic>,
    ) -> Result<bool, Diagnostic> {
        if let Some(failure) = &self.stopped {
            return Err(failure.clone());
        }
        if self.next == self.records.len() {
            return Ok(false);
        }
        let spec = self.records[self.next].spec.clone();
        self.records[self.next].status = PartStatus::Running;
        let result = self.compute(&spec, &mut observer);
        match result {
            Ok((result, key)) => {
                // Serialize acceptance with cancellation across the whole sheet.
                // Computation and user callbacks are always outside this mutex.
                let control = Arc::clone(&self.control);
                match control
                    .publish(|| {
                        self.records[self.next].result = Some(result);
                        self.records[self.next].status = PartStatus::Completed;
                        self.selected.push(key);
                        self.next += 1;
                    })
                    .map_err(crate::stop)
                {
                    Ok(()) => Ok(true),
                    Err(failure) => self.fail(failure),
                }
            }
            Err(failure) => self.fail(failure),
        }
    }

    fn fail(&mut self, failure: Diagnostic) -> Result<bool, Diagnostic> {
        let record = &mut self.records[self.next];
        record.status =
            if failure.code.as_str() == "cancelled" { PartStatus::Cancelled } else { PartStatus::Failed };
        record.error = Some(failure.clone());
        self.stopped = Some(failure.clone());
        Err(failure)
    }

    fn compute(
        &self,
        spec: &PartSpec,
        observer: &mut impl FnMut(&str, PartStage<'_>) -> Result<(), Diagnostic>,
    ) -> Result<(Arc<Reconstruction>, ComponentKey), Diagnostic> {
        check(&self.control)?;
        let evidence = Arc::new(self.source.select_controlled(Some(spec.seed), Arc::clone(&self.control))?);
        check(&self.control)?;
        let key = component_key(&evidence);
        if self.selected.contains(&key) {
            return Err(error(
                "duplicate_component",
                "different part names selected the same material component",
            ));
        }
        self.observe(spec, PartStage::Evidence(&evidence), observer)?;
        let mut host = Host::default();
        let job = host.replace_evidence_controlled(
            evidence,
            Settings { height: spec.height, tolerance: self.tolerance },
            Arc::clone(&self.control),
        )?;
        let coarse = job.coarse()?;
        let preview = host.publish_coarse(&coarse)?;
        self.observe(spec, PartStage::Coarse(&preview), observer)?;
        let completed = coarse.finish()?;
        self.observe(spec, PartStage::Editable(completed.result()), observer)?;
        let completed = completed.render()?;
        let rendered =
            completed.result().review().preview().ok_or_else(|| error("stage", "missing sheet preview"))?;
        self.observe(spec, PartStage::Preview(rendered), observer)?;
        let completed = completed.compare()?;
        let likeness =
            completed.result().review().likeness().ok_or_else(|| error("stage", "missing sheet likeness"))?;
        self.observe(spec, PartStage::Validation(likeness), observer)?;
        Ok((host.publish(completed)?, key))
    }

    fn observe(
        &self,
        spec: &PartSpec,
        stage: PartStage<'_>,
        observer: &mut impl FnMut(&str, PartStage<'_>) -> Result<(), Diagnostic>,
    ) -> Result<(), Diagnostic> {
        check(&self.control)?;
        observer(&spec.id, stage)?;
        check(&self.control)
    }
}

/// Identity only, not geometric similarity: normalize loop starts/direction and
/// signed zero over bounded provider coordinates, then compare exact bit patterns.
fn component_key(evidence: &SharedDrawing) -> ComponentKey {
    let mut loops: ComponentKey = evidence.polygons().into_iter().map(canonical_loop).collect();
    loops.sort();
    loops
}

fn canonical_loop(polygon: Vec<[f64; 2]>) -> Vec<[u64; 2]> {
    let mut points: Vec<[u64; 2]> =
        polygon.into_iter().map(|point| point.map(|v| if v == 0.0 { 0 } else { v.to_bits() })).collect();
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    let count = points.len();
    (0..count)
        .flat_map(|start| [false, true].into_iter().map(move |reverse| (start, reverse)))
        .map(|(start, reverse)| {
            (0..count)
                .map(|offset| {
                    points[if reverse { (start + count - offset) % count } else { (start + offset) % count }]
                })
                .collect::<Vec<_>>()
        })
        .min()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::canonical_loop;

    #[test]
    fn component_identity_ignores_loop_start_winding_closure_and_signed_zero() {
        let original = vec![[0., 0.], [3., 0.], [3., 2.], [0., 2.]];
        let identity = canonical_loop(original.clone());
        for shift in 0..original.len() {
            let mut shifted = original.clone();
            shifted.rotate_left(shift);
            assert_eq!(canonical_loop(shifted.clone()), identity);
            shifted.reverse();
            shifted.push(shifted[0]);
            assert_eq!(canonical_loop(shifted), identity);
        }
        assert_eq!(canonical_loop(vec![[-0., -0.], [3., -0.], [3., 2.], [-0., 2.]]), identity);
        assert_ne!(canonical_loop(vec![[0., 0.], [4., 0.], [4., 2.], [0., 2.]]), identity);
    }
}
