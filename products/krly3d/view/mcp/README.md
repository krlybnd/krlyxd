# krly3d-view-mcp

Standalone MCP 2025-11-25 server and binary. Load one immutable model with
`krly3d-view-mcp --model PATH`; `--help` prints usage to stderr. Stdout contains
only newline-delimited JSON-RPC messages. Model files must be UTF-8 and at most
16 MiB. Incoming messages are limited to 1 MiB before the newline. Oversize or
unterminated frames terminate the connection; invalid JSON or UTF-8 produces a
parse error. Output size follows the view API's image dimension bounds.

Complete `initialize` followed by `notifications/initialized`, then use
`tools/list` or `tools/call`. The single read-only tool is `view_render`. Its
arguments are the **complete v1 RenderCall envelope**, using the view API's call
schema directly:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"view_render","arguments":{"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},"request_id":"preview","input":{"node":"part","width":80,"height":60,"backend":"raycast","camera":{"kind":"named","name":"iso"},"tolerance":{"value":0.05,"unit":"model"}}}}}
```

Only `input.node` is required inside `input`; omitted rendering options use
`RenderInput::new` defaults. One node is rendered per call. The domain
`request_id` is independent of the MCP request ID. Both raycast and mesh rendering
use the existing `ViewEngine::call`; this adapter owns no geometry algorithms or
mesh adapter dependencies. Original argument JSON reaches the domain decoder
without a `Value` roundtrip. Duplicate keys, including escaped aliases anywhere
in a nested object, are rejected before dispatch.

Success returns canonical wire-encoded RenderResult JSON as text content, followed
by MCP image content containing the same base64 PNG. The text includes versions,
request ID, dimensions, faces, quality, provenance, and timing. There is no
`outputSchema` or `structuredContent`. Domain decode/render errors return text
containing a diagnostic and `isError: true`; protocol failures use JSON-RPC errors.

The server is deliberately synchronous: rendering and output writes **block the
request loop**, including pings and notifications. EOF is observed only after the
current render and response finish. No operation handles, progress, MCP tasks,
timeouts, or cancellation capability are advertised or implemented. Notifications
never trigger rendering. The process uses the default SIGINT behavior; Ctrl-C
terminates it. There are no background transport or render workers in the adapter.
