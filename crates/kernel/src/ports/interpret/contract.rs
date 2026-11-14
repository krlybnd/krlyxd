//! What every mesh reading must satisfy: a closed shape reads as a closed mesh whose
//! volume agrees with the shape's to the share the tolerance allows.

use super::{mesh_of, Context};
use crate::ports::shape::Shape;
use std::sync::Arc;

/// `volume` is the shape's volume measured some other way; `area` its surface area. A mesh
/// within `tolerance` of the surface misses at most about `area · tolerance` of the volume.
pub fn check_mesh(
    cx: &Context,
    shape: &Arc<dyn Shape>,
    tolerance: f64,
    volume: f64,
    area: f64,
) -> Result<(), String> {
    let mesh = mesh_of(cx, shape, tolerance).into_result().map_err(|error| error.to_string())?;
    let open = mesh.open_sides();
    if !open.is_empty() {
        return Err(format!(
            "the mesh has {} open sides, such as {:?}",
            open.len(),
            &open[..open.len().min(4)]
        ));
    }
    let off = (mesh.volume() - volume).abs();
    if off > area * tolerance {
        return Err(format!("the mesh holds {} where the shape holds {volume}: off by {off}", mesh.volume()));
    }
    Ok(())
}
