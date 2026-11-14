//! How many faces, edges or vertices a body has.

use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::measure::{Measure, MeasureRequest, Measured};
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};

/// One plugin registered once per thing it counts.
pub struct Count {
    id: &'static str,
    of: fn(&Body) -> usize,
}

impl Count {
    pub fn all() -> [Self; 3] {
        [
            Self { id: "faces", of: |body| body.faces.len() },
            Self { id: "edges", of: |body| body.edges.len() },
            Self { id: "vertices", of: |body| body.vertices.len() },
        ]
    }
}

impl Plugin<Measure> for Count {
    fn id(&self) -> Id {
        Id::new(self.id)
    }

    fn call(&self, _: &Context, request: &MeasureRequest) -> Attempt<Measured> {
        Attempt::Done(Measured { value: (self.of)(&request.body) as f64, exact: Exactness::Exact })
    }
}
