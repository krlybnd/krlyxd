# krly3d-brep

`krly3d-brep-cli` provides the standalone `krly3d-brep` binary. Each invocation
decodes one planar request with the public API, builds a new document with the
public B-rep engine, and writes the canonical API result as one JSON line.

```sh
krly3d-brep < request.json
krly3d-brep --request request.json
krly3d-brep --request - < request.json
krly3d-brep --describe
krly3d-brep --help
```

The request is the complete planar envelope:

```json
{"protocol":"krly3d.brep.planar/1","request_id":"part-1","input":{"polygons":[[[0,0],[6,0],[6,4],[0,4]]],"height":2,"length_unit":"model","source":"example"}}
```

Stdin EOF completes the request. Input must be UTF-8 and at most 1 MiB
(1,048,576 bytes), including whitespace. The bound applies to bytes actually
read from stdin or a file; reading stops at one byte beyond the inclusive limit.
`--help` and `--describe` must be used alone. The descriptor is the unmodified
shared API descriptor, including its call schema. Duplicate options, extra
arguments, and unknown options are rejected.

Success exits 0 and writes exactly one JSON result and newline to stdout. The
result includes the native k3d model, `node: "part@1"`, protocol, request ID,
source, provenance, and quality. Errors exit 1 with a single
`{"code":"...","message":"..."}` diagnostic on stderr and no result on stdout.
An output I/O failure can leave stdout incomplete. Help is plain text on stdout.
The adapter writes no files.

Execution is synchronous with no in-flight request cancellation, timeouts,
operation handles, or background workers. SIGINT retains the default OS
behavior and may terminate the process without a JSON diagnostic or result.
