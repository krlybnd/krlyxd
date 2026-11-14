//! The exchange queries through the one door of the kernel.
//!
//! No STEP reader runs here, so the exported files are verified by a checker written for
//! this test alone. It reads the text as ISO 10303-21, knows what each entity takes, and
//! recomputes the geometry from the numbers in the file with the parameterisations of
//! ISO 10303-42. The kernel is asked only where a second opinion is the point: whether a
//! spot is inside a face, and whether it is inside the solid.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Accuracy, Attempt};
use krly3d_kernel::ports::probe::{side, Side};
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Session};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;
use std::sync::Arc;

type Check<T = ()> = Result<T, String>;

fn ensure(holds: bool, problem: impl FnOnce() -> String) -> Check {
    if holds {
        Ok(())
    } else {
        Err(problem())
    }
}

// ---------------------------------------------------------------------------------------
// Reading ISO 10303-21
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Ref(usize),
    Real(f64),
    Integer(i64),
    Text(String),
    Word(String),
    List(Vec<Arg>),
    Unset,
    Derived,
    Typed(String, Vec<Arg>),
}

#[derive(Clone, Debug, PartialEq)]
struct Record {
    kind: String,
    args: Vec<Arg>,
}

/// A parsed exchange file. An instance is a list of records: one for a simple instance,
/// several for a complex one.
struct Exchange {
    header: Vec<Record>,
    instances: BTreeMap<usize, Vec<Record>>,
}

struct Reader<'a> {
    rest: &'a str,
}

impl Reader<'_> {
    fn eat(&mut self, token: &str) -> bool {
        self.rest = self.rest.trim_start();
        match self.rest.strip_prefix(token) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn expect(&mut self, token: &str) -> Check {
        let found: String = self.rest.trim_start().chars().take(30).collect();
        ensure(self.eat(token), || format!("expected `{token}` before `{found}`"))
    }

    fn take(&mut self, belongs: impl Fn(char) -> bool) -> &str {
        self.rest = self.rest.trim_start();
        let end = self.rest.find(|character| !belongs(character)).unwrap_or(self.rest.len());
        let (taken, rest) = self.rest.split_at(end);
        self.rest = rest;
        taken
    }

    fn keyword(&mut self) -> Check<String> {
        let word = self.take(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_').to_string();
        ensure(!word.is_empty(), || format!("expected an entity name before `{}`", self.rest))?;
        Ok(word)
    }

    fn record(&mut self) -> Check<Record> {
        let kind = self.keyword()?;
        self.expect("(")?;
        Ok(Record { kind, args: self.args()? })
    }

    /// The arguments up to and including the closing bracket.
    fn args(&mut self) -> Check<Vec<Arg>> {
        let mut args = Vec::new();
        if self.eat(")") {
            return Ok(args);
        }
        loop {
            args.push(self.arg()?);
            if self.eat(")") {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    fn arg(&mut self) -> Check<Arg> {
        self.rest = self.rest.trim_start();
        let first = self.rest.chars().next().ok_or("the file ends inside an instance")?;
        Ok(match first {
            '#' => {
                self.rest = &self.rest[1..];
                Arg::Ref(self.take(|c| c.is_ascii_digit()).parse().map_err(|_| "a `#` without a number")?)
            }
            '\'' => Arg::Text(self.text()?),
            '$' | '*' => {
                self.rest = &self.rest[1..];
                if first == '$' {
                    Arg::Unset
                } else {
                    Arg::Derived
                }
            }
            '(' => {
                self.rest = &self.rest[1..];
                Arg::List(self.args()?)
            }
            '.' => {
                self.rest = &self.rest[1..];
                let word = self.take(|c| c.is_ascii_uppercase() || c == '_').to_string();
                self.expect(".")?;
                Arg::Word(word)
            }
            c if c.is_ascii_uppercase() => {
                let record = self.record()?;
                Arg::Typed(record.kind, record.args)
            }
            _ => self.number()?,
        })
    }

    fn number(&mut self) -> Check<Arg> {
        let spelled = self.take(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'E')).to_string();
        if spelled.contains('.') {
            // ISO 10303-21 allows `1.E-7`, which Rust does not read without a digit after the point.
            let readable = spelled.replace(".E", ".0E");
            readable.parse().map(Arg::Real).map_err(|_| format!("`{spelled}` is not a real number"))
        } else {
            spelled.parse().map(Arg::Integer).map_err(|_| format!("`{spelled}` is not a number"))
        }
    }

    /// A quoted text with its escapes undone: `''`, `\\`, `\X2\…\X0\` and `\X4\…\X0\`.
    fn text(&mut self) -> Check<String> {
        let mut text = String::new();
        let mut rest = &self.rest[1..];
        loop {
            let coded = [("\\X2\\", 4), ("\\X4\\", 8)]
                .into_iter()
                .find_map(|(mark, digits)| rest.strip_prefix(mark).map(|after| (digits, after)));
            if let Some(after) = rest.strip_prefix("''") {
                text.push('\'');
                rest = after;
            } else if let Some(after) = rest.strip_prefix('\'') {
                self.rest = after;
                return Ok(text);
            } else if let Some(after) = rest.strip_prefix("\\\\") {
                text.push('\\');
                rest = after;
            } else if let Some((digits, after)) = coded {
                let end = after.find("\\X0\\").ok_or("an escape in a text is not closed")?;
                ensure(end % digits == 0, || "an escape with stray digits".into())?;
                for start in (0..end).step_by(digits) {
                    let code = u32::from_str_radix(&after[start..start + digits], 16)
                        .map_err(|_| "an escape that is not hexadecimal")?;
                    text.push(char::from_u32(code).ok_or("an escape that is no character")?);
                }
                rest = &after[end + 4..];
            } else {
                let character = rest.chars().next().ok_or("a text is not closed")?;
                text.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }
    }
}

fn parse(text: &str) -> Check<Exchange> {
    let mut reader = Reader { rest: text };
    let mut exchange = Exchange { header: Vec::new(), instances: BTreeMap::new() };
    reader.expect("ISO-10303-21;")?;
    reader.expect("HEADER;")?;
    while !reader.eat("ENDSEC;") {
        exchange.header.push(reader.record()?);
        reader.expect(";")?;
    }
    reader.expect("DATA;")?;
    while !reader.eat("ENDSEC;") {
        reader.expect("#")?;
        let id: usize =
            reader.take(|c| c.is_ascii_digit()).parse().map_err(|_| "an instance without a number")?;
        reader.expect("=")?;
        let mut parts = Vec::new();
        if reader.eat("(") {
            while !reader.eat(")") {
                parts.push(reader.record()?);
            }
        } else {
            parts.push(reader.record()?);
        }
        reader.expect(";")?;
        ensure(exchange.instances.insert(id, parts).is_none(), || format!("#{id} is defined twice"))?;
    }
    reader.expect("END-ISO-10303-21;")?;
    ensure(reader.rest.trim().is_empty(), || "there is text after the end of the file".into())?;
    Ok(exchange)
}

// ---------------------------------------------------------------------------------------
// What each entity takes
// ---------------------------------------------------------------------------------------

enum Want {
    Text,
    Real,
    Integer,
    Flag,
    Derived,
    /// `$` or one of these enumeration values.
    Optional(&'static [&'static str]),
    Word(&'static [&'static str]),
    /// A reference to an instance of one of these kinds.
    Ref(&'static [&'static str]),
    /// A list of at least one reference to instances of these kinds.
    Refs(&'static [&'static str]),
    Reals(usize),
    Texts,
    /// A typed value such as `LENGTH_MEASURE(1.E-7)`.
    Measure(&'static str),
}

const CURVES: &[&str] = &["LINE", "CIRCLE"];
const SURFACES: &[&str] =
    &["PLANE", "CYLINDRICAL_SURFACE", "CONICAL_SURFACE", "SPHERICAL_SURFACE", "TOROIDAL_SURFACE"];
const PLACED: &[&str] = &["AXIS2_PLACEMENT_3D"];
/// Geometry, which the writer promises to write once however often it is used.
const GEOMETRY: &[&str] = &[
    "CARTESIAN_POINT",
    "DIRECTION",
    "VECTOR",
    "AXIS2_PLACEMENT_3D",
    "LINE",
    "CIRCLE",
    "PLANE",
    "CYLINDRICAL_SURFACE",
    "CONICAL_SURFACE",
    "SPHERICAL_SURFACE",
    "TOROIDAL_SURFACE",
];

/// The attributes of every entity the writer may emit, in the order of ISO 10303-42, -43
/// and -41 as AP214 uses them. An entity that is not listed here fails the check.
fn signature(kind: &str) -> Option<Vec<Want>> {
    use Want::{Derived, Flag, Integer, Measure, Optional, Real, Reals, Ref, Refs, Text, Texts, Word};
    Some(match kind {
        "FILE_DESCRIPTION" => vec![Texts, Text],
        "FILE_NAME" => vec![Text, Text, Texts, Texts, Text, Text, Text],
        "FILE_SCHEMA" => vec![Texts],
        "APPLICATION_CONTEXT" => vec![Text],
        "APPLICATION_PROTOCOL_DEFINITION" => vec![Text, Text, Integer, Ref(&["APPLICATION_CONTEXT"])],
        "PRODUCT_CONTEXT" | "PRODUCT_DEFINITION_CONTEXT" => vec![Text, Ref(&["APPLICATION_CONTEXT"]), Text],
        "PRODUCT" => vec![Text, Text, Text, Refs(&["PRODUCT_CONTEXT"])],
        "PRODUCT_DEFINITION_FORMATION" => vec![Text, Text, Ref(&["PRODUCT"])],
        "PRODUCT_DEFINITION" => {
            vec![Text, Text, Ref(&["PRODUCT_DEFINITION_FORMATION"]), Ref(&["PRODUCT_DEFINITION_CONTEXT"])]
        }
        "PRODUCT_DEFINITION_SHAPE" => vec![Text, Text, Ref(&["PRODUCT_DEFINITION"])],
        "SHAPE_DEFINITION_REPRESENTATION" => {
            vec![Ref(&["PRODUCT_DEFINITION_SHAPE"]), Ref(&["ADVANCED_BREP_SHAPE_REPRESENTATION"])]
        }
        "ADVANCED_BREP_SHAPE_REPRESENTATION" => vec![
            Text,
            Refs(&["AXIS2_PLACEMENT_3D", "MANIFOLD_SOLID_BREP"]),
            Ref(&["GEOMETRIC_REPRESENTATION_CONTEXT"]),
        ],
        "PRODUCT_RELATED_PRODUCT_CATEGORY" => vec![Text, Optional(&[]), Refs(&["PRODUCT"])],
        "LENGTH_UNIT" | "PLANE_ANGLE_UNIT" | "SOLID_ANGLE_UNIT" => vec![],
        "NAMED_UNIT" => vec![Derived],
        "SI_UNIT" => vec![Optional(&["MILLI"]), Word(&["METRE", "RADIAN", "STERADIAN"])],
        "UNCERTAINTY_MEASURE_WITH_UNIT" => vec![Measure("LENGTH_MEASURE"), Ref(&["LENGTH_UNIT"]), Text, Text],
        "GEOMETRIC_REPRESENTATION_CONTEXT" => vec![Integer],
        "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT" => vec![Refs(&["UNCERTAINTY_MEASURE_WITH_UNIT"])],
        "GLOBAL_UNIT_ASSIGNED_CONTEXT" => vec![Refs(&["NAMED_UNIT"])],
        "REPRESENTATION_CONTEXT" => vec![Text, Text],
        "MANIFOLD_SOLID_BREP" => vec![Text, Ref(&["CLOSED_SHELL"])],
        "CLOSED_SHELL" => vec![Text, Refs(&["ADVANCED_FACE"])],
        "ADVANCED_FACE" => vec![Text, Refs(&["FACE_BOUND", "FACE_OUTER_BOUND"]), Ref(SURFACES), Flag],
        "FACE_BOUND" | "FACE_OUTER_BOUND" => vec![Text, Ref(&["EDGE_LOOP", "VERTEX_LOOP"]), Flag],
        "EDGE_LOOP" => vec![Text, Refs(&["ORIENTED_EDGE"])],
        "VERTEX_LOOP" => vec![Text, Ref(&["VERTEX_POINT"])],
        "ORIENTED_EDGE" => vec![Text, Derived, Derived, Ref(&["EDGE_CURVE"]), Flag],
        "EDGE_CURVE" => vec![Text, Ref(&["VERTEX_POINT"]), Ref(&["VERTEX_POINT"]), Ref(CURVES), Flag],
        "VERTEX_POINT" => vec![Text, Ref(&["CARTESIAN_POINT"])],
        "CARTESIAN_POINT" | "DIRECTION" => vec![Text, Reals(3)],
        "VECTOR" => vec![Text, Ref(&["DIRECTION"]), Real],
        "LINE" => vec![Text, Ref(&["CARTESIAN_POINT"]), Ref(&["VECTOR"])],
        "AXIS2_PLACEMENT_3D" => {
            vec![Text, Ref(&["CARTESIAN_POINT"]), Ref(&["DIRECTION"]), Ref(&["DIRECTION"])]
        }
        "CIRCLE" | "CYLINDRICAL_SURFACE" | "SPHERICAL_SURFACE" => vec![Text, Ref(PLACED), Real],
        "PLANE" => vec![Text, Ref(PLACED)],
        "CONICAL_SURFACE" | "TOROIDAL_SURFACE" => vec![Text, Ref(PLACED), Real, Real],
        _ => return None,
    })
}

impl Exchange {
    /// The record of a simple instance.
    fn one(&self, id: usize) -> &Record {
        match self.instances.get(&id).map(Vec::as_slice) {
            Some([record]) => record,
            other => panic!("#{id} is not a simple instance: {other:?}"),
        }
    }

    fn ids_of(&self, kind: &str) -> Vec<usize> {
        let of_kind = |parts: &Vec<Record>| parts.iter().any(|record| record.kind == kind);
        self.instances.iter().filter(|(_, parts)| of_kind(parts)).map(|(id, _)| *id).collect()
    }

    /// The instances of `kind` whose arguments refer to `target` directly.
    fn users(&self, kind: &str, target: usize) -> Vec<usize> {
        let refers = |id: &usize| self.one(*id).args.iter().any(|arg| refs(arg).contains(&target));
        self.ids_of(kind).into_iter().filter(refers).collect()
    }

    /// Instances are numbered from one without a gap, in the order of the file.
    fn check_numbering(&self) -> Check {
        let ids: Vec<usize> = self.instances.keys().copied().collect();
        let expected: Vec<usize> = (1..=ids.len()).collect();
        ensure(ids == expected, || "the instances are not numbered 1, 2, 3, … without a gap".into())
    }

    /// Every record takes the number and kind of arguments its entity is defined with,
    /// and every reference leads to an instance of a kind allowed there.
    fn check_records(&self) -> Check {
        let numbered =
            self.instances.iter().flat_map(|(id, parts)| parts.iter().map(move |part| (*id, part)));
        for (id, record) in self.header.iter().map(|record| (0, record)).chain(numbered) {
            let wanted = signature(&record.kind)
                .ok_or_else(|| format!("#{id}: `{}` is not an entity this check knows", record.kind))?;
            ensure(wanted.len() == record.args.len(), || {
                format!(
                    "#{id}: {} takes {} argument(s), got {}",
                    record.kind,
                    wanted.len(),
                    record.args.len()
                )
            })?;
            for (index, (want, arg)) in wanted.iter().zip(&record.args).enumerate() {
                ensure(self.fits(want, arg), || {
                    format!("#{id}: argument {} of {} cannot be {arg:?}", index + 1, record.kind)
                })?;
            }
        }
        Ok(())
    }

    fn fits(&self, want: &Want, arg: &Arg) -> bool {
        let leads_to = |kinds: &[&str], arg: &Arg| match arg {
            Arg::Ref(id) => self
                .instances
                .get(id)
                .is_some_and(|parts| parts.iter().any(|part| kinds.contains(&part.kind.as_str()))),
            _ => false,
        };
        match (want, arg) {
            (Want::Text, Arg::Text(_)) | (Want::Real, Arg::Real(_)) | (Want::Integer, Arg::Integer(_)) => {
                true
            }
            (Want::Derived, Arg::Derived) | (Want::Optional(_), Arg::Unset) => true,
            (Want::Flag, Arg::Word(word)) => word == "T" || word == "F",
            (Want::Optional(words) | Want::Word(words), Arg::Word(word)) => words.contains(&word.as_str()),
            (Want::Ref(kinds), arg) => leads_to(kinds, arg),
            (Want::Refs(kinds), Arg::List(items)) => {
                !items.is_empty() && items.iter().all(|item| leads_to(kinds, item))
            }
            (Want::Reals(count), Arg::List(items)) => {
                items.len() == *count && items.iter().all(|item| matches!(item, Arg::Real(_)))
            }
            (Want::Texts, Arg::List(items)) => items.iter().all(|item| matches!(item, Arg::Text(_))),
            (Want::Measure(name), Arg::Typed(kind, args)) => {
                kind == name && matches!(args[..], [Arg::Real(_)])
            }
            _ => false,
        }
    }

    /// No definition of geometry is written twice.
    fn check_shared(&self) -> Check {
        let mut seen: Vec<&Record> = Vec::new();
        for record in
            self.instances.values().flatten().filter(|record| GEOMETRY.contains(&record.kind.as_str()))
        {
            ensure(!seen.contains(&record), || format!("{record:?} is written twice"))?;
            seen.push(record);
        }
        Ok(())
    }
}

fn refs(arg: &Arg) -> Vec<usize> {
    match arg {
        Arg::Ref(id) => vec![*id],
        Arg::List(items) => items.iter().flat_map(refs).collect(),
        _ => Vec::new(),
    }
}

fn reference(arg: &Arg) -> usize {
    let Arg::Ref(id) = arg else { panic!("not a reference: {arg:?}") };
    *id
}

fn real(arg: &Arg) -> f64 {
    let Arg::Real(value) = arg else { panic!("not a real: {arg:?}") };
    *value
}

fn flag(arg: &Arg) -> bool {
    matches!(arg, Arg::Word(word) if word == "T")
}

fn text(arg: &Arg) -> &str {
    let Arg::Text(text) = arg else { panic!("not a text: {arg:?}") };
    text
}

// ---------------------------------------------------------------------------------------
// Geometry recomputed from the numbers in the file
// ---------------------------------------------------------------------------------------

/// How close recomputed geometry must come; the models are a few tens of units across.
const CLOSE: f64 = 1e-9;

/// An `AXIS2_PLACEMENT_3D`: z is the axis, x the reference direction made perpendicular
/// to it, y completes the right-handed frame.
struct Placement {
    origin: Point3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl Placement {
    fn local(&self, point: &Point3) -> Vec3 {
        let offset = point - self.origin;
        Vec3::new(offset.dot(&self.x), offset.dot(&self.y), offset.dot(&self.z))
    }

    fn world(&self, local: Vec3) -> Point3 {
        self.origin + self.x * local.x + self.y * local.y + self.z * local.z
    }
}

enum FileCurve {
    /// `pnt + u·magnitude·dir`
    Line { point: Point3, step: Vec3 },
    /// `centre + r·(cos u·x + sin u·y)`
    Circle { at: Placement, radius: f64 },
}

impl FileCurve {
    fn point(&self, u: f64) -> Point3 {
        match self {
            Self::Line { point, step } => point + step * u,
            Self::Circle { at, radius } => at.world(Vec3::new(radius * u.cos(), radius * u.sin(), 0.0)),
        }
    }

    fn tangent(&self, u: f64) -> Vec3 {
        match self {
            Self::Line { step, .. } => *step,
            Self::Circle { at, radius } => (at.y * u.cos() - at.x * u.sin()) * *radius,
        }
    }

    fn parameter(&self, point: &Point3) -> f64 {
        match self {
            Self::Line { point: start, step } => (point - start).dot(step) / step.norm_squared(),
            Self::Circle { at, .. } => {
                let local = at.local(point);
                local.y.atan2(local.x)
            }
        }
    }
}

enum FileSurface {
    Plane(Placement),
    Cylinder(Placement, f64),
    /// Placement, radius at the placement, semi-angle.
    Cone(Placement, f64, f64),
    Sphere(Placement, f64),
    /// Placement, major radius, minor radius.
    Torus(Placement, f64, f64),
}

impl FileSurface {
    /// The parameterisations of ISO 10303-42.
    fn point(&self, u: f64, v: f64) -> Point3 {
        let around = |radius: f64, height: f64| Vec3::new(radius * u.cos(), radius * u.sin(), height);
        match self {
            Self::Plane(at) => at.world(Vec3::new(u, v, 0.0)),
            Self::Cylinder(at, radius) => at.world(around(*radius, v)),
            Self::Cone(at, radius, semi_angle) => at.world(around(radius + v * semi_angle.tan(), v)),
            Self::Sphere(at, radius) => at.world(around(radius * v.cos(), radius * v.sin())),
            Self::Torus(at, major, minor) => at.world(around(major + minor * v.cos(), minor * v.sin())),
        }
    }

    /// The parameters of the point of the surface a point belongs to, if it is on it.
    fn parameters(&self, point: &Point3) -> (f64, f64) {
        let local = |at: &Placement| {
            let local = at.local(point);
            (local.y.atan2(local.x), local.x.hypot(local.y), local.z)
        };
        match self {
            Self::Plane(at) => (at.local(point).x, at.local(point).y),
            Self::Cylinder(at, _) | Self::Cone(at, _, _) => (local(at).0, local(at).2),
            Self::Sphere(at, _) => (local(at).0, local(at).2.atan2(local(at).1)),
            Self::Torus(at, major, _) => (local(at).0, local(at).2.atan2(local(at).1 - major)),
        }
    }

    /// How far a point is from where the surface has the point's parameters: zero exactly
    /// for the points of the surface. A point on the far nappe of a cone is not one.
    fn gap(&self, point: &Point3) -> f64 {
        let (u, v) = self.parameters(point);
        (self.point(u, v) - point).norm()
    }

    /// The normal as ISO 10303-42 defines it: along ∂/∂u × ∂/∂v of the parameterisation.
    fn normal(&self, point: &Point3) -> Vec3 {
        const STEP: f64 = 1e-6;
        let (u, v) = self.parameters(point);
        let along_u = self.point(u + STEP, v) - self.point(u - STEP, v);
        let along_v = self.point(u, v + STEP) - self.point(u, v - STEP);
        along_u.cross(&along_v).normalize()
    }
}

impl Exchange {
    fn point(&self, id: usize) -> Point3 {
        let Arg::List(coordinates) = &self.one(id).args[1] else { panic!("#{id} has no coordinates") };
        Point3::new(real(&coordinates[0]), real(&coordinates[1]), real(&coordinates[2]))
    }

    fn direction(&self, id: usize) -> Check<Vec3> {
        let direction = self.point(id).coords;
        ensure((direction.norm() - 1.0).abs() <= CLOSE, || format!("direction #{id} is not a unit vector"))?;
        Ok(direction)
    }

    fn placement(&self, id: usize) -> Check<Placement> {
        let args = &self.one(id).args;
        let (z, towards) = (self.direction(reference(&args[2]))?, self.direction(reference(&args[3]))?);
        ensure(z.dot(&towards).abs() <= CLOSE, || {
            format!("the axes of placement #{id} are not at right angles")
        })?;
        let x = (towards - z * towards.dot(&z)).normalize();
        Ok(Placement { origin: self.point(reference(&args[1])), x, y: z.cross(&x), z })
    }

    fn curve(&self, id: usize) -> Check<FileCurve> {
        let record = self.one(id);
        match record.kind.as_str() {
            "LINE" => {
                let vector = &self.one(reference(&record.args[2])).args;
                let magnitude = real(&vector[2]);
                ensure(magnitude > 0.0, || format!("line #{id} has no length to its vector"))?;
                let step = self.direction(reference(&vector[1]))? * magnitude;
                Ok(FileCurve::Line { point: self.point(reference(&record.args[1])), step })
            }
            "CIRCLE" => {
                let radius = real(&record.args[2]);
                ensure(radius > 0.0, || format!("circle #{id} has radius {radius}"))?;
                Ok(FileCurve::Circle { at: self.placement(reference(&record.args[1]))?, radius })
            }
            other => Err(format!("#{id}: {other} is not a curve this check can evaluate")),
        }
    }

    fn surface(&self, id: usize) -> Check<FileSurface> {
        let record = self.one(id);
        let at = self.placement(reference(&record.args[1]))?;
        let size = |index: usize| -> Check<f64> {
            let size = real(&record.args[index]);
            ensure(size > 0.0, || format!("{} #{id} has a size of {size}", record.kind))?;
            Ok(size)
        };
        Ok(match record.kind.as_str() {
            "PLANE" => FileSurface::Plane(at),
            "CYLINDRICAL_SURFACE" => FileSurface::Cylinder(at, size(2)?),
            "SPHERICAL_SURFACE" => FileSurface::Sphere(at, size(2)?),
            "TOROIDAL_SURFACE" => FileSurface::Torus(at, size(2)?, size(3)?),
            "CONICAL_SURFACE" => {
                let (radius, semi_angle) = (real(&record.args[2]), size(3)?);
                ensure(radius >= 0.0 && semi_angle < TAU / 4.0, || {
                    format!("cone #{id} has radius {radius} and semi-angle {semi_angle}")
                })?;
                FileSurface::Cone(at, radius, semi_angle)
            }
            other => return Err(format!("#{id}: {other} is not a surface this check can evaluate")),
        })
    }
}

// ---------------------------------------------------------------------------------------
// Topology and its agreement with the geometry
// ---------------------------------------------------------------------------------------

/// An edge as the file states it, with the stretch of its curve it covers.
struct FileEdge {
    start: usize,
    end: usize,
    curve: FileCurve,
    /// Parameters of the start and the end on the curve; the edge runs from the first to
    /// the second, which is smaller when the edge runs against its curve.
    from: f64,
    to: f64,
}

impl FileEdge {
    fn point(&self, fraction: f64) -> Point3 {
        self.curve.point(self.from + (self.to - self.from) * fraction)
    }

    /// Direction of travel from the start vertex to the end vertex.
    fn travel(&self, fraction: f64) -> Vec3 {
        self.curve.tangent(self.from + (self.to - self.from) * fraction) * (self.to - self.from).signum()
    }
}

/// One use of an edge by a loop of a face.
struct FileUse {
    edge: usize,
    /// Whether the loop runs along the edge from its start to its end, the orientation of
    /// the bound taken into account.
    forward: bool,
}

struct FileFace {
    id: usize,
    name: String,
    surface: FileSurface,
    kind: String,
    same_sense: bool,
    loops: Vec<Vec<FileUse>>,
    /// Vertices that are bounds by themselves.
    lone: Vec<usize>,
    outer_bounds: usize,
}

/// One `MANIFOLD_SOLID_BREP` taken apart.
struct FileSolid {
    faces: Vec<FileFace>,
    edges: BTreeMap<usize, FileEdge>,
    vertices: BTreeSet<usize>,
}

impl Exchange {
    fn edge(&self, id: usize) -> Check<FileEdge> {
        let args = &self.one(id).args;
        let (start, end) = (reference(&args[1]), reference(&args[2]));
        let curve = self.curve(reference(&args[3]))?;
        let ends = [start, end].map(|vertex| self.point(reference(&self.one(vertex).args[1])));
        for point in &ends {
            let foot = curve.point(curve.parameter(point));
            ensure((foot - point).norm() <= CLOSE, || format!("a vertex of edge #{id} is off its curve"))?;
        }
        let along = if flag(&args[4]) { 1.0 } else { -1.0 };
        let (from, mut to) = (curve.parameter(&ends[0]), curve.parameter(&ends[1]));
        if matches!(curve, FileCurve::Circle { .. }) {
            // An arc runs from its start to its end the way the flag says; an edge that
            // ends at the vertex it starts from is the whole circle.
            let turn = ((to - from) * along).rem_euclid(TAU);
            to = from + along * if start == end || turn <= CLOSE { TAU } else { turn };
        } else {
            ensure((to - from) * along > 0.0, || format!("line edge #{id} runs against its stated sense"))?;
        }
        Ok(FileEdge { start, end, curve, from, to })
    }

    fn solid(&self, id: usize) -> Check<FileSolid> {
        let shell = self.one(reference(&self.one(id).args[1]));
        let mut solid = FileSolid { faces: Vec::new(), edges: BTreeMap::new(), vertices: BTreeSet::new() };
        for face_id in refs(&shell.args[1]) {
            let face = self.one(face_id);
            let surface_id = reference(&face.args[2]);
            let mut taken = FileFace {
                id: face_id,
                name: text(&face.args[0]).to_string(),
                surface: self.surface(surface_id)?,
                kind: self.one(surface_id).kind.clone(),
                same_sense: flag(&face.args[3]),
                loops: Vec::new(),
                lone: Vec::new(),
                outer_bounds: 0,
            };
            for bound_id in refs(&face.args[1]) {
                let bound = self.one(bound_id);
                taken.outer_bounds += usize::from(bound.kind == "FACE_OUTER_BOUND");
                let ring = self.one(reference(&bound.args[1]));
                if ring.kind == "VERTEX_LOOP" {
                    let vertex = reference(&ring.args[1]);
                    taken.lone.push(vertex);
                    solid.vertices.insert(vertex);
                    continue;
                }
                let mut uses = Vec::new();
                for oriented_id in refs(&ring.args[1]) {
                    let oriented = self.one(oriented_id);
                    let edge = reference(&oriented.args[3]);
                    if let Entry::Vacant(unseen) = solid.edges.entry(edge) {
                        unseen.insert(self.edge(edge)?);
                    }
                    uses.push(FileUse { edge, forward: flag(&oriented.args[4]) == flag(&bound.args[2]) });
                }
                if !flag(&bound.args[2]) {
                    uses.reverse();
                }
                taken.loops.push(uses);
            }
            solid.faces.push(taken);
        }
        let ends: Vec<usize> = solid.edges.values().flat_map(|edge| [edge.start, edge.end]).collect();
        solid.vertices.extend(ends);
        Ok(solid)
    }
}

impl FileSolid {
    /// Every edge is run once in each direction, every loop is a closed chain, and no
    /// face has two outer bounds.
    fn check_topology(&self) -> Check {
        let mut runs: BTreeMap<usize, (u32, u32)> = BTreeMap::new();
        for face in &self.faces {
            ensure(face.outer_bounds <= 1, || format!("face `{}` has several outer bounds", face.name))?;
            ensure(!face.loops.is_empty() || !face.lone.is_empty(), || {
                format!("face `{}` has no bound", face.name)
            })?;
            for uses in &face.loops {
                for (index, used) in uses.iter().enumerate() {
                    let counts = runs.entry(used.edge).or_default();
                    if used.forward {
                        counts.0 += 1;
                    } else {
                        counts.1 += 1;
                    }
                    let next = &uses[(index + 1) % uses.len()];
                    let arrives =
                        if used.forward { self.edges[&used.edge].end } else { self.edges[&used.edge].start };
                    let leaves =
                        if next.forward { self.edges[&next.edge].start } else { self.edges[&next.edge].end };
                    ensure(arrives == leaves, || {
                        format!("a loop of face `{}` is not a closed chain", face.name)
                    })?;
                }
            }
        }
        for (edge, counts) in runs {
            ensure(counts == (1, 1), || {
                format!("edge #{edge} is run {} time(s) forward and {} backward", counts.0, counts.1)
            })?;
        }
        Ok(())
    }

    /// Euler–Poincaré for one shell: with every face a sphere with one hole per bound,
    /// `V − E + Σ(2 − bounds)` is `2 − 2·handles`. A whole torus written as one face is
    /// not a sphere with holes: the face itself has the handle, which takes two more.
    fn handles(&self) -> Check<i64> {
        let mut characteristic = self.vertices.len() as i64 - self.edges.len() as i64;
        for face in &self.faces {
            let whole_torus = face.kind == "TOROIDAL_SURFACE" && face.loops.is_empty();
            characteristic +=
                2 - (face.loops.len() + face.lone.len()) as i64 - if whole_torus { 2 } else { 0 };
        }
        ensure(characteristic <= 2 && characteristic % 2 == 0, || {
            format!("a shell has the impossible Euler characteristic {characteristic}")
        })?;
        Ok((2 - characteristic) / 2)
    }

    /// Every edge lies on the surface of each face it bounds, all along, and every lone
    /// vertex on the surface of its face.
    fn check_geometry(&self, file: &Exchange) -> Check {
        for face in &self.faces {
            for used in face.loops.iter().flatten() {
                for fraction in [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] {
                    let gap = face.surface.gap(&self.edges[&used.edge].point(fraction));
                    ensure(gap <= CLOSE, || {
                        format!("edge #{} is {gap} off the surface of face `{}`", used.edge, face.name)
                    })?;
                }
            }
            for vertex in &face.lone {
                let gap = face.surface.gap(&file.point(reference(&file.one(*vertex).args[1])));
                ensure(gap <= CLOSE, || {
                    format!("a lone vertex of face `{}` is {gap} off its surface", face.name)
                })?;
            }
        }
        Ok(())
    }

    /// The senses the file states, against the kernel's own judgement of the body. From
    /// the middle of every edge use, the direction the file implies as "into the face"
    /// (normal of the face × direction of travel) must lead into the face and its
    /// opposite out of it; and the normal of the face must lead out of the solid.
    fn check_senses(&self, file: &Exchange, cx: &Context, body: &Arc<Body>) -> Check {
        const STEP: f64 = 0.01;
        for face in &self.faces {
            let (index, kernel_face) =
                body.face(&face.name).ok_or_else(|| format!("the body has no face `{}`", face.name))?;
            let sense = if face.same_sense { 1.0 } else { -1.0 };
            let on_face = |point: Point3| kernel_face.surface.point(kernel_face.surface.project(&point));
            let placed =
                |point: Point3| place(cx, &TrimRequest { body: Arc::clone(body), face: index, point });
            let mut spots: Vec<Point3> =
                face.lone.iter().map(|vertex| file.point(reference(&file.one(*vertex).args[1]))).collect();
            for used in face.loops.iter().flatten() {
                let edge = &self.edges[&used.edge];
                let (middle, along) = (edge.point(0.5), if used.forward { 1.0 } else { -1.0 });
                let inward =
                    (face.surface.normal(&middle) * sense).cross(&(edge.travel(0.5) * along)).normalize();
                let (within, beyond) = (on_face(middle + inward * STEP), on_face(middle - inward * STEP));
                ensure(placed(within) == Attempt::Done(Place::Inside), || {
                    format!(
                        "face `{}` does not lie to the left of edge #{} as the file says",
                        face.name, used.edge
                    )
                })?;
                ensure(placed(beyond) == Attempt::Done(Place::Outside), || {
                    format!("face `{}` also lies to the right of edge #{}", face.name, used.edge)
                })?;
                spots.push(within);
            }
            if face.kind == "CONICAL_SURFACE" {
                // The surface has no normal at an apex, so a lone vertex there says nothing here.
                spots.retain(|spot| face.surface.normal(spot).norm().is_finite());
            }
            for spot in spots {
                let out = face.surface.normal(&spot) * sense * STEP;
                let (above, below) = (side(cx, body, &(spot + out)), side(cx, body, &(spot - out)));
                ensure(
                    above == Attempt::Done(Side::Outside) && below == Attempt::Done(Side::Inside),
                    || {
                        format!(
                            "the normal of face `{}` does not point out of the solid: {above:?} {below:?}",
                            face.name
                        )
                    },
                )?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// The whole check
// ---------------------------------------------------------------------------------------

/// Verifies an exported file against the bodies it was written from. `handles[k]` lists,
/// for node `k`, the number of handles of each of its shells in the order written.
fn verify(text: &str, session: &Session, nodes: &[&str], handles: &[&[i64]]) -> Check {
    let file = parse(text)?;
    let schema = file.header.iter().find(|record| record.kind == "FILE_SCHEMA");
    ensure(schema.is_some_and(|record| format!("{:?}", record.args).contains("AUTOMOTIVE_DESIGN")), || {
        "the file does not name the AP214 schema".into()
    })?;
    file.check_numbering()?;
    file.check_records()?;
    let products = file.ids_of("PRODUCT");
    ensure(products.len() == nodes.len(), || {
        format!("{} products for {} nodes", products.len(), nodes.len())
    })?;
    let mut seen = BTreeSet::new();
    for (node, expected_handles) in nodes.iter().zip(handles) {
        let body = session.kernel().body(node).map_err(|error| error.to_string())?;
        let solids = file.solids_of(node)?;
        let mut found_handles = Vec::new();
        let (mut faces, mut edges, mut vertices, mut lone) = (Vec::new(), 0, Vec::new(), 0);
        for id in solids {
            let solid = file.solid(id)?;
            solid.check_topology()?;
            solid.check_geometry(&file)?;
            solid.check_senses(&file, session.kernel().context(), &body)?;
            found_handles.push(solid.handles()?);
            faces.extend(solid.faces.iter().map(|face| face.name.clone()));
            edges += solid.edges.len();
            lone += solid.faces.iter().map(|face| face.lone.len()).sum::<usize>();
            vertices.extend(
                solid.vertices.iter().map(|vertex| file.point(reference(&file.one(*vertex).args[1]))),
            );
            // Nothing is shared between two solids.
            let faces_of_solid = solid.faces.iter().map(|face| face.id);
            let mut parts = solid.edges.keys().chain(&solid.vertices).copied().chain(faces_of_solid);
            ensure(parts.all(|part| seen.insert(part)), || {
                format!("two solids of `{node}` share a face, an edge or a vertex")
            })?;
        }
        ensure(found_handles == *expected_handles, || {
            format!("the shells of `{node}` have {found_handles:?} handles, expected {expected_handles:?}")
        })?;
        let mut names: Vec<String> = body.faces.iter().map(|face| face.name.clone()).collect();
        names.sort();
        faces.sort();
        ensure(faces == names, || {
            format!("`{node}` is written with the faces {faces:?}, the body has {names:?}")
        })?;
        ensure(edges == body.edges.len(), || format!("`{node}`: {edges} edges for {}", body.edges.len()))?;
        // The file has the vertices of the body, one more for each closed edge, and the lone ones.
        let closed = body.edges.iter().filter(|edge| edge.ends.is_none()).count();
        ensure(vertices.len() == body.vertices.len() + closed + lone, || {
            format!("`{node}`: {} vertices for {} + {closed} + {lone}", vertices.len(), body.vertices.len())
        })?;
        let written = |point: &Point3| vertices.iter().any(|vertex| (vertex - point).norm() <= CLOSE);
        ensure(body.vertices.iter().all(written), || {
            format!("a vertex of `{node}` is missing from the file")
        })?;
    }
    file.check_shared()
}

impl Exchange {
    /// The solids of the product named `node`, found by walking the product structure
    /// from the product to its shape representation.
    fn solids_of(&self, node: &str) -> Check<Vec<usize>> {
        let named = |id: &usize| text(&self.one(*id).args[0]) == node && text(&self.one(*id).args[1]) == node;
        let product = self
            .ids_of("PRODUCT")
            .into_iter()
            .find(named)
            .ok_or_else(|| format!("no product is named `{node}`"))?;
        let mut at = product;
        for kind in [
            "PRODUCT_DEFINITION_FORMATION",
            "PRODUCT_DEFINITION",
            "PRODUCT_DEFINITION_SHAPE",
            "SHAPE_DEFINITION_REPRESENTATION",
        ] {
            let users = self.users(kind, at);
            ensure(users.len() == 1, || {
                format!("`{node}`: {} {kind} instances refer to #{at}", users.len())
            })?;
            at = users[0];
        }
        let categories = self.users("PRODUCT_RELATED_PRODUCT_CATEGORY", product);
        ensure(categories.len() == 1, || format!("product `{node}` is in {} categories", categories.len()))?;
        let representation = self.one(reference(&self.one(at).args[1]));
        ensure(text(&representation.args[0]) == node, || {
            format!("the representation of `{node}` has another name")
        })?;
        let is_solid = |id: &usize| self.one(*id).kind == "MANIFOLD_SOLID_BREP";
        Ok(refs(&representation.args[1]).into_iter().filter(is_solid).collect())
    }
}

// ---------------------------------------------------------------------------------------
// The models
// ---------------------------------------------------------------------------------------

fn session() -> Session {
    let mut builder = krly3d_std::standard();
    krly3d_exchange::register(&mut builder);
    builder.accuracy(Accuracy::for_scale(100.0).unwrap());
    builder.session()
}

/// A session holding the node `part`, made by the calls given: each is a tool, the node
/// it defines and its parameters. A tool that reads a body reads the node made before it.
fn modelled(calls: &[(&str, &str, Params)]) -> Session {
    let mut session = session();
    let mut before: Option<&str> = None;
    for (tool, id, params) in calls {
        let mut args = params.clone().with("id", *id);
        let reads_a_body =
            session.methods().iter().any(|method| method.name == *tool && method.schema.inputs.0 >= 1);
        if let (true, Some(input)) = (reads_a_body, before) {
            args.set("inputs", input);
        }
        let reply = session.call(tool, &args);
        assert!(reply.ok, "{tool}: {}", reply.text);
        before = Some(id);
    }
    session
}

fn upright(path: &str) -> Params {
    Params::new().with("path", path).with("plane", "xz")
}

fn step(session: &mut Session, nodes: &[&str]) -> String {
    let reply = session.call("step", &Params::new().with("nodes", Value::texts(nodes.iter().copied())));
    assert!(reply.ok, "{}", reply.text);
    reply.text
}

/// How many instances of an entity a file holds.
fn count(text: &str, entity: &str) -> usize {
    text.matches(&format!(" = {entity}(")).count()
}

#[test]
fn a_box_is_written_as_six_planes_twelve_lines_and_eight_vertices() {
    let mut session =
        modelled(&[("box", "part", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0))]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(
        ["PLANE", "ADVANCED_FACE", "EDGE_CURVE", "LINE", "VERTEX_POINT", "ORIENTED_EDGE"]
            .map(|kind| count(&text, kind)),
        [6, 6, 12, 12, 8, 24]
    );
    assert!(text.contains("ADVANCED_FACE('front',"), "{text}");
}

#[test]
fn a_cylinder_gets_a_vertex_on_each_of_its_two_closed_edges() {
    let mut session = modelled(&[("cylinder", "part", Params::new().with("r", 3.0).with("h", 10.0))]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(
        ["CYLINDRICAL_SURFACE", "PLANE", "CIRCLE", "EDGE_CURVE", "VERTEX_POINT"]
            .map(|kind| count(&text, kind)),
        [1, 2, 2, 2, 2]
    );
}

#[test]
fn the_apex_of_a_cone_is_a_vertex_loop_of_its_side() {
    let mut session = modelled(&[("cone", "part", Params::new().with("r", 4.0).with("h", 9.0))]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(["CONICAL_SURFACE", "VERTEX_LOOP", "EDGE_CURVE"].map(|kind| count(&text, kind)), [1, 1, 1]);
    assert!(text.contains("CARTESIAN_POINT('',(0.0,0.0,9.0))"), "{text}");
}

#[test]
fn a_frustum_lies_on_one_nappe_of_its_cone_without_a_vertex_loop() {
    let frustum = Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0).with("at", [5.0, -3.0, 2.0]);
    let mut session = modelled(&[("cone", "part", frustum)]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(["CONICAL_SURFACE", "VERTEX_LOOP", "EDGE_CURVE"].map(|kind| count(&text, kind)), [1, 0, 2]);
}

#[test]
fn a_cone_standing_on_its_tip_is_written_on_the_other_nappe() {
    let mut session = modelled(&[
        ("profile", "section", upright("M 0,0 L 4,9 L 0,9 Z")),
        ("revolve", "part", Params::new()),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(count(&text, "VERTEX_LOOP"), 1);
}

#[test]
fn a_sphere_is_one_face_bounded_by_a_vertex_loop() {
    let mut session =
        modelled(&[("sphere", "part", Params::new().with("r", 5.0).with("at", [1.0, 2.0, 3.0]))]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(
        ["SPHERICAL_SURFACE", "VERTEX_LOOP", "EDGE_CURVE", "ADVANCED_FACE"].map(|kind| count(&text, kind)),
        [1, 1, 0, 1]
    );
}

#[test]
fn a_torus_is_one_face_with_a_handle_bounded_by_a_vertex_loop() {
    let mut session = modelled(&[("torus", "part", Params::new().with("ring", 6.0).with("tube", 2.0))]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[1]]), Ok(()));
    assert_eq!(["TOROIDAL_SURFACE", "VERTEX_LOOP", "EDGE_CURVE"].map(|kind| count(&text, kind)), [1, 1, 0]);
}

#[test]
fn a_plate_with_a_hole_has_the_hole_as_an_inner_bound_of_both_flat_faces() {
    let outline = Params::new().with("path", "M -30,-20 L 30,-20 L 30,20 L -30,20 Z O 20,15 4");
    let mut session =
        modelled(&[("profile", "outline", outline), ("extrude", "part", Params::new().with("h", 10.0))]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[1]]), Ok(()));
    assert_eq!(
        ["ADVANCED_FACE", "FACE_OUTER_BOUND", "FACE_BOUND", "CYLINDRICAL_SURFACE"]
            .map(|kind| count(&text, kind)),
        [7, 7, 3, 1]
    );
}

#[test]
fn a_partial_revolution_of_a_rectangle_is_written_with_its_two_flat_ends() {
    let mut session = modelled(&[
        ("profile", "section", upright("M 2,0 L 5,0 L 5,4 L 2,4 Z")),
        ("revolve", "part", Params::new().with("angle", 250.0)),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(["ADVANCED_FACE", "CYLINDRICAL_SURFACE", "PLANE"].map(|kind| count(&text, kind)), [6, 2, 4]);
}

#[test]
fn a_slice_of_a_cone_has_its_apex_as_an_ordinary_vertex() {
    let mut session = modelled(&[
        ("profile", "section", upright("M 0,0 L 4,0 L 0,9 Z")),
        ("revolve", "part", Params::new().with("angle", -120.0)),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(count(&text, "VERTEX_LOOP"), 0);
}

#[test]
fn a_bend_of_a_torus_is_bounded_by_its_two_end_circles() {
    let mut session = modelled(&[
        ("profile", "section", upright("O 6,0 2")),
        ("revolve", "part", Params::new().with("angle", 200.0)),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(["TOROIDAL_SURFACE", "CIRCLE", "VERTEX_LOOP"].map(|kind| count(&text, kind)), [1, 2, 0]);
}

#[test]
fn a_wedge_of_a_sphere_is_bounded_by_two_arcs_between_its_poles() {
    let mut session = modelled(&[
        ("profile", "section", upright("M 0,-5 A 0,0 0,5 ccw Z")),
        ("revolve", "part", Params::new().with("angle", 100.0)),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(["SPHERICAL_SURFACE", "CIRCLE", "VERTEX_POINT"].map(|kind| count(&text, kind)), [1, 2, 2]);
}

#[test]
fn a_dome_is_a_sphere_face_bounded_by_one_circle() {
    let mut session = modelled(&[
        ("profile", "section", upright("M 0,0 L 5,0 A 0,0 0,5 ccw Z")),
        ("revolve", "part", Params::new()),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
    assert_eq!(["SPHERICAL_SURFACE", "PLANE", "EDGE_CURVE"].map(|kind| count(&text, kind)), [1, 1, 1]);
}

#[test]
fn a_pattern_is_written_as_one_solid_per_shell() {
    let mut session = modelled(&[
        ("cylinder", "peg", Params::new().with("r", 3.0).with("h", 6.0).with("at", [20.0, 10.0, 10.0])),
        ("pattern", "part", Params::new().with("n", 3.0).with("step", [-20.0, 0.0, 0.0])),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0, 0, 0]]), Ok(()));
    assert_eq!(["MANIFOLD_SOLID_BREP", "CLOSED_SHELL", "PRODUCT"].map(|kind| count(&text, kind)), [3, 3, 1]);
    assert!(text.contains("MANIFOLD_SOLID_BREP('part/2',"), "{text}");
}

#[test]
fn a_body_turned_out_of_the_world_axes_keeps_every_sense() {
    let skew = Params::new().with("turn", 37.0).with("about", [1.0, 2.0, 3.0]).with("move", [4.0, -5.0, 6.0]);
    let mut session = modelled(&[
        ("cone", "frustum", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0)),
        ("transform", "part", skew),
    ]);

    let text = step(&mut session, &["part"]);

    assert_eq!(verify(&text, &session, &["part"], &[&[0]]), Ok(()));
}

#[test]
fn several_nodes_become_several_products_that_share_their_geometry() {
    let mut session = modelled(&[
        ("box", "plate", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0)),
        ("cylinder", "peg", Params::new().with("r", 3.0).with("h", 6.0).with("at", [0.0, 0.0, 10.0])),
    ]);

    let text = step(&mut session, &["plate", "peg"]);

    assert_eq!(verify(&text, &session, &["plate", "peg"], &[&[0], &[0]]), Ok(()));
    assert_eq!(["PRODUCT", "ADVANCED_BREP_SHAPE_REPRESENTATION"].map(|kind| count(&text, kind)), [2, 2]);
    // The top of the plate and the bottom of the peg lie in one plane, written once.
    assert_eq!(count(&text, "PLANE"), 7);
}

#[test]
fn asking_twice_gives_the_same_file_byte_for_byte() {
    let mut session =
        modelled(&[("cone", "part", Params::new().with("r", 4.0).with("h", 9.0).with("top", 2.0))]);

    let (first, second) = (step(&mut session, &["part"]), step(&mut session, &["part"]));

    assert_eq!(first, second);
}

#[test]
fn the_check_notices_a_face_whose_sense_is_flipped() {
    let mut session = modelled(&[("cylinder", "part", Params::new().with("r", 3.0).with("h", 10.0))]);
    let text = step(&mut session, &["part"]);
    let side = text.lines().find(|line| line.contains("ADVANCED_FACE('side'")).unwrap();

    let flipped = text.replace(side, &side.replace(",.T.);", ",.F.);"));

    let verdict = verify(&flipped, &session, &["part"], &[&[0]]);
    assert!(verdict.clone().is_err_and(|problem| problem.contains("face `side`")), "{verdict:?}");
}

#[test]
fn the_check_notices_an_edge_use_whose_direction_is_flipped() {
    let mut session =
        modelled(&[("box", "part", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0))]);
    let text = step(&mut session, &["part"]);
    let used = text.lines().find(|line| line.contains("ORIENTED_EDGE(") && line.contains(",.T.)")).unwrap();

    let flipped = text.replace(used, &used.replace(",.T.);", ",.F.);"));

    let verdict = verify(&flipped, &session, &["part"], &[&[0]]);
    assert!(verdict.clone().is_err_and(|problem| problem.contains("is not a closed chain")), "{verdict:?}");
}

#[test]
fn the_check_notices_a_closed_edge_that_runs_against_its_curve() {
    let mut session = modelled(&[("cylinder", "part", Params::new().with("r", 3.0).with("h", 10.0))]);
    let text = step(&mut session, &["part"]);
    let edge = text.lines().find(|line| line.contains("EDGE_CURVE(")).unwrap();

    let reversed = text.replace(edge, &edge.replace(",.T.);", ",.F.);"));

    let verdict = verify(&reversed, &session, &["part"], &[&[0]]);
    assert!(
        verdict.clone().is_err_and(|problem| problem.contains("does not lie to the left of edge")),
        "{verdict:?}"
    );
}

#[test]
fn the_check_notices_a_cone_placed_on_the_wrong_nappe() {
    let mut session = modelled(&[("cone", "part", Params::new().with("r", 4.0).with("h", 9.0))]);
    let text = step(&mut session, &["part"]);

    let turned = text.replace("DIRECTION('',(0.0,0.0,-1.0))", "DIRECTION('',(0.0,0.0,1.0))");

    let verdict = verify(&turned, &session, &["part"], &[&[0]]);
    assert!(
        verdict.clone().is_err_and(|problem| problem.contains("off the surface of face `side`")),
        "{verdict:?}"
    );
}

#[test]
fn the_check_notices_a_reference_that_leads_nowhere() {
    let mut session =
        modelled(&[("box", "part", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0))]);
    let text = step(&mut session, &["part"]);
    let shell = text.lines().find(|line| line.contains("CLOSED_SHELL(")).unwrap();

    let dangling = text.replace(shell, &shell.replace("(#", "(#9999,#"));

    let verdict = verify(&dangling, &session, &["part"], &[&[0]]);
    assert!(
        verdict.clone().is_err_and(|problem| problem.contains("of CLOSED_SHELL cannot be")),
        "{verdict:?}"
    );
}

#[test]
fn the_check_notices_an_entity_with_an_argument_missing() {
    let mut session =
        modelled(&[("box", "part", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0))]);
    let text = step(&mut session, &["part"]);

    let short = text.replace("MANIFOLD_SOLID_BREP('part',", "MANIFOLD_SOLID_BREP(");

    let verdict = verify(&short, &session, &["part"], &[&[0]]);
    assert!(
        verdict.clone().is_err_and(|problem| problem.contains("takes 2 argument(s), got 1")),
        "{verdict:?}"
    );
}
