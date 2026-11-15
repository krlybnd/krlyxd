//! Native field-to-mesh conversion, independent of all B-rep engines.
//! Sampling is approximate: sub-cell features can disappear. Boundary sampling
//! rejects clipping but is not a continuous containment or printability proof.
mod field;
mod marching;

use krly3d_field::{Error, Grid, Recipe};
use krly3d_mesh::MeshData;
use krlyxd::CallControl;

#[derive(Clone, Copy, Debug)]
pub enum Progress {
    Sampling { grid_nodes: usize },
    Mesh { grid_nodes: usize },
}

pub fn mesh(recipe: &Recipe) -> Result<MeshData, Error> {
    mesh_controlled(recipe, &CallControl::new(), &mut |_| Ok(()))
}

/// Caller-owned cancellation/deadline checks occur at least every 1024 grid
/// samples/cells. A failed progress callback aborts work and returns its error.
pub fn mesh_controlled(
    recipe: &Recipe,
    control: &CallControl,
    progress: &mut dyn FnMut(Progress) -> Result<(), Error>,
) -> Result<MeshData, Error> {
    check(control)?;
    let grid = recipe.validate()?;
    let field = field::Field::new(recipe);
    progress(Progress::Sampling { grid_nodes: grid.nodes })?;
    let values = sample(recipe, grid, &field, control)?;
    check(control)?;
    progress(Progress::Mesh { grid_nodes: grid.nodes })?;
    let mesh = marching::extract(recipe, grid, &values, control)?;
    check(control)?;
    if mesh.triangles.is_empty() {
        return Err(Error::new("empty", "no surface resolved; check recipe, bounds and cell_size"));
    }
    Ok(mesh)
}

fn sample(
    recipe: &Recipe,
    grid: Grid,
    field: &field::Field<'_>,
    control: &CallControl,
) -> Result<Vec<f64>, Error> {
    let mut values = Vec::with_capacity(grid.nodes);
    let [nx, ny, nz] = grid.dimensions;
    let mut scratch = [0.0; krly3d_field::MAX_NODES];
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                if values.len() % 1024 == 0 {
                    check(control)?;
                }
                let value = field.evaluate(position(recipe, grid, [x, y, z]), &mut scratch);
                if !value.is_finite() {
                    return Err(Error::new("numeric", "field evaluation was not finite"));
                }
                if (x == 0 || y == 0 || z == 0 || x + 1 == nx || y + 1 == ny || z + 1 == nz) && value <= 0.0 {
                    return Err(Error::new(
                        "boundary",
                        "field must be strictly positive at every boundary sample; enlarge bounds",
                    ));
                }
                values.push(value);
            }
        }
    }
    Ok(values)
}

fn position(recipe: &Recipe, grid: Grid, xyz: [usize; 3]) -> [f64; 3] {
    std::array::from_fn(|i| {
        if xyz[i] + 1 == grid.dimensions[i] {
            recipe.bounds.max[i]
        } else {
            recipe.bounds.min[i] + xyz[i] as f64 * grid.spacing[i]
        }
    })
}
fn check(control: &CallControl) -> Result<(), Error> {
    control.check().map_err(|e| Error::new("stopped", e.to_string()))
}
