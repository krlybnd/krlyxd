use crate::field::{cross, dot, sub};
use krly3d_field::{Error, Grid, Recipe, MAX_TRIANGLES};
use krly3d_mesh::MeshData;
use krlyxd::CallControl;
use std::collections::HashMap;

// Freudenthal split: all cells use the same body and shared-face diagonals.
const TETS: [[usize; 4]; 6] =
    [[0, 1, 3, 7], [0, 3, 2, 7], [0, 2, 6, 7], [0, 6, 4, 7], [0, 4, 5, 7], [0, 5, 1, 7]];
#[derive(Clone, Copy)]
struct Vertex {
    id: usize,
    p: [f64; 3],
    value: f64,
}
struct Builder {
    mesh: MeshData,
    edges: HashMap<(usize, usize), u32>,
}

pub(super) fn extract(
    recipe: &Recipe,
    grid: Grid,
    values: &[f64],
    control: &CallControl,
) -> Result<MeshData, Error> {
    let [nx, ny, nz] = grid.dimensions;
    let mut builder =
        Builder { mesh: MeshData { points: Vec::new(), triangles: Vec::new() }, edges: HashMap::new() };
    let mut cells = 0;
    for z in 0..nz - 1 {
        for y in 0..ny - 1 {
            for x in 0..nx - 1 {
                if cells % 1024 == 0 {
                    crate::check(control)?;
                }
                cells += 1;
                let cube: [Vertex; 8] = std::array::from_fn(|i| {
                    let xyz = [x + (i & 1), y + ((i >> 1) & 1), z + ((i >> 2) & 1)];
                    let id = xyz[0] + nx * (xyz[1] + ny * xyz[2]);
                    Vertex { id, p: crate::position(recipe, grid, xyz), value: values[id] }
                });
                if cube.iter().all(|v| v.value <= 0.0) || cube.iter().all(|v| v.value > 0.0) {
                    continue;
                }
                for tet in TETS {
                    builder.tetrahedron(tet.map(|i| cube[i]))?;
                }
            }
        }
    }
    Ok(builder.mesh)
}
impl Builder {
    fn tetrahedron(&mut self, vertices: [Vertex; 4]) -> Result<(), Error> {
        let mut inside = [0; 4];
        let mut outside = [0; 4];
        let (mut ni, mut no) = (0, 0);
        for (i, v) in vertices.iter().enumerate() {
            // The solid includes its zero set. In particular, a face shared by
            // touching union operands is interior, not a pair of exposed faces.
            if v.value <= 0.0 {
                inside[ni] = i;
                ni += 1;
            } else {
                outside[no] = i;
                no += 1;
            }
        }
        if ni == 0 || no == 0 {
            return Ok(());
        }
        let outward = sub(vertices[outside[0]].p, vertices[inside[0]].p);
        if ni == 1 || no == 1 {
            let (single, others) = if ni == 1 { (inside[0], outside) } else { (outside[0], inside) };
            let mut tri = [0; 3];
            for i in 0..3 {
                tri[i] = self.intersection(vertices[single], vertices[others[i]])?;
            }
            self.triangle(tri, outward)?;
        } else {
            let [a, b, _, _] = inside;
            let [c, d, _, _] = outside;
            let ac = self.intersection(vertices[a], vertices[c])?;
            let ad = self.intersection(vertices[a], vertices[d])?;
            let bc = self.intersection(vertices[b], vertices[c])?;
            let bd = self.intersection(vertices[b], vertices[d])?;
            self.triangle([ac, ad, bd], outward)?;
            self.triangle([ac, bd, bc], outward)?;
        }
        Ok(())
    }
    fn intersection(&mut self, a: Vertex, b: Vertex) -> Result<u32, Error> {
        // Exact zeros share the grid-vertex key, including different incident edges.
        let (a, b) = if a.id <= b.id { (a, b) } else { (b, a) };
        let key = if a.value == 0.0 {
            (a.id, a.id)
        } else if b.value == 0.0 {
            (b.id, b.id)
        } else {
            (a.id, b.id)
        };
        if let Some(index) = self.edges.get(&key) {
            return Ok(*index);
        }
        if self.mesh.points.len() >= MAX_TRIANGLES * 3 {
            return Err(Error::new("limit", "mesh vertex budget exceeded"));
        }
        let t = a.value / (a.value - b.value);
        let p = if t == 0.0 {
            a.p
        } else if t == 1.0 {
            b.p
        } else {
            std::array::from_fn(|i| a.p[i] + t * (b.p[i] - a.p[i]))
        };
        let index = self.mesh.points.len() as u32;
        self.mesh.points.push(p);
        self.edges.insert(key, index);
        Ok(index)
    }
    fn triangle(&mut self, mut t: [u32; 3], outward: [f64; 3]) -> Result<(), Error> {
        if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
            return Ok(());
        }
        let [a, b, c] = t.map(|i| self.mesh.points[i as usize]);
        let normal = cross(sub(b, a), sub(c, a));
        if dot(normal, normal) == 0.0 {
            return Ok(());
        }
        if dot(normal, outward) < 0.0 {
            t.swap(1, 2);
        }
        if self.mesh.triangles.len() >= MAX_TRIANGLES {
            return Err(Error::new("limit", "mesh exceeds 500000 triangles; increase cell_size"));
        }
        self.mesh.triangles.push(t);
        Ok(())
    }
}
