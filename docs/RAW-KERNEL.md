# Raw kernel contract (`krlyxd`)

`krlyxd` is the domain-neutral home for shared identifiers, numerical primitives, clocks, exactness, and cooperative per-call stopping. It is a small foundation, not a geometry operation runner. See the [kernel boundary and mesh pilot decision](decisions/0001-kernel-and-domain-contract.md).

## What moved; what remains

Moved from `krly3d-core` are the single implementations of `Id`, `Clock`/`SystemClock`/`FakeClock`, `Exactness`, six `nalgebra` aliases, and the Gauss–Legendre quadrature table and iterator. The seed also defines `ApiVersion`, `RequestId`, `OperationId`, `ArtifactId`, `Diagnostic`, `Length`/`LengthUnit`, and `CallControl`/`CallStop`.

`Accuracy`, frame and NURBS routines depend on core's domain error type and deliberately remain
in the geometry-support layer, as settled in [ADR 0008](decisions/0008-raw-foundation-contract.md).
They are not pending raw exports; their eventual B-rep compatibility migration is #26. Core also retains `Error`/`Attempt`, plugin/schema support, claims, and document/replay types. Bodies, topology, mesh data or algorithms, sessions, CLI/MCP, serialization, and execution policy are outside this seed.

## Public Rust paths

With `krlyxd` in the consumer's dependencies, import the root records and math modules as follows. The definitions are in [lib.rs](../crates/krlyxd/src/lib.rs), [math/mod.rs](../crates/krlyxd/src/math/mod.rs), and [quadrature.rs](../crates/krlyxd/src/math/quadrature.rs).

```rust
use krlyxd::{
    ApiVersion, ArtifactId, CallControl, CallStop, Clock, Diagnostic, Exactness, FakeClock,
    Id, Length, LengthUnit, OperationId, RequestId, SystemClock,
};
use krlyxd::math::{Iso3, Mat4, Point3, Sim3, Uv, Vec3};
use krlyxd::math::quadrature::{nodes, GAUSS_LEGENDRE_8};
```

Equivalent module paths include `krlyxd::id::Id`, `krlyxd::clock::{Clock, FakeClock, SystemClock}`, `krlyxd::control::{CallControl, CallStop}`, and `krlyxd::exact::Exactness`. `Vec3`/`Point3` are 3D vectors/points, `Uv` is a 2D parameter point, `Mat4` is a 4×4 matrix, `Iso3` a rigid placement, and `Sim3` a uniformly scaled similarity.

```rust
use krlyxd::{ApiVersion, Id, Length, LengthUnit};
use krlyxd::math::{Point3, Vec3};

fn example() {
    let api = ApiVersion { major: 1, minor: 0 };
    let tolerance = Length { value: 0.05, unit: LengthUnit::Model };
    let physical = Length { value: 1.0, unit: LengthUnit::Metre };
    let kind = Id::new("plane");
    let origin = Point3::new(0.0, 0.0, 0.0);
    let direction = Vec3::new(0.0, 0.0, 1.0);
    let _ = (api, tolerance, physical, kind, origin, direction);
}
```

`LengthUnit::Model` means the consuming model's coordinate units; `Metre` explicitly means metres. `Length` performs no conversion, selects no model scale, and validates neither values nor compatibility. Area and volume semantics belong to domain consumers.

## Compatibility paths in `krly3d-core`

Existing imports remain available and resolve to the same Rust types, without wrappers or
conversions:

- Root: `krly3d_core::{Id, Exactness, Clock, FakeClock, SystemClock}`.
- Modules: `krly3d_core::id::Id`, `krly3d_core::exact::Exactness`, and
  `krly3d_core::clock::{Clock, FakeClock, SystemClock}`.
- Math: `krly3d_core::math::{Vec3, Point3, Uv, Mat4, Iso3, Sim3}` and
  `krly3d_core::math::quadrature::{nodes, GAUSS_LEGENDRE_8}`.

Core reexports `CallControl` and `CallStop` for the controlled geometry bridge. It does not reexport the new envelope or length records. Compatibility exports are in [core lib.rs](../crates/core/src/lib.rs), [math/mod.rs](../crates/core/src/math/mod.rs), [id.rs](../crates/core/src/id.rs), [clock.rs](../crates/core/src/clock.rs), [exact.rs](../crates/core/src/exact.rs), and [quadrature.rs](../crates/core/src/math/quadrature.rs).

## Cooperative cancellation and deadlines

`CallControl::new()` has no deadline. `with_deadline(Arc<dyn Clock>, Duration)` uses an absolute deadline in the injected clock's time base. `check()` reads cancellation first, then returns `Err(CallStop::DeadlineExceeded)` when `clock.now() >= deadline`. If cancellation is set when read, it returns `Err(CallStop::Cancelled)` even when the deadline has elapsed. A concurrent cancel racing after that read is not serialized against the deadline check. See [control.rs](../crates/krlyxd/src/control.rs).

```rust
use std::sync::Arc;
use std::time::Duration;
use krlyxd::{CallControl, CallStop, FakeClock};

fn example() {
    let clock = Arc::new(FakeClock::default());
    let control = CallControl::with_deadline(clock.clone(), Duration::from_secs(5));
    clock.advance(Duration::from_secs(5));
    assert_eq!(control.check(), Err(CallStop::DeadlineExceeded));
    control.cancel();
    assert_eq!(control.check(), Err(CallStop::Cancelled));
}
```

This is cooperative signaling: callers place `check()` at bounded work checkpoints and map the stop themselves. Sharing `Arc<CallControl>` lets another thread request cancel, but the crate creates no thread, task, scheduler, or runtime and causes no automatic interruption. `publish` serializes a short caller-provided publication closure against `cancel`; cancellation that wins prevents the closure from running. The raw crate still owns no cache or operation records. Publication closures must not re-enter the same control or block on work that needs cancellation.

## Dependency gate and status

The only direct production dependency of `krlyxd` is `nalgebra`; its transitive production closure is restricted to the reviewed numeric set. Core's [one_copy.rs tests](../crates/core/tests/one_copy.rs) check unique definitions, compare legacy/raw `TypeId`s and quadrature behavior, and inspect Cargo metadata for the dependency gate. The crate's [public_api.rs tests](../crates/krlyxd/tests/public_api.rs) cover seed records and cooperative control. The current integration evidence is recorded in [development status](DEVELOPMENT-STATUS.md); the foundation contract is distinct from the remaining domain rollout.

The [typed mesh API and synchronous engine bridge](MESH-API.md) consume this seed as separate domain crates. Controlled mesh execution, the [v2 lifecycle](MESH-OPERATIONS.md) and [standalone adapters](MESH-ADAPTERS.md) are implemented outside raw. Domain requests/results, snapshot bridging, terminal/cache policy and CLI/MCP integration do not belong to the raw seed.


## Coordinates, quality and producer metadata

[metadata.rs](../crates/krlyxd/src/metadata.rs) defines `Coordinates`, `Handedness`,
`Axis`, `PackageMetadata` and `ContractMetadata`. `Coordinates::RIGHT_HANDED_Z_UP`
is the convention of the current mesh/view contracts. Other conventions are representable;
these records never transform coordinates or infer model-to-metre scale.

[quality.rs](../crates/krlyxd/src/quality.rs) defines `Confidence::new(f64)` (finite [0,1])
and `PositionalUncertainty::{unknown, measured}`. Unknown is distinct from measured zero;
measured values preserve their explicit length unit and must be finite/nonnegative.
A domain must explain confidence meaning/calibration. Requested tolerance is not evidence
of achieved uncertainty. Mesh/view reuse these validators without changing their wire shapes.

Aggregate quality/provenance stay domain-owned: view needs camera/backend, mesh needs
meshing settings, and B-rep owns pinned node/session metadata. Raw IDs and producer identity
are shared building blocks, not portable hashes or an all-domain source model.
`PackageMetadata` accepts structural package identifiers and nonempty version tokens;
Cargo is authoritative for SemVer. `ContractMetadata` adds protocol/API versions,
engine revision, coordinates and length unit, not operation schemas or runtime capabilities.

The [central metadata gate](../crates/core/tests/package_metadata.rs) validates workspace
Cargo package identities and the actual mesh v1/v2/view v1 descriptor metadata/version
agreement. No parallel package manifests or CUE files are introduced. Raw still depends
only on nalgebra, and domain/error support stays outside its production graph.
See [ADR 0008](decisions/0008-raw-foundation-contract.md) for the complete ownership and
conversion decision and [development status](DEVELOPMENT-STATUS.md) for verification.
