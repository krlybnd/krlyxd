# View operation protocol/API 2.0

The [operation contract](../products/krly3d/view/api/src/operation.rs) and
[renderer runner](../products/krly3d/view/renderer/src/runner.rs) add background execution
to the existing one-node renderer. The [synchronous v1 contract](VIEW-ADAPTERS.md)
remains unchanged. [ADR 0009](decisions/0009-view-operations.md) records the boundary.

## Call and events

Use protocol and API `{ "major": 2, "minor": 0 }`, `request_id`, the existing render
`input`, and optional `timeout_ms` (null/omitted means no deadline; zero expires
immediately). Timeout is measured from submission, before snapshot preparation.
Only integral JSON-safe unsigned values are accepted. Unknown and duplicate fields
are rejected through the same raw-field parser as v1; v1 still rejects timeouts.

Admission pins the source once and returns a process-unique `view-op:...` identifier.
Exactly two events are retained: `started` (sequence 0, pinned provenance) followed by
one `completed`, `failed`, or `cancelled` (sequence 1). Each envelope has protocol/API
2.0, request and operation IDs. A completed event embeds the unchanged v1 render result,
including PNG, source-face coverage, sampled quality and timing. No progress or partial
images are advertised. The event encoder preserves the result's numeric tokens.

Cancellation and terminal publication share a lock; the first terminal decision wins.
Cancelled work retains its live-worker permit until computation actually exits. A
request ID remains reserved after draining events until explicit release. Release
requires both a terminal decision and worker exit. Unknown/released IDs return
`not_found`. Deadlines are cooperative and become `failed/deadline_exceeded`.

Default limits: four live workers, 64 retained operations, 256-byte request IDs,
64-byte operation IDs, 4096-byte node input, two queued events and 16 MiB canonical
encoded bytes per operation. These bound retained queues, not peak process memory,
render allocations or temporary serialization. Large valid images may exceed retention
and produce `resource_limit`; reduce dimensions. Fallback terminal space is reserved
at admission. Panics are contained and worker permits returned during cleanup.

## CLI

```sh
cargo build -p krly3d-view-cli -p krly3d-view-mcp
printf '%s\n' '{"protocol":{"major":2,"minor":0},"api":{"major":2,"minor":0},"request_id":"preview","timeout_ms":5000,"input":{"node":"part","width":256,"height":192,"backend":"mesh"}}' |
  target/debug/krly3d-view --operations --model models/plate-with-hole.k3d
```

`--describe-operations` prints the v2 descriptor and schemas. Operation mode flushes
each canonical NDJSON event; completed exits 0, failed/cancelled exits 1 with a stderr
diagnostic. Admission errors have no events. Write failure drops the runner and requests
cancellation. Blocked output cannot detect a disconnection until the write returns;
SIGINT retains default OS termination, not a guaranteed terminal event. The ordinary
single-shot CLI and `--describe` continue using v1.

## MCP

The stdio server exposes the original `view_render` plus:

- `view_start`: full v2 call envelope, returns `operation_id`.
- `view_next`: `{ "operation_id": "..." }`, drains one canonical event as text, or
  text `null` when no event is queued. Consumers track the terminal event themselves.
- `view_cancel`: same ID object, requests cancellation idempotently while retained.
- `view_release`: same ID object, discards a terminal operation after worker exit;
  retry if computation is still leaving its final checkpoint.

Computation no longer blocks ping or operation tools. EOF, malformed frame/read errors
and output errors drop the runner, request cancellation and detach workers without
waiting indefinitely. Writes still block the transport loop under backpressure.
Legacy `view_render` remains synchronous and cannot receive notifications while running.
MCP `notifications/cancelled` is not bound to operation IDs; use `view_cancel`.
No MCP tasks/progress extension is advertised. Schema/version/error handling remains
shared with the API; adapters contain no geometry calculations.

## Evidence and remaining scope

Rust tests cover pinned detached owner lifetime, ray/mesh PNG/face/quality parity,
zero deadlines, retained-byte overflow and retry, admission/retention limits,
cancellation races with worker-capacity retention, runner-drop cancellation and CLI
terminal exit codes. The [official SDK replay](../tests/view_adapter_smoke.py) checks
v1 six-case parity plus v2 CLI/MCP parity for both backends, JSON Schemas, ping during
work, deadlines, cancellation, release/reuse and actual stdio EOF while rendering.

This completes the standalone one-node view lifecycle slice of #26. Multi-node scenes,
paint/pick/likeness in the standalone contract, legacy event binding, remote peer
cancellation (#16), partial artifact producers (#24) and remaining domain migrations
are still open. Existing geometry providers remain active; no whole file is retired.
