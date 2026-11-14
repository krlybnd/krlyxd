//! Boundary representation: a body is faces on surfaces, bounded by edges on curves,
//! meeting in vertices.
//!
//! Conventions, the same everywhere:
//! * everything is in world coordinates;
//! * the material of a face lies to the left of each of its boundary edges when seen from
//!   outside, so outer boundaries run counter-clockwise and holes clockwise;
//! * there are no seam edges: a face may have several boundary loops or none at all
//!   (a whole sphere), and a closed edge (a full circle) has no vertex.

pub mod anchor;
pub mod body;
pub mod bounds;
pub mod builder;
pub mod check;
pub mod names;

pub use anchor::{anchor, Anchor};
pub use body::{nearest_parameter, Body, Edge, EdgeId, Face, FaceId, Use, VertexId};
pub use bounds::{body_bounds, face_bounds};
pub use builder::{BodyBuilder, Bound};
pub use check::check;
pub use names::{edge_by_name, edge_faces, edge_names};
