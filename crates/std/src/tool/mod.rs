//! Tools: every way of making or changing a body.

pub mod blend;
pub mod boolean;
pub mod circle;
pub mod cone;
pub mod cuboid;
pub mod cylinder;
pub mod ellipsoid;
pub mod extrude;
pub mod group;
pub mod loft;
pub mod mirror;
pub mod pattern;
pub mod pipe;
pub mod profile;
pub mod rectangle;
pub mod revolve;
pub mod sphere;
pub mod taper;
pub mod torus;
pub mod transform;

use krly3d_core::math::frame::turning;
use krly3d_core::math::{Point3, Sim3, Vec3};
use krly3d_core::{Accuracy, Attempt, Error, Id};
use krly3d_kernel::ports::tool::{run, Tool};
use krly3d_kernel::schema::{Kind, Param, Params};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, KernelBuilder};
use nalgebra::Translation3;
use std::sync::Arc;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Tool>(profile::Profile)
        .plugin::<Tool>(rectangle::Rectangle)
        .plugin::<Tool>(circle::Circle)
        .plugin::<Tool>(extrude::Extrude)
        .plugin::<Tool>(taper::Taper)
        .plugin::<Tool>(loft::Loft)
        .plugin::<Tool>(revolve::Revolve)
        .plugin::<Tool>(cuboid::Cuboid)
        .plugin::<Tool>(cylinder::Cylinder)
        .plugin::<Tool>(cone::Cone)
        .plugin::<Tool>(sphere::Sphere)
        .plugin::<Tool>(ellipsoid::Ellipsoid)
        .plugin::<Tool>(torus::Torus)
        .plugin::<Tool>(pipe::Pipe)
        .plugin::<Tool>(transform::Transform)
        .plugin::<Tool>(mirror::Mirror)
        .plugin::<Tool>(group::Group)
        .plugin::<Tool>(pattern::Pattern);
    for boolean in boolean::Boolean::all() {
        builder.plugin::<Tool>(boolean);
    }
    for blend in blend::Blend::all() {
        builder.plugin::<Tool>(blend);
    }
}

/// The two placement parameters every drawing tool shares.
fn placement() -> [Param; 2] {
    [
        Param::optional("plane", Kind::Text, "xy", "the plane drawn in: xy (the ground), xz or yz (upright)"),
        Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "where the origin of that plane goes"),
    ]
}

/// Where a solid built about an upright axis goes: the centre of its base, and the
/// direction it stands along. Every tool that builds about an axis takes the same three
/// parameters and reads them here, so that no caller ever works out a turn by hand.
struct Standing {
    at: Point3,
    axis: Vec3,
    /// The distance from `at` to `to`, when the axis was given by its end point.
    reach: Option<f64>,
}

/// `at` and `axis`, and with `reaching` also `to`, the end of the axis.
fn standing(reaching: bool) -> Vec<Param> {
    let mut params = vec![
        Param::optional("at", Kind::Triple, [0.0, 0.0, 0.0], "centre of the base"),
        Param::alternative(
            "axis",
            Kind::Triple,
            "direction the solid stands along; upright (0,0,1) when left out",
        ),
    ];
    if reaching {
        params.push(Param::alternative(
            "to",
            Kind::Triple,
            "where the axis ends: sets the direction and the height at once, in place of `axis` and `h`",
        ));
    }
    params
}

fn standing_from(params: &Params, accuracy: &Accuracy) -> Result<Standing, Error> {
    let at = params.point("at")?;
    let along = |direction: Vec3, what: &str| {
        let length = direction.norm();
        if length > accuracy.length() {
            Ok((direction / length, length))
        } else {
            Err(Error::input(format!("{what} has no direction")))
        }
    };
    match (params.get("axis"), params.get("to")) {
        (Some(_), Some(_)) => Err(Error::input("give the direction by `axis` or by `to`, not both")),
        (None, Some(_)) => {
            let (axis, reach) = along(params.point("to")? - at, "the axis from `at` to `to`")?;
            Ok(Standing { at, axis, reach: Some(reach) })
        }
        (Some(_), None) => Ok(Standing { at, axis: along(params.vector("axis")?, "`axis`")?.0, reach: None }),
        (None, None) => Ok(Standing { at, axis: Vec3::z(), reach: None }),
    }
}

impl Standing {
    /// The height: `h`, or the length of the axis given by `to`, but not both.
    fn height(&self, params: &Params) -> Result<f64, Error> {
        match (params.get("h"), self.reach) {
            (Some(_), Some(_)) => Err(Error::input("give the height by `h` or by `to`, not both")),
            (Some(_), None) => params.number("h"),
            (None, Some(reach)) => Ok(reach),
            (None, None) => Err(Error::input("give the height `h`, or `to` for where the axis ends")),
        }
    }

    /// A body built upright on the origin, stood on `at` along the axis.
    fn stand(&self, made: Attempt<Body>) -> Attempt<Body> {
        let Attempt::Done(body) = made else { return made };
        if self.axis == Vec3::z() && self.at == Point3::origin() {
            return Attempt::Done(body);
        }
        turning(&Vec3::z(), &self.axis)
            .map(|turn| body.transformed(&Sim3::from_parts(Translation3::from(self.at.coords), turn, 1.0)))
            .into()
    }
}

/// Runs tools one after the other, each taking the body made by the one before. This is
/// what makes a primitive a recipe of a few lines instead of code of its own.
fn recipe(cx: &Context, steps: Vec<(&str, Params)>) -> Attempt<Body> {
    let mut made: Option<Body> = None;
    for (tool, params) in steps {
        let inputs: Vec<Arc<Body>> = made.take().map(Arc::new).into_iter().collect();
        match run(cx, &Id::new(tool), &params, &inputs) {
            Attempt::Done(body) => made = Some(body),
            other => return other,
        }
    }
    made.map_or_else(|| Attempt::declined("an empty recipe makes nothing"), Attempt::Done)
}

/// The named parameters of `given`, for handing on to a step of a recipe.
fn carry(given: &Params, names: &[&str]) -> Params {
    let mut carried = Params::new();
    for name in names {
        if let Some(value) = given.get(name) {
            carried.set(name, value.clone());
        }
    }
    carried
}

#[cfg(any(test, feature = "testing"))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test support: a failure here is a failed test
pub mod testing {
    use crate::standard;
    use krly3d_core::{Accuracy, Id};
    use krly3d_kernel::ports::content::{content, contract as content_contract, ContentRequest};
    use krly3d_kernel::ports::measure::{contract as measure_contract, measure};
    use krly3d_kernel::ports::tool::{contract as tool_contract, run};
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::Context;
    use std::sync::Arc;

    pub fn context() -> Context {
        let mut builder = standard();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        builder.context()
    }

    /// Runs a tool through the standard registry and checks the tool contract on what it made.
    pub fn made(cx: &Context, tool: &str, params: Params, inputs: &[Arc<Body>]) -> Arc<Body> {
        let id = Id::new(tool);
        let body = run(cx, &id, &params, inputs).into_result().unwrap();
        tool_contract::check_result(cx, &id, &body).unwrap();
        Arc::new(body)
    }

    /// A measure of a body, after checking the contracts of the measure and of the
    /// content rule behind every face.
    pub fn measured(cx: &Context, name: &str, dimension: i32, body: &Arc<Body>) -> f64 {
        for face in 0..body.faces.len() {
            let request = ContentRequest { body: Arc::clone(body), face };
            let stated = content(cx, &request).into_result().unwrap();
            content_contract::check(cx, &request, &stated).unwrap();
        }
        let stated = measure(cx, name, body).into_result().unwrap();
        measure_contract::check(cx, name, body, &stated, dimension).unwrap();
        stated.value
    }

    pub fn volume(cx: &Context, body: &Arc<Body>) -> f64 {
        measured(cx, "volume", 3, body)
    }

    pub fn area(cx: &Context, body: &Arc<Body>) -> f64 {
        measured(cx, "area", 2, body)
    }

    pub fn face_names(body: &Body) -> Vec<&str> {
        let mut names: Vec<&str> = body.faces.iter().map(|face| face.name.as_str()).collect();
        names.sort_unstable();
        names
    }

    /// Whether two numbers agree to the relative accuracy asked of closed forms.
    pub fn agree(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1.0)
    }
}
