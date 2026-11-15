# Mesh CLI and MCP adapters

The standalone `krly3d-mesh-cli` and `krly3d-mesh-mcp` packages adapt the mesh
operation v2 contract to a one-request CLI and an MCP stdio session. Both load one
native `.k3d` model at startup and use the same engine and operation runner. The model
is fixed for that process lifetime.

## CLI

Build or install the `krly3d-mesh-cli` package; its executable is `krly3d-mesh`.
`--describe` prints the shared v2 descriptor and schemas as one JSON object; `--help`
prints usage.

Pass one v2 request on stdin (the default) or with `--request PATH`:

```sh
krly3d-mesh --model models/plate-with-hole.k3d --request - <<'JSON'
{
  "request_id": "plate-mesh-1",
  "protocol": {"major": 2, "minor": 0},
  "api": {"major": 2, "minor": 0},
  "timeout_ms": 30000,
  "input": {"node": "plate"}
}
JSON
```

The request has a nonempty `request_id`, protocol and API `2.0`, and an `input.node`;
`timeout_ms` is optional (null or omitted means no deadline; zero requests an
immediate deadline). Input defaults and options are described by `--describe`.
When stdin carries the request, **stdin EOF completes that single request**; it does
not cancel the operation.

Stdout is NDJSON: one canonical `started` event, followed by one terminal event
(`completed`, `failed`, or `cancelled`). There are no progress events or partial mesh
results. Diagnostics go to stderr. Exit status is `0` for completion, `1` for rejected
input, operation failure, or output error, and `130` for cancellation. SIGINT requests
cooperative cancellation. A disconnected stdout also cancels work; if output is
unreadable or blocked, delivery of a final event is not guaranteed.

The CLI accepts UTF-8 model files up to 16 MiB and one UTF-8 request up to 1 MiB,
including whitespace.

## MCP stdio

Configure the MCP client with a separate server process. For example, use absolute
paths appropriate to the installation:

```json
{
  "mcpServers": {
    "krly3d-mesh": {
      "command": "/path/to/krly3d-mesh-mcp",
      "args": ["--model", "/path/to/models/plate-with-hole.k3d"]
    }
  }
}
```

The server speaks MCP `2025-11-25` over stdio; an MCP client handles initialization,
framing, and tool calls. **Client stdin EOF ends the MCP session** and drops its runner,
requesting cancellation of its outstanding work. This is different from CLI stdin EOF,
which only ends the one request body.

The available tools are:

- `mesh` starts an operation and returns text JSON `{"operation_id":"<id>"}`
  immediately. Its `arguments` are the same canonical v2 request shown above.
  Rejected calls set `isError`. The MCP request ID and the domain `request_id` are
  independent.
- `operation_events` drains events available now; it does not wait. For example,
  `arguments: {"operation_id":"<id>"}` returns text JSON of the form
  `{"events":[...]}`. An empty array means no event is available yet. Failed or
  cancelled events set `isError`; inspect the terminal event to learn the outcome.
- `operation_cancel` takes the same `operation_id` arguments and returns text JSON
  `{"operation_id":"<id>","cancel_requested":true}`. This acknowledges the
  request; it does not establish which terminal outcome won.
- `operation_release` takes the same arguments and returns
  `{"operation_id":"<id>","released":true}` after releasing a finished operation
  and any unread events. If its worker is still exiting, retry after it finishes.

Tool results carry these JSON payloads as MCP text content. Operation handles are
domain handles, not MCP Tasks. Use `operation_cancel` to cancel work after `mesh` has
returned; MCP cancellation notifications for that completed tool call do not cancel
the operation. A disconnected or saturated MCP output ends the session and cancels
its owned work.

Each MCP JSON-RPC message is limited to 1 MiB, including the tool arguments. Both
executables enforce a 16 MiB model limit. MCP input and output use separate bounded
pumps; at most eight responses queue behind the active writer. Saturating that queue
ends the session and cancels its work. EOF cancels work immediately and allows up to
one second for queued replies to drain. On Unix, stdout hangup is checked even while
stdin is idle. Diagnostics use bounded, best-effort delivery so a full stderr pipe
cannot prevent process shutdown.

## Scope and smoke run

These adapters do not add progress events or partial mesh output. They do not change
the legacy socket listener, renderer, or peer forwarding; legacy socket/render fixes
remain separate work.

After building both binaries, run the SDK smoke check:

```sh
cargo build -p krly3d-mesh-cli -p krly3d-mesh-mcp
uv run tests/mesh_adapter_smoke.py
```

The smoke script exercises a real MCP Python SDK client and compares CLI/MCP geometry
results. Run results should be recorded separately; this reference does not imply
that an integration run has passed.
