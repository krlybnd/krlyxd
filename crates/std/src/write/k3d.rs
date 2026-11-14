//! Writer of the native text format, whose grammar lives with the document in `core`.

use krly3d_core::document::text;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::model::Document;
use krly3d_kernel::ports::write::Write;
use krly3d_kernel::{Context, Plugin};

pub struct K3dWriter;

impl Plugin<Write> for K3dWriter {
    fn id(&self) -> Id {
        Id::new("k3d")
    }

    fn call(&self, _: &Context, document: &Document) -> Attempt<String> {
        text::print(document).into()
    }
}
