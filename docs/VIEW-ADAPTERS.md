# View wire 1.0 and standalone adapters

The separate [view v2 operation contract](VIEW-OPERATIONS.md) now supplies bounded
background rendering, deadlines, cancellation and CLI/MCP operation tools. This page
describes the preserved v1 synchronous path.

The [API codec and descriptor](../products/krly3d/view/api/src/wire) are shared by
[CLI](../products/krly3d/view/cli) and [MCP](../products/krly3d/view/mcp).
Both call the existing [typed renderer](VIEW-API.md); adapters contain no geometry.
[ADR 0007](decisions/0007-view-wire-adapters.md) records this incremental scope.

## Build and call

```sh
cargo build -p krly3d-view-cli -p krly3d-view-mcp
printf '%s\n' '{"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},"request_id":"preview","input":{"node":"part","width":256,"height":192,"backend":"mesh"}}' |
  target/debug/krly3d-view --model models/plate-with-hole.k3d
```

CLI accepts `--model PATH [--request PATH|-]`; request defaults to stdin. `--describe`
prints the canonical descriptor and JSON Schemas, `--help` prints usage. One successful
call writes one complete JSON result line; errors go to stderr and exit with status 1.
Model and request inputs are bounded at 16 MiB and 1 MiB respectively and require UTF-8.
The model path is chosen by the process owner, never by tool arguments.

Start MCP with `target/debug/krly3d-view-mcp --model models/plate-with-hole.k3d`.
It uses MCP 2025-11-25 over newline-delimited stdio; complete initialization before
calling `view_render`. Tool arguments are the same full envelope as the CLI input.
`tools/list` derives its input schema directly from the API descriptor. Success contains
canonical result JSON text followed by the same PNG as an MCP image. Errors set
`isError`; malformed transport requests use JSON-RPC errors. Input messages are bounded
at 1 MiB. Stdout contains protocol messages only.

## Contract

Both `protocol` and `api` require `{ "major": 1, "minor": 0 }`, with a nonempty
`request_id`. `input.node` is required. Defaults: 480×360, raycast, iso camera,
mesh requested tolerance 0.05 model units. Each side must be between 1 and 2048.

Camera forms:

- `{ "kind": "named", "name": "iso" }`; names: iso, front, back, left, right, top, bottom.
- `{ "kind": "orthographic", "toward": [1,2,-3], "up": [0,0,1] }`.
- `{ "kind": "pinhole", "eye": [100,-100,80], "toward": [-1,1,-0.8], "up": [0,0,1], "focal": 80 }`;
  optional `centre` is a two-number pixel coordinate, omitted for automatic centring.

Tolerance is `{ "value": 0.05, "unit": "model" }`. The codec rejects unknown and
duplicate fields, null options, unsupported versions/units, invalid vector shapes,
non-finite numbers and integer tokens outside ±(2^53−1). Integer dimensions/version
components require integer JSON tokens. Decimal/exponent quantities explicitly request
floating-point approximation. Schema describes structure; API/renderer also check
semantic constraints, e.g. nonzero/nonparallel camera directions and model tolerance.

Output contains `protocol`, `api`, `request_id`, `width`, `height`,
`image: { mime_type: "image/png", encoding: "base64", data: "..." }`, visible-face
coverage, quality, provenance and separate render/PNG durations in milliseconds.
Requested tolerance never substitutes for unknown achieved error; uncertainty and
confidence remain null. Provenance IDs are session-local, not durable content hashes.
The API validates output numeric metadata before JSON serialization. PNG is an opaque
payload in the API; actual image decoding is checked through renderer/adapter tests.

## Deliberate limitations

This version is synchronous: no operation handle, progress, partial image, wire timeout,
MCP cancellation or interruptible transport is advertised. MCP rendering and writes
block its request loop; queued notifications/pings wait until the call finishes. EOF or
closed output does not cooperatively cancel an in-flight render. SIGINT retains the OS
process-termination behavior. Rust `execute_controlled` remains available independently.
Long-lived asynchronous work uses the separate [view v2 operation contract](VIEW-OPERATIONS.md),
which now supplies deadlines, cancellation and disconnect cleanup. The mesh v2 runner
remains unchanged.

Multi-node scenes, paint, pick, likeness and removal of legacy geometry bridges are
also pending. Active legacy files remain in place; no whole file was superseded here.

## Reproduce cross-surface verification

```sh
cargo test -p krly3d-view-api -p krly3d-view-cli -p krly3d-view-mcp
cargo build -p krly3d-view-cli -p krly3d-view-mcp
uv run tests/view_adapter_smoke.py
```

The [official SDK smoke](../tests/view_adapter_smoke.py) pins the same MCP SDK revision
as the mesh smoke, validates descriptor schemas, compares six ray/mesh × camera cases
between CLI and MCP, checks PNG chunks/decompression and retries after rejected calls.
Only session identity and timing are normalized for cross-process comparison.
