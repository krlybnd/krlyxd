//! Looking at the model: the camera, the renderer and the queries that put them on the
//! protocol. A package of its own, registered from outside like any plugin: modelling
//! needs no camera, since the model is known from every side.
//!
//! One renderer (`raycast`) and the two queries that put it on the protocol: `render`
//! answers with a picture, `view` with the same look in words. What the two share — which
//! nodes are drawn, from where, how large — is settled here once.

pub mod base64;
pub mod likeness;
pub mod outline;
pub mod pick;
pub mod picture;
pub mod png;
pub mod ports;
pub mod raycast;
pub mod view;

use crate::ports::render::{render, Frame, Image, Paint, Render, RenderRequest, Seen, View, CHANNELS};
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::text::{direction, num};
use krly3d_core::{Attempt, Error, Exactness};
use krly3d_kernel::model::{Built, NodeRef};
use krly3d_kernel::ports::query::{Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Value};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, KernelBuilder};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn register(builder: &mut KernelBuilder) {
    builder
        .plugin::<Render>(raycast::Raycast)
        .plugin::<krly3d_kernel::ports::likeness::Likeness>(likeness::Raster)
        .plugin::<Query>(picture::Picture)
        .plugin::<Query>(view::Visible)
        .plugin::<Query>(outline::Silhouette)
        .plugin::<Query>(pick::Pick)
        .plugin::<Query>(likeness::LikenessQuery);
}

/// The views that have a name: the direction of sight and what points upward in the
/// image. A view is named after the side the model is seen from, so `front` looks at
/// the face a box calls `front`; `iso` looks down on the front right corner. Seen from
/// above the back is up, seen from below the front is, as on a drawing.
const VIEWS: [(&str, [f64; 3], [f64; 3]); 7] = [
    ("iso", [-1.0, 1.0, -1.0], [0.0, 0.0, 1.0]),
    ("front", [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ("back", [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
    ("left", [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ("right", [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ("top", [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ("bottom", [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
];

/// The largest width or height of an image, in pixels. A size comes from outside, and a
/// larger one is refused instead of being given the memory it asks for.
const LARGEST_SIDE: usize = 2048;

/// As many node names are written out in a text answer; more are counted.
const NAMES_WRITTEN: usize = 6;

/// The parameters of a look at the model.
fn shot_params() -> Vec<Param> {
    vec![
        Param::optional(
            "nodes",
            Kind::Texts,
            Vec::new(),
            "the nodes to draw, as name or name@version; every node when left out",
        ),
        Param::optional(
            "view",
            Kind::Any,
            "iso",
            "from where: iso, front, back, left, right, top or bottom, or the direction of sight as x,y,z (iso is -1,1,-1)",
        ),
        Param::optional("width", Kind::Count, 480_usize, "width of the image in pixels"),
        Param::optional("height", Kind::Count, 360_usize, "height of the image in pixels"),
    ]
}

/// One look at the model: what was drawn, from where, and the image of it.
struct Shot {
    /// The nodes drawn, as they were asked for.
    nodes: Vec<String>,
    bodies: Vec<Arc<Body>>,
    /// The view in words: its name, or its direction of sight.
    view: String,
    image: Image,
}

impl Shot {
    fn take(cx: &Context, request: &QueryRequest) -> Attempt<Self> {
        let asked = nodes(request).and_then(|(nodes, bodies)| {
            let paint = paints(&request.params, &nodes, &bodies)?;
            Ok(((nodes, bodies), view(cx, &request.params)?, size(&request.params)?, paint))
        });
        let ((nodes, bodies), (view, look), size, paint) = match asked {
            Ok(asked) => asked,
            Err(error) => return Attempt::Failed(error),
        };
        let drawn =
            render(cx, raycast::NAME, &RenderRequest { bodies: bodies.clone(), view: look, size, paint });
        drawn.map(|image| Self { nodes, bodies, view, image })
    }

    /// The faces seen with the number of pixels that show each, the largest first.
    fn tally(&self) -> Vec<(Seen, usize)> {
        let mut pixels: BTreeMap<Seen, usize> = BTreeMap::new();
        for seen in self.image.seen.iter().flatten() {
            *pixels.entry(*seen).or_insert(0) += 1;
        }
        let mut tally: Vec<(Seen, usize)> = pixels.into_iter().collect();
        // The order of bodies and faces settles ties, so the same look reads the same.
        tally.sort_by_key(|(_, pixels)| Reverse(*pixels));
        tally
    }

    /// `view size of nodes: n faces seen, 1 px = length` — the line both answers open with.
    fn headline(&self, verb: &str, faces: usize) -> String {
        let nodes = if self.nodes.len() <= NAMES_WRITTEN {
            self.nodes.join(", ")
        } else {
            format!("{} nodes", self.nodes.len())
        };
        format!(
            "{verb} {} {}x{} of {nodes}: {faces} face{} seen, 1 px = {}",
            self.view,
            self.image.width(),
            self.image.height(),
            if faces == 1 { "" } else { "s" },
            num(self.image.projection.pixel),
        )
    }

    /// The entries every answer about a look carries as data.
    fn data(&self) -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("view".to_string(), Value::from(self.view.as_str())),
            ("width".to_string(), Value::from(self.image.width())),
            ("height".to_string(), Value::from(self.image.height())),
            ("pixel".to_string(), Value::from(self.image.projection.pixel)),
        ])
    }

    /// A face is one of finitely many lines of sight away from being missed or seen.
    fn exactness(&self) -> Exactness {
        Exactness::Sampled { samples: u32::try_from(self.image.seen.len()).unwrap_or(u32::MAX) }
    }
}

/// The nodes to draw and their bodies (see [`built`]).
fn nodes(request: &QueryRequest) -> Result<(Vec<String>, Vec<Arc<Body>>), Error> {
    let found = built(request)?;
    let bodies = found.iter().map(|(_, built)| Arc::clone(&built.body)).collect();
    Ok((found.into_iter().map(|(name, _)| name).collect(), bodies))
}

/// The nodes a look takes in, built. Left to itself a look takes in every node, in the
/// order of its latest definition: where a node coincides with what it was made from, the
/// one defined later is the one seen.
fn built(request: &QueryRequest) -> Result<Vec<(String, &Built)>, Error> {
    let mut names = request.params.texts("nodes")?;
    if names.is_empty() {
        for statement in request.document.statements() {
            names.retain(|name| *name != statement.name);
            names.push(statement.name.clone());
        }
    }
    if names.is_empty() {
        return Err(Error::not_found("the model has no node to draw"));
    }
    names
        .into_iter()
        .map(|name| {
            let found = request.model.built(&NodeRef::parse(&name)?);
            found
                .map(|built| (name.clone(), built))
                .ok_or_else(|| Error::not_found(format!("there is no node `{name}`")))
        })
        .collect()
}

/// The `paint` parameter of a picture, which the text view does not take.
fn paint_param() -> Param {
    Param::alternative(
        "paint",
        Kind::Any,
        "colours as {name: \"#rrggbb\"}: a name is a node drawn, node#face for one face, or node#start* for the faces whose names start so; a face takes its own colour before its node's",
    )
}

/// The colour of each drawn body from the `paint` parameter, in the order of `names`.
fn paints(params: &Params, names: &[String], bodies: &[Arc<Body>]) -> Result<Vec<Paint>, Error> {
    let Some(given) = params.get("paint") else { return Ok(Vec::new()) };
    let Value::Map(entries) = given else {
        return Err(Error::input("`paint` is a map from names to colours, such as {\"plate\": \"#c04030\"}"));
    };
    let mut paint = vec![Paint::default(); bodies.len()];
    for (key, value) in entries {
        let colour = match value {
            Value::Text(text) => hex_colour(text)?,
            other => {
                return Err(Error::input(format!(
                    "the colour of `{key}` must be text like \"#c04030\", got {other:?}"
                )))
            }
        };
        let (node, face) =
            key.split_once('#').map_or((key.as_str(), None), |(node, face)| (node, Some(face)));
        let index = names
            .iter()
            .position(|name| name == node)
            .ok_or_else(|| Error::input(format!("`paint` names `{node}`, which is not drawn")))?;
        match face {
            None => paint[index].body = Some(colour),
            Some(face) => {
                let matches = |name: &str| match face.strip_suffix('*') {
                    Some(start) => name.starts_with(start),
                    None => name == face,
                };
                let found: Vec<usize> = (0..bodies[index].faces.len())
                    .filter(|at| matches(&bodies[index].faces[*at].name))
                    .collect();
                if found.is_empty() {
                    return Err(Error::input(format!("`{node}` has no face `{face}` to paint")));
                }
                for at in found {
                    paint[index].faces.insert(at, colour);
                }
            }
        }
    }
    Ok(paint)
}

/// `#rrggbb` (the `#` may be left out) as three channels.
fn hex_colour(text: &str) -> Result<[u8; CHANNELS], Error> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    let channel = |at: usize| digits.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
    match (digits.len(), channel(0), channel(2), channel(4)) {
        (6, Some(red), Some(green), Some(blue)) => Ok([red, green, blue]),
        _ => Err(Error::input(format!("`{text}` is not a colour; write it as #rrggbb"))),
    }
}

/// The view asked for, in words and as the renderer takes it.
fn view(cx: &Context, params: &Params) -> Result<(String, View), Error> {
    let looking = |toward: [f64; 3], up: [f64; 3]| View {
        toward: Vec3::from(toward),
        up: Vec3::from(up),
        frame: Frame::Fit,
    };
    match params.get("view") {
        Some(Value::Text(name)) => match VIEWS.iter().find(|(known, ..)| known == name) {
            Some((name, toward, up)) => Ok((name.to_string(), looking(*toward, *up))),
            None => {
                let known: Vec<&str> = VIEWS.iter().map(|(name, ..)| *name).collect();
                Err(Error::input(format!(
                    "there is no view `{name}`; the views are {}, or a direction of sight as x,y,z",
                    known.join(", ")
                )))
            }
        },
        Some(Value::List(_)) => {
            let toward = params.vector("view")?;
            // Looking straight up or down, the world's up says nothing; the back is up then.
            let up = if cx.accuracy().parallel(&toward, &Vec3::z()) { Vec3::y() } else { Vec3::z() };
            Ok((format!("toward {}", direction(&toward)), View { toward, up, frame: Frame::Fit }))
        }
        Some(Value::Map(entries)) => camera(cx, entries),
        _ => Err(Error::input("`view` is the name of a view, a direction of sight as x,y,z, or a camera {eye, toward or target, up, focal, centre}")),
    }
}

/// A pinhole camera given as `{eye, toward | target, up, focal, centre}`: where it stands,
/// where it looks (a direction, or a point to look at), which way is up in the picture (z
/// when left out), its focal length in pixels, and the pixel its lens axis meets (the
/// middle of the image when left out). A camera worked out from a photograph renders an
/// image that lies on the photograph pixel for pixel.
fn camera(cx: &Context, entries: &BTreeMap<String, Value>) -> Result<(String, View), Error> {
    let numbers = |name: &str, count: usize| -> Result<Option<Vec<f64>>, Error> {
        match entries.get(name) {
            None => Ok(None),
            Some(Value::List(items)) if items.len() == count => items
                .iter()
                .map(|item| match item {
                    Value::Number(number) if number.is_finite() => Ok(*number),
                    _ => Err(Error::input(format!("the camera's `{name}` holds {count} numbers"))),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(Error::input(format!("the camera's `{name}` holds {count} numbers"))),
        }
    };
    let triple = |values: Vec<f64>| Vec3::new(values[0], values[1], values[2]);
    let eye =
        Point3::from(triple(numbers("eye", 3)?.ok_or_else(|| Error::input("a camera needs its `eye`"))?));
    let toward = match (numbers("toward", 3)?, numbers("target", 3)?) {
        (Some(toward), None) => triple(toward),
        (None, Some(target)) => triple(target) - eye.coords,
        _ => return Err(Error::input("a camera needs either `toward` or `target`, not both")),
    };
    let up = numbers("up", 3)?.map_or_else(Vec3::z, triple);
    let focal = match entries.get("focal") {
        Some(Value::Number(focal)) => *focal,
        _ => return Err(Error::input("a camera needs its `focal` length in pixels")),
    };
    let centre = numbers("centre", 2)?.map(|values| (values[0], values[1]));
    if cx.accuracy().parallel(&up, &toward) {
        return Err(Error::input("the camera's `up` must not run along the direction it looks"));
    }
    let name = format!("camera at {},{},{}", num(eye.x), num(eye.y), num(eye.z));
    Ok((name, View { toward, up, frame: Frame::Camera { eye, focal, centre } }))
}

fn size(params: &Params) -> Result<(usize, usize), Error> {
    let side = |name: &str| match params.count(name)? {
        pixels if pixels <= LARGEST_SIDE => Ok(pixels),
        pixels => Err(Error::input(format!("`{name}` may be at most {LARGEST_SIDE} pixels, got {pixels}"))),
    };
    Ok((side("width")?, side("height")?))
}

#[cfg(test)]
pub(crate) mod testing {
    use krly3d_kernel::{Context, KernelBuilder};

    /// The standard plugins with this package's.
    pub(crate) fn builder() -> KernelBuilder {
        let mut builder = krly3d_std::standard();
        super::register(&mut builder);
        builder
    }

    pub(crate) fn context() -> Context {
        let mut builder = builder();
        builder.accuracy(krly3d_core::Accuracy::for_scale(100.0).unwrap());
        builder.context()
    }

    use krly3d_core::Accuracy;
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::Session;

    /// A session holding a plate of 60 by 40 by 10 and, in front of it, a ball.
    pub(crate) fn session() -> Session {
        let mut builder = builder();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        let mut session = builder.session();
        let plate = Params::new().with("id", "plate").with("w", 60.0).with("d", 40.0).with("h", 10.0);
        let ball = Params::new().with("id", "ball").with("r", 4.0).with("at", [0.0, -40.0, 5.0]);
        assert!(session.call("box", &plate).ok && session.call("sphere", &ball).ok);
        session
    }
}
