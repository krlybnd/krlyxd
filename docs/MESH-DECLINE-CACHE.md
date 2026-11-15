# Deterministic mesh decline cache

The active B-rep tessellator in [std mesh](../crates/std/src/interpret/mesh) owns the
negative cache. This is a geometry-engine detail behind the existing mesh API and
legacy bridge; no triangulation internals move into raw or the adapters.

## Identity and reuse

[identity.rs](../crates/std/src/interpret/mesh/identity.rs) records exact, finite,
length-delimited geometry/topology bytes, not a sampled signature or a pointer hash.
Supported built-in curves: line, circle, ellipse, NURBS. Supported surfaces: plane,
sphere, cylinder, cone, torus, ellipsoid, NURBS. The key includes vertices, edges and
ranges, endpoints/labels, face names/orientation/loops, geometric parameters and
stored NURBS closure/projection state actually consumed by the mesher.

Configuration includes characteristic model scale, length/angular resolution,
triangle/refinement/flip budgets and API/engine/artifact/solver revisions. Exact
tolerance bits select an entry within that content/configuration bucket. Equal
content in separately allocated supported bodies shares a decline in one context;
changed diagnostic face names, topology, geometry, accuracy or relevant settings
select a different bucket. Context isolation separates plugin registries.

The namespace is in-process and deliberately conservative: equivalent surfaces with
different parameterizations or floating-point bit patterns need not compare equal.
It is not a persisted cache or a portable content digest. Unknown/custom geometry,
nonfinite stored geometry and identities larger than 256 KiB opt out safely. They
still execute and expose misses; no unverified identity is allowed to retain a decline.

## Retention and correctness

[memo.rs](../crates/std/src/interpret/mesh/memo.rs) retains at most 64 content/configuration
buckets per context. Each bucket uses the existing bounded
[MeshDeclines](../crates/kernel/src/ports/interpret/decline.rs): 64 tolerance entries,
4 KiB maximum diagnostic. Eviction can make a later request execute again; counters
are scoped to the retained bucket, not a persistent lifetime log.

Only deterministic triangulation-budget refusals are stored. Face/tolerance/reason
text is returned verbatim. Unsupported layout, plugin-dependent work, failed control,
timeouts and cancellation remain retriable. A budget failure after consulting trim
plugins is not retained. Successful meshes continue to use the existing success cache.
Publication is guarded against cancellation; concurrent misses may both compute,
so this is not a single-flight scheduler.

`BrepMesh::decline_stats(context, body)` is the public Rust stats path: hit/miss/entry
counts are shared for equal supported content. The domain CLI/MCP response format
is unchanged. There are no new public quality knobs; internal budgets are serialized
because they affect actual triangulation and are covered by invalidation tests.

## Acceptance evidence

[cache_tests.rs](../crates/std/src/interpret/mesh/cache_tests.rs) uses the real mesher
with reduced internal budgets to reproduce refusals cheaply. Independently built
sphere and NURBS loft cases demonstrate reuse without re-entering triangulation.
Further checks cover each configuration namespace, geometry/topology/face names,
accuracy, NURBS effective state, unknown/custom data, bounded identity and plugin
fallbacks. Bounded eviction and independent-context tests verify recomputation and
counter isolation. Existing control tests retain cancellation/publication and retry coverage.

```sh
cargo test -p krly3d-std --lib interpret::mesh
```

Final integration evidence and issue status: [development status](DEVELOPMENT-STATUS.md).
