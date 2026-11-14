//! Query `step`: solids as an ISO 10303-21 file of exact boundary representations.
//!
//! The file follows AP214 (`AUTOMOTIVE_DESIGN`). Nothing is approximated: every face is
//! written on its own surface and every edge on its own curve, with the numbers of the
//! kernel in their shortest exact spelling.
//!
//! The kernel keeps a body without seams, STEP asks for a little more:
//! * a closed edge gets a vertex where its parameter range starts, used as both of its ends;
//! * a face without boundary (a whole sphere or torus) is bounded by a `VERTEX_LOOP`;
//! * the apex of a cone inside a face is a `VERTEX_LOOP` bound of that face;
//! * each shell of a body is a `MANIFOLD_SOLID_BREP` of its own.
//!
//! Model units are written as millimetres, angles as radians.

use super::part21::{list, logical, real, string, Part21, Ref};
use krly3d_core::math::frame::{origin, x_axis, z_axis};
use krly3d_core::math::{Iso3, Point3, Uv, Vec3};
use krly3d_core::{Attempt, Error, ErrorKind, Exactness, Id};
use krly3d_kernel::model::NodeRef;
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::measure::measure;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::ports::trim::{place, Place, TrimRequest};
use krly3d_kernel::schema::{Kind, Param, Schema, Value};
use krly3d_kernel::topo::{Body, EdgeId, Face, FaceId, VertexId};
use krly3d_kernel::{Context, Plugin};
use krly3d_std::curve::{Circle, Line};
use krly3d_std::surface::{Cone, Cylinder, Plane, Sphere, Torus};
use std::collections::VecDeque;
use std::sync::Arc;

pub struct Step;

impl Plugin<Query> for Step {
    fn id(&self) -> Id {
        Id::new("step")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "solids as a STEP file (ISO 10303-21, AP214) of exact faces, edges and vertices; faces keep their names, each node is a product, model units are written as millimetres",
            inputs: (0, 0),
            params: vec![Param::required(
                "nodes",
                Kind::Texts,
                "the nodes to write, as name or name@version; each becomes a product of the file",
            )],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        match export(cx, request) {
            Ok(answer) => Attempt::Done(answer),
            // What this writer has no mapping for may be written by another one behind it.
            Err(error) if error.kind == ErrorKind::Unsupported => Attempt::Declined(error.message),
            Err(error) => Attempt::Failed(error),
        }
    }
}

/// How many of each thing went into the file.
#[derive(Default)]
struct Counts {
    solids: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
}

fn export(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let names = request.params.texts("nodes")?;
    if names.is_empty() {
        return Err(Error::input("`nodes` names no node to write"));
    }
    let mut file = Part21::default();
    let world = World::new(&mut file, cx)?;
    let mut counts = Counts::default();
    let mut products = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        if names[..index].contains(name) {
            return Err(Error::input(format!("node `{name}` is listed twice")));
        }
        let reference = NodeRef::parse(name)?;
        let built = request
            .model
            .built(&reference)
            .ok_or_else(|| Error::not_found(format!("there is no node `{reference}`")))?;
        if !built.body.solid {
            return Err(Error::unsupported(format!(
                "`{name}` is a sheet, not a solid; only solids are written to STEP"
            )));
        }
        let solids = Shape::new(cx, &mut file, &built.body, &mut counts).solids(name)?;
        products.push(world.product(&mut file, name, &solids));
    }
    file.own(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,{})", list(&products)));
    let text = file.file(&names.join("+"));
    let data = Value::map([
        ("solids", Value::from(counts.solids)),
        ("faces", Value::from(counts.faces)),
        ("edges", Value::from(counts.edges)),
        ("vertices", Value::from(counts.vertices)),
        ("entities", Value::from(file.len())),
        ("bytes", Value::from(text.len())),
    ]);
    Ok(Answer { text, data, exact: Exactness::Exact })
}

/// What every product of a file refers to: the application it belongs to, and the units
/// and accuracy of its numbers.
struct World {
    product_context: Ref,
    definition_context: Ref,
    representation_context: Ref,
    axes: Ref,
}

impl World {
    fn new(file: &mut Part21, cx: &Context) -> Result<Self, Error> {
        let application =
            file.own("APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
        file.own(format!(
            "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,{application})"
        ));
        let product_context = file.own(format!("PRODUCT_CONTEXT('',{application},'mechanical')"));
        let definition_context =
            file.own(format!("PRODUCT_DEFINITION_CONTEXT('part definition',{application},'design')"));
        // The kernel has no unit of its own; one model unit is written as one millimetre.
        let length = file.own("(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".into());
        let angle = file.own("(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".into());
        let solid_angle = file.own("(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())".into());
        let uncertainty = file.own(format!(
            "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}),{length},'distance_accuracy_value','points closer than this are one point')",
            real(cx.accuracy().length())?
        ));
        let representation_context = file.own(format!(
            "(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({uncertainty})) GLOBAL_UNIT_ASSIGNED_CONTEXT(({length},{angle},{solid_angle})) REPRESENTATION_CONTEXT('','3D'))"
        ));
        // Every body is in world coordinates, so every representation has the world frame.
        let axes = file.placement(&Point3::origin(), &Vec3::z(), &Vec3::x())?;
        Ok(Self { product_context, definition_context, representation_context, axes })
    }

    /// The product structure a reader expects around the solids of one node; the name of
    /// the node is the id and the name of the product.
    fn product(&self, file: &mut Part21, name: &str, solids: &[Ref]) -> Ref {
        let name = string(name);
        let product = file.own(format!("PRODUCT({name},{name},'',({}))", self.product_context));
        let formation = file.own(format!("PRODUCT_DEFINITION_FORMATION('','',{product})"));
        let definition =
            file.own(format!("PRODUCT_DEFINITION('design','',{formation},{})", self.definition_context));
        let shape = file.own(format!("PRODUCT_DEFINITION_SHAPE('','',{definition})"));
        let mut items = vec![self.axes];
        items.extend_from_slice(solids);
        let representation = file.own(format!(
            "ADVANCED_BREP_SHAPE_REPRESENTATION({name},{},{})",
            list(&items),
            self.representation_context
        ));
        file.own(format!("SHAPE_DEFINITION_REPRESENTATION({shape},{representation})"));
        product
    }
}

/// One body on its way into the file. Vertices and edges are remembered by their number
/// in the body, so each is written once however many faces use it.
struct Shape<'a> {
    cx: &'a Context,
    file: &'a mut Part21,
    body: &'a Arc<Body>,
    counts: &'a mut Counts,
    vertices: Vec<Option<Ref>>,
    edges: Vec<Option<Ref>>,
}

impl<'a> Shape<'a> {
    fn new(cx: &'a Context, file: &'a mut Part21, body: &'a Arc<Body>, counts: &'a mut Counts) -> Self {
        let (vertices, edges) = (vec![None; body.vertices.len()], vec![None; body.edges.len()]);
        Self { cx, file, body, counts, vertices, edges }
    }

    /// One `MANIFOLD_SOLID_BREP` per shell of the body.
    fn solids(mut self, name: &str) -> Result<Vec<Ref>, Error> {
        let shells = shells(self.body);
        let mut solids = Vec::with_capacity(shells.len());
        for (index, faces) in shells.iter().enumerate() {
            if !self.encloses_material(faces)? {
                return Err(Error::unsupported(format!(
                    "`{name}` has a cavity, bounded by face `{}`; solids with voids are not written to STEP yet",
                    self.body.faces[faces[0]].name
                )));
            }
            let mut written = Vec::with_capacity(faces.len());
            for face in faces {
                written.push(self.face(*face)?);
            }
            let shell = self.file.own(format!("CLOSED_SHELL('',{})", list(&written)));
            let label = if shells.len() == 1 { name.to_string() } else { format!("{name}/{}", index + 1) };
            solids.push(self.file.own(format!("MANIFOLD_SOLID_BREP({},{shell})", string(&label))));
            self.counts.solids += 1;
        }
        Ok(solids)
    }

    /// Whether the faces of a shell look out of the material they enclose, as the outer
    /// shell of a solid does, rather than into a cavity: the shell alone has a positive volume.
    fn encloses_material(&self, faces: &[FaceId]) -> Result<bool, Error> {
        let alone = Arc::new(Body {
            vertices: self.body.vertices.clone(),
            edges: self.body.edges.clone(),
            faces: faces.iter().map(|face| self.body.faces[*face].clone()).collect(),
            solid: true,
        });
        let volume = measure(self.cx, "volume", &alone).into_result()?.value;
        if volume.abs() <= self.cx.accuracy().volume() {
            return Err(Error::topology(format!(
                "the shell of face `{}` encloses no volume",
                self.body.faces[faces[0]].name
            )));
        }
        Ok(volume > 0.0)
    }

    fn face(&mut self, id: FaceId) -> Result<Ref, Error> {
        let body = self.body;
        let face = &body.faces[id];
        let geometry = self.surface(face)?;
        let outer = outer_loop(body, face);
        let mut bounds = Vec::with_capacity(face.loops.len() + 1);
        for (index, uses) in face.loops.iter().enumerate() {
            let mut oriented = Vec::with_capacity(uses.len());
            for used in uses {
                let edge = self.edge(used.edge)?;
                // The edge is written running the way the kernel edge runs, so the
                // orientation of its use in a loop is `Use::forward` as it stands.
                oriented
                    .push(self.file.own(format!("ORIENTED_EDGE('',*,*,{edge},{})", logical(used.forward))));
            }
            let ring = self.file.own(format!("EDGE_LOOP('',{})", list(&oriented)));
            let kind = if outer == Some(index) { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
            // STEP: seen against the normal of the face, a loop runs with the face on its
            // left. The kernel keeps its loops the same way about the outward normal, which
            // is the normal of the face below, so no loop is ever reversed.
            bounds.push(self.file.own(format!("{kind}('',{ring},.T.)")));
        }
        for point in self.lone_points(id, face)? {
            let vertex = self.vertex_at(&point)?;
            let ring = self.file.own(format!("VERTEX_LOOP('',{vertex})"));
            bounds.push(self.file.own(format!("FACE_BOUND('',{ring},.T.)")));
        }
        self.counts.faces += 1;
        // STEP: the normal of a face of a solid points out of the material, and `same_sense`
        // says whether that is the normal of the surface. Every surface is written with the
        // normal it has in the kernel (see `surface`), so the flag is `Face::outward`.
        Ok(self.file.own(format!(
            "ADVANCED_FACE({},{},{geometry},{})",
            string(&face.name),
            list(&bounds),
            logical(face.outward)
        )))
    }

    /// The points that bound a face without lying on any of its edges; each becomes a
    /// `VERTEX_LOOP`.
    fn lone_points(&self, id: FaceId, face: &Face) -> Result<Vec<Point3>, Error> {
        if face.loops.is_empty() {
            // A whole sphere or torus. STEP wants every face bounded, and a vertex loop
            // somewhere on the surface is how such a face is usually written.
            return Ok(vec![face.surface.point(Uv::origin())]);
        }
        let Some(cone) = face.surface.as_any().downcast_ref::<Cone>() else {
            return Ok(Vec::new());
        };
        // The kernel leaves the apex of a whole cone unmarked inside its face. In STEP the
        // point where the surface has no normal is a bound of the face.
        let apex = origin(cone.frame());
        let request = TrimRequest { body: Arc::clone(self.body), face: id, point: apex };
        Ok(match place(self.cx, &request).into_result()? {
            Place::Inside => vec![apex],
            Place::Boundary | Place::Outside => Vec::new(),
        })
    }

    fn edge(&mut self, id: EdgeId) -> Result<Ref, Error> {
        if let Some(written) = self.edges.get(id).copied().flatten() {
            return Ok(written);
        }
        let body = self.body;
        let edge = body.edges.get(id).ok_or_else(|| Error::topology("a face refers to a missing edge"))?;
        let (start, end) = match edge.ends {
            Some((start, end)) => (self.vertex(start)?, self.vertex(end)?),
            // A closed edge has no vertex in the kernel, but a STEP edge runs from a vertex
            // to a vertex: it gets one where its parameter range starts and runs from
            // there once around, back to the same vertex.
            None => {
                let only = self.vertex_at(&edge.point(0.0))?;
                (only, only)
            }
        };
        let geometry = curve(self.file, edge.curve.as_ref())?;
        // `same_sense` is true for every edge: a kernel edge runs from the lower end of its
        // range to the upper, and the curve is written with the parameter it has in the
        // kernel (see `curve`).
        let written = self.file.own(format!("EDGE_CURVE('',{start},{end},{geometry},.T.)"));
        self.edges[id] = Some(written);
        self.counts.edges += 1;
        Ok(written)
    }

    fn vertex(&mut self, id: VertexId) -> Result<Ref, Error> {
        if let Some(written) = self.vertices.get(id).copied().flatten() {
            return Ok(written);
        }
        let body = self.body;
        let point =
            body.vertices.get(id).ok_or_else(|| Error::topology("an edge refers to a missing vertex"))?;
        let written = self.vertex_at(point)?;
        self.vertices[id] = Some(written);
        Ok(written)
    }

    /// A new vertex. The point is shared with whatever else lies there; the vertex is not,
    /// because two vertices of a body are two even where they touch.
    fn vertex_at(&mut self, point: &Point3) -> Result<Ref, Error> {
        let geometry = self.file.point(point)?;
        self.counts.vertices += 1;
        Ok(self.file.own(format!("VERTEX_POINT('',{geometry})")))
    }

    /// The surface of a face. STEP places a surface by a frame whose z axis is the axis of
    /// the surface and whose x axis is where its angle starts, exactly as the kernel does,
    /// and gives it the same normal: the z axis for a plane, away from the axis for a
    /// cylinder and a cone, from the centre for a sphere and from the centre line of the
    /// tube for a torus.
    fn surface(&mut self, face: &Face) -> Result<Ref, Error> {
        let surface = face.surface.as_any();
        if let Some(plane) = surface.downcast_ref::<Plane>() {
            let position = axes(self.file, plane.frame())?;
            Ok(self.file.shared(format!("PLANE('',{position})")))
        } else if let Some(cylinder) = surface.downcast_ref::<Cylinder>() {
            let position = axes(self.file, cylinder.frame())?;
            Ok(self.file.shared(format!("CYLINDRICAL_SURFACE('',{position},{})", real(cylinder.radius())?)))
        } else if let Some(cone) = surface.downcast_ref::<Cone>() {
            self.nappe(face, cone)
        } else if let Some(sphere) = surface.downcast_ref::<Sphere>() {
            let position = axes(self.file, sphere.frame())?;
            Ok(self.file.shared(format!("SPHERICAL_SURFACE('',{position},{})", real(sphere.radius())?)))
        } else if let Some(torus) = surface.downcast_ref::<Torus>() {
            let position = axes(self.file, torus.frame())?;
            let (major, minor) = (real(torus.major())?, real(torus.minor())?);
            Ok(self.file.shared(format!("TOROIDAL_SURFACE('',{position},{major},{minor})")))
        } else {
            Err(Error::unsupported(format!(
                "face `{}` lies on a {}, which this writer has no STEP surface for",
                face.name,
                face.surface.classes().first().copied().unwrap_or("surface")
            )))
        }
    }

    /// The `CONICAL_SURFACE` a face lies on. The cone of the kernel is both nappes about
    /// its apex; the cone of STEP is the one nappe that widens along the z axis of its
    /// placement, with a stated radius at the placement. So the placement is put at the
    /// apex with radius zero, and its z axis points along the axis of the kernel's cone
    /// or against it, towards the nappe the face is on. The normal points away from the
    /// axis on either nappe in both conventions, so the sense of the face is not touched.
    fn nappe(&mut self, face: &Face, cone: &Cone) -> Result<Ref, Error> {
        let frame = cone.frame();
        let mut side = 0.0;
        for used in face.uses() {
            let edge = &self.body.edges[used.edge];
            for fraction in [0.0, 0.5, 1.0] {
                let height = frame.inverse_transform_point(&edge.point(fraction)).z;
                if self.cx.accuracy().is_zero_length(height) {
                    continue;
                }
                if side * height < 0.0 {
                    return Err(Error::unsupported(format!(
                        "face `{}` lies on both nappes of its cone, which STEP cannot write as one face",
                        face.name
                    )));
                }
                side = height.signum();
            }
        }
        if side == 0.0 {
            return Err(Error::topology(format!(
                "face `{}` has no edge away from the apex of its cone",
                face.name
            )));
        }
        let position = self.file.placement(&origin(frame), &(z_axis(frame) * side), &x_axis(frame))?;
        let (radius, semi_angle) = (real(0.0)?, real(cone.half_angle())?);
        Ok(self.file.shared(format!("CONICAL_SURFACE('',{position},{radius},{semi_angle})")))
    }
}

/// The frame of a kernel curve or surface as a placement: same origin, same z, same x.
fn axes(file: &mut Part21, frame: &Iso3) -> Result<Ref, Error> {
    file.placement(&origin(frame), &z_axis(frame), &x_axis(frame))
}

/// The curve of an edge, written so that a point has the same parameter in the file as
/// in the kernel. A `LINE` is `pnt + u·magnitude·dir`, the kernel's `origin + t·direction`
/// with the length of the direction as the magnitude. A `CIRCLE` is parameterised by the
/// angle from the x axis of its placement about the z axis, like the kernel's.
fn curve(file: &mut Part21, curve: &dyn Curve) -> Result<Ref, Error> {
    if let Some(line) = curve.as_any().downcast_ref::<Line>() {
        let (point, direction) = (file.point(&line.origin())?, file.direction(&line.direction())?);
        let vector = file.shared(format!("VECTOR('',{direction},{})", real(line.direction().norm())?));
        Ok(file.shared(format!("LINE('',{point},{vector})")))
    } else if let Some(circle) = curve.as_any().downcast_ref::<Circle>() {
        let position = axes(file, circle.frame())?;
        Ok(file.shared(format!("CIRCLE('',{position},{})", real(circle.radius())?)))
    } else {
        Err(Error::unsupported(format!(
            "an edge runs along a {}, which this writer has no STEP curve for",
            curve.classes().first().copied().unwrap_or("curve")
        )))
    }
}

/// The loop written as `FACE_OUTER_BOUND`. On a plane that is the loop the others lie
/// within: the one running counter-clockwise seen from outside, which is the one with a
/// positive area about the outward normal. On a curved face no loop need enclose the
/// others (the two circles of a cylinder do not) and STEP allows one outer bound at most,
/// so the first loop is taken.
fn outer_loop(body: &Body, face: &Face) -> Option<usize> {
    let Some(plane) = face.surface.as_any().downcast_ref::<Plane>() else {
        return (!face.loops.is_empty()).then_some(0);
    };
    let outward = if face.outward { plane.unit_normal() } else { -plane.unit_normal() };
    let area = |uses: &Vec<_>| {
        let enclosed = Face { loops: vec![uses.clone()], ..face.clone() };
        body.vector_area(&enclosed).dot(&outward)
    };
    (0..face.loops.len()).max_by(|a, b| area(&face.loops[*a]).total_cmp(&area(&face.loops[*b])))
}

/// The faces of a body grouped into shells: faces that share an edge belong to one shell.
/// Shells come in the order of their first face, and so do the faces within each.
fn shells(body: &Body) -> Vec<Vec<FaceId>> {
    let mut users: Vec<Vec<FaceId>> = vec![Vec::new(); body.edges.len()];
    for (id, face) in body.faces.iter().enumerate() {
        for used in face.uses() {
            if let Some(faces) = users.get_mut(used.edge) {
                faces.push(id);
            }
        }
    }
    let mut placed = vec![false; body.faces.len()];
    let mut shells = Vec::new();
    for first in 0..body.faces.len() {
        if placed[first] {
            continue;
        }
        placed[first] = true;
        let mut shell = Vec::new();
        let mut waiting = VecDeque::from([first]);
        while let Some(face) = waiting.pop_front() {
            shell.push(face);
            for used in body.faces[face].uses() {
                for neighbour in users.get(used.edge).into_iter().flatten() {
                    if !placed[*neighbour] {
                        placed[*neighbour] = true;
                        waiting.push_back(*neighbour);
                    }
                }
            }
        }
        shell.sort_unstable();
        shells.push(shell);
    }
    shells
}

#[cfg(test)]
mod tests {
    use krly3d_core::{Accuracy, Attempt, ErrorKind, Id};
    use krly3d_kernel::model::Statement;
    use krly3d_kernel::ports::query::{ask, contract, Answer};
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::Kernel;

    fn kernel(statements: &[(&str, &str, Params, &[&str])]) -> Kernel {
        let mut builder = krly3d_std::standard();
        crate::register(&mut builder);
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        let mut kernel = builder.build();
        for (name, tool, params, inputs) in statements {
            kernel.apply(Statement::new(name, tool, params.clone(), inputs).unwrap()).unwrap();
        }
        kernel
    }

    fn block() -> (&'static str, &'static str, Params, &'static [&'static str]) {
        ("plate", "box", Params::new().with("w", 60.0).with("d", 40.0).with("h", 10.0), &[])
    }

    fn asked(kernel: &Kernel, nodes: &[&str]) -> Attempt<Answer> {
        let (document, model) = kernel.snapshot();
        let params = Params::new().with("nodes", Value::texts(nodes.iter().copied()));
        ask(kernel.context(), &Id::new("step"), &params, &document, &model)
    }

    #[test]
    fn the_step_query_satisfies_the_query_contract() {
        let kernel = kernel(&[block()]);
        let (document, model) = kernel.snapshot();
        let params = Params::new().with("nodes", "plate");

        let verdict = contract::check(kernel.context(), &Id::new("step"), &params, &document, &model);

        assert_eq!(verdict, Ok(()));
    }

    #[test]
    fn the_answer_counts_what_was_written() {
        let kernel = kernel(&[block()]);

        let answer = asked(&kernel, &["plate"]).into_result().unwrap();

        let Value::Map(data) = &answer.data else { panic!("{:?}", answer.data) };
        let count = |name: &str| data.get(name).cloned();
        assert_eq!(
            [count("solids"), count("faces"), count("edges"), count("vertices")],
            [1, 6, 12, 8].map(|number: usize| Some(Value::from(number)))
        );
        assert_eq!(count("bytes"), Some(Value::from(answer.text.len())));
        assert_eq!(count("entities"), Some(Value::from(answer.text.matches(" = ").count())));
    }

    #[test]
    fn a_sheet_is_declined_as_not_a_solid() {
        let outline = ("outline", "rectangle", Params::new().with("w", 4.0).with("d", 2.0), &[] as &[&str]);
        let kernel = kernel(&[outline]);

        let answer = asked(&kernel, &["outline"]);

        let Attempt::Declined(reason) = answer else { panic!("{answer:?}") };
        assert!(reason.contains("`outline` is a sheet, not a solid"), "{reason}");
    }

    #[test]
    fn a_solid_with_a_cavity_is_declined_rather_than_written_inside_out() {
        let section = Params::new().with("path", "M 2,0 L 8,0 L 8,6 L 2,6 Z O 5,3 1").with("plane", "xz");
        let kernel = kernel(&[
            ("section", "profile", section, &[]),
            ("hollow", "revolve", Params::new(), &["section"]),
        ]);

        let answer = asked(&kernel, &["hollow"]);

        let Attempt::Declined(reason) = answer else { panic!("{answer:?}") };
        assert!(reason.contains("`hollow` has a cavity"), "{reason}");
    }

    #[test]
    fn a_node_that_does_not_exist_is_reported_by_name() {
        let kernel = kernel(&[block()]);

        let error = asked(&kernel, &["plate", "peg"]).into_result().unwrap_err();

        assert_eq!((error.kind, error.message.as_str()), (ErrorKind::NotFound, "there is no node `peg`"));
    }

    #[test]
    fn a_node_listed_twice_is_rejected() {
        let kernel = kernel(&[block()]);

        let error = asked(&kernel, &["plate", "plate"]).into_result().unwrap_err();

        assert_eq!((error.kind, error.message.as_str()), (ErrorKind::Input, "node `plate` is listed twice"));
    }

    #[test]
    fn an_earlier_version_of_a_node_is_written_under_its_versioned_name() {
        let lifted =
            ("plate", "transform", Params::new().with("move", [0.0, 0.0, 5.0]), &["plate"] as &[&str]);
        let kernel = kernel(&[block(), lifted]);

        let answer = asked(&kernel, &["plate@1", "plate"]).into_result().unwrap();

        assert!(answer.text.contains("PRODUCT('plate@1','plate@1','',"), "{}", answer.text);
        assert!(answer.text.contains("PRODUCT('plate','plate','',"), "{}", answer.text);
        assert!(answer.text.contains("CARTESIAN_POINT('',(30.0,20.0,15.0))"), "{}", answer.text);
    }
}
