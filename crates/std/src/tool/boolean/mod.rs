//! Union, difference and intersection of solids.
//!
//! One algorithm for every pair of surfaces. What is specific to a pair lives in the
//! `section` and `meet` rules; nothing here names a kind of surface.
//!
//! 1. `cut`: the curves each pair of faces has in common, kept where they lie in both faces.
//! 2. `split`: every face is divided along the cuts that lie in it.
//! 3. `classify`: each piece is inside the other solid, outside it, or on its boundary.
//! 4. `sew`: the pieces the operation keeps become the result.
//!
//! Contacts that are not crossings — surfaces that touch, edges that run along each
//! other — are declined with the reason, never answered with a wrong body.

mod classify;
mod cut;
mod sew;
mod split;
mod work;

use classify::Class;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::tool::{Tool, ToolRequest};
use krly3d_kernel::schema::Schema;
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;
use work::{Stop, Work};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Union,
    Subtract,
    Intersect,
}

/// One plugin registered once per operation.
pub struct Boolean {
    id: &'static str,
    about: &'static str,
    operation: Operation,
}

impl Boolean {
    pub fn all() -> [Self; 3] {
        [
            Self { id: "union", about: "the first solid joined with the others; faces keep their names, those of the others as node:face", operation: Operation::Union },
            Self { id: "subtract", about: "the first solid with the others cut out of it; the faces they leave are named node:face", operation: Operation::Subtract },
            Self { id: "intersect", about: "what the first solid has in common with all the others", operation: Operation::Intersect },
        ]
    }
}

impl Plugin<Tool> for Boolean {
    fn id(&self) -> Id {
        Id::new(self.id)
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema { about: self.about, inputs: (2, usize::MAX), params: Vec::new() })
    }

    fn call(&self, cx: &Context, request: &ToolRequest) -> Attempt<Body> {
        let Some((first, others)) = request.inputs.split_first() else {
            return Attempt::Failed(Error::input("a boolean needs at least two solids"));
        };
        let mut result = Arc::clone(first);
        for (index, other) in others.iter().enumerate() {
            let source = request.sources.get(index + 1).map_or("", String::as_str);
            match combine(cx, &result, other, self.operation, source) {
                Ok(body) => result = Arc::new(body),
                Err(Stop::Declined(reason)) => return Attempt::Declined(reason),
                Err(Stop::Failed(error)) => return Attempt::Failed(error),
            }
        }
        Attempt::Done(Body::clone(&result))
    }
}

fn combine(
    cx: &Context,
    target: &Arc<Body>,
    tool: &Arc<Body>,
    operation: Operation,
    source: &str,
) -> Result<Body, Stop> {
    if !target.solid || !tool.solid {
        return Err(Stop::Failed(Error::input(
            "a boolean takes solids; a profile has to be extruded or revolved first",
        )));
    }
    let mut work = Work::new(cx.accuracy(), [target, tool]);
    cut::sections(cx, &mut work)?;
    work.split_edges();
    cut::shared_faces(cx, &mut work)?;
    let mut kept = Vec::new();
    for side in [0, 1] {
        let pieces = Arc::new(split::pieces(cx, &work, side)?);
        for piece in 0..pieces.faces.len() {
            let class = classify::classify(cx, &pieces, piece, work.bodies[1 - side])?;
            if let Some(turned) = keeps(operation, side, class) {
                let prefix = if side == 0 {
                    String::new()
                } else {
                    format!("{}:", if source.is_empty() { "tool" } else { source })
                };
                kept.push(sew::Kept {
                    face: pieces.faces[piece].clone(),
                    turned,
                    name: format!("{prefix}{}", pieces.faces[piece].name),
                });
            }
        }
    }
    if kept.is_empty() {
        return Err(Stop::Failed(Error::geometry("nothing is left of the solids")));
    }
    Ok(sew::assemble(&work, kept))
}

/// Whether the operation keeps a piece of the first (`side` 0) or the second solid of the
/// given class, and if so whether turned inside out. A piece lying on the other solid's
/// boundary is kept at most once, as the first solid's.
fn keeps(operation: Operation, side: usize, class: Class) -> Option<bool> {
    match (operation, side, class) {
        (Operation::Union, 0, Class::Outside | Class::OnSame) | (Operation::Union, 1, Class::Outside) => {
            Some(false)
        }
        (Operation::Subtract, 0, Class::Outside | Class::OnOpposite) => Some(false),
        (Operation::Subtract, 1, Class::Inside) => Some(true),
        (Operation::Intersect, 0, Class::Inside | Class::OnSame)
        | (Operation::Intersect, 1, Class::Inside) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::testing::{agree, area, context, face_names, made, volume};
    use krly3d_core::Id;
    use krly3d_kernel::ports::tool::{run, run_named};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::Context;
    use std::f64::consts::PI;
    use std::sync::Arc;

    fn plate(cx: &Context) -> Arc<Body> {
        made(cx, "box", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0), &[])
    }

    fn rod(cx: &Context, radius: f64, height: f64, at: [f64; 3]) -> Arc<Body> {
        made(cx, "cylinder", Params::new().with("r", radius).with("h", height).with("at", at), &[])
    }

    fn ball(cx: &Context, radius: f64, at: [f64; 3]) -> Arc<Body> {
        made(cx, "sphere", Params::new().with("r", radius).with("at", at), &[])
    }

    #[test]
    fn a_rod_through_a_plate_leaves_a_hole_of_its_volume() {
        let cx = context();

        let drilled =
            made(&cx, "subtract", Params::new(), &[plate(&cx), rod(&cx, 4.0, 30.0, [20.0, 15.0, -10.0])]);

        assert_eq!(face_names(&drilled), ["back", "bottom", "front", "left", "right", "tool:side", "top"]);
        assert!(agree(volume(&cx, &drilled), 24_000.0 - 160.0 * PI));
        assert!(agree(area(&cx, &drilled), 2.0 * (2400.0 - 16.0 * PI) + 2000.0 + 80.0 * PI));
    }

    #[test]
    fn a_rod_ending_flush_with_the_plate_drills_the_same_hole() {
        let cx = context();

        let drilled =
            made(&cx, "subtract", Params::new(), &[plate(&cx), rod(&cx, 4.0, 10.0, [20.0, 15.0, 0.0])]);

        assert_eq!(drilled.faces.len(), 7);
        assert!(agree(volume(&cx, &drilled), 24_000.0 - 160.0 * PI));
    }

    #[test]
    fn a_rod_ending_inside_the_plate_leaves_a_blind_hole_with_a_floor() {
        let cx = context();

        let drilled =
            made(&cx, "subtract", Params::new(), &[plate(&cx), rod(&cx, 4.0, 10.0, [0.0, 0.0, 5.0])]);

        assert_eq!(
            face_names(&drilled),
            ["back", "bottom", "front", "left", "right", "tool:bottom", "tool:side", "top"]
        );
        assert!(agree(volume(&cx, &drilled), 24_000.0 - 80.0 * PI));
    }

    #[test]
    fn the_faces_a_tool_leaves_are_named_after_its_node() {
        let cx = context();
        let inputs = [plate(&cx), rod(&cx, 4.0, 30.0, [20.0, 15.0, -10.0])];
        let sources = ["plate".to_string(), "hole".to_string()];

        let drilled =
            run_named(&cx, &Id::new("subtract"), &Params::new(), &inputs, &sources).into_result().unwrap();

        assert!(drilled.face("hole:side").is_some() && drilled.face("top").is_some());
    }

    #[test]
    fn a_peg_standing_on_a_plate_joins_it_where_they_touch() {
        let cx = context();

        let joined = made(&cx, "union", Params::new(), &[plate(&cx), rod(&cx, 3.0, 6.0, [0.0, 0.0, 10.0])]);

        assert_eq!(joined.faces.len(), 8);
        assert!(agree(volume(&cx, &joined), 24_000.0 + 54.0 * PI));
        assert!(agree(area(&cx, &joined), 2.0 * 2400.0 + 2000.0 + 36.0 * PI));
    }

    #[test]
    fn two_overlapping_balls_have_a_lens_in_common() {
        let cx = context();
        let (one, other) = (ball(&cx, 5.0, [0.0, 0.0, 0.0]), ball(&cx, 5.0, [6.0, 0.0, 0.0]));
        // Two caps of height 2 on spheres of radius 5.
        let lens = 2.0 * PI * 4.0 * (15.0 - 2.0) / 3.0;

        let common = made(&cx, "intersect", Params::new(), &[Arc::clone(&one), Arc::clone(&other)]);
        let joined = made(&cx, "union", Params::new(), &[Arc::clone(&one), Arc::clone(&other)]);
        let bitten = made(&cx, "subtract", Params::new(), &[one, other]);

        let whole = 500.0 * PI / 3.0;
        assert!(agree(volume(&cx, &common), lens));
        assert!(agree(volume(&cx, &joined), 2.0 * whole - lens));
        assert!(agree(volume(&cx, &bitten), whole - lens));
    }

    #[test]
    fn a_ball_in_a_cube_is_cut_off_by_all_six_faces() {
        let cx = context();
        let cube = made(&cx, "box", Params::new().with("w", 10.0).with("d", 10.0).with("h", 10.0), &[]);
        // Each face cuts a cap of height 1 off the ball of radius 6.
        let cap = PI * (18.0 - 1.0) / 3.0;

        let common = made(&cx, "intersect", Params::new(), &[cube, ball(&cx, 6.0, [0.0, 0.0, 5.0])]);

        assert_eq!(common.faces.len(), 7);
        assert!(agree(volume(&cx, &common), 288.0 * PI - 6.0 * cap));
    }

    #[test]
    fn two_overlapping_rods_join_along_two_straight_edges() {
        let cx = context();
        let (one, other) = (rod(&cx, 5.0, 10.0, [0.0, 0.0, 0.0]), rod(&cx, 5.0, 10.0, [6.0, 0.0, 0.0]));
        // The overlap of two circles of radius 5 whose centres are 6 apart.
        let overlap = 2.0 * 25.0 * (0.6_f64).acos() - 6.0 * 4.0;

        let joined = made(&cx, "union", Params::new(), &[one, other]);

        assert!(agree(volume(&cx, &joined), (2.0 * 25.0 * PI - overlap) * 10.0));
    }

    #[test]
    fn two_blocks_of_one_height_join_into_one_solid() {
        let cx = context();
        let one = made(&cx, "box", Params::new().with("w", 10.0).with("d", 10.0).with("h", 5.0), &[]);
        let other = made(
            &cx,
            "box",
            Params::new().with("w", 10.0).with("d", 10.0).with("h", 5.0).with("at", [6.0, 4.0, 0.0]),
            &[],
        );

        let joined = made(&cx, "union", Params::new(), &[one, other]);

        assert!(agree(volume(&cx, &joined), (200.0 - 4.0 * 6.0) * 5.0));
    }

    #[test]
    fn solids_apart_stay_two_shells_when_joined_and_leave_the_first_whole_when_subtracted() {
        let cx = context();
        let (one, other) = (ball(&cx, 2.0, [0.0, 0.0, 0.0]), ball(&cx, 2.0, [10.0, 0.0, 0.0]));

        let joined = made(&cx, "union", Params::new(), &[Arc::clone(&one), Arc::clone(&other)]);
        let untouched = made(&cx, "subtract", Params::new(), &[one, other]);

        assert_eq!((joined.faces.len(), untouched.faces.len()), (2, 1));
        assert!(agree(volume(&cx, &joined), 2.0 * 32.0 * PI / 3.0));
    }

    #[test]
    fn a_solid_inside_another_is_cut_out_as_a_cavity() {
        let cx = context();

        let hollow = made(
            &cx,
            "subtract",
            Params::new(),
            &[ball(&cx, 5.0, [0.0, 0.0, 0.0]), ball(&cx, 2.0, [1.0, 0.0, 0.0])],
        );

        assert_eq!(hollow.faces.len(), 2);
        assert!(agree(volume(&cx, &hollow), (500.0 - 32.0) * PI / 3.0));
    }

    #[test]
    fn several_tools_are_applied_one_after_the_other() {
        let cx = context();
        let holes = [rod(&cx, 2.0, 30.0, [-15.0, 0.0, -10.0]), rod(&cx, 3.0, 30.0, [15.0, 0.0, -10.0])];

        let drilled =
            made(&cx, "subtract", Params::new(), &[plate(&cx), Arc::clone(&holes[0]), Arc::clone(&holes[1])]);

        assert!(agree(volume(&cx, &drilled), 24_000.0 - (4.0 + 9.0) * PI * 10.0));
    }

    #[test]
    fn what_has_no_section_rule_is_declined_with_the_reason() {
        let cx = context();
        let lying = made(
            &cx,
            "transform",
            Params::new().with("turn", 90.0).with("about", [0.0, 1.0, 0.0]),
            &[rod(&cx, 3.0, 40.0, [0.0, 0.0, -20.0])],
        );

        let error =
            run(&cx, &Id::new("subtract"), &Params::new(), &[rod(&cx, 5.0, 10.0, [0.0, 0.0, -5.0]), lying])
                .into_result()
                .unwrap_err();

        assert!(error.message.contains("curve of degree four"), "{error}");
    }

    #[test]
    fn solids_with_nothing_in_common_have_no_intersection() {
        let cx = context();
        let inputs = [ball(&cx, 2.0, [0.0, 0.0, 0.0]), ball(&cx, 2.0, [10.0, 0.0, 0.0])];

        let error = run(&cx, &Id::new("intersect"), &Params::new(), &inputs).into_result().unwrap_err();

        assert_eq!(error.message, "nothing is left of the solids");
    }
}
