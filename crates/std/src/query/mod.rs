//! Queries: what can be asked about the model. Every answer is a short text for a reader
//! and the same content as data; a query never changes anything.

pub mod bounds;
pub mod brief;
pub mod edges;
pub mod faces;
pub mod find;
pub mod here;
pub mod keys;
pub mod measure;
pub mod mesh;
pub mod ray;

use crate::surface::Plane;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::Error;
use krly3d_kernel::ports::query::Query;
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::schema::{Kind, Param, Value};
use krly3d_kernel::topo::{Anchor, Body, Face, FaceId};
use krly3d_kernel::KernelBuilder;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Query>(faces::Faces)
        .plugin::<Query>(ray::Ray)
        .plugin::<Query>(measure::Measure)
        .plugin::<Query>(bounds::Bounds)
        .plugin::<Query>(edges::Edges)
        .plugin::<Query>(here::Here)
        .plugin::<Query>(find::Find)
        .plugin::<Query>(keys::Keys)
        .plugin::<Query>(brief::Brief)
        .plugin::<Query>(mesh::Mesh);
}

/// The parameter by which a query about one node names it.
fn node() -> Param {
    Param::required("node", Kind::Text, "the node to ask about, as name or name@version")
}

/// How many face names an error message lists before it refers to the `faces` query.
const NAMES_LISTED: usize = 12;

/// The face called `name`, or an error that says which faces there are.
fn face_named<'a>(body: &'a Body, name: &str) -> Result<(FaceId, &'a Face), Error> {
    body.face(name).ok_or_else(|| {
        let known: Vec<&str> = body.faces.iter().take(NAMES_LISTED).map(|face| face.name.as_str()).collect();
        let rest = match body.faces.len().saturating_sub(NAMES_LISTED) {
            0 => String::new(),
            more => format!(" and {more} more, which `faces` lists"),
        };
        Error::not_found(format!("there is no face `{name}`; the faces are {}{rest}", known.join(", ")))
    })
}

/// The surface of a face in a few words, as the face shows it. A flat face is described
/// by the direction it faces, which is against the normal of its plane when the material
/// lies on that side; a reader told `n=+z` about a face looking down would be misled.
fn surface_text(face: &Face, anchor: &Anchor) -> Result<String, Error> {
    if face.surface.as_any().is::<Plane>() {
        Ok(Plane::through(&anchor.point, &anchor.normal)?.summary())
    } else {
        Ok(face.surface.summary())
    }
}

/// The name of a face by its number, for answers that name the faces a probe found.
fn face_name(body: &Body, face: FaceId) -> &str {
    body.faces.get(face).map_or("?", |face| face.name.as_str())
}

fn point_value(point: &Point3) -> Value {
    Value::from([point.x, point.y, point.z])
}

fn vector_value(vector: &Vec3) -> Value {
    Value::from([vector.x, vector.y, vector.z])
}

#[cfg(test)]
pub(crate) mod testing {
    use krly3d_core::{Accuracy, Error, Id};
    use krly3d_kernel::model::Statement;
    use krly3d_kernel::ports::query::{ask, contract, Answer};
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::Kernel;

    /// A kernel whose document holds the given statements: node, tool, parameters, inputs.
    pub(crate) fn kernel(statements: Vec<(&str, &str, Params, &[&str])>) -> Kernel {
        let mut builder = crate::standard();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        let mut kernel = builder.build();
        for (node, tool, params, inputs) in statements {
            kernel.apply(Statement::new(node, tool, params, inputs).unwrap()).unwrap();
        }
        kernel
    }

    /// A kernel with the node `part`: a block 40 wide, 10 deep and 20 high.
    pub(crate) fn block() -> Kernel {
        kernel(vec![("part", "box", Params::new().with("w", 40.0).with("d", 10.0).with("h", 20.0), &[])])
    }

    /// A kernel with the node `part`: a plate 60 by 40 by 10 with a hole of radius 4
    /// through it at (20, 15). Its sides are `s1` … `s4` and the wall of the hole is `s5`.
    pub(crate) fn plate_with_hole() -> Kernel {
        let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
        kernel(vec![
            ("outline", "profile", outline, &[]),
            ("part", "extrude", Params::new().with("h", 10.0), &["outline"]),
        ])
    }

    /// What the query answers, after checking the contract of the query family on it.
    pub(crate) fn asked(kernel: &Kernel, query: &str, params: Params) -> Answer {
        let (document, model) = kernel.snapshot();
        let id = Id::new(query);
        contract::check(kernel.context(), &id, &params, &document, &model).unwrap();
        ask(kernel.context(), &id, &params, &document, &model).into_result().unwrap()
    }

    /// Why the query does not answer.
    pub(crate) fn refused(kernel: &Kernel, query: &str, params: Params) -> Error {
        let (document, model) = kernel.snapshot();
        ask(kernel.context(), &Id::new(query), &params, &document, &model).into_result().unwrap_err()
    }

    /// The parameters of a query about the node `part`.
    pub(crate) fn part() -> Params {
        Params::new().with("node", "part")
    }

    pub(crate) fn entry<'a>(value: &'a Value, name: &str) -> &'a Value {
        let Value::Map(entries) = value else { panic!("not a map: {value:?}") };
        entries.get(name).unwrap_or_else(|| panic!("no `{name}` in {value:?}"))
    }

    pub(crate) fn items(value: &Value) -> &[Value] {
        let Value::List(items) = value else { panic!("not a list: {value:?}") };
        items
    }

    pub(crate) fn number(value: &Value) -> f64 {
        let Value::Number(number) = value else { panic!("not a number: {value:?}") };
        *number
    }

    pub(crate) fn word(value: &Value) -> &str {
        let Value::Text(text) = value else { panic!("not a text: {value:?}") };
        text
    }

    /// The three numbers of a point or a vector given as data.
    pub(crate) fn triple(value: &Value) -> [f64; 3] {
        let [x, y, z] = items(value) else { panic!("not three numbers: {value:?}") };
        [number(x), number(y), number(z)]
    }
}
