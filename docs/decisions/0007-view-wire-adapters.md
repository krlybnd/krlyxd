# ADR 0007 — Synchronous view wire and separate adapters

Date: 2026-11-14. Status: accepted implementation slice of #26.

The typed view boundary from [ADR 0006](0006-typed-view-renderer.md) can render one
immutable node but external callers previously needed Rust or the legacy facade.
Add protocol/API 1.0 codecs and descriptor under the view API, with standalone
`products/krly3d/view/{cli,mcp}` packages calling the existing renderer.

The contract owns one strict call schema, numeric/unit validation, JSON result encoding
and base64 image representation. MCP derives its tool schema and image from that result.
Neither adapter implements camera projection, tessellation, rasterization or PNG encoding.
Dependency gates prevent direct adapter imports of geometry internals.

Choose an explicitly synchronous first wire version, consistent with the existing typed
call. Do not wrap it in fabricated Started/progress events or claim cancellation based
only on Rust-side control. Render and transport writes can block the MCP loop; this
limitation is documented and capabilities explicitly say no cancellation/timeouts/events.
A future controlled operation version must provide genuine lifecycle and disconnect
semantics, preserving this compatibility contract. The mesh v2 pilot is unaffected.

Quality remains sampled, achieved uncertainty/confidence unknown, source identity
session-local. Preserve active legacy providers and the user's `.tmp/` retention policy;
no implementation has become obsolete merely because new adapter directories exist.

Evidence and commands: [view adapters](../VIEW-ADAPTERS.md),
[development status](../DEVELOPMENT-STATUS.md).
