# krly2d native API revision 1

This crate stores immutable planar geometry. Its only dependency is the shared
`krlyxd` contract. Package identity/version come from Cargo through `metadata()`;
the native API version is `API_VERSION = ApiVersion { major: 1, minor: 0 }`.
The shared metadata's `engine_revision` is `krly2d.native/1`, identifying this
representation rather than an executing engine.

```rust
pub type Point2 = [f64; 2];

Path::new(start: Point2, segments: Vec<Segment>, closed: bool)
    -> Result<Path, krlyxd::Diagnostic>;
Region::new(outer: Path, holes: Vec<Path>, unit: krlyxd::LengthUnit)
    -> Result<Region, krlyxd::Diagnostic>;
Region::from_polygons(polygons: Vec<Vec<Point2>>, unit: krlyxd::LengthUnit)
    -> Result<Region, krlyxd::Diagnostic>;
```

Segment construction uses named fields:

```rust
Segment::Line { to: Point2 }
Segment::QuadraticBezier { control: Point2, to: Point2 }
Segment::CubicBezier { control1: Point2, control2: Point2, to: Point2 }
Segment::EllipticArc {
    radii: Point2,
    x_axis_rotation: f64,
    large_arc: bool,
    sweep: bool,
    to: Point2,
}
```

Each segment starts exactly at the path start or previous endpoint. A closed path
adds an implicit straight edge back to its start. Ellipse radii are positive
semi-axis lengths; rotation is counterclockwise in radians in the XY plane.
`large_arc` selects a span greater than pi; `sweep = true` selects increasing
ellipse parameter. Parameters are retained without normalization or correction.
Providers determine geometric realizability; coincident arc endpoints do not
encode a complete ellipse.

`Path` has private owned fields and getters `start() -> Point2`,
`segments() -> &[Segment]`, and `closed() -> bool`. `Segment::end() -> Point2`
returns the endpoint. Open paths may contain no segments. Closed line-only paths
require at least three vertex entries, counting the start. `Region` requires
closed paths and exposes `outer() -> &Path`, `holes() -> &[Path]`, and
`length_unit() -> krlyxd::LengthUnit`. Both `Model` and `Metre` are retained
explicitly; no physical scale is inferred or converted.

Coordinates and controls are finite-only, including all finite large values,
subnormals, signed zero and repeated points. Arc radii are positive-finite with
no magnitude floor or ceiling. Rotation is finite and bounded by
`MAX_ABS_ANGLE = 1e6` radians. `MAX_SEGMENTS = 65_536` bounds explicit segments
per path and across a region; `MAX_HOLES = 1_024` bounds holes. Implicit closing
edges are additional and bounded by the loop count. Constructors check counts
before conversion allocations, and stored slices discard spare input capacity.
They cannot bound allocations callers have already made in supplied vectors.

`Region::polygons() -> Result<Vec<Vec<Point2>>, krlyxd::Diagnostic>` returns
independent copies, outer first and then holes in supplied order. Line-only
round trips preserve every coordinate bit and vertex entry, including an
explicit repeated closing point. Any curve produces diagnostic code
`unsupported` before output allocation. Invalid structural values use `input`;
count limits use `resource_limit`.

Structural validation does not establish topology or physical accuracy.
Self-intersection, touching, containment, winding, degeneracy and numerical
operation limits remain the responsibility of existing BRep providers. No
parser, geometry algorithm, engine or curve flattening is implemented here.
