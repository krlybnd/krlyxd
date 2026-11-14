//! The ray-casting renderer: one line of sight per pixel against the exact faces, with
//! no tessellation in between.
//!
//! A face is shaded by how squarely it takes the light of a lamp that travels with the
//! eye, and a dark line is drawn where neighbouring pixels show different faces that meet
//! at a crease or lie at different depths, so that a shape reads at a glance; faces that
//! run smoothly into each other, as the pieces of a bent bar do, show no line between them.

use crate::ports::render::{Image, Paint, Projection, Render, RenderRequest, Seen, CHANNELS};
use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::Vec3;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::probe::{cast, Hit, Ray};
use krly3d_kernel::topo::{body_bounds, Body};
use krly3d_kernel::{Context, Plugin};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::sync::Arc;

pub struct Raycast;

/// The name the renderer is registered under.
pub const NAME: &str = "raycast";

const BACKGROUND: [u8; CHANNELS] = [255, 255, 255];
/// The colour of a face that takes the light squarely.
const MATERIAL: [f64; CHANNELS] = [168.0, 192.0, 224.0];
/// The colour of the lines between faces and around what is drawn.
const OUTLINE: [u8; CHANNELS] = [24, 32, 48];
/// Neighbouring faces whose normals differ by less than this angle (as its cosine) run
/// smoothly into each other and get no line between them.
const CREASE: f64 = 0.94;
/// Neighbouring pixels at depths further apart than this many pixel sizes show one
/// thing in front of another, and get a line.
const DEPTH_STEP: f64 = 3.0;

/// The share of the light that reaches a face however it is turned.
const AMBIENT: f64 = 0.35;
/// Where the lamp stands as the viewer sees it: to the left, above, and on the viewer's
/// side. It stands off the line of sight so that faces turned differently are shaded
/// differently.
const LAMP: [f64; 3] = [-0.35, 0.5, 1.0];

impl Plugin<Render> for Raycast {
    fn id(&self) -> Id {
        Id::new(NAME)
    }

    fn call(&self, cx: &Context, request: &RenderRequest) -> Attempt<Image> {
        let boxes = match boxes(cx, &request.bodies) {
            Attempt::Done(boxes) => boxes,
            Attempt::Declined(reason) => return Attempt::Declined(reason),
            Attempt::Failed(error) => return Attempt::Failed(error),
        };
        let around = boxes.iter().fold(Bounds::empty(), |all, one| all.union(one));
        let projection = match request.view.projection(cx.accuracy(), &around, request.size) {
            Ok(projection) => projection,
            Err(error) => return Attempt::Failed(error),
        };
        let to_lamp =
            (projection.right * LAMP[0] + projection.up * LAMP[1] - projection.toward * LAMP[2]).normalize();
        // The view is fitted to the boxes as they are; for telling what a line of sight can
        // meet they are widened by the length tolerance, so that one which only brushes a
        // box is still put to the body.
        let boxes: Vec<Bounds> = boxes.iter().map(|bounds| bounds.grown(cx.accuracy().length())).collect();
        let scene = Scene { cx, bodies: &request.bodies, boxes: &boxes, projection, to_lamp };
        // A row is one batch of rays for each body; the rows are shared out among the threads.
        let rows: Vec<Attempt<Vec<Option<Sight>>>> =
            (0..projection.size.1).into_par_iter().map(|row| scene.row(row)).collect();
        let mut sights = Vec::with_capacity(projection.size.0 * projection.size.1);
        for row in rows {
            match row {
                Attempt::Done(of_row) => sights.extend(of_row),
                Attempt::Declined(reason) => return Attempt::Declined(reason),
                Attempt::Failed(error) => return Attempt::Failed(error),
            }
        }
        Attempt::Done(painted(projection, &sights, &request.paint))
    }
}

/// The box around each body.
fn boxes(cx: &Context, bodies: &[Arc<Body>]) -> Attempt<Vec<Bounds>> {
    let mut boxes = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.iter().enumerate() {
        match body_bounds(cx, body) {
            Attempt::Done(bounds) => boxes.push(bounds),
            Attempt::Declined(reason) => return Attempt::Declined(format!("body {}: {reason}", index + 1)),
            Attempt::Failed(error) => return Attempt::Failed(error),
        }
    }
    Attempt::Done(boxes)
}

/// What one pixel shows.
#[derive(Clone, Copy)]
struct Sight {
    seen: Seen,
    /// How far along the line of sight the face is met.
    depth: f64,
    /// How much light the face takes there, from `AMBIENT` to 1.
    light: f64,
    /// The outward normal turned towards the eye.
    facing: Vec3,
    /// The size of a pixel at the depth of the hit.
    size: f64,
}

struct Scene<'a> {
    cx: &'a Context,
    bodies: &'a [Arc<Body>],
    boxes: &'a [Bounds],
    projection: Projection,
    /// Unit vector from anywhere in the scene to the lamp.
    to_lamp: Vec3,
}

impl Scene<'_> {
    /// What the pixels of one row show.
    fn row(&self, row: usize) -> Attempt<Vec<Option<Sight>>> {
        let rays: Vec<Ray> =
            (0..self.projection.size.0).map(|column| self.projection.ray(column, row)).collect();
        let mut sights: Vec<Option<Sight>> = vec![None; rays.len()];
        for (index, (body, bounds)) in self.bodies.iter().zip(self.boxes).enumerate() {
            // Only the lines of sight that pass through the box of the body are put to it.
            let passes = |ray: &Ray| bounds.line(&ray.origin, &ray.direction).is_some();
            let columns: Vec<usize> = (0..rays.len()).filter(|column| passes(&rays[*column])).collect();
            if columns.is_empty() || bounds.is_empty() {
                continue;
            }
            let per_ray = match cast(self.cx, body, columns.iter().map(|column| rays[*column]).collect()) {
                Attempt::Done(per_ray) => per_ray,
                Attempt::Declined(reason) => {
                    return Attempt::Declined(format!("body {}, row {row} of the image: {reason}", index + 1))
                }
                Attempt::Failed(error) => return Attempt::Failed(error),
            };
            for (column, hits) in columns.into_iter().zip(per_ray) {
                // A body does not reach beyond its box. The rule that spares the rays which
                // miss the box is applied to every hit as well, so that what a pixel shows
                // does not depend on whether its ray happened to be cast.
                let within = |hit: &&Hit| hit.t > 0.0 && bounds.contains(&hit.point);
                let Some(hit) = hits.iter().find(within) else { continue };
                // Where bodies coincide the later one is seen, so "nearer" includes "as near".
                let nearer =
                    sights[column].is_none_or(|sight| hit.t <= sight.depth + self.cx.accuracy().length());
                if nearer {
                    sights[column] = Some(self.sight(index, hit));
                }
            }
        }
        Attempt::Done(sights)
    }

    fn sight(&self, body: usize, hit: &Hit) -> Sight {
        let normal = self.bodies[body].faces[hit.face].outward_normal(&hit.point);
        // A sheet has no inside: seen from behind it is lit as from the front.
        let facing = if normal.dot(&self.projection.toward) > 0.0 { -normal } else { normal };
        let light = AMBIENT + (1.0 - AMBIENT) * facing.dot(&self.to_lamp).max(0.0);
        // Along a spreading line of sight a pixel grows with the distance from the eye.
        let reach = match self.projection.pinhole {
            Some(pinhole) => (hit.point - pinhole.eye).norm() * self.projection.pixel,
            None => self.projection.pixel,
        };
        Sight { seen: Seen { body, face: hit.face }, depth: hit.t, light, facing, size: reach }
    }
}

fn painted(projection: Projection, sights: &[Option<Sight>], paint: &[Paint]) -> Image {
    let (width, height) = projection.size;
    let seen: Vec<Option<Seen>> = sights.iter().map(|sight| sight.map(|sight| sight.seen)).collect();
    let mut pixels = Vec::with_capacity(sights.len() * CHANNELS);
    for row in 0..height {
        for column in 0..width {
            pixels.extend(match sights[row * width + column] {
                None => BACKGROUND,
                Some(_) if at_a_border(sights, (width, height), column, row, projection) => OUTLINE,
                Some(sight) => {
                    let colour = paint
                        .get(sight.seen.body)
                        .and_then(|paint| paint.of(sight.seen.face))
                        .map_or(MATERIAL, |colour| colour.map(f64::from));
                    colour.map(|channel| (channel * sight.light).round() as u8)
                }
            });
        }
    }
    Image { projection, pixels, seen }
}

/// Whether a pixel beside this one, or above or below it, shows something else: nothing,
/// or another face across a crease or at another depth.
fn at_a_border(
    sights: &[Option<Sight>],
    size: (usize, usize),
    column: usize,
    row: usize,
    projection: Projection,
) -> bool {
    let Some(here) = sights[row * size.0 + column] else { return false };
    // A step of the depth along a line of sight is a step across the image of this size.
    let along = projection.toward.norm();
    let differs = |column: usize, row: usize| match sights[row * size.0 + column] {
        None => true,
        Some(there) if there.seen == here.seen => false,
        Some(there) => {
            let step = (there.depth - here.depth).abs() * along;
            there.facing.dot(&here.facing) < CREASE || step > DEPTH_STEP * here.size.max(there.size)
        }
    };
    (column > 0 && differs(column - 1, row))
        || (column + 1 < size.0 && differs(column + 1, row))
        || (row > 0 && differs(column, row - 1))
        || (row + 1 < size.1 && differs(column, row + 1))
}

#[cfg(test)]
mod tests {
    use super::NAME;
    use crate::ports::render::{contract, render, Frame, Image, Paint, RenderRequest, Seen, View, CHANNELS};
    use crate::testing::context;
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::Attempt;
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::Context;
    use krly3d_std::testing::made;
    use std::collections::BTreeMap;
    use std::f64::consts::PI;
    use std::sync::Arc;

    const ISO: [f64; 3] = [-1.0, 1.0, -1.0];
    const FRONT: [f64; 3] = [0.0, 1.0, 0.0];

    fn plate(cx: &Context) -> Arc<Body> {
        made(cx, "box", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0), &[])
    }

    fn request(bodies: &[&Arc<Body>], toward: [f64; 3], frame: Frame, size: (usize, usize)) -> RenderRequest {
        let view = View { toward: Vec3::from(toward), up: Vec3::z(), frame };
        RenderRequest {
            bodies: bodies.iter().map(|body| Arc::clone(body)).collect(),
            view,
            size,
            paint: Vec::new(),
        }
    }

    /// How many pixels show each face, by the name of the face.
    fn shares(image: &Image, bodies: &[&Arc<Body>]) -> BTreeMap<String, usize> {
        let mut shares = BTreeMap::new();
        for seen in image.seen.iter().flatten() {
            *shares.entry(bodies[seen.body].faces[seen.face].name.clone()).or_insert(0) += 1;
        }
        shares
    }

    fn colour(image: &Image, column: usize, row: usize) -> [u8; 3] {
        let at = (row * image.width() + column) * CHANNELS;
        [image.pixels[at], image.pixels[at + 1], image.pixels[at + 2]]
    }

    /// The colour of a pixel in the middle of the pixels that show the named face.
    fn colour_of(image: &Image, body: &Body, name: &str) -> [u8; 3] {
        let face = body.face(name).unwrap().0;
        let shown: Vec<usize> =
            (0..image.seen.len()).filter(|at| image.seen[*at] == Some(Seen { body: 0, face })).collect();
        let (columns, rows): (Vec<usize>, Vec<usize>) =
            shown.iter().map(|at| (at % image.width(), at / image.width())).unzip();
        let middle = |of: &[usize]| of.iter().sum::<usize>() / of.len();
        colour(image, middle(&columns), middle(&rows))
    }

    #[test]
    fn a_painted_body_shows_its_colour_shaded_and_a_painted_face_its_own() {
        let cx = context();
        let plate = plate(&cx);
        let mut asked = request(&[&plate], ISO, Frame::Fit, (200, 150));
        let mut faces = BTreeMap::new();
        faces.insert(plate.face("top").unwrap().0, [40, 200, 60]);
        asked.paint = vec![Paint { body: Some([200, 40, 40]), faces }];

        let image = render(&cx, NAME, &asked).into_result().unwrap();

        let [red, green, blue] = colour_of(&image, &plate, "front");
        assert!(red > 2 * green && green == blue, "front {:?}", [red, green, blue]);
        let [red, green, blue] = colour_of(&image, &plate, "top");
        assert!(green > 3 * red && green > 3 * blue, "top {:?}", [red, green, blue]);
    }

    #[test]
    fn a_box_seen_from_the_front_shows_its_front_over_exactly_the_pixels_it_covers() {
        let cx = context();
        let plate = plate(&cx);
        let window = Frame::Window { center: Point3::new(0.0, 0.0, 5.0), height: 25.0 };

        let image = contract::check(&cx, NAME, &request(&[&plate], FRONT, window, (400, 100))).unwrap();

        assert_eq!(shares(&image, &[&plate]), BTreeMap::from([("front".to_string(), 240 * 40)]));
    }

    #[test]
    fn a_box_seen_from_a_corner_above_shows_its_top_front_and_right() {
        let cx = context();
        let plate = plate(&cx);

        let image = contract::check(&cx, NAME, &request(&[&plate], ISO, Frame::Fit, (160, 120))).unwrap();

        let names: Vec<String> = shares(&image, &[&plate]).into_keys().collect();
        assert_eq!(names, ["front", "right", "top"]);
    }

    #[test]
    fn the_outline_of_a_ball_encloses_the_area_of_its_great_circle() {
        let cx = context();
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0).with("at", [1.0, 2.0, 3.0]), &[]);

        let image = contract::check(&cx, NAME, &request(&[&ball], ISO, Frame::Fit, (200, 200))).unwrap();

        let pixel = image.projection.pixel;
        let covered = image.seen.iter().flatten().count() as f64 * pixel * pixel;
        let rim_pixels = 2.0 * PI * 5.0 * pixel;
        assert!((covered - PI * 25.0).abs() < rim_pixels, "{covered} against {}", PI * 25.0);
    }

    #[test]
    fn a_fitted_view_leaves_the_rim_of_the_image_empty() {
        let cx = context();
        let plate = plate(&cx);

        let image = contract::check(&cx, NAME, &request(&[&plate], ISO, Frame::Fit, (80, 60))).unwrap();

        let on_rim = |at: usize| [0, 79].contains(&(at % 80)) || [0, 59].contains(&(at / 80));
        assert!((0..image.seen.len()).filter(|at| on_rim(*at)).all(|at| image.seen[at].is_none()));
        assert!(image.seen.iter().flatten().count() > 0);
    }

    #[test]
    fn the_more_a_face_is_turned_to_the_lamp_above_and_to_the_left_the_brighter_it_is() {
        let cx = context();
        let plate = plate(&cx);

        let image = contract::check(&cx, NAME, &request(&[&plate], ISO, Frame::Fit, (160, 120))).unwrap();

        let brightness = |name: &str| u32::from(colour_of(&image, &plate, name)[0]);
        assert!(brightness("top") > brightness("front") + 20, "top against front");
        assert!(brightness("front") > brightness("right") + 20, "front against right");
    }

    #[test]
    fn a_curved_face_grows_darker_as_it_turns_from_the_lamp() {
        let cx = context();
        let ball = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);

        let image = contract::check(&cx, NAME, &request(&[&ball], FRONT, Frame::Fit, (120, 120))).unwrap();

        let toward_the_lamp = u32::from(colour(&image, 50, 45)[0]);
        let away_from_it = u32::from(colour(&image, 95, 95)[0]);
        assert!(toward_the_lamp > away_from_it + 40, "{toward_the_lamp} against {away_from_it}");
    }

    #[test]
    fn faces_that_run_smoothly_into_each_other_show_no_line_between_them() {
        let cx = context();
        let points: Vec<krly3d_kernel::schema::Value> =
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0].iter().map(|value| (*value).into()).collect();
        let bar = made(
            &cx,
            "pipe",
            Params::new()
                .with("r", 1.0)
                .with("bend", 4.0)
                .with("points", krly3d_kernel::schema::Value::List(points)),
            &[],
        );

        let image = render(&cx, NAME, &request(&[&bar], [-0.3, 1.0, -0.5], Frame::Fit, (200, 200)))
            .into_result()
            .unwrap();

        // Pixels inside the bar where the straight run meets the bend are painted, not lined.
        let name = |seen: Option<Seen>| seen.map(|seen| bar.faces[seen.face].name.clone());
        let mut seams = 0;
        for row in 1..199 {
            for column in 1..198 {
                // Beside the outline of the bar a pixel is lined for the outline's sake.
                let inside = [
                    (column - 1, row),
                    (column + 2, row),
                    (column, row - 1),
                    (column, row + 1),
                    (column + 1, row - 1),
                    (column + 1, row + 1),
                ]
                .iter()
                .all(|(c, r)| image.seen[r * 200 + c].is_some());
                if !inside {
                    continue;
                }
                let (here, there) =
                    (name(image.seen[row * 200 + column]), name(image.seen[row * 200 + column + 1]));
                if matches!(
                    (here.as_deref(), there.as_deref()),
                    (Some("run1"), Some("bend1")) | (Some("bend1"), Some("run1"))
                ) {
                    seams += 1;
                    assert_ne!(colour(&image, column, row), [24, 32, 48], "a line at {column},{row}");
                }
            }
        }
        assert!(seams > 0, "the seam was not in view");
    }

    #[test]
    fn a_dark_line_is_drawn_where_two_faces_meet_and_around_the_body() {
        let cx = context();
        let plate = plate(&cx);
        let window = Frame::Window { center: Point3::new(0.0, 0.0, 5.0), height: 25.0 };
        let straight_on = request(&[&plate], FRONT, window, (400, 100));
        let across_an_edge = request(&[&plate], [0.0, 1.0, -1.0], Frame::Fit, (200, 100));

        let flat = render(&cx, NAME, &straight_on).into_result().unwrap();
        let folded = render(&cx, NAME, &across_an_edge).into_result().unwrap();

        // Straight on, the front covers columns 80 to 319 and rows 30 to 69.
        let (rim, inside, outside) = (colour(&flat, 80, 50), colour(&flat, 200, 50), colour(&flat, 79, 50));
        assert!(rim[0] < inside[0] / 2 && outside == [255, 255, 255], "{rim:?} {inside:?} {outside:?}");
        // From above the front, the top lies over the front and a line parts the two.
        let column: Vec<[u8; 3]> = (0..100).map(|row| colour(&folded, 100, row)).collect();
        let seen: Vec<_> = (0..100).map(|row| folded.seen[row * 200 + 100]).collect();
        let fold = (1..100)
            .find(|row| seen[*row].is_some() && seen[*row - 1].is_some() && seen[*row] != seen[*row - 1]);
        let fold = fold.unwrap();
        assert!(column[fold][0] < column[fold + 3][0] / 2 && column[fold - 1][0] < column[fold - 4][0] / 2);
    }

    #[test]
    fn where_two_bodies_coincide_the_later_one_is_seen() {
        let cx = context();
        let (first, second) = (plate(&cx), plate(&cx));

        let image =
            render(&cx, NAME, &request(&[&first, &second], ISO, Frame::Fit, (40, 30))).into_result().unwrap();

        assert!(image.seen.iter().flatten().all(|seen| seen.body == 1));
    }

    #[test]
    fn a_nearer_body_hides_the_one_behind_it() {
        let cx = context();
        let wall = plate(&cx);
        let block = Params::new().with("w", 10.0).with("d", 10.0).with("h", 10.0);
        let behind = made(&cx, "box", block.clone().with("at", [0.0, 100.0, 0.0]), &[]);
        let before = made(&cx, "box", block.with("at", [0.0, -100.0, 0.0]), &[]);
        let window = Frame::Window { center: Point3::new(0.0, 0.0, 5.0), height: 20.0 };

        let image = contract::check(&cx, NAME, &request(&[&behind, &wall, &before], FRONT, window, (80, 20)))
            .unwrap();

        let middle = image.seen[10 * 80 + 40].unwrap();
        let beside = image.seen[10 * 80 + 10].unwrap();
        assert_eq!((middle.body, beside.body), (2, 1));
        assert!(image.seen.iter().flatten().all(|seen| seen.body != 0));
    }

    #[test]
    fn a_sheet_is_drawn_from_either_side() {
        let cx = context();
        let sheet = made(&cx, "rectangle", Params::new().with("w", 4.0).with("d", 2.0), &[]);

        let above =
            render(&cx, NAME, &request(&[&sheet], [0.3, 0.2, -1.0], Frame::Fit, (60, 40))).into_result();
        let below =
            render(&cx, NAME, &request(&[&sheet], [0.3, 0.2, 1.0], Frame::Fit, (60, 40))).into_result();

        let lit = |image: &Image| image.seen.iter().flatten().count();
        assert!(lit(&above.unwrap()) > 100 && lit(&below.unwrap()) > 100);
    }

    #[test]
    fn every_kind_of_solid_is_drawn_from_every_side() {
        let cx = context();
        let section = Params::new().with("path", "M 2,0 L 5,0 L 5,4 L 2,4 Z").with("plane", "xz");
        let tube = made(&cx, "profile", section, &[]);
        let solids = [
            made(&cx, "cylinder", Params::new().with("r", 3.0).with("h", 10.0), &[]),
            made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0), &[]),
            made(&cx, "torus", Params::new().with("ring", 6.0).with("tube", 2.0), &[]),
            made(&cx, "revolve", Params::new().with("angle", 250.0), &[tube]),
        ];
        let sides = [ISO, FRONT, [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.2, 0.1, 1.0]];
        for (index, solid) in solids.iter().enumerate() {
            for side in sides {
                let drawn = contract::check(&cx, NAME, &request(&[solid], side, Frame::Fit, (48, 36)));

                let lit = drawn.map(|image| image.seen.iter().flatten().count());
                assert!(lit.as_ref().is_ok_and(|lit| *lit > 0), "solid {index} from {side:?}: {lit:?}");
            }
        }
    }

    #[test]
    fn a_cone_is_drawn_up_to_its_apex_and_no_further() {
        // Checked by its outline and not against the probe: for a cone made to its apex
        // the probe also reports the mirror image of the cone beyond the apex as hits on
        // `side`. Those hits lie outside the box of the body, which is why they are not drawn.
        let cx = context();
        let cone = made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]);
        let window = Frame::Window { center: Point3::new(0.0, 0.0, 9.0), height: 30.0 };

        let image = render(&cx, NAME, &request(&[&cone], FRONT, window, (200, 300))).into_result().unwrap();

        let pixel = image.projection.pixel;
        let covered = image.seen.iter().flatten().count() as f64 * pixel * pixel;
        let outline_pixels = (8.0 + 2.0 * 97.0_f64.sqrt()) * pixel;
        let above_the_apex = image.seen[..150 * 200].iter().flatten().count();
        assert!((covered - 4.0 * 9.0).abs() < outline_pixels, "{covered} against 36");
        assert_eq!(above_the_apex, 0);
    }

    #[test]
    fn nothing_to_draw_is_refused_rather_than_drawn_as_an_empty_image() {
        let cx = context();

        let drawn = render(&cx, NAME, &request(&[], ISO, Frame::Fit, (40, 30)));

        assert!(matches!(drawn, Attempt::Failed(_)), "{drawn:?}");
    }
}
