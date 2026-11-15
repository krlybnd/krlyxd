# krly3d-brep-mcp

Run `krly3d-brep-mcp` with no arguments to serve MCP `2025-11-25` over stdio.
`--help` prints to stderr; stdout is reserved for newline-delimited JSON-RPC.
Every call builds a new document. No model is loaded at startup, no document is
retained between calls, and the adapter writes no files.

Complete `initialize` and then `notifications/initialized` before `tools/list`
or `tools/call`. `ping` works before initialization. The sole tool is
`brep_planar`; its `inputSchema` is the public planar descriptor's `call_schema`.
Arguments are the complete request envelope, independent of the JSON-RPC ID:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"brep_planar","arguments":{"protocol":"krly3d.brep.planar/1","request_id":"part-1","input":{"polygons":[[[0,0],[6,0],[6,4],[0,4]]],"height":2,"length_unit":"model","source":"example"}}}}
```

Success returns one text content item containing canonical API result JSON,
including the native k3d model, `node: "part@1"`, source, provenance, and quality,
with `isError: false`. Domain decoding or building failures return diagnostic
JSON text with `isError: true`. Protocol failures use JSON-RPC errors. There is
no output schema or structured content; canonical result text is never parsed
and re-encoded by this adapter. Original argument JSON reaches the API decoder
without a `Value` roundtrip. Duplicate keys, including escaped aliases in nested
objects, are rejected recursively before dispatch.

Incoming frames must be newline terminated UTF-8 JSON, with at most 1 MiB
(1,048,576 bytes) before the newline. CRLF is accepted; the CR counts toward the
bound. Oversized or unterminated frames terminate the connection with a stderr
message and nonzero exit. Invalid JSON or UTF-8 returns a parse error and the
next frame can proceed. EOF between frames exits successfully. Responses are
flushed individually.

Execution is synchronous: building and output block all further input,
including pings and notifications. There is no in-flight request cancellation,
timeout, progress, MCP task, operation handle, or worker. Notifications never
trigger builds. Cancellation notifications are ignored; EOF is observed after
the current call and response finish. SIGINT retains default OS behavior.

The strict JSON utility, lifecycle, and bounded framing are adapted from the
existing view MCP adapter with provenance comments in source. Dependencies are
limited to the B-rep API/engine and serialization crates; no view or mesh adapter
or geometry implementation is reused.
