# 0002 — Mesh operation lifecycle

2026-11-14 · Implemented locally; standalone adapters are now covered by [ADR 0003](0003-mesh-transport-adapters.md). Continues [ADR 0001](0001-kernel-and-domain-contract.md) and the [controlled computation foundation](../MESH-API.md#local-controlled-execution).

## Compatibility decision

Keep the synchronous mesh API/protocol 1.0 and its descriptor unchanged. Add a separate `operation` API/protocol 2.0 for submitting background work and encoding events. Its request carries the same mesh input and defaults, plus an optional timeout. Zero timeout is an accepted call whose deadline is immediately reached, not an invalid input. Unsupported versions are rejected before acceptance.

This is an explicit major-version boundary because acceptance now returns a handle and execution has a lifecycle. It does not silently reinterpret the synchronous timeout field. Both paths use the same prepared snapshot, tessellator, statistics, quality, and result representation. No CLI/MCP protocol is added by this decision.

## Minimal real event stream

The pilot emits `Started` followed by exactly one of `Completed`, `Failed`, or `Cancelled`. Sequence numbers are 0 and 1. There is no progress estimate, warning stream, provisional artifact, or preview claim. With triangles enabled, completion holds the existing final artifact revision 1.

Only two events can exist per operation, so no producer waits for a consumer or needs progress coalescing. Both event count and canonical encoded byte size are bounded. Space for terminal failure/cancellation is reserved before acceptance; an oversized result is a `resource_limit` failure, never truncated. These limits bound retained operation/event state, not the tessellator's total temporary memory or allocations during serialization.

## Session and ownership

Validation resolves and pins the model before starting a worker. The worker owns a context view sharing immutable plugin registries and derived caches; it does not borrow the mutable engine/session. Edits and rollback after acceptance cannot retarget the operation. Artifact identity allocation remains shared with synchronous calls.

The runner owns its retained handles. Duplicate request IDs are rejected while their operation is retained. Explicit release removes a finished record and permits reuse of that request ID; request IDs are correlation keys, not a durable idempotency store. There is no automatic eviction or durable replay. Unknown or released handles report `not_found`.

Cancellation and terminal publication are serialized. A cancellation that wins produces `Cancelled`; completion or failure that wins remains final. Repeated cancel on a retained terminal handle succeeds without changing its outcome. A deadline produces `Failed(deadline_exceeded)`. Work still unwinding after cancellation continues to consume a worker slot; reporting a terminal event alone must not free execution capacity.

Dropping the runner requests cooperative cancellation of owned work. It cannot forcibly interrupt arbitrary plugin calls. Network disconnect detection and half-close behavior remain adapter responsibilities and are not implemented here.

## Verification gates

Check real box results and metadata, stable snapshots across edits/rollback, request rejection without a handle, live/retained capacity, idle consumers, event bytes, repeated cancel, terminal races, timeout zero, worker panic, release/reuse, and no events after terminal. Preserve synchronous contract tests and the recorded legacy CLI responses. Record actual limits and full integration results in [development status](../DEVELOPMENT-STATUS.md) when verified.

Verified integration: 821 workspace tests, including the doctest phase, and strict workspace Clippy pass. The [development status](../DEVELOPMENT-STATUS.md) records the full checks and remaining adapter work.
