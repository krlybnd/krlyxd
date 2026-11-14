//! krlyvis as an endpoint, so that the transports of krly3d serve it unchanged: the shell
//! form, plain JSON-RPC, MCP, and joining krly3d as a peer.
//!
//! Like krly3d it keeps a document: every `picture`, `outline` and `frame` call adds a
//! statement to the vis layer, nodes have versions, `expect` claims are judged, and what a
//! node made is remembered, so a picture is decoded once however often it is asked about.

use krly3d_cli::endpoint::Endpoint;
use krly3d_core::document::{text as written, Cache, Document, NodeRef, Statement};
use krly3d_core::{Error, Exactness, Id};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::{describe, Method, MethodKind, Reply};
use krlyvis_kernel::model::{build, judge, Model, VISION};
use krlyvis_kernel::ports::make::Make;
use krlyvis_kernel::report::report;
use krlyvis_kernel::{Ask, Axis, Datum, Known, Pixel, Scene, Vision};
use std::collections::BTreeMap;
use std::sync::Arc;

const INSTRUCTIONS: &str = "krlyvis reads pictures without any learned model and keeps what it found as \
     nodes, as krly3d does: `picture id=photo path=…` reads a file once, `outline id=top inputs=photo` \
     finds the subject as polygons, `frame id=topw inputs=top view=top points=px,py,a,b,px,py,a,b` places \
     a drawing's outline in the world; `polygons node=…` gives them, `save` the statements. Call \
     `analyse` with `path`, an image file (.png, .jpg, .pgm, .ppm, .pnm), or `node`, a picture. The first call lists the straight segments (s0, s1, … \
     longest first), the vanishing points and the camera. Call again with `plane` (xz, yz or xy; z is \
     up, x the horizontal direction with most edges) and `known` (`s12=4.5`: segment 12 is 4.5 long) \
     to get lengths in that plane in the unit of the known one. Every vertical segment whose lower \
     end is below the horizon is listed with `h` (its height) and `d` (how far its foot is), taken to \
     stand on a flat ground: give `eye` (the camera's height) or `known` naming such a segment to set \
     the unit; window edges and other segments that do not touch the ground come out wrong. `~` \
     marks numbers found numerically; when `y off square` exceeds a few degrees the camera is not to \
     be trusted.";

/// Reading every number of an image is good to about a pixel.
const PIXEL: Exactness = Exactness::Numeric { tolerance: 1.0 };

pub struct VisionSession {
    vision: Vision,
    document: Document,
    cache: Cache<Datum>,
    model: Model,
}

impl VisionSession {
    pub fn new(vision: Vision) -> Self {
        Self { vision, document: Document::default(), cache: Cache::default(), model: Model::default() }
    }

    /// The standard plugins.
    pub fn standard() -> Self {
        Self::new(krlyvis_std::standard().build())
    }

    fn analyse(&self, args: &Params) -> Result<Reply, Error> {
        let args = analyse_schema().admit(&Id::new("analyse"), args, 0)?;
        let ask = ask_of(&args)?;
        let given = |name: &str| args.get(name).is_some().then(|| args.text(name)).transpose();
        let image = match (given("path")?, given("node")?) {
            (None, None) => {
                return Err(Error::input("`analyse` needs `path`, an image file, or `node`, a picture"))
            }
            (Some(path), None) => {
                let format = krlyvis_std::read::format_of(path).ok_or_else(|| {
                    Error::input(format!(
                        "{path}: the readers take .png, .jpg, .jpeg, .pgm, .ppm and .pnm files"
                    ))
                })?;
                let bytes =
                    std::fs::read(path).map_err(|error| Error::not_found(format!("{path}: {error}")))?;
                Arc::new(self.vision.read(format, bytes)?)
            }
            (None, Some(node)) => match &self.datum(node)? {
                Datum::Picture { image, .. } => Arc::clone(image),
                other => return Err(Error::input(format!("`{node}` is a {}, not a picture", other.kind()))),
            },
            _ => return Err(Error::input("`analyse` takes `path` or `node`, not both")),
        };
        let scene = self.vision.analyse(image, ask)?;
        Ok(Reply {
            ok: true,
            text: report(&scene, args.count("show")?),
            data: scene_value(&scene),
            exact: PIXEL,
        })
    }

    fn datum(&self, node: &str) -> Result<Datum, Error> {
        let reference = NodeRef::parse(node)?;
        self.model
            .get(&reference)
            .map(|built| built.datum.clone())
            .ok_or_else(|| Error::not_found(format!("there is no node `{node}`")))
    }

    /// A statement of the vis layer, kept only when it builds and its claims hold.
    fn make(&mut self, tool: &str, args: &Params) -> Result<Reply, Error> {
        let mut params = args.clone();
        let id = match params.remove("id") {
            Some(Value::Text(id)) => id,
            _ => return Err(Error::input(format!("`{tool}` needs `id`, the name of the node it makes"))),
        };
        let inputs = texts(params.remove("inputs"), "inputs")?;
        let expect = texts(params.remove("expect"), "expect")?;
        let schema = self
            .vision
            .context()
            .registry::<Make>()
            .and_then(|registry| registry.schema(&Id::new(tool)))
            .ok_or_else(|| Error::not_found(format!("there is no tool `{tool}`")))?;
        let params = schema.admit(&Id::new(tool), &params, inputs.len())?;
        let reads: Vec<&str> = inputs.iter().map(String::as_str).collect();
        let mut statement = Statement::new(&id, tool, params, &reads)?;
        statement.expect = expect.clone();
        let next = self.document.with_in(VISION, statement);
        let model = build(self.vision.context(), &next, &self.cache)?;
        let latest = NodeRef { name: id.clone(), version: None };
        let built = model.get(&latest).ok_or_else(|| Error::not_found(format!("`{id}` was not built")))?;
        let version = model.nodes().find(|(name, _)| *name == id).map_or(1, |(_, count)| count);
        let verdicts =
            expect.iter().map(|claim| judge(&id, &built.datum, claim)).collect::<Result<Vec<_>, _>>()?;
        let holds = verdicts.iter().all(|verdict| verdict.holds);
        let mut text = format!(
            "{} {id}@{version} {tool}. {}\n",
            if holds { "ok" } else { "refused" },
            summary(&built.datum)
        );
        for verdict in &verdicts {
            let outcome =
                if verdict.holds { "ok".to_string() } else { format!("FAILED, found {}", verdict.found) };
            text.push_str(&format!("expect {id}@{version} {}: {outcome}\n", verdict.claim));
        }
        let data = built.datum.value();
        if holds {
            self.document = next;
            self.model = model;
        }
        Ok(Reply { ok: holds, text, data, exact: PIXEL })
    }

    fn polygons(&self, args: &Params) -> Result<Reply, Error> {
        let args = polygons_schema().admit(&Id::new("polygons"), args, 0)?;
        let node = args.text("node")?;
        let datum = self.datum(node)?;
        if datum.shape().is_none() {
            return Err(Error::input(format!("`{node}` is a {}, which has no outline", datum.kind())));
        }
        Ok(Reply {
            ok: true,
            text: format!("{node}: {}\n", summary(&datum)),
            data: datum.value(),
            exact: PIXEL,
        })
    }

    /// Cross-sections across `axis` at the stations `at`, read from two placed outlines
    /// that both show that axis: how wide in the one, how high in the other, and the
    /// rounded rectangle a loft section could start from.
    fn sections(&self, args: &Params) -> Result<Reply, Error> {
        let args = sections_schema().admit(&Id::new("sections"), args, 0)?;
        let axis_name = args.text("axis")?;
        let axis = krlyvis_kernel::frame::AXIS_NAMES
            .iter()
            .position(|name| *name == axis_name)
            .ok_or_else(|| Error::input(format!("`axis` is x, y or z, got `{axis_name}`")))?;
        let views: Vec<Datum> =
            args.texts("views")?.iter().map(|node| self.datum(node)).collect::<Result<_, _>>()?;
        if views.len() != 2 {
            return Err(Error::input("`views` names two placed outlines, such as top-w,side-w"));
        }
        let names = krlyvis_kernel::frame::AXIS_NAMES;
        let mut text = String::new();
        let mut rows = Vec::new();
        for at in args.numbers("at")? {
            let found: Vec<Option<(usize, [f64; 2])>> =
                views.iter().map(|view| view.extent_at(axis, at)).collect();
            let [Some((first_axis, first)), Some((second_axis, second))] = found[..] else {
                text.push_str(&format!(
                    "{axis_name}={}: outside one of the views\n",
                    krly3d_core::text::num(at)
                ));
                continue;
            };
            // The section is drawn in the plane of the other two axes, in their order.
            let ((u_axis, u), (v_axis, v)) = if first_axis < second_axis {
                ((first_axis, first), (second_axis, second))
            } else {
                ((second_axis, second), (first_axis, first))
            };
            let (width, height) = (u[1] - u[0], v[1] - v[0]);
            let centre = [0.5 * (u[0] + u[1]), 0.5 * (v[0] + v[1])];
            let radius = 0.25 * width.min(height);
            let num = krly3d_core::text::num;
            let path = format!(
                "R {},{} {},{} {}",
                num(centre[0]),
                num(centre[1]),
                num(width),
                num(height),
                num(radius)
            );
            let plane = format!("{}{}", names[u_axis], names[v_axis]);
            text.push_str(&format!(
                "{axis_name}={}: {} {}..{}, {} {}..{} -> plane={plane} path=\"{path}\"\n",
                num(at),
                names[u_axis],
                num(u[0]),
                num(u[1]),
                names[v_axis],
                num(v[0]),
                num(v[1])
            ));
            rows.push(Value::map([
                ("at", Value::from(at)),
                ("plane", Value::from(plane.as_str())),
                ("low", Value::List(vec![u[0].into(), v[0].into()])),
                ("high", Value::List(vec![u[1].into(), v[1].into()])),
                ("path", Value::from(path.as_str())),
            ]));
        }
        Ok(Reply { ok: true, text, data: Value::List(rows), exact: PIXEL })
    }

    fn open(&mut self, args: &Params) -> Result<Reply, Error> {
        let text = args.text("text")?;
        let document = written::parse(text, &[VISION])?;
        let model = build(self.vision.context(), &document, &self.cache)?;
        let count = document.statements().count();
        self.document = document;
        self.model = model;
        Ok(Reply {
            ok: true,
            text: format!("opened {count} statements\n"),
            data: Value::map([]),
            exact: Exactness::Exact,
        })
    }

    fn save(&self) -> Result<Reply, Error> {
        let text = written::print(&self.document)?;
        Ok(Reply {
            ok: true,
            text: text.clone(),
            data: Value::map([("text", Value::from(text.as_str()))]),
            exact: Exactness::Exact,
        })
    }

    fn tools(&self) -> Vec<Method> {
        let Some(registry) = self.vision.context().registry::<Make>() else { return Vec::new() };
        registry
            .ids()
            .filter_map(|id| {
                let mut schema = registry.schema(id)?;
                schema.params.splice(0..0, statement_params());
                Some(Method { name: id.to_string(), kind: MethodKind::Tool, schema })
            })
            .collect()
    }
}

/// What every tool call takes besides the tool's own parameters, as in krly3d.
fn statement_params() -> Vec<Param> {
    vec![
        Param::required("id", Kind::Text, "the node made; naming it again makes a new version"),
        Param::optional(
            "inputs",
            Kind::Texts,
            Value::List(Vec::new()),
            "the nodes read, as name or name@version",
        ),
        Param::optional(
            "expect",
            Kind::Texts,
            Value::List(Vec::new()),
            "claims that must hold, such as polygons=1",
        ),
    ]
}

fn texts(value: Option<Value>, name: &str) -> Result<Vec<String>, Error> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Text(one)) => Ok(vec![one]),
        Some(Value::List(items)) => items
            .into_iter()
            .map(|item| match item {
                Value::Text(text) => Ok(text),
                other => Err(Error::input(format!("`{name}` holds texts, got {other:?}"))),
            })
            .collect(),
        Some(other) => Err(Error::input(format!("`{name}` holds texts, got {other:?}"))),
    }
}

/// What a node made, in a few words.
fn summary(datum: &Datum) -> String {
    let measure = |name: &str| datum.measure(name).map_or("?".to_string(), krly3d_core::text::num);
    match datum {
        Datum::Picture { path, .. } => {
            format!("picture {}x{} of {path}", measure("width"), measure("height"))
        }
        Datum::Outline { shape } => format!(
            "outline: {} polygon(s), {} points, {} pixels, threshold {}",
            measure("polygons"),
            measure("points"),
            measure("pixels"),
            krly3d_core::text::num(shape.threshold)
        ),
        Datum::Placed { frame, .. } => format!(
            "placed outline, {} view: {} points, {} by {} along {},{}, area {}",
            frame.view.name(),
            measure("points"),
            measure("width"),
            measure("height"),
            krlyvis_kernel::frame::AXIS_NAMES[frame.axes[0]],
            krlyvis_kernel::frame::AXIS_NAMES[frame.axes[1]],
            measure("area")
        ),
    }
}

fn sections_schema() -> Schema {
    Schema {
        about: "cross-sections across an axis read from two placed outlines that both show it (a plan and an elevation): their extents and a rounded rectangle for a loft section",
        inputs: (0, 0),
        params: vec![
            Param::required("views", Kind::Texts, "two placed outlines, such as top-w,side-w"),
            Param::required("axis", Kind::Text, "the axis the sections are across: x, y or z"),
            Param::required("at", Kind::Numbers, "where along the axis, in world units"),
        ],
    }
}

fn polygons_schema() -> Schema {
    Schema {
        about: "the polygons of an outline node, in pixels and, when it is placed, in world coordinates",
        inputs: (0, 0),
        params: vec![Param::required(
            "node",
            Kind::Text,
            "the outline or placed outline, as name or name@version",
        )],
    }
}

impl Endpoint for VisionSession {
    fn name(&self) -> &str {
        "krlyvis"
    }

    fn instructions(&self) -> String {
        INSTRUCTIONS.to_string()
    }

    fn methods(&self) -> Vec<Method> {
        let session = |name: &str, about: &'static str, params: Vec<Param>| Method {
            name: name.to_string(),
            kind: MethodKind::Session,
            schema: Schema { about, inputs: (0, 0), params },
        };
        let mut methods = vec![
            session("methods", "every method with its arguments", Vec::new()),
            session(
                "open",
                "replaces the document with one written as text",
                vec![Param::required("text", Kind::Text, "the document, `k3d 1` with a `layer vis`")],
            ),
            session("save", "the document as text", Vec::new()),
            Method { name: "analyse".to_string(), kind: MethodKind::Query, schema: analyse_schema() },
            Method { name: "polygons".to_string(), kind: MethodKind::Query, schema: polygons_schema() },
            Method { name: "sections".to_string(), kind: MethodKind::Query, schema: sections_schema() },
        ];
        methods.extend(self.tools());
        methods
    }

    fn call(&mut self, method: &str, args: &Params) -> Reply {
        let is_tool = self
            .vision
            .context()
            .registry::<Make>()
            .is_some_and(|registry| registry.contains(&Id::new(method)));
        let outcome = match method {
            "methods" => Ok(describe(&self.methods())),
            "analyse" => self.analyse(args),
            "polygons" => self.polygons(args),
            "sections" => self.sections(args),
            "open" => self.open(args),
            "save" => self.save(),
            tool if is_tool => self.make(tool, args),
            other => Err(Error::not_found(format!("there is no method `{other}`; `methods` lists them"))),
        };
        outcome.unwrap_or_else(|error| Reply {
            ok: false,
            text: error.to_string(),
            data: Value::map([("error", Value::from(error.kind.label()))]),
            exact: Exactness::Exact,
        })
    }
}

fn analyse_schema() -> Schema {
    Schema {
        about: "segments, vanishing points, camera and, in a plane, true lengths found in one picture",
        inputs: (0, 0),
        params: vec![
            Param::alternative("path", Kind::Text, "the image file: .png, .jpg, .pgm, .ppm or .pnm"),
            Param::alternative("node", Kind::Text, "a picture node, in place of `path`"),
            Param::optional("focal", Kind::Number, 0.0, "focal length in pixels; 0 when unknown"),
            Param::optional(
                "camera",
                Kind::Text,
                "camera",
                "`camera` assumes the lens centre in the middle; `orthocentre` finds it",
            ),
            Param::optional("plane", Kind::Text, "", "plane to measure in: xz, yz or xy; none when empty"),
            Param::optional(
                "known",
                Kind::Text,
                "",
                "`s12=4.5`: segment 12 is 4.5 long; sets the unit in the plane, or of depths when it stands",
            ),
            Param::optional("eye", Kind::Number, 0.0, "the camera's height above the ground; 0 when unknown"),
            Param::optional("show", Kind::Count, 40usize, "how many segments to list, longest first"),
        ],
    }
}

fn ask_of(args: &Params) -> Result<Ask, Error> {
    let focal = args.number("focal")?;
    let plane = match args.text("plane")? {
        "" => None,
        named => Some(Axis::square_to(named)?),
    };
    let known = match args.text("known")? {
        "" => None,
        stated => Some(known_of(stated)?),
    };
    let eye = args.number("eye")?;
    for (name, value) in [("focal", focal), ("eye", eye)] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(Error::input(format!("`{name}` must be positive, or 0 when unknown; got {value}")));
        }
    }
    Ok(Ask {
        focal: (focal > 0.0).then_some(focal),
        camera: Id::new(args.text("camera")?),
        plane,
        known,
        eye: (eye > 0.0).then_some(eye),
    })
}

/// `s12=4.5` or `12=4.5`.
fn known_of(text: &str) -> Result<Known, Error> {
    let refused = || Error::input(format!("`known` takes s<id>=<length>, such as s12=4.5; got `{text}`"));
    let (id, length) = text.split_once('=').ok_or_else(refused)?;
    let segment = id.strip_prefix('s').unwrap_or(id).parse().map_err(|_| refused())?;
    let length: f64 = length.parse().map_err(|_| refused())?;
    if !(length.is_finite() && length > 0.0) {
        return Err(refused());
    }
    Ok(Known { segment, length })
}

/// The scene as data: the same facts as the report, every segment included.
fn scene_value(scene: &Scene) -> Value {
    let point = |at: Pixel| Value::List(vec![Value::from(at.x), Value::from(at.y)]);
    let towards = scene.vanishing_of();
    let segments = scene.segments.iter().enumerate().map(|(id, segment)| {
        let mut entries = BTreeMap::from([
            ("id".to_string(), Value::from(id)),
            ("from".to_string(), point(segment.from)),
            ("to".to_string(), point(segment.to)),
            ("length".to_string(), Value::from(segment.length())),
        ]);
        if let Some(Some(vanishing)) = towards.get(id) {
            entries.insert("vanishing".to_string(), Value::from(*vanishing));
        }
        Value::Map(entries)
    });
    let vanishing = scene.vanishing.iter().map(|found| {
        Value::map([
            ("point", Value::from([found.point.x, found.point.y, found.point.z])),
            ("members", Value::List(found.members.iter().map(|&member| Value::from(member)).collect())),
            ("support", Value::from(found.support)),
        ])
    });
    let measured = scene.measured.iter().map(|found| {
        Value::map([
            ("segment", Value::from(found.segment)),
            ("length", Value::from(found.length)),
            ("from", Value::List(vec![Value::from(found.from.x), Value::from(found.from.y)])),
            ("to", Value::List(vec![Value::from(found.to.x), Value::from(found.to.y)])),
        ])
    });
    let mut data = BTreeMap::from([
        (
            "size".to_string(),
            Value::List(vec![Value::from(scene.image.width()), Value::from(scene.image.height())]),
        ),
        ("segments".to_string(), Value::List(segments.collect())),
        ("vanishing".to_string(), Value::List(vanishing.collect())),
        ("measured".to_string(), Value::List(measured.collect())),
        ("notes".to_string(), Value::texts(scene.notes.iter().map(String::as_str))),
    ]);
    if let Some(depths) = &scene.depth {
        let standing = depths.standing.iter().map(|found| {
            Value::map([
                ("segment", Value::from(found.segment)),
                ("height", Value::from(found.height)),
                ("distance", Value::from(found.distance)),
                ("foot", Value::List(vec![Value::from(found.foot.x), Value::from(found.foot.y)])),
            ])
        });
        data.insert("standing".to_string(), Value::List(standing.collect()));
    }
    if let Some(camera) = &scene.camera {
        let mut entries = BTreeMap::from([
            ("focal".to_string(), Value::from(camera.focal)),
            ("centre".to_string(), point(camera.centre)),
            ("method".to_string(), Value::from(camera.method.as_str())),
        ]);
        for axis in Axis::ALL {
            let direction = camera.axis(axis);
            entries.insert(axis.letter().to_string(), Value::from([direction.x, direction.y, direction.z]));
        }
        if let Some(degrees) = camera.off_square {
            entries.insert("off_square".to_string(), Value::from(degrees));
        }
        data.insert("camera".to_string(), Value::Map(entries));
    }
    Value::Map(data)
}

#[cfg(test)]
mod tests {
    use super::{known_of, VisionSession};
    use krly3d_cli::endpoint::Endpoint;
    use krly3d_kernel::schema::Params;
    use krlyvis_kernel::Known;

    #[test]
    fn known_is_read_with_or_without_the_s() {
        assert_eq!(known_of("s12=4.5").unwrap(), Known { segment: 12, length: 4.5 });
        assert_eq!(known_of("3=1").unwrap(), Known { segment: 3, length: 1.0 });
        for wrong in ["12", "s=1", "sx=1", "1=-2", "1=wide"] {
            assert!(known_of(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn methods_lists_analyse_with_its_arguments() {
        let reply = VisionSession::standard().call("methods", &Params::new());

        assert!(reply.ok);
        assert!(
            reply
                .text
                .contains("analyse (query) [path] [node] [focal] [camera] [plane] [known] [eye] [show]"),
            "{}",
            reply.text
        );
    }

    #[test]
    fn wrong_calls_are_failed_replies_that_say_why() {
        let mut session = VisionSession::standard();

        let unknown = session.call("look", &Params::new());
        let missing = session.call("analyse", &Params::new());
        let gif = session.call("analyse", &Params::new().with("path", "a.gif"));
        let low_eye = session.call("analyse", &Params::new().with("path", "a.pgm").with("eye", -1.6));

        assert!(unknown.text.starts_with("not found: there is no method `look`"));
        assert!(missing.text.contains("needs `path`"), "{}", missing.text);
        assert!(gif.text.contains("the readers take .png"), "{}", gif.text);
        assert!(low_eye.text.contains("`eye` must be positive"), "{}", low_eye.text);
        assert!(![unknown, missing, gif, low_eye].iter().any(|reply| reply.ok));
    }

    /// A 40x30 grey picture, light, with a dark 20x10 block from (10, 5), written where
    /// only this test looks.
    fn block_picture(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("krlyvis-session-{}-{name}.pgm", std::process::id()));
        let mut text = String::from("P2 40 30 255\n");
        for y in 0..30 {
            for x in 0..40 {
                let dark = (10..30).contains(&x) && (5..15).contains(&y);
                text.push_str(if dark { "30 " } else { "220 " });
            }
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn tool(session: &mut VisionSession, tool: &str, params: Params) -> krly3d_kernel::Reply {
        session.call(tool, &params)
    }

    #[test]
    fn a_picture_is_outlined_and_placed_in_the_world_as_nodes() {
        let mut session = VisionSession::standard();
        let path = block_picture("placed");

        let read =
            tool(&mut session, "picture", Params::new().with("id", "photo").with("path", path.as_str()));
        let outlined = tool(
            &mut session,
            "outline",
            Params::new().with("id", "top").with("inputs", "photo").with("expect", "polygons=1"),
        );
        let placed = tool(
            &mut session,
            "frame",
            Params::new()
                .with("id", "side")
                .with("inputs", "top")
                .with("view", "side")
                .with(
                    "points",
                    krly3d_kernel::schema::Value::List(
                        [0.0, 0.0, 0.0, 30.0, 40.0, 30.0, 80.0, -30.0]
                            .map(krly3d_kernel::schema::Value::from)
                            .to_vec(),
                    ),
                )
                .with("expect", "width=40±1e-9"),
        );
        std::fs::remove_file(&path).unwrap();

        assert!(read.ok && outlined.ok && placed.ok, "{} {} {}", read.text, outlined.text, placed.text);
        let saved = session.call("save", &Params::new()).text;
        assert!(saved.contains("layer vis\nphoto = picture"), "{saved}");
        let answer = session.call("polygons", &Params::new().with("node", "side"));
        assert!(answer.text.contains("40 by 20 along x,z"), "{}", answer.text);
    }

    #[test]
    fn a_claim_that_does_not_hold_keeps_the_node_out() {
        let mut session = VisionSession::standard();
        let path = block_picture("refused");
        tool(&mut session, "picture", Params::new().with("id", "photo").with("path", path.as_str()));

        let refused = tool(
            &mut session,
            "outline",
            Params::new().with("id", "top").with("inputs", "photo").with("expect", "polygons=2"),
        );
        std::fs::remove_file(&path).unwrap();

        assert!(!refused.ok && refused.text.contains("FAILED, found 1"), "{}", refused.text);
        assert!(!session.call("save", &Params::new()).text.contains("top ="));
    }

    #[test]
    fn a_picture_read_once_is_not_read_again_for_the_same_statement() {
        let mut session = VisionSession::standard();
        let path = block_picture("once");
        tool(&mut session, "picture", Params::new().with("id", "photo").with("path", path.as_str()));
        let saved = session.call("save", &Params::new()).text;
        std::fs::remove_file(&path).unwrap();

        let reopened = session.call("open", &Params::new().with("text", saved.as_str()));
        let outlined = tool(&mut session, "outline", Params::new().with("id", "top").with("inputs", "photo"));

        assert!(reopened.ok && outlined.ok, "{} {}", reopened.text, outlined.text);
    }
}
