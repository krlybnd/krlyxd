//! The volume of a solid: one third of the flux of the position vector through its faces.

use super::total;
use krly3d_core::{Attempt, Error, Id};
use krly3d_kernel::ports::measure::{Measure, MeasureRequest, Measured};
use krly3d_kernel::{Context, Plugin};

pub struct Volume;

impl Plugin<Measure> for Volume {
    fn id(&self) -> Id {
        Id::new("volume")
    }

    fn call(&self, cx: &Context, request: &MeasureRequest) -> Attempt<Measured> {
        if !request.body.solid {
            return Attempt::Failed(Error::input("only a solid has a volume"));
        }
        total(cx, &request.body, |face| face.flux / 3.0)
    }
}
