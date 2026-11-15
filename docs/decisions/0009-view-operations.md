# ADR 0009 — Bounded standalone view operations

Status: accepted, 2026-11-14.

The synchronous view v1 adapter blocks input during rendering. Add a separate v2
operation contract without changing v1 request or result semantics. Keep public
calls/events/codecs/schema in `products/krly3d/view/api`, detached pinned rendering
and bounded admission/event retention in `renderer`, transport composition in CLI/MCP.

The view runner follows the proven mesh lifecycle policy: Started plus one terminal,
cooperative injected-clock deadlines/cancellation, reserved fallback bytes, explicit
release, and worker permits that outlive cancellation. View and mesh retain typed
domain runners; this change does not introduce a generic executor or move rendering
into the raw foundation. Their orchestration follows the same pattern but is not a
shared implementation. Rendering, projection, tessellation and PNG encoding still use
the existing single algorithm providers. A future executor extraction must preserve
both domains' independently verified publication and capacity semantics.

The CLI uses opt-in `--operations`; MCP adds four nonblocking operation tools alongside
`view_render`. Completed events embed v1 results. EOF/output failure cancels retained
operations through runner destruction. Backpressure can still block writes; legacy
synchronous rendering and default signal handling remain explicit limitations.

No progress or partial image producer is invented. Active legacy sources remain until
consumer migration completes; no file qualifies for `.tmp/` archival in this slice.
See [operation contract and verification](../VIEW-OPERATIONS.md).
