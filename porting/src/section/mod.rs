//! Closed-form sections: the curve in which two analytic surfaces meet.
//!
//! Step one of a general boolean. Every rule is a small type implementing
//! `PairRule`; `Rules` is the registry. A new surface pair is a new rule
//! appended to the registry – existing rules are never edited.
//!
//! Pairs without a rule (general quadric–quadric, anything with a torus)
//! answer `Section::NoClosedForm`; those go to numerical tracing.

mod rules;
#[cfg(test)]
mod tests;

pub use rules::{CylinderCylinderParallel, PlaneCylinder, PlanePlane, PlaneSphere, SphereCylinderCoaxial, SphereSphere};

use crate::brep::SurfaceGeometry;
use crate::math::{add, cross, norm, scale, Point3};

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub point: Point3,
    /// unit direction
    pub direction: Point3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Circle {
    pub center: Point3,
    /// unit normal of the circle's plane
    pub normal: Point3,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Section {
    Empty,
    /// the surfaces touch in one point
    Point(Point3),
    Lines(Vec<Line>),
    Circles(Vec<Circle>),
    Ellipse { center: Point3, normal: Point3, major: Point3, semi_major: f64, semi_minor: f64 },
    /// the same surface
    Coincident,
    NoClosedForm,
}

/// Two unit vectors spanning the plane with the given unit normal.
pub(crate) fn basis(normal: Point3) -> (Point3, Point3) {
    let helper = if normal[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = cross(normal, helper);
    let u = scale(u, 1.0 / norm(u));
    (u, cross(normal, u))
}

impl Section {
    /// `n` points per curve (lines are sampled around their base point).
    pub fn points(&self, n: usize) -> Vec<Point3> {
        let turn = |k: usize| std::f64::consts::TAU * k as f64 / n.max(1) as f64;
        match self {
            Section::Point(p) => vec![*p],
            Section::Lines(lines) => lines
                .iter()
                .flat_map(|l| (0..n).map(move |k| add(l.point, scale(l.direction, k as f64 - n as f64 / 2.0))))
                .collect(),
            Section::Circles(circles) => circles
                .iter()
                .flat_map(|c| {
                    let (u, v) = basis(c.normal);
                    (0..n).map(move |k| add(c.center, add(scale(u, c.radius * turn(k).cos()), scale(v, c.radius * turn(k).sin()))))
                })
                .collect(),
            Section::Ellipse { center, normal, major, semi_major, semi_minor } => {
                let minor = cross(*normal, *major);
                (0..n)
                    .map(|k| add(*center, add(scale(*major, semi_major * turn(k).cos()), scale(minor, semi_minor * turn(k).sin()))))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

/// One closed-form rule for one (ordered) pair of surface kinds.
pub trait PairRule: Send + Sync {
    fn name(&self) -> &'static str;
    /// `None` when this rule does not cover the pair in this configuration.
    fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Option<Section>;
}

pub struct Rules {
    rules: Vec<Box<dyn PairRule>>,
}

impl Rules {
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }
    pub fn standard() -> Self {
        Self::empty()
            .with(PlanePlane)
            .with(PlaneSphere)
            .with(PlaneCylinder)
            .with(SphereSphere)
            .with(SphereCylinderCoaxial)
            .with(CylinderCylinderParallel)
    }
    pub fn with(mut self, rule: impl PairRule + 'static) -> Self {
        self.rules.push(Box::new(rule));
        self
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.rules.iter().map(|r| r.name()).collect()
    }
    /// The section of two surfaces, trying each rule in both argument orders.
    pub fn section(&self, a: &SurfaceGeometry, b: &SurfaceGeometry, tol: f64) -> Section {
        for rule in &self.rules {
            if let Some(found) = rule.section(a, b, tol).or_else(|| rule.section(b, a, tol)) {
                return found;
            }
        }
        Section::NoClosedForm
    }
}
