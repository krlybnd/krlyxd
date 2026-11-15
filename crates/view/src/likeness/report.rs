//! Station reporting over the same projection/fill implementation as legacy likeness.
use super::{filled_polygons, filled_triangles, grid, projected, resemblance, Grid, Raster, APART, REPORTED};
use krly3d_core::{Attempt, Error, Exactness, Id};
use krly3d_kernel::ports::likeness::report::{
    DrawingFrame, StairPolicy, StationDifference, StationLikeness, StationOptions, StationPrecision,
    StationReport, StationRequest,
};
use krly3d_kernel::ports::likeness::{Drawing, Station};
use krly3d_kernel::ports::query::Answer;
use krly3d_kernel::schema::{Params, Value};
use krly3d_kernel::{Context, Plugin};

impl Plugin<StationLikeness> for Raster {
    fn id(&self) -> Id {
        Id::new("raster")
    }

    fn call(&self, cx: &Context, request: &StationRequest) -> Attempt<StationReport> {
        compared(cx, request).into()
    }
}

pub(super) fn options(params: &Params) -> Result<Option<StationOptions>, Error> {
    let detailed = match params.text("report")? {
        "legacy" => false,
        "stations" => true,
        _ => return Err(Error::input("likeness report must be legacy or stations")),
    };
    let precision = match params.text("precision")? {
        "raster" => StationPrecision::Raster,
        "contour" => StationPrecision::Contour,
        _ => return Err(Error::input("likeness precision must be raster or contour")),
    };
    let stairs = match params.text("stairs")? {
        "none" => StairPolicy::None,
        "pixel-diagonal" => StairPolicy::PixelDiagonal,
        _ => return Err(Error::input("likeness stairs must be none or pixel-diagonal")),
    };
    let tolerance = params.get("tolerance").map(|_| params.number("tolerance")).transpose()?;
    let frame = if params.get("frame").is_some() {
        let numbers = params.numbers("frame")?;
        let [ox, oy, ux, uy, vx, vy] = numbers[..] else {
            return Err(Error::input(
                "likeness frame is six numbers: ox,oy,ux,uy,vx,vy in the drawing's world-axis plane",
            ));
        };
        DrawingFrame { origin: [ox, oy], u: [ux, uy], v: [vx, vy] }
    } else {
        DrawingFrame::default()
    };
    if !detailed {
        if tolerance.is_some()
            || params.get("frame").is_some()
            || precision != StationPrecision::Raster
            || stairs != StairPolicy::None
        {
            return Err(Error::input(
                "likeness frame, tolerance, stairs and precision options require report=stations",
            ));
        }
        return Ok(None);
    }
    Ok(Some(StationOptions { frame, precision, tolerance, stairs }))
}

fn compared(cx: &Context, request: &StationRequest) -> Result<StationReport, Error> {
    let StationRequest { comparison, options, evidence } = request;
    options.validate(&comparison.drawing, evidence)?;
    cx.check_control()?;
    let triangles = projected(cx, comparison)?;
    let triangles: Vec<_> = triangles.iter().map(|t| t.map(|p| options.frame.local(p))).collect();
    let drawing = Drawing {
        axes: [0, 1],
        unit: comparison.drawing.unit,
        polygons: comparison
            .drawing
            .polygons
            .iter()
            .map(|p| p.iter().map(|p| options.frame.local(*p)).collect())
            .collect(),
    };
    let grid = grid(&drawing, &triangles)?;
    if (options.precision == StationPrecision::Raster || !evidence.vector)
        && options.tolerance.is_some_and(|t| t < grid.cell)
    {
        return Err(Error::input(format!(
            "likeness tolerance is below the effective raster cell {}; raster evidence cannot establish subpixel precision",
            grid.cell,
        )));
    }
    let allowance = match options.stairs {
        StairPolicy::None => 0.0,
        StairPolicy::PixelDiagonal => std::f64::consts::SQRT_2 * grid.cell,
    };
    let tolerance = options.tolerance.map(|t| t + allowance);
    if tolerance.is_some_and(|t| !t.is_finite()) {
        return Err(Error::input("likeness tolerance plus stair allowance must be finite"));
    }
    let model = filled_triangles(&grid, &triangles);
    cx.check_control()?;
    let drawn = filled_polygons(&grid, &drawing.polygons);
    let resemblance = resemblance(&grid, &model, &drawn);
    let mut stations = Vec::new();
    let (mut compared, mut failed) = ([0; 2], [0; 2]);
    // Edge-on projected triangles have no silhouette area and cannot establish a
    // contour extent, just as they cannot fill raster cells.
    let area_triangles: Vec<_> = triangles
        .iter()
        .filter(|[a, b, c]| {
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() >= f64::EPSILON
        })
        .map(|t| t.as_slice())
        .collect();
    let polygons: Vec<_> = drawing.polygons.iter().map(Vec::as_slice).collect();
    for along in 0..2 {
        for line in 0..grid.size[along] {
            cx.check_control()?;
            let at = grid.low[along] + (line as f64 + 0.5) * grid.cell;
            let (m, d) = match options.precision {
                StationPrecision::Raster => {
                    (raster_extent(&grid, &model, along, line), raster_extent(&grid, &drawn, along, line))
                }
                StationPrecision::Contour => (
                    contour_extent(&area_triangles, along, at, false),
                    contour_extent(&polygons, along, at, true),
                ),
            };
            if m.is_none() && d.is_none() {
                continue;
            }
            let station = Station { along, at, model: m, drawing: d };
            let delta = m.zip(d).map(|(m, d)| [m[0] - d[0], m[1] - d[1]]);
            let matches = tolerance.map(|t| delta.is_some() && station.off() <= t);
            compared[along] += 1;
            failed[along] += usize::from(matches == Some(false));
            stations.push(StationDifference { index: line, station, delta, tolerance, matches });
        }
    }
    if !stations.iter().any(|s| s.station.drawing.is_some()) {
        return Err(Error::geometry(
            "no drawing extent intersects the sampled stations; supply a finer drawing resolution",
        ));
    }
    let mut ranked: Vec<_> = stations.iter().collect();
    ranked.sort_by(|a, b| b.station.off().total_cmp(&a.station.off()));
    let mut worst: Vec<StationDifference> = Vec::new();
    for station in ranked {
        let along = station.station.along;
        let apart = (APART * grid.size[along] as f64).ceil() as usize;
        if worst.iter().filter(|s| s.station.along == along).count() < REPORTED
            && worst.iter().all(|s| s.station.along != along || s.index.abs_diff(station.index) > apart)
        {
            worst.push(station.clone());
        }
    }
    let samples = (grid.size[0] * grid.size[1]) as u32;
    let report = StationReport {
        resemblance,
        stations,
        worst,
        compared,
        failed,
        matches: tolerance.map(|_| failed == [0, 0]),
        grid_low: grid.low,
        cell: grid.cell,
        size: grid.size,
        options: options.clone(),
        evidence: evidence.clone(),
        samples,
    };
    cx.publish_control(|| report)
}

fn raster_extent(grid: &Grid, mask: &[bool], along: usize, line: usize) -> Option<[f64; 2]> {
    let cell = |k: usize| if along == 0 { k * grid.size[0] + line } else { line * grid.size[0] + k };
    let across = grid.size[1 - along];
    let first = (0..across).find(|k| mask[cell(*k)])?;
    let last = (0..across).rfind(|k| mask[cell(*k)])?;
    Some([
        grid.low[1 - along] + first as f64 * grid.cell,
        grid.low[1 - along] + (last + 1) as f64 * grid.cell,
    ])
}

fn contour_extent(polygons: &[&[[f64; 2]]], along: usize, at: f64, even_odd: bool) -> Option<[f64; 2]> {
    let mut crossings = Vec::new();
    for polygon in polygons {
        for i in 0..polygon.len() {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            if (a[along] <= at) != (b[along] <= at) {
                let c = 1 - along;
                crossings.push(a[c] + (at - a[along]) / (b[along] - a[along]) * (b[c] - a[c]));
            }
        }
    }
    crossings.sort_by(f64::total_cmp);
    // Projected triangles form a union; drawing polygons follow the same even-odd
    // fill as IoU. In particular, coincident/cancelling contours establish no extent.
    if !even_odd {
        return crossings.first().zip(crossings.last()).map(|(a, b)| [*a, *b]);
    }
    let mut extent: Option<[f64; 2]> = None;
    for &[low, high] in crossings.as_chunks::<2>().0 {
        if low < high {
            extent = Some(extent.map_or([low, high], |[first, _]| [first, high]));
        }
    }
    extent
}

pub(super) fn answer(
    found: &StationReport,
    against: &str,
    method: &str,
    nodes: Vec<String>,
    drawing: &Drawing,
    peer_data: Value,
    peer_exact: Exactness,
) -> Answer {
    let extent = |e: Option<[f64; 2]>| e.map_or(Value::List(vec![]), pair);
    let station = |s: &StationDifference| {
        Value::map([
            ("id", Value::from(format!("{}:{}", ["u", "v"][s.station.along], s.index))),
            ("along", Value::from(["u", "v"][s.station.along])),
            ("index", Value::from(s.index)),
            ("at", Value::from(s.station.at)),
            ("model", extent(s.station.model)),
            ("drawing", extent(s.station.drawing)),
            ("delta", extent(s.delta)),
            ("off", Value::from(s.station.off())),
            ("tolerance", optional(s.tolerance)),
            ("matches", verdict(s.matches)),
        ])
    };
    let frame = found.options.frame;
    let mut text = format!(
        "like {against}: IoU {}; stations in local u/v; cell {}\n",
        krly3d_core::text::num(found.resemblance.iou),
        found.cell
    );
    for s in &found.worst {
        let threshold = s.tolerance.map_or_else(|| "not requested".into(), krly3d_core::text::num);
        let matched = match s.matches {
            Some(true) => "yes",
            Some(false) => "no",
            None => "not requested",
        };
        text.push_str(&format!(
            "{}:{} at {}: off {}, tolerance {}, matches {}\n",
            ["u", "v"][s.station.along],
            s.index,
            krly3d_core::text::num(s.station.at),
            krly3d_core::text::num(s.station.off()),
            threshold,
            matched
        ));
    }
    text.push_str(
        "Sampled endpoint evidence; model mesh achieved error and continuous agreement are unknown.\n",
    );
    let data = Value::map([
        ("report_version", "krly3d.likeness.stations/1".into()),
        ("iou", found.resemblance.iou.into()), ("model_area", found.resemblance.model_area.into()),
        ("drawing_area", found.resemblance.drawing_area.into()),
        ("matches", verdict(found.matches)),
        ("compared", Value::List(found.compared.map(Value::from).to_vec())),
        ("failed", Value::List(found.failed.map(Value::from).to_vec())),
        ("stations", Value::List(found.stations.iter().map(station).collect())),
        ("worst", Value::List(found.worst.iter().map(station).collect())),
        ("frame", Value::map([("origin", pair(frame.origin)), ("u", pair(frame.u)), ("v", pair(frame.v)),
            ("world_axes", Value::List(drawing.axes.map(|a| Value::from(["x","y","z"][a])).to_vec()))])),
        ("grid", Value::map([("low", pair(found.grid_low)), ("cell", found.cell.into()),
            ("size", Value::List(found.size.map(Value::from).to_vec())), ("samples", (found.samples as usize).into())])),
        ("policy", Value::map([
            ("precision", match found.options.precision { StationPrecision::Raster => "raster", StationPrecision::Contour => "contour" }.into()),
            ("tolerance", optional(found.options.tolerance)),
            ("stairs", match found.options.stairs { StairPolicy::None => "none", StairPolicy::PixelDiagonal => "pixel-diagonal" }.into()),
            ("stair_allowance", match found.options.stairs { StairPolicy::None => 0.0, StairPolicy::PixelDiagonal => std::f64::consts::SQRT_2 * found.cell }.into()),
            ("raster_precision_floor", if found.options.precision == StationPrecision::Raster || !found.evidence.vector { found.cell.into() } else { "not-applicable".into() }),
            ("coordinate_method", if found.options.precision == StationPrecision::Raster { "pixel-cell-edges" } else { "linear-contour-intersections" }.into()),
            ("subpixel_accuracy", "not certified; raster evidence retains effective pixel floor; vector uncertainty is producer-declared".into()),
            ("scope", "sampled outer extents only; missing side always fails; holes and continuous agreement not tested".into()),
            ("stair_limit", "chosen allowance, not a bound on shallow-edge scanline error".into()),
        ])),
        ("provenance", Value::map([
            ("against", against.into()), ("nodes", Value::List(nodes.into_iter().map(Value::from).collect())),
            ("method", method.into()), ("drawing_unit", drawing.unit.into()),
            ("drawing_evidence", if found.evidence.vector {"vector"} else {"raster"}.into()),
            ("drawing_uncertainty", optional(found.evidence.uncertainty)),
            ("drawing_source", found.evidence.source.clone().map_or("unknown".into(), Value::from)),
            ("peer_exactness", peer_exact.symbol().to_string().into()),
            ("reference", peer_data),
            ("mesh_tolerance_requested", drawing.unit.into()), ("mesh_achieved_error", "unknown".into()),
            ("accuracy", "sampled nominal differences; interpolation is not an accuracy guarantee".into()),
        ])),
    ]);
    Answer { text, data, exact: Exactness::Sampled { samples: found.samples } }
}

fn pair(p: [f64; 2]) -> Value {
    Value::List(p.map(Value::from).to_vec())
}
fn optional(n: Option<f64>) -> Value {
    n.map_or("unknown".into(), Value::from)
}
fn verdict(v: Option<bool>) -> Value {
    v.map_or("not-requested".into(), Value::from)
}

#[cfg(test)]
mod tests {
    use super::{contour_extent, grid, Drawing};

    #[test]
    fn drawing_contours_obey_even_odd_fill_while_mesh_triangles_form_a_union() {
        let outer = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]];
        let cut = [[0., 0.], [2., 0.], [2., 4.], [0., 4.]];
        assert_eq!(contour_extent(&[&outer, &cut], 1, 2., true), Some([2., 4.]));
        assert_eq!(contour_extent(&[&outer, &outer], 1, 2., true), None);
        assert_eq!(contour_extent(&[&outer, &cut], 1, 2., false), Some([0., 4.]));
    }

    #[test]
    fn thin_grid_is_budgeted_using_padded_dimensions_and_reports_coarsening() {
        let drawing = Drawing {
            axes: [0, 1],
            unit: 1.,
            polygons: vec![vec![[0., 0.], [10_000_000., 0.], [10_000_000., 1.], [0., 1.]]],
        };
        let grid = grid(&drawing, &[]).unwrap();
        assert!(grid.size[0] * grid.size[1] <= super::super::MOST_CELLS as usize);
        assert!(grid.cell > drawing.unit);
    }
}
