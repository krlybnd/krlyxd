//! The vocabulary every krly3d crate shares.
//!
//! Nothing here knows about geometry beyond plain vectors: ids, the outcome of asking a
//! plugin, the one error type, tolerances, how far a number can be trusted, and time.
//! Independent leaves are compatibility re-exports of `krlyxd`; document and domain
//! support remain here.

pub mod accuracy;
pub mod attempt;
pub mod claim;
pub mod clock;
pub mod document;
pub mod error;
pub mod exact;
pub mod id;
pub mod math;
pub mod plugin;
pub mod schema;
pub mod text;

pub use accuracy::Accuracy;
pub use attempt::Attempt;
pub use clock::{Clock, FakeClock, SystemClock};
pub use error::{Error, ErrorKind};
pub use exact::Exactness;
pub use id::Id;
pub use krlyxd::{CallControl, CallStop};
