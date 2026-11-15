//! Likeness by raster: the model cut into triangles, laid flat in the drawing's plane and
//! filled into the drawing's own grid of pixels, beside the drawing's outline filled into
//! the same grid; then the overlap, and the extents across at every row and column.
//!
//! The mesh is the generic mesh reading of the kernel at the drawing's resolution, so any
//! surface any tool makes is compared the same way, and a pixel is the finest difference
//! either side can show.

use krly3d_core::text::num;
use krly3d_core::{Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::interpret::mesh_of;
use krly3d_kernel::ports::likeness::{
    drawing_of, likeness, Drawing, Likeness, LikenessRequest, Resemblance, Station,
};
use krly3d_kernel::ports::peer::ask;
use krly3d_kernel::ports::query::{Answer, Query, QueryRequest};
use krly3d_kernel::schema::{Kind, Param, Params, Schema, Value};
use krly3d_kernel::{Context, Plugin};
use std::sync::Arc;

mod report;

pub struct Raster;

impl Plugin<Likeness> for Raster {
    fn id(&self) -> Id {
        Id::new("raster")
    }

    fn call(&self, cx: &Context, request: &LikenessRequest) -> Attempt<Resemblance> {
        compared(cx, request).into()
    }
}

/// The query `likeness`: the named nodes as one silhouette against a drawing a peer holds,
/// `against=vis:top` asking the peer `vis` for its node `top`.
pub struct LikenessQuery;

impl Plugin<Query> for LikenessQuery {
    fn id(&self) -> Id {
        Id::new("likeness")
    }

    fn schema(&self) -> Option<Schema> {
        Some(Schema {
            about: "how much the nodes look like a drawing a peer has placed in the world: the overlap (IoU) and the stations where their extents differ most, in world units",
            inputs: (0, 0),
            params: vec![
                Param::optional("nodes", Kind::Texts, Value::List(Vec::new()), "the nodes compared together, as name or name@version; every node when left out"),
                Param::required("against", Kind::Text, "the drawing, as peer:node, such as vis:top"),
                Param::optional("method", Kind::Text, "raster", "the likeness plugin"),
                Param::optional("report", Kind::Text, "legacy", "legacy reply or stations: versioned evidence across local u/v, with per-station matches when tolerance is given"),
                Param::alternative("frame", Kind::Numbers, "with report=stations: ox,oy,ux,uy,vx,vy; orthonormal local basis in the drawing's world-axis plane; identity by default"),
                Param::alternative("tolerance", Kind::Number, "with report=stations: nonnegative endpoint threshold in world units; raster needs at least the effective cell; omitted means no verdict"),
                Param::optional("precision", Kind::Text, "raster", "raster or contour; contour requires an explicit peer evidence source; raster contours retain the pixel floor, vector subpixel tolerance requires declared uncertainty"),
                Param::optional("stairs", Kind::Text, "none", "none or pixel-diagonal: explicitly add one effective pixel diagonal to each raster station threshold"),
            ],
        })
    }

    fn call(&self, cx: &Context, request: &QueryRequest) -> Attempt<Answer> {
        answered(cx, request).into()
    }
}

fn answered(cx: &Context, request: &QueryRequest) -> Result<Answer, Error> {
    let options = report::options(&request.params)?;
    let against = request.params.text("against")?;
    let (peer, node) =
        against.split_once(':').filter(|(peer, node)| !peer.is_empty() && !node.is_empty()).ok_or_else(
            || Error::input(format!("`against` names a peer's node as peer:node, got `{against}`")),
        )?;
    let reply = ask(cx, peer, "polygons", Params::new().with("node", node)).into_result()?;
    let drawing = drawing_of(&reply.data)?;
    let built = crate::built(request)?;
    let shapes = built.iter().map(|(_, built)| Arc::clone(&built.shape)).collect();
    if let Some(options) = options {
        use krly3d_kernel::model::NodeRef;
        use krly3d_kernel::ports::likeness::report::{station_likeness, DrawingEvidence, StationRequest};
        let evidence = DrawingEvidence::from_value(&reply.data)?;
        let method = request.params.text("method")?;
        let found = station_likeness(
            cx,
            method,
            &StationRequest {
                comparison: LikenessRequest { shapes, drawing: drawing.clone() },
                options,
                evidence,
            },
        )
        .into_result()?;
        let nodes = built
            .iter()
            .map(|(name, _)| {
                let mut node = NodeRef::parse(name)?;
                if node.version.is_none() {
                    node.version = request
                        .model
                        .nodes()
                        .find(|(name, _)| *name == node.name)
                        .map(|(_, count)| count as u32);
                }
                Ok(node.to_string())
            })
            .collect::<Result<_, Error>>()?;
        return Ok(report::answer(&found, against, method, nodes, &drawing, reply.data, reply.exact));
    }
    let found =
        likeness(cx, request.params.text("method")?, &LikenessRequest { shapes, drawing: drawing.clone() })
            .into_result()?;
    let mut text = format!(
        "like {against}: IoU {}; area model {}, drawing {}\n",
        num(found.iou),
        num(found.model_area),
        num(found.drawing_area)
    );
    for station in &found.worst {
        text.push_str(&station.text(drawing.axes));
        text.push('\n');
    }
    let station_value = |station: &Station| {
        let extent = |found: Option<[f64; 2]>| {
            found.map_or(Value::List(Vec::new()), |[low, high]| Value::List(vec![low.into(), high.into()]))
        };
        Value::map([
            ("along", Value::from(["x", "y", "z"][drawing.axes[station.along]])),
            ("at", Value::from(station.at)),
            ("model", extent(station.model)),
            ("drawing", extent(station.drawing)),
            ("off", Value::from(station.off())),
        ])
    };
    Ok(Answer {
        text,
        data: Value::map([
            ("iou", Value::from(found.iou)),
            ("model_area", Value::from(found.model_area)),
            ("drawing_area", Value::from(found.drawing_area)),
            ("worst", Value::List(found.worst.iter().map(station_value).collect())),
        ]),
        // Both silhouettes are sampled at the centres of the drawing's pixels.
        exact: Exactness::Sampled {
            samples: ((found.model_area + found.drawing_area) / (drawing.unit * drawing.unit)) as u32,
        },
    })
}

/// A grid can hold this many pixels at most; a drawing that would need more is compared
/// on a coarser grid.
const MOST_CELLS: f64 = 4.0e6;
/// How many of the worst stations are reported along each axis, and how far apart two of
/// them must be, as a share of the grid's length along that axis, so that one bad stretch
/// is not reported many times over.
const REPORTED: usize = 4;
const APART: f64 = 0.05;

struct Grid {
    low: [f64; 2],
    cell: f64,
    size: [usize; 2],
}

impl Grid {
    fn centre(&self, column: usize, row: usize) -> [f64; 2] {
        [self.low[0] + (column as f64 + 0.5) * self.cell, self.low[1] + (row as f64 + 0.5) * self.cell]
    }
}

fn compared(cx: &Context, request: &LikenessRequest) -> Result<Resemblance, Error> {
    request.drawing.validate()?;
    let triangles = projected(cx, request)?;
    let grid = grid(&request.drawing, &triangles)?;
    let model = filled_triangles(&grid, &triangles);
    let drawn = filled_polygons(&grid, &request.drawing.polygons);
    Ok(resemblance(&grid, &model, &drawn))
}

fn projected(cx: &Context, request: &LikenessRequest) -> Result<Vec<[[f64; 2]; 3]>, Error> {
    let drawing = &request.drawing;
    let mut triangles = Vec::new();
    for shape in &request.shapes {
        cx.check_control()?;
        let mesh = mesh_of(cx, shape, drawing.unit).into_result()?;
        let flat = |index: usize| [mesh.points[index][drawing.axes[0]], mesh.points[index][drawing.axes[1]]];
        triangles.extend(mesh.triangles.iter().map(|corners| corners.map(flat)));
    }
    Ok(triangles)
}

fn resemblance(grid: &Grid, model: &[bool], drawn: &[bool]) -> Resemblance {
    let (mut both, mut either, mut model_cells, mut drawn_cells) = (0_usize, 0_usize, 0_usize, 0_usize);
    for (m, d) in model.iter().zip(drawn) {
        both += usize::from(*m && *d);
        either += usize::from(*m || *d);
        model_cells += usize::from(*m);
        drawn_cells += usize::from(*d);
    }
    let area = grid.cell * grid.cell;
    let iou = if either == 0 { 1.0 } else { both as f64 / either as f64 };
    Resemblance {
        iou,
        model_area: model_cells as f64 * area,
        drawing_area: drawn_cells as f64 * area,
        worst: worst(grid, model, drawn),
    }
}

/// A grid of the drawing's pixels over both silhouettes, a pixel's width around them.
fn grid(drawing: &Drawing, triangles: &[[[f64; 2]; 3]]) -> Result<Grid, Error> {
    let points = triangles.iter().flatten().chain(drawing.polygons.iter().flatten());
    let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for point in points {
        for k in 0..2 {
            if !point[k].is_finite() {
                return Err(Error::geometry("likeness projection contains nonfinite coordinates"));
            }
            low[k] = low[k].min(point[k]);
            high[k] = high[k].max(point[k]);
        }
    }
    if !(low[0] <= high[0] && low[1] <= high[1]) {
        return Err(Error::geometry("neither the model nor the drawing has anything to compare"));
    }
    let span = [high[0] - low[0], high[1] - low[1]];
    let mut cell = (span[0] / MOST_CELLS.sqrt()) * (span[1] / MOST_CELLS.sqrt());
    // Keep the legacy cell for ordinary drawings. Count padded cells too, including
    // extremely thin drawings where an area-only budget would allocate without bound.
    cell = drawing.unit.max(cell.sqrt());
    let dimensions = |cell: f64| [(span[0] / cell).ceil() + 2.0, (span[1] / cell).ceil() + 2.0];
    let mut dims = dimensions(cell);
    for _ in 0..64 {
        if dims[0] * dims[1] <= MOST_CELLS {
            break;
        }
        cell *= (dims[0] * dims[1] / MOST_CELLS).sqrt().max(1.01);
        dims = dimensions(cell);
    }
    let low = [low[0] - cell, low[1] - cell];
    if !cell.is_finite()
        || cell <= 0.0
        || !(cell * cell).is_finite()
        || cell * cell == 0.0
        || dims.iter().any(|d| !d.is_finite() || *d < 2.0)
        || dims[0] * dims[1] > MOST_CELLS
        || low.iter().any(|n| !n.is_finite() || *n + cell == *n)
    {
        return Err(Error::geometry(
            "likeness grid cannot represent this coordinate range at a finite resolution",
        ));
    }
    let size = [dims[0] as usize, dims[1] as usize];
    Ok(Grid { low, cell, size })
}

/// The pixels whose centres lie in any of the triangles.
fn filled_triangles(grid: &Grid, triangles: &[[[f64; 2]; 3]]) -> Vec<bool> {
    let mut cells = vec![false; grid.size[0] * grid.size[1]];
    let index = |value: f64, k: usize| (value - grid.low[k]) / grid.cell - 0.5;
    for [a, b, c] in triangles {
        let doubled = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if doubled.abs() < f64::EPSILON {
            continue;
        }
        let span = |values: [f64; 3], k: usize| {
            let first = index(values[0].min(values[1]).min(values[2]), k).ceil().max(0.0);
            let last =
                index(values[0].max(values[1]).max(values[2]), k).floor().min(grid.size[k] as f64 - 1.0);
            (last >= first).then_some((first as usize, last as usize))
        };
        let (Some(columns), Some((row_low, row_high))) =
            (span([a[0], b[0], c[0]], 0), span([a[1], b[1], c[1]], 1))
        else {
            continue;
        };
        for row in row_low..=row_high {
            for column in columns.0..=columns.1 {
                let p = grid.centre(column, row);
                let side = |u: &[f64; 2], v: &[f64; 2]| {
                    (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
                };
                let (s0, s1, s2) = (side(a, b), side(b, c), side(c, a));
                if (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0) {
                    cells[row * grid.size[0] + column] = true;
                }
            }
        }
    }
    cells
}

/// The pixels whose centres lie inside the polygons, by the even-odd rule along each row.
fn filled_polygons(grid: &Grid, polygons: &[Vec<[f64; 2]>]) -> Vec<bool> {
    let mut cells = vec![false; grid.size[0] * grid.size[1]];
    for row in 0..grid.size[1] {
        let y = grid.centre(0, row)[1];
        let mut crossings: Vec<f64> = Vec::new();
        for polygon in polygons {
            for i in 0..polygon.len() {
                let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                if (a[1] <= y) != (b[1] <= y) {
                    crossings.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            let first = ((pair[0] - grid.low[0]) / grid.cell - 0.5).ceil().max(0.0) as usize;
            let last = ((pair[1] - grid.low[0]) / grid.cell - 0.5).floor();
            if last < 0.0 {
                continue;
            }
            for column in first..=(last as usize).min(grid.size[0] - 1) {
                cells[row * grid.size[0] + column] = true;
            }
        }
    }
    cells
}

/// The rows and columns where the extents of the two silhouettes differ most.
fn worst(grid: &Grid, model: &[bool], drawn: &[bool]) -> Vec<Station> {
    let mut stations = Vec::new();
    for along in 0..2 {
        let (lines, across) = (grid.size[along], grid.size[1 - along]);
        for line in 0..lines {
            let cell = |k: usize| if along == 0 { k * grid.size[0] + line } else { line * grid.size[0] + k };
            let extent = |mask: &[bool]| {
                let first = (0..across).find(|k| mask[cell(*k)])?;
                let last = (0..across).rfind(|k| mask[cell(*k)])?;
                let edge = |k: usize, end: f64| grid.low[1 - along] + (k as f64 + end) * grid.cell;
                Some([edge(first, 0.0), edge(last, 1.0)])
            };
            let at = grid.low[along] + (line as f64 + 0.5) * grid.cell;
            stations.push((line, Station { along, at, model: extent(model), drawing: extent(drawn) }));
        }
    }
    stations.sort_by(|p, q| q.1.off().total_cmp(&p.1.off()));
    let mut chosen: Vec<(usize, Station)> = Vec::new();
    for (line, station) in stations {
        let apart = (APART * grid.size[station.along] as f64).ceil() as usize;
        let taken = chosen.iter().filter(|(_, known)| known.along == station.along).count();
        if station.off() <= grid.cell || taken == REPORTED {
            continue;
        }
        if chosen.iter().all(|(other, known)| known.along != station.along || other.abs_diff(line) > apart) {
            chosen.push((line, station));
        }
    }
    chosen.sort_by(|p, q| q.1.off().total_cmp(&p.1.off()));
    chosen.into_iter().map(|(_, station)| station).collect()
}

#[cfg(test)]
mod tests {
    use super::Raster;
    use krly3d_kernel::ports::likeness::{contract, Likeness};
    use krly3d_kernel::schema::Params;
    use krly3d_std::testing::made;

    #[test]
    fn satisfies_the_likeness_contract() {
        let mut builder = krly3d_std::standard();
        builder.plugin::<Likeness>(Raster);
        let cx = builder.context();
        let block = made(
            &cx,
            "box",
            Params::new().with("w", 40.0).with("d", 20.0).with("h", 10.0).with("at", [20.0, 10.0, 0.0]),
            &[],
        );

        contract::check(&cx, "raster", &block).unwrap();
    }

    /// A peer that holds one drawing, the top view of a 40 by 20 box, and answers
    /// `polygons` with it as krlyvis does for a placed outline.
    struct Drawer;

    impl krly3d_kernel::Plugin<krly3d_kernel::ports::peer::Peer> for Drawer {
        fn id(&self) -> krly3d_core::Id {
            krly3d_core::Id::new("vis")
        }

        fn call(
            &self,
            _: &krly3d_kernel::Context,
            request: &krly3d_kernel::ports::peer::PeerCall,
        ) -> krly3d_core::Attempt<krly3d_kernel::Reply> {
            use krly3d_kernel::schema::Value;
            let point = |x: f64, y: f64| Value::List(vec![x.into(), y.into()]);
            let data = Value::map([
                ("axes", Value::List(vec!["x".into(), "y".into()])),
                ("unit", Value::from(0.1)),
                (
                    "polygons",
                    Value::List(vec![Value::List(vec![
                        point(0.0, 0.0),
                        point(40.0, 0.0),
                        point(40.0, 20.0),
                        point(0.0, 20.0),
                    ])]),
                ),
            ]);
            let ok = request.method == "polygons";
            krly3d_core::Attempt::Done(krly3d_kernel::Reply {
                ok,
                text: String::new(),
                data,
                exact: krly3d_core::Exactness::Exact,
            })
        }
    }

    #[test]
    fn the_query_compares_the_nodes_with_the_drawing_a_peer_holds() {
        let mut builder = krly3d_std::standard();
        crate::register(&mut builder);
        builder.plugin::<krly3d_kernel::ports::peer::Peer>(Drawer);
        let mut session = builder.session();
        let made = session.call(
            "box",
            &Params::new()
                .with("id", "block")
                .with("w", 40.0)
                .with("d", 20.0)
                .with("h", 10.0)
                .with("at", [20.0, 15.0, 0.0]),
        );

        let answer = session.call("likeness", &Params::new().with("against", "vis:top"));

        assert!(made.ok && answer.ok, "{} {}", made.text, answer.text);
        assert!(answer.text.starts_with("like vis:top: IoU 0.6"), "{}", answer.text);
        assert!(answer.text.contains("model y 5..25, drawing y 0..20 (off 5)"), "{}", answer.text);
    }

    #[test]
    fn an_edit_that_does_not_look_like_the_drawing_is_refused_with_where_it_differs() {
        let mut builder = krly3d_std::standard();
        crate::register(&mut builder);
        builder.plugin::<krly3d_kernel::ports::peer::Peer>(Drawer);
        let mut session = builder.session();
        let block = |y: f64| {
            Params::new()
                .with("id", "block")
                .with("w", 40.0)
                .with("d", 20.0)
                .with("h", 10.0)
                .with("at", [20.0, y, 0.0])
                .with("expect", "iou:vis:top>=0.95")
        };

        let shifted = session.call("box", &block(15.0));
        let placed = session.call("box", &block(10.0));

        assert!(!shifted.ok && shifted.text.contains("FAILED, found ?IoU 0.6"), "{}", shifted.text);
        assert!(placed.ok, "{}", placed.text);
    }
}
