//! What every section rule must satisfy: each curve it returns lies in both surfaces.

use super::{Cut, SectionRequest};
use krly3d_core::Accuracy;

/// `parameters` are where each returned curve is sampled.
pub fn check(
    cut: &Cut,
    request: &SectionRequest,
    parameters: &[f64],
    accuracy: &Accuracy,
) -> Result<(), String> {
    let (Cut::Curves { curves, .. } | Cut::Touching { curves, .. }) = cut else {
        return Ok(());
    };
    for curve in curves {
        for &t in parameters {
            let point = curve.point(t);
            for (surface, name) in [(&request.first, "first"), (&request.second, "second")] {
                let gap = surface.distance(&point);
                if !accuracy.is_zero_length(gap) {
                    return Err(format!("the {} at t={t} is {gap} off the {name} surface", curve.summary()));
                }
            }
        }
    }
    Ok(())
}
