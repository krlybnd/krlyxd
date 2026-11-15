//! Optional native review stages on the same retained snapshot and stop token.
use super::{check, error, Completed};
use krly3d_view_api::likeness::{LikenessCall, LikenessResult, RasterEvidence};
use krly3d_view_api::{Backend, Diagnostic, RenderCall, RenderInput, RenderResult, RequestId};
use std::sync::Arc;

#[derive(Default)]
pub struct Review {
    preview: Option<Arc<RenderResult>>,
    likeness: Option<Arc<LikenessResult>>,
}

impl Review {
    pub fn preview(&self) -> Option<&Arc<RenderResult>> {
        self.preview.as_ref()
    }
    pub fn likeness(&self) -> Option<&Arc<LikenessResult>> {
        self.likeness.as_ref()
    }
}

impl Completed {
    /// Provisional immutable result. Only Host::publish makes it current.
    pub fn result(&self) -> &super::Reconstruction {
        &self.result
    }

    /// Consumes the provisional result on failure; no half-reviewed publication.
    pub fn render(mut self) -> Result<Self, Diagnostic> {
        check(&self.control)?;
        let snapshot = self.result.model.snapshot.clone();
        let mut input = RenderInput::new(snapshot.identity().node.clone());
        input.backend = Backend::Mesh;
        input.tolerance.value = self.result.settings.tolerance;
        let rendered = krly3d_view_renderer::render_snapshot_controlled(
            snapshot,
            RenderCall::new(RequestId::new("reconstruction-preview"), input),
            Arc::clone(&self.control),
        )?;
        self.result.review.preview = Some(Arc::new(rendered));
        Ok(self)
    }

    /// Sampled outer-extent agreement, not proof of holes or physical accuracy.
    pub fn compare(mut self) -> Result<Self, Diagnostic> {
        check(&self.control)?;
        if self.result.review.preview.is_none() {
            return Err(error("stage", "render must precede comparison"));
        }
        let snapshot = self.result.model.snapshot.clone();
        let frame = self.result.model.frame;
        let evidence = RasterEvidence {
            polygons: self
                .result
                .evidence
                .polygons()
                .into_iter()
                .map(|polygon| polygon.into_iter().map(|point| frame.model_point(point)).collect())
                .collect(),
            pixel_unit: self.result.evidence.provenance().pixel_unit,
            source: format!("sha256:{}", self.result.evidence.provenance().image_sha256),
        };
        let call = LikenessCall::new(
            RequestId::new("reconstruction-likeness"),
            snapshot.identity().node.clone(),
            evidence,
        );
        let compared =
            krly3d_view_renderer::likeness::compare_snapshot(snapshot, call, Arc::clone(&self.control))?;
        self.result.review.likeness = Some(Arc::new(compared));
        Ok(self)
    }
}
