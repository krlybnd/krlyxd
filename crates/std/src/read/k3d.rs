//! Reader of the native text format, whose grammar lives with the document in `core`.

use krly3d_core::document::text;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::model::{Document, GEOMETRY};
use krly3d_kernel::ports::read::Read;
use krly3d_kernel::{Context, Plugin};

pub struct K3dReader;

impl Plugin<Read> for K3dReader {
    fn id(&self) -> Id {
        Id::new("k3d")
    }

    fn call(&self, _: &Context, written: &String) -> Attempt<Document> {
        text::parse(written, &[GEOMETRY]).into()
    }
}
