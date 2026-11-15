//! Exact, bounded in-process identity of the data actually read by this mesher.
//! No sampling, rounded summaries, addresses or unchecked content hashes establish
//! equality. New concrete geometry types must opt in here after a field audit.

use super::Config;
use crate::{curve, surface};
use krly3d_core::math::{nurbs, Iso3};
use krly3d_core::Error;
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::topo::Body;
use krly3d_kernel::Context;

pub(super) const MAX_BYTES: usize = 256 * 1024;

#[derive(Debug)]
enum Unavailable {
    OptOut,
    Stopped(Error),
}

type Result<T> = std::result::Result<T, Unavailable>;

pub(super) fn key(cx: &Context, body: &Body, config: Config) -> std::result::Result<Option<Vec<u8>>, Error> {
    let mut identity = Identity { bytes: Vec::new(), cx };
    match identity.write(body, config) {
        Ok(()) => Ok(Some(identity.bytes)),
        Err(Unavailable::OptOut) => Ok(None),
        Err(Unavailable::Stopped(error)) => Err(error),
    }
}

struct Identity<'a> {
    bytes: Vec<u8>,
    cx: &'a Context,
}

impl Identity<'_> {
    fn raw(&mut self, bytes: &[u8]) -> Result<()> {
        self.cx.check_control().map_err(Unavailable::Stopped)?;
        if bytes.len() > MAX_BYTES - self.bytes.len() {
            return Err(Unavailable::OptOut);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn count(&mut self, value: usize) -> Result<()> {
        self.raw(&(value as u64).to_le_bytes())
    }

    fn blob(&mut self, value: &[u8]) -> Result<()> {
        self.count(value.len())?;
        self.raw(value)
    }

    fn flag(&mut self, value: bool) -> Result<()> {
        self.raw(&[u8::from(value)])
    }

    fn floats(&mut self, values: &[f64]) -> Result<()> {
        for value in values {
            if !value.is_finite() {
                return Err(Unavailable::OptOut);
            }
            self.raw(&value.to_bits().to_le_bytes())?;
        }
        Ok(())
    }

    fn frame(&mut self, value: &Iso3) -> Result<()> {
        self.floats(value.translation.vector.as_slice())?;
        self.floats(value.rotation.matrix().as_slice())
    }

    fn basis(&mut self, value: &nurbs::Basis) -> Result<()> {
        self.count(value.degree)?;
        self.count(value.knots.len())?;
        self.floats(&value.knots)
    }

    fn write(&mut self, body: &Body, config: Config) -> Result<()> {
        self.blob(b"krly3d.std.brep.decline-content/1")?;
        self.blob(config.solver.as_bytes())?;
        self.blob(config.engine.as_bytes())?;
        self.blob(config.artifact.as_bytes())?;
        self.raw(&config.api.major.to_le_bytes())?;
        self.raw(&config.api.minor.to_le_bytes())?;
        let accuracy = self.cx.accuracy();
        self.floats(&[accuracy.scale(), accuracy.relative(), accuracy.angle()])?;
        self.count(config.quality.triangles)?;
        self.count(config.quality.refinement_rounds)?;
        self.count(config.quality.flip_rounds)?;
        self.flag(body.solid)?;
        self.count(body.vertices.len())?;
        for point in &body.vertices {
            self.floats(point.coords.as_slice())?;
        }
        self.count(body.edges.len())?;
        for edge in &body.edges {
            self.curve(edge.curve.as_ref())?;
            self.floats(&[edge.range.0, edge.range.1])?;
            self.flag(edge.ends.is_some())?;
            if let Some((start, end)) = edge.ends {
                self.count(start)?;
                self.count(end)?;
            }
            self.flag(edge.label.is_some())?;
            if let Some(label) = &edge.label {
                self.blob(label.as_bytes())?;
            }
        }
        self.count(body.faces.len())?;
        for face in &body.faces {
            self.blob(face.name.as_bytes())?;
            self.surface(face.surface.as_ref())?;
            self.flag(face.outward)?;
            self.count(face.loops.len())?;
            for boundary in &face.loops {
                self.count(boundary.len())?;
                for used in boundary {
                    self.count(used.edge)?;
                    self.flag(used.forward)?;
                }
            }
        }
        Ok(())
    }

    fn curve(&mut self, value: &dyn Curve) -> Result<()> {
        let concrete = value.as_any();
        if let Some(line) = concrete.downcast_ref::<curve::Line>() {
            self.raw(&[0])?;
            self.floats(line.origin().coords.as_slice())?;
            self.floats(line.direction().as_slice())
        } else if let Some(circle) = concrete.downcast_ref::<curve::Circle>() {
            self.raw(&[1])?;
            self.frame(circle.frame())?;
            self.floats(&[circle.radius()])
        } else if let Some(ellipse) = concrete.downcast_ref::<curve::Ellipse>() {
            self.raw(&[2])?;
            self.frame(ellipse.frame())?;
            let (a, b) = ellipse.semi_axes();
            self.floats(&[a, b])
        } else if let Some(nurbs) = concrete.downcast_ref::<curve::Nurbs>() {
            self.raw(&[3])?;
            let data = nurbs.data();
            // Transforms retain the original closed flag; it is not safely
            // reconstructed from transformed endpoints near the closure threshold.
            self.flag(nurbs.period().is_some())?;
            self.basis(&data.basis)?;
            self.count(data.points.len())?;
            for point in &data.points {
                self.floats(point.as_slice())?;
            }
            Ok(())
        } else {
            Err(Unavailable::OptOut)
        }
    }

    fn surface(&mut self, value: &dyn Surface) -> Result<()> {
        let concrete = value.as_any();
        if let Some(plane) = concrete.downcast_ref::<surface::Plane>() {
            self.raw(&[0])?;
            self.frame(plane.frame())
        } else if let Some(sphere) = concrete.downcast_ref::<surface::Sphere>() {
            self.raw(&[1])?;
            self.frame(sphere.frame())?;
            self.floats(&[sphere.radius()])
        } else if let Some(cylinder) = concrete.downcast_ref::<surface::Cylinder>() {
            self.raw(&[2])?;
            self.frame(cylinder.frame())?;
            self.floats(&[cylinder.radius()])
        } else if let Some(cone) = concrete.downcast_ref::<surface::Cone>() {
            self.raw(&[3])?;
            self.frame(cone.frame())?;
            self.floats(&[cone.half_angle()])
        } else if let Some(torus) = concrete.downcast_ref::<surface::Torus>() {
            self.raw(&[4])?;
            self.frame(torus.frame())?;
            self.floats(&[torus.major(), torus.minor()])
        } else if let Some(ellipsoid) = concrete.downcast_ref::<surface::Ellipsoid>() {
            self.raw(&[5])?;
            self.frame(ellipsoid.frame())?;
            self.floats(&ellipsoid.axes())
        } else if let Some(nurbs) = concrete.downcast_ref::<surface::Nurbs>() {
            self.raw(&[6])?;
            let data = nurbs.data();
            self.flag(nurbs.periods()[0].is_some())?;
            self.basis(&data.u)?;
            self.basis(&data.v)?;
            self.count(data.points.len())?;
            for row in &data.points {
                self.count(row.len())?;
                for point in row {
                    self.floats(point.as_slice())?;
                }
            }
            // These effective samples/thresholds feed projection. Reconstructing
            // them from only the net would silently conflate transformed surfaces.
            let (grid, spacing) = nurbs.projection_data();
            self.floats(&[spacing])?;
            self.count(grid.len())?;
            for (uv, point) in grid {
                self.floats(uv.coords.as_slice())?;
                self.floats(point.coords.as_slice())?;
            }
            Ok(())
        } else {
            Err(Unavailable::OptOut)
        }
    }
}
