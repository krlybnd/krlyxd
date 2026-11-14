//! What every likeness plugin must satisfy: a box compared with its own outline is alike
//! to within the drawing's resolution, and a box drawn half as long is found half alike,
//! with a station where they differ by the half that is missing.

use super::{likeness, Drawing, LikenessRequest};
use crate::context::Context;
use crate::ports::shape::Leaf;
use crate::topo::Body;
use std::sync::Arc;

/// `body` must be a box from (0, 0, 0) to (40, 20, 10); it is compared in its top view.
pub fn check(cx: &Context, method: &str, body: &Arc<Body>) -> Result<(), String> {
    let drawing = |width: f64| Drawing {
        axes: [0, 1],
        unit: 0.1,
        polygons: vec![vec![[0.0, 0.0], [width, 0.0], [width, 20.0], [0.0, 20.0]]],
    };
    let ask = |drawing: Drawing| {
        likeness(cx, method, &LikenessRequest { shapes: vec![Leaf::of(Arc::clone(body))], drawing })
            .into_result()
            .map_err(|error| error.to_string())
    };
    let same = ask(drawing(40.0))?;
    if same.iou < 0.98 {
        return Err(format!("`{method}` finds a box only {} like its own outline", same.iou));
    }
    let half = ask(drawing(20.0))?;
    if (half.iou - 0.5).abs() > 0.02 {
        return Err(format!("`{method}` finds a box {} like an outline half its size", half.iou));
    }
    let worst = half.worst.first().ok_or(format!("`{method}` names no station where they differ"))?;
    if (worst.off() - 20.0).abs() > 0.2 {
        return Err(format!(
            "`{method}` gives {worst:?} as the worst station of a box twice as long as drawn"
        ));
    }
    Ok(())
}
