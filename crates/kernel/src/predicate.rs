//! Claims about a body, written as text: what an expectation states and a watcher watches.
//!
//! ```text
//! volume>=100          a measure against a number
//! faces=+1             a measure against its value before the operation
//! volume=-502.65±0.01  the same, with a stated tolerance
//! adds=hole:side       face names that the operation adds
//! removes=top          face names that are gone after it
//! keeps=front,back     face names that are there before and after
//! contains=20,20,50    the point lies in the material or on its boundary
//! meets=frame          the body has a point in common with the node `frame`
//! iou:vis:top>=0.95    the body looks like the drawing `top` of the peer `vis` (overlap)
//! ```
//!
//! A number with a sign states a change, measured against the body the tool was given.
//! A number claims what it shows: without a stated tolerance, `502.65` is met by anything
//! that rounds to it.
//!
//! The claims about where a body is say what a placement was meant to do, so that a
//! wrong turn or a wrong sign is refused when it is made, not found later in a picture.

use crate::context::Context;
use crate::ports::likeness::{drawing_of, likeness, LikenessRequest};
use crate::ports::measure::measure;
use crate::ports::peer::ask;
use crate::ports::probe::{cast, side, Ray, Side};
use crate::ports::shape::Leaf;
use crate::schema::Params;
use crate::topo::Body;
pub use krly3d_core::claim::Relation;
use krly3d_core::claim::{Amount, Phrase};
use krly3d_core::math::{Point3, Uv};
use krly3d_core::{text, Attempt, Error};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The likeness plugin a claim asks.
const CLAIMED: &str = "raster";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Naming {
    Adds,
    Removes,
    Keeps,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Claim {
    Measure {
        measure: String,
        relation: Relation,
        value: f64,
        tolerance: f64,
        /// Whether the value is a change against the body before the operation.
        change: bool,
    },
    Names {
        naming: Naming,
        names: Vec<String>,
    },
    /// The point lies in the material or on its boundary.
    Contains {
        point: Point3,
    },
    /// The body has a point in common with the body of another node.
    Meets {
        node: String,
    },
    /// The body's silhouette overlaps a drawing a peer holds by at least so much.
    Likeness {
        peer: String,
        drawing: String,
        relation: Relation,
        value: f64,
        tolerance: f64,
    },
}

/// A claim with whether it holds and what was found instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    /// The node version the claim is about, as `name@version`.
    pub node: String,
    /// The claim as it was written.
    pub claim: String,
    pub holds: bool,
    pub found: String,
}

impl Claim {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let malformed = || {
            Error::input(format!(
                "`{text}` is not a claim; write it like `volume>=100`, `faces=+1`, `adds=name` or `iou:vis:top>=0.95`"
            ))
        };
        let Phrase { subject, relation, value } = Phrase::parse(text).ok_or_else(malformed)?;
        if let Some(against) = subject.strip_prefix("iou:") {
            let (peer, drawing) = against
                .split_once(':')
                .filter(|(peer, drawing)| !peer.is_empty() && !drawing.is_empty())
                .ok_or_else(malformed)?;
            let amount = Amount::parse(value, relation).ok_or_else(malformed)?;
            return Ok(Self::Likeness {
                peer: peer.to_string(),
                drawing: drawing.to_string(),
                relation,
                value: amount.value,
                tolerance: amount.tolerance,
            });
        }
        match (subject, relation) {
            ("contains", Relation::Equal) => {
                let numbers: Option<Vec<f64>> =
                    value.split(',').map(|part| part.trim().parse().ok()).collect();
                return match numbers.as_deref() {
                    Some([x, y, z]) if [x, y, z].iter().all(|number| number.is_finite()) => {
                        Ok(Self::Contains { point: Point3::new(*x, *y, *z) })
                    }
                    _ => Err(malformed()),
                };
            }
            ("meets", Relation::Equal) => return Ok(Self::Meets { node: value.to_string() }),
            ("contains" | "meets", _) => return Err(malformed()),
            _ => {}
        }
        let naming = match subject {
            "adds" => Some(Naming::Adds),
            "removes" => Some(Naming::Removes),
            "keeps" => Some(Naming::Keeps),
            _ => None,
        };
        match naming {
            Some(naming) if relation == Relation::Equal => {
                Ok(Self::Names { naming, names: value.split(',').map(str::to_string).collect() })
            }
            Some(_) => Err(malformed()),
            None => number(subject, relation, value).ok_or_else(malformed),
        }
    }

    /// Whether the claim holds for `after`, the body an operation made, given `before`,
    /// the body it was given, and `node`, which finds the body of another node by name;
    /// and what was found, for the message when it does not hold.
    pub fn judge(
        &self,
        cx: &Context,
        before: Option<&Arc<Body>>,
        after: &Arc<Body>,
        node: &dyn Fn(&str) -> Option<Arc<Body>>,
    ) -> Result<(bool, String), Error> {
        match self {
            Self::Contains { point } => Ok(match side(cx, after, point) {
                Attempt::Done(Side::Inside) => (true, "inside".into()),
                Attempt::Done(Side::Boundary) => (true, "on the boundary".into()),
                Attempt::Done(Side::Outside) => (false, "outside".into()),
                Attempt::Declined(reason) => (false, format!("cannot tell: {reason}")),
                Attempt::Failed(error) => return Err(error),
            }),
            Self::Meets { node: name } => {
                let other =
                    node(name).ok_or_else(|| Error::not_found(format!("there is no node `{name}`")))?;
                Ok(match common_point(cx, after, &other)? {
                    Some(point) => (true, format!("at {}", text::point(&point))),
                    None => (false, "?no common point among the vertices, edges and faces".into()),
                })
            }
            Self::Likeness { peer, drawing, relation, value, tolerance } => {
                let reply =
                    ask(cx, peer, "polygons", Params::new().with("node", drawing.as_str())).into_result()?;
                let request = LikenessRequest {
                    shapes: vec![Leaf::of(Arc::clone(after))],
                    drawing: drawing_of(&reply.data)?,
                };
                let found = likeness(cx, CLAIMED, &request).into_result()?;
                let worst = found.worst.first().map_or(String::new(), |station| {
                    format!("; worst {}", station.text(request.drawing.axes))
                });
                Ok((
                    relation.holds(found.iou, *value, *tolerance),
                    format!("?IoU {}{worst}", text::num(found.iou)),
                ))
            }
            Self::Measure { measure: name, relation, value, tolerance, change } => {
                let now = measure(cx, name, after).into_result()?;
                let earlier = match (change, before) {
                    (true, Some(before)) => measure(cx, name, before).into_result()?.value,
                    _ => 0.0,
                };
                let actual = now.value - earlier;
                let holds = relation.holds(actual, *value, *tolerance);
                let sign = if *change && actual >= 0.0 { "+" } else { "" };
                Ok((holds, format!("{}{sign}{}", now.exact.symbol(), text::num(actual))))
            }
            Self::Names { naming, names } => {
                let (was, is) = (face_names(before), face_names(Some(after)));
                let fails = |name: &&String| match naming {
                    Naming::Adds => was.contains(name.as_str()) || !is.contains(name.as_str()),
                    Naming::Removes => !was.contains(name.as_str()) || is.contains(name.as_str()),
                    Naming::Keeps => !was.contains(name.as_str()) || !is.contains(name.as_str()),
                };
                let failing: Vec<&str> = names.iter().filter(fails).map(String::as_str).collect();
                let found = if failing.is_empty() {
                    "as stated".to_string()
                } else {
                    format!("not so for {}", failing.join(", "))
                };
                Ok((failing.is_empty(), found))
            }
        }
    }
}

/// A point the two bodies have in common, searched where one is sure to be when they
/// meet at all, except where only curved edges and faces cross: a vertex, a point along an
/// edge or a point of a face of one in the other, or a straight edge of one passing through
/// a face of the other. A point found proves the bodies meet; none found is only as good as
/// that search.
fn common_point(cx: &Context, first: &Arc<Body>, second: &Arc<Body>) -> Result<Option<Point3>, Error> {
    for (one, other) in [(first, second), (second, first)] {
        let mut candidates: Vec<Point3> = one.vertices.clone();
        candidates
            .extend(one.edges.iter().flat_map(|edge| [0.25, 0.5, 0.75].map(|fraction| edge.point(fraction))));
        candidates.extend(
            one.faces
                .iter()
                .filter(|face| face.loops.is_empty())
                .map(|face| face.surface.point(Uv::origin())),
        );
        for point in candidates {
            if within(cx, other, &point)? {
                return Ok(Some(point));
            }
        }
        let mut rays = Vec::new();
        let mut lengths = Vec::new();
        for edge in one.edges.iter().filter(|edge| edge.curve.period().is_none() && edge.ends.is_some()) {
            let (start, end) = (edge.point(0.0), edge.point(1.0));
            let length = (end - start).norm();
            let straight =
                cx.accuracy().is_zero_length((edge.point(0.5) - nalgebra::center(&start, &end)).norm());
            if straight && length > cx.accuracy().length() {
                rays.push(Ray { origin: start, direction: (end - start) / length });
                lengths.push(length);
            }
        }
        if rays.is_empty() {
            continue;
        }
        let hits = cast(cx, other, rays).into_result()?;
        for (hits, length) in hits.iter().zip(lengths) {
            if let Some(hit) = hits.iter().find(|hit| hit.t >= 0.0 && hit.t <= length) {
                return Ok(Some(hit.point));
            }
        }
    }
    Ok(None)
}

fn within(cx: &Context, body: &Arc<Body>, point: &Point3) -> Result<bool, Error> {
    match side(cx, body, point) {
        Attempt::Done(found) => Ok(found != Side::Outside),
        Attempt::Declined(_) => Ok(false),
        Attempt::Failed(error) => Err(error),
    }
}

/// The face names of a body, or none when there is no body.
fn face_names(body: Option<&Arc<Body>>) -> BTreeSet<&str> {
    body.map(|body| body.faces.iter().map(|face| face.name.as_str()).collect()).unwrap_or_default()
}

/// `value` as a number with an optional `±tolerance`.
fn number(measure: &str, relation: Relation, value: &str) -> Option<Claim> {
    let Amount { value, tolerance, change } = Amount::parse(value, relation)?;
    Some(Claim::Measure { measure: measure.to_string(), relation, value, tolerance, change })
}

#[cfg(test)]
mod tests {
    use super::{Claim, Naming, Relation};
    use krly3d_core::math::Point3;

    fn measure(text: &str) -> (String, Relation, f64, f64, bool) {
        match Claim::parse(text).unwrap() {
            Claim::Measure { measure, relation, value, tolerance, change } => {
                (measure, relation, value, tolerance, change)
            }
            other => panic!("not a measure claim: {other:?}"),
        }
    }

    #[test]
    fn a_plain_number_is_an_absolute_value_and_a_signed_one_a_change() {
        assert_eq!(measure("faces=6"), ("faces".to_string(), Relation::Equal, 6.0, 0.5, false));
        assert_eq!(measure("faces=+1"), ("faces".to_string(), Relation::Equal, 1.0, 0.5, true));
        assert!(measure("volume=-502.65").4);
    }

    #[test]
    fn a_number_claims_the_precision_it_shows_unless_a_tolerance_is_stated() {
        assert_eq!(measure("volume=502.65").3, 0.005);
        assert_eq!(measure("volume=5.0265e2").3, 0.005);
        assert_eq!(measure("volume=502.65±0.1").3, 0.1);
        assert_eq!(measure("volume=502.65+-1e-6").3, 1e-6);
    }

    #[test]
    fn every_comparison_is_understood() {
        let relations = ["a<1", "a<=1", "a>1", "a>=1", "a=1"].map(|text| measure(text).1);

        assert_eq!(
            relations,
            [Relation::Less, Relation::AtMost, Relation::Greater, Relation::AtLeast, Relation::Equal]
        );
        assert_eq!(measure("wall>=2").3, 0.0);
    }

    #[test]
    fn claims_about_names_take_a_list() {
        let claim = Claim::parse("adds=hole:side,rim").unwrap();

        assert_eq!(
            claim,
            Claim::Names { naming: Naming::Adds, names: vec!["hole:side".into(), "rim".into()] }
        );
    }

    #[test]
    fn claims_about_place_take_a_point_or_a_node() {
        assert_eq!(
            Claim::parse("contains=20, 20,-5.5").unwrap(),
            Claim::Contains { point: Point3::new(20.0, 20.0, -5.5) }
        );
        assert_eq!(Claim::parse("meets=frame").unwrap(), Claim::Meets { node: "frame".into() });
    }

    #[test]
    fn what_is_not_a_claim_is_rejected() {
        for text in [
            "volume",
            "=3",
            "volume=",
            "volume=big",
            "adds>top",
            "volume=5±-1",
            "contains=1,2",
            "contains>1,2,3",
            "meets<a",
        ] {
            assert!(Claim::parse(text).is_err(), "`{text}` was accepted");
        }
    }

    #[test]
    fn a_likeness_claim_names_the_peer_and_its_drawing() {
        let claim = Claim::parse("iou:vis:top-w>=0.95").unwrap();

        assert_eq!(
            claim,
            Claim::Likeness {
                peer: "vis".into(),
                drawing: "top-w".into(),
                relation: Relation::AtLeast,
                value: 0.95,
                tolerance: 0.0
            }
        );
        assert!(Claim::parse("iou:vis>=0.9").is_err() && Claim::parse("iou::top>=0.9").is_err());
    }
}
