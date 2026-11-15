# ADR 0011 — Local shared runtime with domain-owned immutable artifacts

Date: 2026-11-14. Status: accepted direction; native single-drawing stream implemented; sheet/lifecycle migration ongoing.
Supersedes mandatory internal MCP/process orchestration in #24/#26, not the public
contracts or standalone adapters in ADRs 0001–0010.

## Decision

Use one local process to compose statically linked domain engines through typed Rust
APIs. Share immutable native evidence, B-rep snapshots and mesh results using `Arc`.
Keep mutable domain state under its domain owner. The application host coordinates
commands, revisions, dependencies, admission and result publication. It does not own
another geometry implementation or move domain types into raw `krlyxd`.

The first host lives in `products/krly3d/reconstruction/runtime`, next to its product
flow. Its file adapter lives in `host-cli`. The existing `cli` Python process/sheet
flow remains an active compatibility and parity consumer. CTM keeps assembly and
constraints; it is not renamed into a giant universal object store. A neutral generic
runtime crate can be extracted once a second real host demonstrates the shared need.

### State and publication

- A drawing revision owns immutable measured evidence; a B-rep owner/snapshot is an
  explicit derived artifact; meshing consumes that owner without export/reload.
- Drawing changes invalidate evidence, B-rep and mesh. Height changes retain evidence;
  tolerance-only changes also retain a completed B-rep. No implicit mutable aliases.
- Accept commands under the single mutable host owner. Give each computation an
  immutable input snapshot and host-scoped revision. Compute without a host mutex.
- Publish only if both host identity and requested revision still match. Stale results
  cannot become current. Failed work leaves dependent results invalidated. Existing
  readers can retain an older result, with its original revision and provenance.
- Admission includes unpublished results. Four outstanding jobs/results per host;
  dropping/publishing releases a slot. An edit does not free a still-running worker's
  capacity. Host retention is latest evidence, reusable model, current result and 256
  compact events; caller-held handles extend lifetime explicitly. The domain success-mesh
  cache keeps at most eight distinct tolerances per body in FIFO order, deduplicates
  concurrent fills, and evicts only its own references. This is not a total RAM quota.
- Notifications follow accepted command/publication order. Lagged readers receive
  an explicit gap and resnapshot. No callbacks execute inside host mutation. An
  event sequence is deterministic for a specified command/publication sequence,
  not independent of thread scheduling. It is not a durable replay log.

### AI and persistence

AI watches committed versions and proposes commands with source revisions. Future
proposal acceptance must check those revisions exactly like worker publication.
Record prompt/model/configuration, input hashes, proposal and accepted command for
replay. AI output, wall-clock timing and cross-platform floating-point computation
are not promised to be deterministic. AI cannot mutate shared buffers directly.

Durable state uses explicit schemas, content identity and input snapshots, not pointer
addresses, process-local session IDs or `Arc` handles. OS shared memory across processes
is a later measured option, requiring offsets, synchronization, versioned layout and
crash/lifetime recovery. It is not implemented by this decision.

### Compatibility and limits

CLI/MCP retain versioned wire contracts and are external/remote entry points. Peer-link
batching, asynchronous RPC, cancellation and schema lifetime remain #16 compatibility
work. Removing those transports from the local hot path does not complete that issue.

No general zero-copy claim: contour projection and mesh generation allocate, adapters
serialize, and existing profile providers build native paths. The eliminated copies
are internal JSON evidence conversion and B-rep save/parse between local engines.
Measure identical fixtures and comparable work before claiming speed/memory savings.

The first local host is bounded planar reconstruction, not a generic DAG scheduler,
cross-process shared memory or complete CTM. The seven-stage single-drawing path
is present, while the broader legacy sheet/lifecycle behavior still needs migration.
Native coarse preview, snapshot rendering and station likeness now compose locally.
One shared CallControl guards native stages and final host publication; vision decode
and planar construction currently check only at stage boundaries. The opt-in CLI
stream flushes each stage but has no timeout/cancel command or interruptible writer.
Sheet ownership, full lifecycle, durable command log and AI watcher
execution are explicit subsequent #24/#26/#15 work. Retire a bridge only after its
replacement supplies the behavior and parity evidence; archive genuinely superseded
whole files under root `.tmp/` with original path and replacement recorded.

## Evidence

The runtime tests cover threaded native composition, actual hole/volume/closure,
shared allocation identity across edits, inverse completion order, foreign hosts,
failed work, admission release and event gaps. CLI/process parity uses the same
image, scale, thickness and mesh tolerance, checks evidence and geometry, and reports
timings plus serialized boundary bytes. See [migration plan](../SHARED-RUNTIME.md).
