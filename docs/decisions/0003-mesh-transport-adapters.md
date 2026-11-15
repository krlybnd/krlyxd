# 0003 — Separate mesh CLI and MCP adapters

2026-11-14 · Continues [the v2 lifecycle](0002-mesh-operation-lifecycle.md).

## Shared computation and composition

The pilot has separate `krly3d-mesh-cli` and `krly3d-mesh-mcp` packages. Both decode
the canonical v2 `OperationCall` and use `MeshRunner`; neither implements geometry.
Both composition roots load an existing native `.k3d` document through the engine's
`MeshEngine::from_k3d` helper, which uses the existing parser, standard plugin registry
and document validation. Refused expectations fail model loading. The model is fixed
for the lifetime of each adapter process; these adapters do not introduce editing tools.

The engine retains its explicit temporary legacy B-rep bridge. Production dependency
checks keep adapters out of the raw kernel, mesh API and mesh engine. The monolithic
geometry and vision CLIs retain their existing behavior.

The new MCP envelope parser retains raw argument tokens and rejects duplicate keys
before invoking the shared domain codec. It does not route typed mesh requests through
the legacy schema/`Params` conversion, which would lose the strict v2 envelope and
numeric-token guarantees. Mesh validation, event encoding and schemas remain single
implementations in the domain API.

## CLI binding

`krly3d-mesh --model PATH [--request PATH]` accepts one canonical v2 request. Without
`--request`, stdin supplies the request and EOF terminates that input, normally.
`--describe` exposes the v2 descriptor. Stdout carries canonical NDJSON events, with
Started followed by the winning terminal event. Exit codes are 0 for completion,
1 for rejection/failure, and 130 for cancellation. SIGINT requests cooperative
cancellation. A disconnected output reader cancels owned work; a stalled output must
not prevent the process from responding to SIGINT. Unreadable/disconnected streams
cannot be guaranteed to receive a terminal event.

## MCP binding

The server binary `krly3d-mesh-mcp --model PATH` uses MCP stdio, negotiating
`2025-11-25`. The domain's API/protocol version remains 2.0: these are different
version namespaces. The tool list derives the mesh call schema from the API descriptor.

- `mesh`: accepts a canonical v2 call and returns `{ "operation_id": "…" }` immediately.
- `operation_events`: accepts `operation_id` and drains available canonical events as
  `{ "events": […] }` without waiting for the computation. An empty array means no
  event is available; the terminal event's kind identifies completion.
- `operation_cancel`: requests cooperative cancellation and acknowledges it. Inspect
  the event stream for the winning outcome; an acknowledgement is not proof that
  cancellation won over completion.
- `operation_release`: releases a finished record. It may need to be retried while
  a cancelled worker is still unwinding. It also discards unread events.

Tool payloads are canonical JSON carried in text content. Failed/cancelled events and
domain rejections set `isError`; cancellation acknowledgement itself does not. MCP
request IDs and domain request/operation IDs are independent. Operation handles are
not MCP Tasks, and no Tasks capability is advertised. MCP cancellation notifications
for already-answered calls do not cancel domain work; use `operation_cancel`.

Input frames are limited to 1 MiB and model files to 16 MiB. Separate bounded input
and output pumps keep the coordinator available during blocked writes. Eight replies
may queue behind the writer; saturation closes the session and cancels its work.
EOF drops the runner before allowing up to one second for queued output to drain.
Unix stdout hangup is observed even when stdin is idle. Detached I/O workers use
owned output descriptors so Rust's global stdout cleanup cannot block shutdown.
Diagnostics are best effort, with a bounded wait. These transport limits are separate
from the runner's retained-event limits and do not bound solver peak memory.

This pull binding is verified using the official Python SDK by
[the executable acceptance smoke](../../tests/mesh_adapter_smoke.py). It compares CLI
and MCP results while normalizing only process-local source/artifact IDs. It also
checks schema discovery, errors, deadlines, duplicate requests, repeated cancellation,
release and subsequent session use. Run evidence belongs in
[development status](../DEVELOPMENT-STATUS.md); this is not a claim of universal client
or full protocol conformance.

Protocol references: [MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools),
[MCP stdio](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
and [official Python SDK](https://github.com/modelcontextprotocol/python-sdk).

## Remaining scope

These are the mesh pilot's standalone process adapters. They do not migrate the legacy
Unix-socket listener, renderer or peer forwarding. In particular, normal socket write
half-close versus full disconnect remains a separate legacy transport test/fix under
#2. This delivery does not close #2 or #16. The original single-pilot acceptance of
[#21](https://github.com/krlybnd/krlyxd/issues/21) is complete; remaining domain contracts
and event capabilities are tracked in [#26](https://github.com/krlybnd/krlyxd/issues/26). Real
progress, early partial mesh, HTTP transport, other domain adapters and reconstruction
remain future work.
