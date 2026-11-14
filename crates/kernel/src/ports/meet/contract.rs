//! What every meet rule must satisfy: each reported parameter is a point of the surface,
//! and no crossing inside the inspected range is missed.

use super::{MeetRequest, Meeting};
use krly3d_core::Accuracy;

/// `range` is a parameter interval whose ends are not on the surface; it is scanned in
/// `steps` steps for sign changes of the distance, as an oracle independent of the rule.
pub fn check(
    meeting: &Meeting,
    request: &MeetRequest,
    range: (f64, f64),
    steps: u32,
    accuracy: &Accuracy,
) -> Result<(), String> {
    let distance = |t: f64| request.surface.distance(&request.curve.point(t));
    let (from, to) = range;
    let scanned = (0..=steps).map(|step| distance(from + (to - from) * f64::from(step) / f64::from(steps)));
    match meeting {
        Meeting::Within => match scanned.map(f64::abs).find(|gap| *gap > accuracy.length()) {
            Some(gap) => {
                Err(format!("the curve is reported to lie in the surface but is {gap} away from it"))
            }
            None => Ok(()),
        },
        Meeting::At { parameters, .. } => {
            if !parameters.is_sorted() {
                return Err(format!("parameters are not ascending: {parameters:?}"));
            }
            if let Some(stray) = parameters.iter().find(|t| distance(**t).abs() > accuracy.length()) {
                return Err(format!("the point at t={stray} is {} off the surface", distance(*stray)));
            }
            let signs: Vec<bool> = scanned.map(|value| value > 0.0).collect();
            let crossings = signs.windows(2).filter(|pair| pair[0] != pair[1]).count();
            let reported = parameters.iter().filter(|t| (from..=to).contains(*t)).count();
            if crossings > reported || (reported - crossings) % 2 != 0 {
                return Err(format!(
                    "{crossings} crossings were scanned in {range:?} but {reported} parameters are reported"
                ));
            }
            Ok(())
        }
    }
}
