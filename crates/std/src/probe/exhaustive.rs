//! The reference ray probe: every ray against every face.

use super::{face_hits, settled};
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::probe::{Hit, Probe, ProbeRequest, RAYS};
use krly3d_kernel::{Context, Plugin};

pub struct Exhaustive;

impl Plugin<Probe> for Exhaustive {
    fn id(&self) -> Id {
        Id::new(RAYS)
    }

    fn call(&self, cx: &Context, request: &ProbeRequest) -> Attempt<Vec<Vec<Hit>>> {
        let mut per_ray = Vec::with_capacity(request.rays.len());
        for ray in &request.rays {
            let mut hits = Vec::new();
            for face in 0..request.body.faces.len() {
                if let Err(unanswered) = face_hits(cx, &request.body, face, ray, &mut hits) {
                    return unanswered;
                }
            }
            per_ray.push(settled(hits, ray, cx.accuracy()));
        }
        Attempt::Done(per_ray)
    }
}
