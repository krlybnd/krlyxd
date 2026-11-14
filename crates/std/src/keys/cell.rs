//! `cell:` — where a face is: the cell of a grid that holds its anchor, at a coarse and
//! at a fine scale. Faces near each other share `cell:L1:…`; close ones `cell:L2:…` too.

use krly3d_core::{Attempt, Id};
use krly3d_kernel::ports::keys::{FaceKeys, Keys, KeysRequest};
use krly3d_kernel::{Context, Plugin};

pub struct CellKey;

/// The levels of the grid: the number in the key, and how many cells make the diagonal
/// of the body.
const LEVELS: [(u32, f64); 2] = [(1, 4.0), (2, 16.0)];

impl Plugin<Keys> for CellKey {
    fn id(&self) -> Id {
        Id::new("cell")
    }

    fn call(&self, cx: &Context, request: &KeysRequest) -> Attempt<FaceKeys> {
        let diagonal = request.bounds.size().norm();
        if !(diagonal.is_finite() && diagonal > cx.accuracy().length()) {
            return Attempt::declined("a body without size has no grid");
        }
        let keys = LEVELS.map(|(level, cells)| {
            let size = diagonal / cells;
            // A point within the length tolerance below a cell wall counts as on it, so
            // that a face meant to sit on the origin is not put to one side by round-off.
            let cell =
                request.anchor.point.coords.map(|at| ((at + cx.accuracy().length()) / size).floor() as i64);
            format!("cell:L{level}:{},{},{}", cell.x, cell.y, cell.z)
        });
        Attempt::Done(FaceKeys { keys: keys.to_vec(), related: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use crate::keys::testing::{face, keys};
    use crate::tool::testing::{context, made};
    use krly3d_core::math::Point3;
    use krly3d_core::{Attempt, Id};
    use krly3d_kernel::ports::keys::Keys;
    use krly3d_kernel::schema::Params;

    #[test]
    fn a_face_is_keyed_by_the_grid_cell_of_its_anchor_at_two_scales() {
        let cx = context();
        // The diagonal of this block is 9, so the cells are 2.25 and 0.5625 wide.
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 4.0).with("h", 7.0), &[]);

        assert_eq!(keys(&cx, "cell", &block, "top"), ["cell:L1:0,0,3", "cell:L2:0,0,12"]);
        assert_eq!(keys(&cx, "cell", &block, "left"), ["cell:L1:-1,0,1", "cell:L2:-4,0,6"]);
    }

    #[test]
    fn faces_near_each_other_share_the_coarse_cell_and_only_close_ones_the_fine_cell() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 4.0).with("h", 7.0), &[]);
        let cells = |name: &str| keys(&cx, "cell", &block, name);

        // `back` stands at (0, 2, 3.5) and `right` at (2, 0, 3.5): within one coarse cell
        // of 2.25, several fine cells of 0.5625 apart.
        assert_eq!(cells("back")[0], cells("right")[0]);
        assert_ne!(cells("back")[1], cells("right")[1]);
        assert_ne!(cells("top")[0], cells("bottom")[0]);
    }

    #[test]
    fn round_off_below_a_cell_wall_does_not_move_a_face_to_the_next_cell() {
        let cx = context();
        let block = made(&cx, "box", Params::new().with("w", 4.0).with("d", 4.0).with("h", 7.0), &[]);
        let mut request = face(&cx, &block, "top");
        request.anchor.point = Point3::new(-1e-15, 0.0, 7.0);

        let Attempt::Done(found) = cx.call::<Keys>(&Id::new("cell"), &request) else { panic!() };

        assert_eq!(found.keys, ["cell:L1:0,0,3", "cell:L2:0,0,12"]);
    }
}
