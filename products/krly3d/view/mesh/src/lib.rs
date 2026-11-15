//! Triangle preview renderer. Tessellation and its caching belong to the mesh bridge;
//! the pixels retain the original body's face indices for paint and visibility queries.

use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::Vec3;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::topo::{body_bounds, Body};
use krly3d_kernel::{Context, Plugin};
use krly3d_mesh::preview::RenderMesh;
use krly3d_view::ports::render::{style, Image, Paint, Projection, Render, RenderRequest, Seen, CHANNELS};

pub struct Raster;

/// The opt-in preview renderer's registry name.
pub const NAME: &str = "mesh";

const BATCH: usize = 64;
// The camera/query facade has the same public-input limit. Keep direct Rust calls
// bounded too, including the integer edge arithmetic and buffer allocations.
const LARGEST_SIDE: usize = 2048;
// Integer edge equations make ownership of shared edges exact, including at pixel
// centres. Clipping before conversion keeps their products comfortably within i64.
const SUBPIXEL: i64 = 256;
const HALF: i64 = SUBPIXEL / 2;

impl Plugin<Render> for Raster {
    fn id(&self) -> Id {
        Id::new(NAME)
    }

    fn call(&self, cx: &Context, request: &RenderRequest) -> Attempt<Image> {
        render_mesh(cx, request, krly3d_mesh::DEFAULT_TOLERANCE)
    }
}

/// Draw a public mesh preview at the requested model-space tessellation tolerance.
///
/// Perspective triangles are clipped to a numerical near plane at the context's
/// length resolution and to the viewport before division. Depth is the parameter of
/// `Projection::ray`, not Euclidean eye distance. This is an approximate image; exact
/// picking remains the responsibility of the existing probe/pick path.
pub fn render_mesh(cx: &Context, request: &RenderRequest, tolerance: f64) -> Attempt<Image> {
    let result = drawn(cx, request, tolerance);
    // A concurrent stop also wins over a bridge decline or validation failure.
    cx.publish_control(|| result).unwrap_or_else(Attempt::Failed)
}

fn drawn(cx: &Context, request: &RenderRequest, tolerance: f64) -> Attempt<Image> {
    let count = match dimensions(cx, request, tolerance) {
        Ok(count) => count,
        Err(error) => return Attempt::Failed(error),
    };
    // Use the same exact bounds as raycast so a fitted camera does not vary with
    // tessellation quality (particularly at the poles of curved surfaces).
    let mut around = Bounds::empty();
    for (index, body) in request.bodies.iter().enumerate() {
        match body_bounds(cx, body) {
            Attempt::Done(bounds) => around = around.union(&bounds),
            Attempt::Declined(reason) => {
                return Attempt::Declined(format!("body {} bounds: {reason}", index + 1))
            }
            Attempt::Failed(error) => return Attempt::Failed(error),
        }
    }
    let projection = match request.view.projection(cx.accuracy(), &around, request.size) {
        Ok(projection) => projection,
        Err(error) => return Attempt::Failed(error),
    };
    let mut raster = match Buffer::new(cx, projection, count) {
        Ok(raster) => raster,
        Err(error) => return Attempt::Failed(error),
    };
    for (index, body) in request.bodies.iter().enumerate() {
        let mesh = match krly3d_mesh_engine::preview::mesh_for_body(cx, body, tolerance) {
            Attempt::Done(mesh) => mesh,
            Attempt::Declined(reason) => {
                return Attempt::Declined(format!("body {} mesh preview: {reason}", index + 1))
            }
            Attempt::Failed(error) => return Attempt::Failed(error),
        };
        if let Err(error) = validate_mesh(cx, &mesh, body).and_then(|()| raster.body(cx, index, &mesh)) {
            return Attempt::Failed(error);
        }
    }
    raster.paint(cx, &request.paint).into()
}

fn dimensions(cx: &Context, request: &RenderRequest, tolerance: f64) -> Result<usize, Error> {
    cx.check_control()?;
    if !tolerance.is_finite() || tolerance <= cx.accuracy().length() {
        return Err(Error::input("a mesh preview tolerance must be finite and above the length tolerance"));
    }
    let (width, height) = request.size;
    if width == 0 || height == 0 || width > LARGEST_SIDE || height > LARGEST_SIDE {
        return Err(Error::input(format!(
            "a mesh preview must be between 1 and {} pixels on each side",
            LARGEST_SIDE
        )));
    }
    width
        .checked_mul(height)
        .filter(|count| count.checked_mul(CHANNELS).is_some())
        .ok_or_else(|| Error::input("mesh preview dimensions overflow"))
}

fn validate_mesh(cx: &Context, mesh: &RenderMesh, body: &Body) -> Result<(), Error> {
    cx.check_control()?;
    let count = mesh.geometry.triangles.len();
    if mesh.source_faces.len() != count || mesh.corner_normals.len() != count {
        return Err(Error::geometry("mesh preview triangle attributes have inconsistent lengths"));
    }
    if mesh.face_names.len() != body.faces.len() {
        return Err(Error::geometry("mesh preview face names do not identify the source body"));
    }
    for (index, (name, face)) in mesh.face_names.iter().zip(&body.faces).enumerate() {
        if index % BATCH == 0 {
            cx.check_control()?;
        }
        if *name != face.name {
            return Err(Error::geometry("mesh preview face names do not identify the source body"));
        }
    }
    for points in mesh.geometry.points.chunks(BATCH) {
        cx.check_control()?;
        if points.iter().flatten().any(|value| !value.is_finite()) {
            return Err(Error::geometry("mesh preview contains a non-finite point"));
        }
    }
    for (index, triangle) in mesh.geometry.triangles.iter().enumerate() {
        if index % BATCH == 0 {
            cx.check_control()?;
        }
        if triangle.iter().any(|point| *point as usize >= mesh.geometry.points.len()) {
            return Err(Error::geometry("mesh preview contains an invalid point index"));
        }
        if mesh.source_faces[index] as usize >= body.faces.len() {
            return Err(Error::geometry("mesh preview contains an invalid source face"));
        }
        if mesh.corner_normals[index].iter().any(|normal| unit(Vec3::from(*normal)).is_none()) {
            return Err(Error::geometry("mesh preview contains a non-finite or zero corner normal"));
        }
    }
    Ok(())
}

fn unit(vector: Vec3) -> Option<Vec3> {
    let scale = vector.iter().map(|v| v.abs()).fold(0.0, f64::max);
    if !vector.iter().all(|v| v.is_finite()) || scale == 0.0 {
        return None;
    }
    let scaled = vector / scale;
    Some(scaled / scaled.norm())
}

#[derive(Clone, Copy)]
struct Vertex {
    /// Camera coordinates: right, down, forward.
    point: Vec3,
    normal: Vec3,
}

#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    offset: f64,
}

impl Plane {
    fn new(normal: Vec3, offset: f64) -> Self {
        let scale = normal.iter().map(|v| v.abs()).fold(offset.abs(), f64::max);
        Self { normal: normal / scale, offset: offset / scale }
    }
}

/// Sutherland-Hodgman clipping in camera space. A common edge scale keeps plane
/// distances finite even for enormous offscreen vertices. Canonical endpoint order
/// gives shared edges identical intersections when adjacent triangles traverse them
/// in opposite directions.
fn clip(vertices: Vec<Vertex>, plane: Plane) -> Vec<Vertex> {
    let scale_of = |a: Vertex, b: Vertex| {
        let a = plane.normal.component_mul(&a.point);
        let b = plane.normal.component_mul(&b.point);
        a.iter()
            .chain(b.iter())
            .map(|value| value.abs())
            .fold(plane.offset.abs().max(f64::MIN_POSITIVE), f64::max)
    };
    let distance = |vertex: Vertex, scale: f64| {
        (plane.normal.component_mul(&vertex.point) / scale).sum() + plane.offset / scale
    };
    let mut clipped = Vec::with_capacity(vertices.len() + 1);
    let Some(mut previous) = vertices.last().copied() else { return clipped };
    let mut before = distance(previous, scale_of(previous, previous));
    for current in vertices {
        let after = distance(current, scale_of(current, current));
        if (before >= 0.0) != (after >= 0.0) {
            let (a, b) = if ordered(previous.point, current.point) {
                (previous, current)
            } else {
                (current, previous)
            };
            let scale = scale_of(a, b);
            let (da, db) = (distance(a, scale), distance(b, scale));
            let t = (da / (da - db)).clamp(0.0, 1.0);
            let u = (-db / (da - db)).clamp(0.0, 1.0);
            let mut point = a.point * u + b.point * t;
            // Preserve a small clip-plane offset even when the intersection is
            // extremely close to an endpoint. In particular, near-plane vertices
            // must stay strictly in front of the eye after rounding.
            let mut axis = 0;
            for candidate in 1..3 {
                if plane.normal[candidate].abs() > plane.normal[axis].abs() {
                    axis = candidate;
                }
            }
            let mut residual = plane.offset;
            for other in 0..3 {
                if other != axis {
                    residual += plane.normal[other] * point[other];
                }
            }
            point[axis] = -residual / plane.normal[axis];
            clipped.push(Vertex { point, normal: a.normal * u + b.normal * t });
        }
        if after >= 0.0 {
            clipped.push(current);
        }
        previous = current;
        before = after;
    }
    clipped
}

fn ordered(a: Vec3, b: Vec3) -> bool {
    for axis in 0..3 {
        if a[axis] != b[axis] {
            return a[axis] < b[axis];
        }
    }
    true
}

#[derive(Clone, Copy)]
struct Screen {
    x: i64,
    y: i64,
    depth: f64,
    normal: Vec3,
}

fn edge(a: Screen, b: Screen, x: i64, y: i64) -> i64 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}

fn top_left(a: Screen, b: Screen) -> bool {
    b.y < a.y || (b.y == a.y && b.x > a.x)
}

#[derive(Clone, Copy)]
struct Sight {
    seen: Seen,
    depth: f64,
    facing: Vec3,
    size: f64,
}

struct Buffer {
    projection: Projection,
    planes: [Plane; 5],
    sights: Vec<Option<Sight>>,
}

impl Buffer {
    fn new(cx: &Context, projection: Projection, count: usize) -> Result<Self, Error> {
        cx.check_control()?;
        let finite = |vector: &Vec3| vector.iter().all(|value| value.is_finite());
        if !finite(&projection.center.coords)
            || !finite(&projection.right)
            || !finite(&projection.up)
            || !finite(&projection.toward)
            || !projection.pixel.is_finite()
            || projection.pixel <= 0.0
        {
            return Err(Error::input("mesh preview camera has non-finite coordinates or pixel size"));
        }
        let (width, height) = (projection.size.0 as f64, projection.size.1 as f64);
        let sides = match projection.pinhole {
            Some(pinhole) => {
                let (x, y) = pinhole.centre;
                if !finite(&pinhole.eye.coords) || !x.is_finite() || !y.is_finite() {
                    return Err(Error::input("mesh preview camera has non-finite eye or image centre"));
                }
                [
                    Plane::new(Vec3::new(pinhole.focal, 0.0, x), 0.0),
                    Plane::new(Vec3::new(-pinhole.focal, 0.0, width - x), 0.0),
                    Plane::new(Vec3::new(0.0, pinhole.focal, y), 0.0),
                    Plane::new(Vec3::new(0.0, -pinhole.focal, height - y), 0.0),
                ]
            }
            None => {
                let (x, y) = (width * 0.5 * projection.pixel, height * 0.5 * projection.pixel);
                if !x.is_finite() || !y.is_finite() {
                    return Err(Error::input("mesh preview window extents overflow"));
                }
                [
                    Plane::new(Vec3::x(), x),
                    Plane::new(-Vec3::x(), x),
                    Plane::new(Vec3::y(), y),
                    Plane::new(-Vec3::y(), y),
                ]
            }
        };
        let planes = [
            Plane::new(Vec3::z(), -cx.accuracy().length().max(f64::MIN_POSITIVE)),
            sides[0],
            sides[1],
            sides[2],
            sides[3],
        ];
        let mut sights = Vec::new();
        sights.try_reserve_exact(count).map_err(|_| Error::input("mesh preview buffer is too large"))?;
        while sights.len() < count {
            cx.check_control()?;
            sights.resize((sights.len() + BATCH).min(count), None);
        }
        Ok(Self { projection, planes, sights })
    }

    fn body(&mut self, cx: &Context, body: usize, mesh: &RenderMesh) -> Result<(), Error> {
        for (index, triangle) in mesh.geometry.triangles.iter().enumerate() {
            cx.check_control()?;
            let mut vertices = Vec::with_capacity(3);
            for (corner, point) in triangle.iter().enumerate() {
                let relative =
                    Vec3::from(mesh.geometry.points[*point as usize]) - self.projection.center.coords;
                let point = Vec3::new(
                    relative.dot(&self.projection.right),
                    -relative.dot(&self.projection.up),
                    relative.dot(&self.projection.toward),
                );
                if !point.iter().all(|value| value.is_finite()) {
                    return Err(Error::geometry("mesh preview camera coordinates overflow"));
                }
                // Validation has already checked these vectors; keep normals in world
                // coordinates so shading agrees with the raycast lamp.
                let normal = unit(Vec3::from(mesh.corner_normals[index][corner]))
                    .ok_or_else(|| Error::geometry("mesh preview has an invalid normal"))?;
                vertices.push(Vertex { point, normal });
            }
            for plane in self.planes {
                vertices = clip(vertices, plane);
                if vertices.len() < 3 {
                    break;
                }
            }
            let seen = Seen { body, face: mesh.source_faces[index] as usize };
            for corner in 1..vertices.len().saturating_sub(1) {
                let triangle = [
                    self.screen(vertices[0])?,
                    self.screen(vertices[corner])?,
                    self.screen(vertices[corner + 1])?,
                ];
                self.triangle(cx, triangle, seen)?;
            }
        }
        Ok(())
    }

    fn screen(&self, vertex: Vertex) -> Result<Screen, Error> {
        let p = self.projection;
        let (x, y) = match p.pinhole {
            Some(pinhole) => (
                (vertex.point.x / vertex.point.z) * pinhole.focal + pinhole.centre.0,
                (vertex.point.y / vertex.point.z) * pinhole.focal + pinhole.centre.1,
            ),
            None => (
                vertex.point.x / p.pixel + p.size.0 as f64 * 0.5,
                vertex.point.y / p.pixel + p.size.1 as f64 * 0.5,
            ),
        };
        if !x.is_finite() || !y.is_finite() || vertex.point.z <= 0.0 {
            return Err(Error::geometry("mesh preview projection overflow"));
        }
        // Roundoff at a clip plane can leave a coordinate just outside the viewport.
        Ok(Screen {
            x: (x.clamp(0.0, p.size.0 as f64) * SUBPIXEL as f64).round() as i64,
            y: (y.clamp(0.0, p.size.1 as f64) * SUBPIXEL as f64).round() as i64,
            depth: vertex.point.z,
            normal: vertex.normal,
        })
    }

    fn triangle(&mut self, cx: &Context, mut v: [Screen; 3], seen: Seen) -> Result<(), Error> {
        let mut area = edge(v[0], v[1], v[2].x, v[2].y);
        if area == 0 {
            return Ok(());
        }
        if area < 0 {
            v.swap(1, 2);
            area = -area;
        }
        let (width, height) = self.projection.size;
        let range = |a: i64, b: i64, c: i64, limit: usize| {
            let low = (a.min(b).min(c) + HALF - 1).div_euclid(SUBPIXEL).max(0);
            let high = (a.max(b).max(c) - HALF).div_euclid(SUBPIXEL).min(limit as i64 - 1);
            low..=high
        };
        let columns = range(v[0].x, v[1].x, v[2].x, width);
        let rows = range(v[0].y, v[1].y, v[2].y, height);
        let inclusive = [top_left(v[1], v[2]), top_left(v[2], v[0]), top_left(v[0], v[1])];
        for row in rows {
            cx.check_control()?;
            let y = row * SUBPIXEL + HALF;
            for column in columns.clone() {
                if column % BATCH as i64 == 0 {
                    cx.check_control()?;
                }
                let x = column * SUBPIXEL + HALF;
                let edges = [edge(v[1], v[2], x, y), edge(v[2], v[0], x, y), edge(v[0], v[1], x, y)];
                if edges
                    .iter()
                    .zip(inclusive)
                    .any(|(edge, inclusive)| *edge < 0 || (*edge == 0 && !inclusive))
                {
                    continue;
                }
                let barycentric = edges.map(|edge| edge as f64 / area as f64);
                let (depth, normal) = interpolate(v, barycentric, self.projection.pinhole.is_some())?;
                let at = row as usize * width + column as usize;
                if !nearer(self.sights[at], seen, depth, cx.accuracy().length()) {
                    continue;
                }
                let facing = if normal.dot(&self.projection.toward) > 0.0 { -normal } else { normal };
                let size = match self.projection.pinhole {
                    Some(_) => {
                        let ray = self.projection.ray(column as usize, row as usize);
                        depth * self.projection.pixel * ray.direction.norm()
                    }
                    None => self.projection.pixel,
                };
                if !size.is_finite() {
                    return Err(Error::geometry("mesh preview pixel footprint overflow"));
                }
                self.sights[at] = Some(Sight { seen, depth, facing, size });
            }
        }
        Ok(())
    }

    fn paint(self, cx: &Context, paint: &[Paint]) -> Result<Image, Error> {
        cx.check_control()?;
        let mut pixels = Vec::new();
        let mut seen = Vec::new();
        pixels
            .try_reserve_exact(self.sights.len() * CHANNELS)
            .map_err(|_| Error::input("mesh preview colour buffer is too large"))?;
        seen.try_reserve_exact(self.sights.len())
            .map_err(|_| Error::input("mesh preview visibility buffer is too large"))?;
        let lamp = style::lamp(&self.projection);
        for (at, sight) in self.sights.iter().enumerate() {
            if at % BATCH == 0 {
                cx.check_control()?;
            }
            seen.push(sight.map(|sight| sight.seen));
            let colour = match sight {
                None => style::BACKGROUND,
                Some(_) if self.border(at) => style::OUTLINE,
                Some(sight) => style::colour(paint, sight.seen, style::light(&sight.facing, &lamp)),
            };
            pixels.extend(colour);
        }
        let image = Image { projection: self.projection, pixels, seen };
        cx.publish_control(|| image)
    }

    fn border(&self, at: usize) -> bool {
        let Some(here) = self.sights[at] else { return false };
        let (width, height) = self.projection.size;
        let (column, row) = (at % width, at / width);
        let differs = |other: usize| match self.sights[other] {
            None => true,
            Some(there) if here.seen == there.seen => false,
            Some(there) => style::boundary(
                here.facing.dot(&there.facing),
                (here.depth - there.depth).abs() * self.projection.toward.norm(),
                here.size.max(there.size),
            ),
        };
        (column > 0 && differs(at - 1))
            || (column + 1 < width && differs(at + 1))
            || (row > 0 && differs(at - width))
            || (row + 1 < height && differs(at + width))
    }
}

fn interpolate(v: [Screen; 3], mut weights: [f64; 3], perspective: bool) -> Result<(f64, Vec3), Error> {
    let depth = if perspective {
        let reciprocal = weights[0] / v[0].depth + weights[1] / v[1].depth + weights[2] / v[2].depth;
        let depth = 1.0 / reciprocal;
        for corner in 0..3 {
            weights[corner] *= depth / v[corner].depth;
        }
        depth
    } else {
        weights[0] * v[0].depth + weights[1] * v[1].depth + weights[2] * v[2].depth
    };
    if !depth.is_finite() || depth <= 0.0 {
        return Err(Error::geometry("mesh preview depth interpolation overflow"));
    }
    let normal = v[0].normal * weights[0] + v[1].normal * weights[1] + v[2].normal * weights[2];
    unit(normal)
        .map(|normal| (depth, normal))
        .ok_or_else(|| Error::geometry("mesh preview interpolated normal is undefined"))
}

fn nearer(previous: Option<Sight>, seen: Seen, depth: f64, tolerance: f64) -> bool {
    previous.is_none_or(|previous| {
        if previous.seen.body == seen.body {
            // Within one body, do not let a later back triangle hide the front.
            depth < previous.depth || (depth == previous.depth && seen.face < previous.seen.face)
        } else {
            depth <= previous.depth + tolerance
        }
    })
}

#[cfg(test)]
mod tests;
