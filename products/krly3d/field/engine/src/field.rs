use krly3d_field::{Node, Recipe, MAX_NODES};

type Matrix = [[f64; 3]; 3];
pub(super) struct Field<'a> {
    recipe: &'a Recipe,
    inverse_rotations: Vec<Matrix>,
}
impl<'a> Field<'a> {
    pub(super) fn new(recipe: &'a Recipe) -> Self {
        let inverse_rotations = recipe
            .nodes
            .iter()
            .map(|node| match node {
                Node::Ellipsoid { rotation, .. } | Node::RoundedBox { rotation, .. } => {
                    rotation_matrix(*rotation)
                }
                _ => rotation_matrix([0.0; 3]),
            })
            .collect();
        Self { recipe, inverse_rotations }
    }
    pub(super) fn evaluate(&self, p: [f64; 3], values: &mut [f64; MAX_NODES]) -> f64 {
        for (i, node) in self.recipe.nodes.iter().enumerate().take(self.recipe.root + 1) {
            values[i] = match *node {
                Node::Ellipsoid { center, radii, .. } => {
                    let q = local(p, center, self.inverse_rotations[i]);
                    let q = std::array::from_fn(|j| q[j] / radii[j]);
                    (dot(q, q).sqrt() - 1.0) * radii[0].min(radii[1]).min(radii[2])
                }
                Node::Capsule { a, b, radius } => {
                    let ba = sub(b, a);
                    let pa = sub(p, a);
                    let squared = dot(ba, ba);
                    let t = if squared == 0.0 { 0.0 } else { (dot(pa, ba) / squared).clamp(0.0, 1.0) };
                    let q = std::array::from_fn(|j| pa[j] - t * ba[j]);
                    dot(q, q).sqrt() - radius
                }
                Node::RoundedBox { center, half_size, radius, .. } => {
                    let p = local(p, center, self.inverse_rotations[i]);
                    let q: [f64; 3] = std::array::from_fn(|j| p[j].abs() - half_size[j]);
                    let positive = q.map(|x| x.max(0.0));
                    dot(positive, positive).sqrt() + q[0].max(q[1]).max(q[2]).min(0.0) - radius
                }
                Node::Union { a, b } => values[a].min(values[b]),
                Node::Intersection { a, b } => values[a].max(values[b]),
                Node::Subtract { a, b } => values[a].max(-values[b]),
                Node::SmoothUnion { a, b, k } => {
                    let h = (0.5 + 0.5 * (values[b] - values[a]) / k).clamp(0.0, 1.0);
                    values[b] * (1.0 - h) + values[a] * h - k * h * (1.0 - h)
                }
            };
        }
        values[self.recipe.root]
    }
}
fn rotation_matrix(degrees: [f64; 3]) -> Matrix {
    let [(sx, cx), (sy, cy), (sz, cz)] = degrees.map(|d| d.to_radians().sin_cos());
    // Transpose of Rz * Ry * Rx: world-to-local rotation.
    [
        [cz * cy, sz * cy, -sy],
        [cz * sy * sx - sz * cx, sz * sy * sx + cz * cx, cy * sx],
        [cz * sy * cx + sz * sx, sz * sy * cx - cz * sx, cy * cx],
    ]
}
fn local(p: [f64; 3], center: [f64; 3], matrix: Matrix) -> [f64; 3] {
    let delta = sub(p, center);
    matrix.map(|row| dot(row, delta))
}
pub(super) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(super) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
