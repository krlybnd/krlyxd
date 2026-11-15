use crate::{check, error, stop, Completed, EventKind, Host, Job};
use krly3d_brep::planar::{PlanarCall, PlanarInput, PROTOCOL};
use krly3d_brep_engine::planar::build_planar_retained_controlled;
use krly3d_mesh::{MeshCall, MeshInput, MeshResult, RequestId};
use krly3d_mesh_engine::MeshEngine;
use krlyxd::Diagnostic;
use std::sync::Arc;

/// A bounding-rectangle prism approximation of drawing evidence, not the contour
/// reconstruction. Holes and concavities are filled. Mesh quality describes only
/// tessellation of this prism, not its error relative to the evidence.
pub struct CoarsePreview {
    revision: u64,
    mesh: Arc<MeshResult>,
}

impl CoarsePreview {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn mesh(&self) -> &Arc<MeshResult> {
        &self.mesh
    }
}

/// A completed approximation plus the original job and its admission permit.
/// Publishing or retaining the preview does not finish or release the job.
pub struct CoarseReady {
    job: Job,
    preview: Arc<CoarsePreview>,
}

impl CoarseReady {
    pub fn preview(&self) -> &Arc<CoarsePreview> {
        &self.preview
    }

    /// Build the contour reconstruction through the normal job execution path.
    pub fn finish(self) -> Result<Completed, Diagnostic> {
        self.job.run()
    }
}

impl Job {
    /// Build only a bounding rectangle prism. The evidence, optional reusable
    /// contour model and permit remain with this job until finish or drop.
    pub fn coarse(self) -> Result<CoarseReady, Diagnostic> {
        check(&self.control)?;
        let (placed, _) = super::frame::centered_top_view(self.evidence.region())?;
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for polygon in placed.polygons()? {
            check(&self.control)?;
            for point in polygon {
                min[0] = min[0].min(point[0]);
                min[1] = min[1].min(point[1]);
                max[0] = max[0].max(point[0]);
                max[1] = max[1].max(point[1]);
            }
        }
        if !min.iter().chain(&max).all(|value| value.is_finite()) || min[0] >= max[0] || min[1] >= max[1] {
            return Err(error("geometry", "coarse preview requires finite, nondegenerate evidence bounds"));
        }
        check(&self.control)?;
        let retained = build_planar_retained_controlled(
            PlanarCall {
                protocol: PROTOCOL.into(),
                request_id: format!("reconstruction-coarse:{}", self.revision),
                input: PlanarInput {
                    polygons: vec![vec![min, [max[0], min[1]], max, [min[0], max[1]]]],
                    height: self.settings.height,
                    length_unit: "model".into(),
                    source: format!(
                        "coarse-bounding-rectangle:sha256:{}:drawing-revision:{}",
                        self.evidence.provenance().image_sha256,
                        self.revision
                    ),
                },
            },
            Arc::clone(&self.control),
        )?;
        check(&self.control)?;
        let mut input = MeshInput::new(retained.snapshot.identity().node.clone());
        input.triangles = true;
        input.tolerance.value = self.settings.tolerance;
        let engine = MeshEngine::from_brep(retained.engine);
        let prepared = engine.prepare(MeshCall::new(RequestId::new("reconstruction-coarse"), input))?;
        let mesh = engine.execute_controlled(prepared, Arc::clone(&self.control))?;
        let preview = Arc::new(CoarsePreview { revision: self.revision, mesh: Arc::new(mesh) });
        check(&self.control)?;
        Ok(CoarseReady { job: self, preview })
    }
}

impl Host {
    /// Publish one explicitly coarse approximation for the pending revision.
    /// The borrowed ready handle keeps its job/permit for subsequent finishing.
    pub fn publish_coarse(&mut self, ready: &CoarseReady) -> Result<Arc<CoarsePreview>, Diagnostic> {
        if !Arc::ptr_eq(&self.identity, &ready.job.host) {
            return Err(error("foreign_host", "coarse preview belongs to another host"));
        }
        if ready.job.revision != self.revision || self.current.is_some() {
            return Err(error("stale_revision", "coarse preview no longer matches the requested revision"));
        }
        if self.coarse.is_some() {
            return Err(error("stale_revision", "coarse preview already published for this revision"));
        }
        ready
            .job
            .control
            .publish(|| {
                self.coarse = Some(Arc::clone(&ready.preview));
                self.emit(EventKind::CoarsePreview, ready.job.settings);
                Arc::clone(&ready.preview)
            })
            .map_err(stop)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::{EventKind, Host, Job, Settings};
    use krlyvis_api::{DrawingCall, DrawingImage, DrawingInput, PROTOCOL};
    use krlyxd::{CallControl, Clock, FakeClock};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn job(host: &mut Host, control: Arc<CallControl>) -> Job {
        let mut data = b"P5 8 8 255\n".to_vec();
        for y in 0..8 {
            for x in 0..8 {
                data.push(if (2..6).contains(&x) && (2..6).contains(&y) { 0 } else { 255 });
            }
        }
        let call = DrawingCall {
            protocol: PROTOCOL.into(),
            request_id: "coarse-control".into(),
            input: DrawingInput {
                image: DrawingImage { format: "pnm".into(), data },
                pixel_unit: 1.0,
                simplify: 0.0,
                seed: None,
                source: "rectangle".into(),
            },
        };
        let mut job = host.replace_drawing(call, Settings { height: 3.0, tolerance: 0.05 }).unwrap();
        job.control = control;
        job
    }

    #[test]
    fn stopped_coarse_computation_releases_its_permit_without_publication() {
        let cancelled = Arc::new(CallControl::new());
        cancelled.cancel();
        let expired = Arc::new(CallControl::with_deadline(Arc::new(FakeClock::default()), Duration::ZERO));
        for (control, code) in [(cancelled, "cancelled"), (expired, "deadline_exceeded")] {
            let mut host = Host::default();
            let job = job(&mut host, control);
            let before = host.events_after(0).unwrap();
            assert_eq!(job.coarse().err().unwrap().code.as_str(), code);
            assert_eq!(host.in_flight(), 0);
            assert!(host.coarse.is_none());
            assert!(host.current().is_none());
            assert_eq!(host.events_after(0).unwrap(), before);
        }
    }

    struct ExpiringClock {
        reads: AtomicUsize,
        expire_at: usize,
    }

    impl Clock for ExpiringClock {
        fn now(&self) -> Duration {
            if self.reads.fetch_add(1, Ordering::SeqCst) >= self.expire_at {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            }
        }
    }

    #[test]
    fn deadline_is_observed_during_controlled_mesh_work() {
        let mut host = Host::default();
        // A single rectangle loop uses four coarse checks before entering the
        // mesh engine. Expiry at the eighth read requires mesh checkpoints.
        let clock = Arc::new(ExpiringClock { reads: AtomicUsize::new(0), expire_at: 7 });
        let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
        let job = job(&mut host, control);
        let before = host.events_after(0).unwrap();
        assert_eq!(job.coarse().err().unwrap().code.as_str(), "deadline_exceeded");
        assert_eq!(clock.reads.load(Ordering::SeqCst), 8);
        assert_eq!(host.in_flight(), 0);
        assert!(host.coarse.is_none());
        assert!(host.current().is_none());
        assert_eq!(host.events_after(0).unwrap(), before);
    }

    #[test]
    fn cancellation_or_deadline_before_host_publication_cannot_mutate_state_or_events() {
        for cancel in [false, true] {
            let clock = Arc::new(FakeClock::default());
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            let mut host = Host::default();
            let ready = job(&mut host, Arc::clone(&control)).coarse().unwrap();
            assert!(ready.job.reusable.is_none(), "coarse does not create a reusable contour model");
            if cancel {
                control.cancel();
            } else {
                clock.advance(Duration::from_secs(1));
            }
            let before = host.events_after(0).unwrap();
            let code = if cancel { "cancelled" } else { "deadline_exceeded" };
            assert_eq!(host.publish_coarse(&ready).err().unwrap().code.as_str(), code);
            assert!(host.coarse.is_none());
            assert!(host.current().is_none());
            assert_eq!(host.events_after(0).unwrap(), before);
            assert_eq!(host.in_flight(), 1, "failed borrowed publication keeps the job alive");
            assert_eq!(ready.finish().err().unwrap().code.as_str(), code);
            assert_eq!(host.in_flight(), 0);
            assert_eq!(host.events_after(0).unwrap(), before);
        }
    }

    #[test]
    fn published_preview_survives_stopped_finish_and_clears_at_next_begin_and_commit() {
        let mut host = Host::default();
        let control = Arc::new(CallControl::new());
        let ready = job(&mut host, Arc::clone(&control)).coarse().unwrap();
        let preview = host.publish_coarse(&ready).unwrap();
        control.cancel();
        let before = host.events_after(0).unwrap();
        assert_eq!(ready.finish().err().unwrap().code.as_str(), "cancelled");
        assert_eq!(host.in_flight(), 0);
        assert!(host.current().is_none());
        assert!(Arc::ptr_eq(host.coarse.as_ref().unwrap(), &preview));
        assert_eq!(host.events_after(0).unwrap(), before);
        let next = host.revise(Settings { height: 4.0, tolerance: 0.05 }).unwrap();
        assert!(host.coarse.is_none());
        let next = next.coarse().unwrap();
        host.publish_coarse(&next).unwrap();
        host.publish(next.finish().unwrap()).unwrap();
        assert!(host.coarse.is_none());
        assert!(host.current().is_some());
        assert_eq!(host.events_after(0).unwrap().last().unwrap().kind, EventKind::Committed);
        assert_eq!(preview.mesh().volume, 48.0);
    }
}
