# Mesh operations: developer reference

Mesh operations add background execution with cancellation and deadlines while preserving the synchronous mesh contract. Protocol/API v1 remains a synchronous call returning a mesh result; it has no operation handle, event stream, cancellation, or supported timeout. The separate operation API uses protocol/API v2, returns an `OperationId`, and reports results through events. Both paths use the same mesh input, snapshot preparation, tessellator, and result representation. See the [v2 operation contract](../products/krly3d/mesh/api/src/operation.rs) and [lifecycle decision](decisions/0002-mesh-operation-lifecycle.md).

The [standalone CLI/MCP adapters](MESH-ADAPTERS.md) now expose this contract. The CLI
streams canonical events; MCP returns a handle immediately and lets the client drain
events, cancel and release through separate tools. Model loading is shared through
`MeshEngine::from_k3d`. See [ADR 0003](decisions/0003-mesh-transport-adapters.md) for
transport ownership and the distinction from the unchanged legacy socket listener.

## Request and events

A v2 request has a nonempty `request_id`, `protocol` and `api` set to `{"major":2,"minor":0}`, optional `timeout_ms`, and the usual mesh `input`. Timeout is milliseconds from submission; omitted or `null` means no deadline, and `0` is accepted as an immediate deadline. For example:

```json
{
  "request_id": "mesh-42",
  "protocol": {"major": 2, "minor": 0},
  "api": {"major": 2, "minor": 0},
  "timeout_ms": 30000,
  "input": {
    "node": "housing",
    "tolerance": {"value": 0.05, "unit": "model"},
    "triangles": false
  }
}
```

The input also permits the normal v1 defaults: tolerance `0.05 model` and `triangles: false`. Decode JSON with [`decode_operation_call`](../products/krly3d/mesh/api/src/operation.rs); encode an `OperationCall` with `encode_operation_call`.

An accepted operation queues exactly two events: `started` at `seq: 0`, then one terminal event at `seq: 1`: `completed`, `failed`, or `cancelled`. There are no progress events. A deadline reports `failed` with diagnostic code `deadline_exceeded`; a result too large for the queue reports `resource_limit` rather than being truncated. The queue reserves space for a terminal event before accepting work. Event encoding and the complete event shape are defined by [`encode_event`](../products/krly3d/mesh/api/src/operation.rs).

## Runner entrypoints

The transport-independent runner is [`MeshRunner`](../products/krly3d/mesh/engine/src/runner.rs). Its lifecycle methods are:

- `start(&self, engine: &MeshEngine, call: OperationCall) -> Result<OperationId, Diagnostic>` validates the request, prepares and pins the engine snapshot, and admits a worker. A rejected request has no handle or events.
- `next(&self, id: &OperationId) -> Result<Option<MeshEvent>, Diagnostic>` drains one queued event without waiting. `None` means the queue is empty, including after the terminal event was drained.
- `wait_next(&self, id: &OperationId) -> Result<Option<MeshEvent>, Diagnostic>` waits for an event or for a terminal operation whose queue has been drained. It waits on the same queue as `next`.
- `cancel(&self, id: &OperationId) -> Result<(), Diagnostic>` requests cancellation. Terminal publication and cancellation are serialized; the first terminal decision wins. Repeating cancel on a retained operation is harmless.
- `release(&self, id: &OperationId) -> Result<(), Diagnostic>` removes a terminal operation only after its worker has exited. It also discards unread events. Draining the terminal event alone does not release the record.

Unknown or released operation IDs return `not_found`. Dropping the runner requests cooperative cancellation of retained work and does not join workers or forcibly interrupt plugin calls. A worker still unwinding after cancellation continues to occupy a live-worker slot until its cleanup runs. See the [`MeshRunner` methods and worker lifecycle](../products/krly3d/mesh/engine/src/runner.rs) and the engine's [snapshot preparation and detached task](../products/krly3d/mesh/engine/src/lib.rs).

## Limits, IDs, and snapshots

Default [`RunnerLimits`](../products/krly3d/mesh/engine/src/runner.rs) are four live workers and 64 retained operations per runner; at most two queued events and 16 MiB of canonical encoded event bytes per operation; request IDs up to 256 bytes; operation IDs up to 64 bytes; and input node strings up to 4096 bytes. The runner currently counts UTF-8 string bytes for these limits. Exceeding admission or event limits yields `resource_limit`.

A request ID remains reserved for as long as its operation is retained, including after completion and event draining. Reusing it before `release` is rejected. Release permits reuse; request IDs are therefore retained-request deduplication keys, not durable idempotency records. There is no automatic eviction or replay.

Before accepting work, the engine resolves the requested node/version and pins an immutable model snapshot. Later edits or rollback do not retarget that operation. The task uses detached execution state and shared immutable registries/caches; artifact identity allocation remains shared with synchronous calls.

The event-count and canonical queued-byte limits bound retained operation/event state. They do not bound solver peak memory, temporary allocations, or memory used while constructing and encoding a candidate event. See the [lifecycle decision](decisions/0002-mesh-operation-lifecycle.md) for the contract rationale and the [development status](DEVELOPMENT-STATUS.md) for verification status.
