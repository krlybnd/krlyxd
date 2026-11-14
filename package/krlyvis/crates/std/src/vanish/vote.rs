//! Vanishing points by vote. Every pair of the longest free segments proposes the point
//! their lines meet in; the proposal backed by the most segment length wins, is refined
//! in closed form, and its segments leave the vote. Repeated once per point.
//!
//! Every pair is tried, so the answer does not depend on luck as with random sampling.
//! The refined point minimises the weighted sum of squared distances to the lines of its
//! segments: the eigenvector of the smallest eigenvalue of `sum w l lᵀ`.

use krly3d_core::{Attempt, Id};
use krlyvis_kernel::ports::detect::Segment;
use krlyvis_kernel::ports::vanish::{conditioning, unconditioning, Vanish, VanishRequest, Vanishing};
use krlyvis_kernel::{Context, Plugin};
use nalgebra::{Matrix3, SymmetricEigen, Vector2, Vector3};

pub struct Vote {
    /// The most points to find.
    pub count: usize,
    /// How far, in degrees, a segment may point away from a point and still run to it.
    pub tolerance: f64,
    /// How many of the longest free segments propose points, in pairs.
    pub proposers: usize,
    /// The fewest segments a point must gather.
    pub fewest: usize,
}

impl Default for Vote {
    fn default() -> Self {
        Self { count: 3, tolerance: 2.0, proposers: 100, fewest: 3 }
    }
}

impl Plugin<Vanish> for Vote {
    fn id(&self) -> Id {
        Id::new("vote")
    }

    fn call(&self, _: &Context, request: &VanishRequest) -> Attempt<Vec<Vanishing>> {
        Attempt::Done(self.run(request))
    }
}

/// A segment in conditioned coordinates.
struct Line {
    /// Homogeneous, scaled so that a point's dot product with it is its distance.
    line: Vector3<f64>,
    middle: Vector2<f64>,
    along: Vector2<f64>,
    half: f64,
    /// The length in pixels.
    weight: f64,
}

impl Line {
    fn new(segment: &Segment, condition: &Matrix3<f64>) -> Option<Self> {
        let from = condition * segment.from.to_homogeneous();
        let to = condition * segment.to.to_homogeneous();
        let line = from.cross(&to);
        let scale = line.xy().norm();
        let span = to.xy() - from.xy();
        (scale > f64::EPSILON && span.norm() > f64::EPSILON).then(|| Self {
            line: line / scale,
            middle: (from.xy() + to.xy()) / 2.0,
            along: span.normalize(),
            half: span.norm() / 2.0,
            weight: segment.length(),
        })
    }

    /// Whether the segment runs towards a homogeneous point without the point lying on
    /// the segment itself.
    fn runs_to(&self, point: &Vector3<f64>, cos_tolerance: f64) -> bool {
        let towards = point.xy() - self.middle * point.z;
        let norm = towards.norm();
        if norm <= f64::EPSILON * point.norm() {
            return false;
        }
        let reach = towards.dot(&self.along);
        let on_segment = point.z != 0.0 && (reach / point.z).abs() <= self.half;
        !on_segment && reach.abs() / norm >= cos_tolerance
    }
}

impl Vote {
    fn run(&self, request: &VanishRequest) -> Vec<Vanishing> {
        let condition = conditioning(request.size);
        let lines: Vec<Option<Line>> =
            request.segments.iter().map(|segment| Line::new(segment, &condition)).collect();
        let mut free: Vec<bool> = lines.iter().map(Option::is_some).collect();
        let cos_tolerance = self.tolerance.to_radians().cos();
        let mut found = Vec::new();
        for _ in 0..self.count {
            let Some(mut point) = self.best_proposal(&lines, &free, cos_tolerance) else { break };
            let mut members = gathered(&lines, &free, &point, cos_tolerance);
            // Refining can gather segments the proposal missed; twice settles it.
            for _ in 0..2 {
                if let Some(refined) = refined(&lines, &members) {
                    point = refined;
                    members = gathered(&lines, &free, &point, cos_tolerance);
                }
            }
            if members.len() < self.fewest {
                break;
            }
            for &member in &members {
                free[member] = false;
            }
            let support =
                members.iter().filter_map(|&member| lines[member].as_ref()).map(|line| line.weight).sum();
            found.push(Vanishing {
                point: (unconditioning(request.size) * point).normalize(),
                members,
                support,
            });
        }
        found.sort_by(|a, b| b.support.total_cmp(&a.support));
        found
    }

    /// The point, proposed by a pair of the longest free segments, that the most free
    /// segment length runs to.
    fn best_proposal(
        &self,
        lines: &[Option<Line>],
        free: &[bool],
        cos_tolerance: f64,
    ) -> Option<Vector3<f64>> {
        let mut proposers: Vec<&Line> =
            lines.iter().zip(free).filter(|(_, &free)| free).filter_map(|(line, _)| line.as_ref()).collect();
        proposers.sort_by(|a, b| b.weight.total_cmp(&a.weight));
        proposers.truncate(self.proposers);
        let mut best: Option<(f64, Vector3<f64>)> = None;
        for (index, first) in proposers.iter().enumerate() {
            for second in &proposers[index + 1..] {
                let point = first.line.cross(&second.line);
                let norm = point.norm();
                if norm <= f64::EPSILON {
                    continue;
                }
                let point = point / norm;
                let score = support(lines, free, &point, cos_tolerance);
                if best.is_none_or(|(most, _)| score > most) {
                    best = Some((score, point));
                }
            }
        }
        best.map(|(_, point)| point)
    }
}

fn support(lines: &[Option<Line>], free: &[bool], point: &Vector3<f64>, cos_tolerance: f64) -> f64 {
    lines
        .iter()
        .zip(free)
        .filter_map(|(line, &free)| line.as_ref().filter(|_| free))
        .filter(|line| line.runs_to(point, cos_tolerance))
        .map(|line| line.weight)
        .sum()
}

/// The free segments running to a point, by index.
fn gathered(lines: &[Option<Line>], free: &[bool], point: &Vector3<f64>, cos_tolerance: f64) -> Vec<usize> {
    (0..lines.len())
        .filter(|&index| {
            free[index] && lines[index].as_ref().is_some_and(|line| line.runs_to(point, cos_tolerance))
        })
        .collect()
}

/// The point nearest, in the weighted least-squares sense, to the lines of the members.
fn refined(lines: &[Option<Line>], members: &[usize]) -> Option<Vector3<f64>> {
    if members.len() < 2 {
        return None;
    }
    let scatter = members
        .iter()
        .filter_map(|&member| lines[member].as_ref())
        .fold(Matrix3::zeros(), |sum, line| sum + line.line * line.line.transpose() * line.weight);
    let eigen = SymmetricEigen::new(scatter);
    let smallest = eigen.eigenvalues.imin();
    Some(eigen.eigenvectors.column(smallest).normalize())
}

#[cfg(test)]
mod tests {
    use super::Vote;
    use krlyvis_kernel::ports::vanish::{contract, Vanish};
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn vote_passes_the_contract_of_vanishing_point_methods() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Vanish>(Vote::default());

        contract::check(&builder.context(), "vote").unwrap();
    }
}
