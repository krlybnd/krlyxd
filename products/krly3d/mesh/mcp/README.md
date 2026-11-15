# Mesh MCP stdio adapter

Run `krly3d-mesh-mcp --model PATH`. The file must be UTF-8 `.k3d` input of at most
16 MiB. Loading delegates to `MeshEngine::from_k3d` and the existing model reader,
including expectation validation. No geometry implementation lives here.

The adapter supports MCP **2025-11-25**, following the official
[lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle),
[tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools), and
[stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
contracts. Initialize with `protocolVersion`, `capabilities`, and `clientInfo`;
then send `notifications/initialized`. Unsupported version proposals negotiate
2025-11-25. `ping` is available during initialization. Only tools are advertised;
mesh operation handles are independent of MCP tasks and MCP request IDs.

## Tools and payloads

Every tool result contains one `text` content item with JSON. The `mesh` tool's
`inputSchema` is the **complete operation v2 call schema**, cloned from the mesh
API descriptor. Pass `request_id`, `protocol`, `api`, optional `timeout_ms`, and
`input`. Defaults and validation come from the domain codec. For example:

```json
{"request_id":"box-1","protocol":{"major":2,"minor":0},"api":{"major":2,"minor":0},"input":{"node":"part","triangles":true}}
```

- `mesh`: returns `{"operation_id":"…"}` immediately after runner acceptance.
- `operation_events`: takes `{"operation_id":"…"}` and drains available events
  without waiting. Returns `{"events":[…]}`; an empty array means none available
  now, including a retained operation whose terminal event was already drained.
  Each event is copied directly from the canonical encoder; no number tokens or
  metadata are rewritten. A failed or cancelled event sets `isError: true`.
- `operation_cancel`: takes the same handle and returns
  `{"operation_id":"…","cancel_requested":true}`. Repeated cancellation succeeds;
  events report which terminal outcome won. The acknowledgement is not an error.
- `operation_release`: takes the same handle and returns
  `{"operation_id":"…","released":true}` once the terminal worker has exited.
  A worker still computing or unwinding returns a tool error immediately; retry
  later. Release discards unread events and permits reuse of the domain request ID.

Domain rejection/runner errors return `isError: true` with text JSON
`{"diagnostic":{"code":"…","message":"…"}}`. Malformed MCP calls and unknown
tools use JSON-RPC errors. Notifications never invoke tools or receive responses.
MCP cancellation notifications do not cancel previously returned domain handles.

## Connection bounds and shutdown

Input messages must be individual newline-terminated UTF-8 JSON objects, at most
1 MiB before the newline (CRLF is accepted). Batches, null/fractional/boolean IDs,
duplicate object keys (including escaped aliases), unknown call fields, invalid
domain inputs, and unsupported operation versions are rejected. Recursion is
bounded by serde_json's parser limit. Raw arguments go directly to the strict
mesh codec, avoiding integer rounding before validation. String/integer MCP IDs
are echoed independently of domain correlation IDs. Clients must use unique MCP
request IDs within a session as required by MCP.

A bounded reader pump and writer pump keep the coordinator independent of pipe
syscalls. The input/control channel holds four messages plus one frame in the
reader; output holds at most eight queued responses plus one being written. If
output saturates, the connection terminates with a diagnostic rather than growing
an unbounded queue. MeshRunner separately bounds retained operations and events.
No tool waits for computation, a terminal event, or worker cleanup.

EOF, input errors, broken output, or output saturation **drop MeshRunner**, which
requests cooperative cancellation of all owned work. On EOF, cancellation occurs
before allowing queued responses up to one second to flush; a blocked writer is
detached. Unix stdio polls stdout hangup/error every 20 ms while idle. It uses an
owned duplicate of stdout, so Rust runtime stdout-lock cleanup cannot wait on the
writer. Stderr diagnostics use an owned duplicate with a 20 ms best-effort wait.
These intervals govern transport polling/grace, not plugin cancellation latency.
Non-Unix builds detect output failures on writes; idle output closure detection
and descriptor-based runtime cleanup are Unix-specific.

## Verification

```sh
CARGO_TARGET_DIR=/tmp/krlyxd-wave5-mcp-target cargo test -p krly3d-mesh-mcp --locked
CARGO_TARGET_DIR=/tmp/krlyxd-wave5-mcp-target cargo clippy -p krly3d-mesh-mcp --all-targets --locked -- -D warnings
```

Unit tests cover negotiation, descriptor equality, strict JSON and domain
validation, canonical float/error preservation, controlled real-mesh nonblocking
drains and ping, cancel/release, and runner cancellation on EOF, broken output,
and EOF during a blocked output syscall. Process tests exercise real `.k3d`
loading, geometry/artifacts/metadata, deadline errors, lifecycle operations,
busy sphere cancellation/EOF, output closure with stdin open, framing limits,
and CLI argument failures. The integrator owns the separate official SDK smoke.
