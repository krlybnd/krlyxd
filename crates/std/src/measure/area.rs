//! The surface area of a body: the sum of the areas of its faces.

use super::total;
use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::measure::{Measure, MeasureRequest, Measured};
use krly3d_kernel::{Context, Plugin};

pub struct Area;

impl Plugin<Measure> for Area {
    fn id(&self) -> Id {
        Id::new("area")
    }

    fn call(&self, cx: &Context, request: &MeasureRequest) -> Attempt<Measured> {
        total(cx, &request.body, |face| face.area)
    }
}
