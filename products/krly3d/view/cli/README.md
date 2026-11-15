# krly3d-view

`krly3d-view-cli` provides the standalone `krly3d-view` binary. It reads one native
`.k3d` model and one view protocol/API 1.0 JSON request, then synchronously calls
the public view renderer. Build it from the workspace with
`cargo build -p krly3d-view-cli`.

```sh
krly3d-view --model model.k3d --request request.json
krly3d-view --model model.k3d < request.json
krly3d-view --model model.k3d --request - < request.json
krly3d-view --describe
krly3d-view --help
```

`--model` requires a file path. `--request` defaults to stdin, where EOF completes
the request. Both inputs must be UTF-8. The inclusive size limits are 16 MiB
(16,777,216 bytes) for the model and 1 MiB (1,048,576 bytes) for the request,
including whitespace. Reading stops after at most one byte beyond a limit.
Options can appear in either order; duplicate options, positional arguments and
unknown options are errors. `--describe` and `--help` must each be used alone.

A minimal request uses the same defaults as `RenderInput::new`:

```json
{"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},"request_id":"image-1","input":{"node":"part"}}
```

Defaults are 480 by 360 pixels, raycast backend, fitted named `iso` camera, and
tolerance `{"value":0.05,"unit":"model"}`. Optional input fields are `width`,
`height`, `backend`, `camera`, and `tolerance`. `backend` is `raycast` or `mesh`.
Camera forms are `{"kind":"named","name":"iso"}`,
`{"kind":"orthographic","toward":[0,1,0],"up":[0,0,1]}`, and
`{"kind":"pinhole","eye":[0,-20,3],"toward":[0,1,0],"up":[0,0,1],"focal":80}`
with optional pixel `centre`. `--describe` prints the shared API descriptor,
including `call_schema` and `output_schema`, as one JSON object.

A successful render exits 0 and writes exactly one result JSON object followed
by a newline to stdout. Its `image` contains `mime_type: "image/png"`,
`encoding: "base64"`, and PNG bytes encoded in `data`. The result also includes
dimensions, request and version identifiers, face coverage, quality, provenance,
and timing. Diagnostics have the form `{"code":"input","message":"..."}` on
stderr, exit 1, and no result on stdout. An output I/O failure may leave stdout
incomplete. `--help` prints plain usage text and exits 0.

Wire v1 is synchronous: it has no events, cancellation operation, or timeout
field. The CLI installs no signal handlers; SIGINT uses default OS handling and
may terminate the process immediately without a JSON diagnostic or result.
There is no operation runner, worker thread, or nonblocking transport layer.
