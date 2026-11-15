# 0005 — B-rep snapshot ownership

2026-11-14. First B-rep migration slice under [#26](https://github.com/krlybnd/krlyxd/issues/26), following [ADR 0004](0004-mesh-preview-boundary.md).

## Ownership and placement

The public Rust identity record lives in `products/krly3d/brep/api` (`krly3d-brep`), depending only on raw `krlyxd` primitives. `SnapshotIdentity` describes session, resolved node version, build key and source identity. `krly3d.brep.snapshot/1` identifies this in-process metadata; it does not advertise a serializable geometry format, wire operation or event stream.

`products/krly3d/brep/engine` (`krly3d-brep-engine`) owns the native document loader, session identity, version resolution and opaque geometry snapshot. A snapshot pins geometry and originating execution services. Callers may copy identity metadata but cannot construct or rebind a snapshot handle. Snapshots survive edits, rollback and owner destruction. Independent owners remain distinguishable even for identical documents. Session identity is process-local provenance, not a portable digest or a security capability.

The mesh engine composes this owner instead of defining its own model/version resolution. Existing mesh constructors, mutation methods, prepared calls, runner and transport contracts remain. `MeshEngine::from_brep` accepts an existing owner without assigning another identity. Mesh artifact allocation and mesh tolerance checks remain mesh responsibilities. The mesh engine revision advances to `krly3d.mesh.brep/2` to identify the B-rep-owned provenance bridge; v1/v2 request/result schemas are unchanged. One native parser, tessellator and cache implementation serve both paths.

## Temporary bridge and limits

The engine's explicitly named `legacy_shape` and `legacy_context` snapshot accessors expose the active geometry implementation during migration. The B-rep API itself has no legacy imports. The engine still delegates algorithms to legacy kernel/std, and std transitively imports mesh data types; this is an allowed, tested bridge, not the completed target dependency graph. No new B-rep CLI/MCP tools or progressive events are advertised.

Standalone B-rep operation/geometry contracts and view API/adapters remain in #26. The preview renderer's Context/Body entry point is not removed by this slice. Future migration replaces these bridges only with tested consumers; moving all legacy geometry now would obscure this boundary.

## Verification and retirement

Require immutable snapshot, explicit/latest version, ownership, owner-drop and diagnostic tests; preserve mesh cancellation/runner tests, CLI/MCP parity and dependency gates. The API must remain engine/adapter-independent and the B-rep engine must not import mesh engine, view or transports.

No whole file is retired by this extraction: the mesh engine still owns execution and artifacts. Active legacy geometry stays in place. Only actually superseded files belong in root [.tmp](../../.tmp/README.md), with origin and replacement recorded; no archive copy of each edited source revision is needed.
