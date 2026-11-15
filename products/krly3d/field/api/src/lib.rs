//! Version 1 bounded millimetre field recipes. Negative values denote the interior.
//!
//! Rotation is Euler degrees: local X, then Y, then Z (Rz * Ry * Rx).
//! Rounded-box half sizes describe the core; its radius expands the exterior.
//! Ellipsoids use a conservative distance-like field, not an exact signed distance.
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_NODES: usize = 128;
pub const MAX_GRID_NODES: usize = 4_000_000;
pub const MAX_TRIANGLES: usize = 500_000;
pub const MAX_RECIPE_BYTES: usize = 262_144;
pub const ENGINE_REVISION: &str = "krly3d.field.native/1";
const MAX_LENGTH: f64 = 1_000_000.0;
const MIN_LENGTH: f64 = 0.000001;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    pub units: String,
    pub bounds: Bounds,
    pub cell_size: f64,
    pub nodes: Vec<Node>,
    pub root: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Ellipsoid {
        center: [f64; 3],
        radii: [f64; 3],
        #[serde(default)]
        rotation: [f64; 3],
    },
    Capsule {
        a: [f64; 3],
        b: [f64; 3],
        radius: f64,
    },
    RoundedBox {
        center: [f64; 3],
        half_size: [f64; 3],
        radius: f64,
        #[serde(default)]
        rotation: [f64; 3],
    },
    Union {
        a: usize,
        b: usize,
    },
    SmoothUnion {
        a: usize,
        b: usize,
        k: f64,
    },
    Subtract {
        a: usize,
        b: usize,
    },
    Intersection {
        a: usize,
        b: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

/// Validated grid layout. Intervals are rounded up so spacing never exceeds cell_size.
/// All six supplied bounds are sampled exactly; no implicit cropping or enlargement.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub dimensions: [usize; 3],
    pub spacing: [f64; 3],
    pub nodes: usize,
}

pub fn decode(bytes: &[u8]) -> Result<Recipe, Error> {
    if bytes.len() > MAX_RECIPE_BYTES {
        return Err(Error::new("limit", "recipe exceeds 262144 bytes"));
    }
    let recipe: Recipe = serde_json::from_slice(bytes).map_err(|e| Error::new("input", e.to_string()))?;
    recipe.validate()?;
    Ok(recipe)
}

impl Recipe {
    pub fn validate(&self) -> Result<Grid, Error> {
        if self.version != 1 || self.units != "mm" {
            return Err(Error::new("input", "requires recipe version 1 and units mm"));
        }
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES || self.root >= self.nodes.len() {
            return Err(Error::new("input", "requires 1..128 nodes and an in-range root"));
        }
        length(self.cell_size)?;
        vector(self.bounds.min)?;
        vector(self.bounds.max)?;
        for (index, node) in self.nodes.iter().enumerate() {
            node.validate(index)?;
        }
        let mut dimensions = [0; 3];
        let mut spacing = [0.0; 3];
        let mut nodes = 1_usize;
        for axis in 0..3 {
            let extent = self.bounds.max[axis] - self.bounds.min[axis];
            if extent < MIN_LENGTH {
                return Err(Error::new("input", "bounds must have positive extents >= 1e-6 mm"));
            }
            let intervals = (extent / self.cell_size).ceil();
            if intervals >= MAX_GRID_NODES as f64 {
                return Err(Error::new("limit", "grid exceeds 4000000 samples"));
            }
            dimensions[axis] = intervals as usize + 1;
            spacing[axis] = extent / (dimensions[axis] - 1) as f64;
            nodes = nodes
                .checked_mul(dimensions[axis])
                .filter(|n| *n <= MAX_GRID_NODES)
                .ok_or_else(|| Error::new("limit", "grid exceeds 4000000 samples"))?;
        }
        Ok(Grid { dimensions, spacing, nodes })
    }
}

impl Node {
    fn validate(&self, index: usize) -> Result<(), Error> {
        match *self {
            Self::Ellipsoid { center, radii, rotation } => {
                vector(center)?;
                sizes(radii)?;
                vector(rotation)?;
            }
            Self::Capsule { a, b, radius } => {
                vector(a)?;
                vector(b)?;
                length(radius)?;
            }
            Self::RoundedBox { center, half_size, radius, rotation } => {
                vector(center)?;
                sizes(half_size)?;
                vector(rotation)?;
                if radius != 0.0 {
                    length(radius)?;
                }
            }
            Self::Union { a, b } | Self::Subtract { a, b } | Self::Intersection { a, b } => {
                children(a, b, index)?
            }
            Self::SmoothUnion { a, b, k } => {
                children(a, b, index)?;
                length(k)?;
            }
        }
        Ok(())
    }
}
fn children(a: usize, b: usize, index: usize) -> Result<(), Error> {
    if a >= index || b >= index {
        return Err(Error::new("input", "DAG children must precede their parent"));
    }
    Ok(())
}
fn vector(v: [f64; 3]) -> Result<(), Error> {
    if v.iter().any(|x| !x.is_finite() || x.abs() > MAX_LENGTH) {
        return Err(Error::new("input", "coordinates and rotations must be finite and within +/-1000000"));
    }
    Ok(())
}
fn sizes(v: [f64; 3]) -> Result<(), Error> {
    for x in v {
        length(x)?;
    }
    Ok(())
}
fn length(x: f64) -> Result<(), Error> {
    if !x.is_finite() || !(MIN_LENGTH..=MAX_LENGTH).contains(&x) {
        return Err(Error::new("input", "length must be finite and within 1e-6..1000000 mm"));
    }
    Ok(())
}
