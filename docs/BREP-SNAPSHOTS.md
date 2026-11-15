# B-rep snapshots

The first B-rep migration slice separates immutable source ownership from mesh conversion. The [typed view renderer](VIEW-API.md) now also consumes pinned source ownership. [ADR 0005](decisions/0005-brep-snapshot-boundary.md) defines the temporary bridge and remaining work.

## Packages

- [B-rep API](../products/krly3d/brep/api): engine-independent Rust snapshot identity metadata, using raw `krlyxd` types only.
- [B-rep engine](../products/krly3d/brep/engine): native model loading, edit/checkpoint/rollback delegation, unique owner identity and immutable geometry resolution over the existing kernel.
- [Mesh engine](../products/krly3d/mesh/engine): consumes B-rep snapshots; keeps tessellation, mesh tolerance, artifacts and operation execution.

No B-rep CLI/MCP tools, wire descriptor, serialized geometry or event stream are advertised by this slice. `krly3d.brep.snapshot/1` names the Rust metadata contract.

## Local composition

```rust,ignore
use krly3d_brep_engine::BrepEngine;
use krly3d_mesh::{MeshCall, MeshInput, RequestId};
use krly3d_mesh_engine::MeshEngine;

let brep = BrepEngine::from_k3d(model_text)?;
let pinned = brep.snapshot("part")?;
let source = pinned.identity().source.clone();
let mesh = MeshEngine::from_brep(brep);
let result = mesh.call(MeshCall::new(RequestId::new("mesh-1"), MeshInput::new("part")))?;
assert_eq!(result.provenance.source, source);
```

Resolving `part` fixes the latest version at that moment, such as `part@1`. Explicit version references resolve through the same kernel rules. A snapshot retains its original geometry, build key and services after edits, rollback or owner destruction. The mesh prepared-call and worker paths use that pinned input even if the live document changes.

Identity contains an owner/session ID, normalized node/version and build key. It is provenance, not a portable content hash or a promise of geometric equivalence. Copying an identity record does not construct a snapshot or grant access to another owner; snapshots have private fields. `owns` tests association with a B-rep owner. It is not an authorization mechanism.

The snapshot's `legacy_shape` and `legacy_context` accessors explicitly expose the transitional engine boundary. Mesh conversion still calls the existing standard query/tessellator through that boundary. Snapshot creation does not tessellate or claim achieved accuracy. Controlled mesh execution and v2 runner semantics remain as documented in [mesh operations](MESH-OPERATIONS.md).

The mesh bridge revision is `krly3d.mesh.brep/2`. Existing request/result schemas stay unchanged; source IDs are opaque and now originate with the B-rep owner. Clients must not infer a portable model identifier from their string prefix.

## Remaining migration

#26 retains typed B-rep operation/geometry conversion contracts, standalone domain adapters and the view wire contract. The existing kernel/std and preview Context/Body facade remain active. No whole source file was retired by extracting snapshot responsibilities; archive only genuinely superseded files in [.tmp](../.tmp/README.md).
