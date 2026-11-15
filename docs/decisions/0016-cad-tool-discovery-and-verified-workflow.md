# 0016 — CAD tool discovery and verified feature workflows

Date: 2026-11-15. Status: feature-workflow foundation implemented; MCP discovery
and file-backed retrieval are proposed, not yet integrated into Studio.

## Problem and evidence

The Studio adapter exposed a narrower, separately maintained operation schema
than the native kernel. An open-end wrench request became touching boxes and
failed a native Boolean. A subsequent polygon extrusion was numerically valid
but had the wrong shape and a 200 mm extrusion instead of a 200 mm planar length.
Successful execution and closed meshes therefore cannot establish design intent.
The small planner also produced invalid circular arcs and disconnected feature
ancestry. Retrieval alone does not solve those coordinate/planning errors.

The existing [native MCP dialect](../../crates/cli/src/dialect/mcp.rs) already
builds tools/list from the [session registry](../../crates/kernel/src/api.rs),
including parameter descriptions and common statement parameters. A live audit
returned 45 tools and 42,284 bytes of JSON tool definitions. Studio currently
still uses its bounded recipe adapter and internal native processes. The separate
[B-rep MCP product](../../products/krly3d/brep/mcp/README.md) exposes the narrower
planar API; it must not be confused with the full session dialect.

## Decision direction

Agent-visible geometry access should use MCP. Keep Rust schemas and actual
registered capabilities authoritative. Generate parameter names, types, defaults
and required fields directly; do not ask an embedding or language model to invent
or translate executable schemas from prose. Rust documentation contributes
operation scope, coordinate conventions, failure conditions and usage examples.
Tutorial-derived modeling methods are separate, cited records, not evidence of
manufacturer dimensions or standards compliance.

Expose current model state separately: model/revision ID, units, UCS, feature DAG,
body/face/edge identifiers and measurements. Read this state from the live session
through a structured tool/resource. Do not retrieve a stale model snapshot from
an embedding index. Mutations should bind to the expected model revision and
return the new revision and operation/validation record.

### Files, not a database service

A proposed immutable catalog generation contains:

- `manifest.json`: catalog format, engine build/schema digest, source hashes,
  embedding model/revision/dimension and record-to-vector mapping.
- `tools.jsonl`: exact tool schemas plus supported/unsupported scope and examples.
- `methods.jsonl`: short modeling patterns with source links, required operations,
  prerequisites, dimensional assumptions and verification requirements.
- `vectors.f32`: optional normalized dense vectors; original text remains readable.

These are proposed names, not already generated deliverables. Build a generation
in a staging directory, validate it, then atomically publish its manifest. Embed
only changed textual records, keyed by content hash and embedding configuration.
The runtime loads this small corpus into memory; a network vector database is not
needed. Embedding failure must leave lexical retrieval available. Never pair
vectors from different models/dimensions or silently use a catalog for a different
engine schema.

### Retrieval and execution

Start with bounded lexical/BM25 retrieval and domain aliases, including Hungarian
terms. Add dense retrieval for paraphrases only if local evaluation improves
coverage; merge ranks with RRF rather than comparing incompatible raw scores.
For this 45-tool catalog, measure whether query embedding adds value before making
it mandatory. Cache repeated query results by catalog digest.

Do not stop at the top few individually similar tools. Expand selected method
records through explicit prerequisites and compatibility constraints. A pocket
needs a sketch, an existing support solid, plane/depth semantics and validation.
The host supplies the resulting small set of original tool schemas. If the set
cannot fit its context budget, narrow the task explicitly; never drop a required
operation silently. A discovery tool itself is an application tool, not a new MCP
protocol method.

A bounded workflow is: interpret request/known dimensions → retrieve method and
capabilities → author feature DAG → validate/execute → measure geometry and review
intent → publish recorded construction history. Execute a validated dependent
feature batch without an LLM call for every line or arc. Known geometric failures
get one targeted repair; infrastructure failures and cancellation do not trigger
paid retries. Failed attempts must not replace the last accepted model.

## Implemented foundation and limits

The Studio recipe now has separate XY line/arc sketches, extrusion, top-plane
blind/through-all pockets and native supported fillets. Existing primitives and
polygon extrusion remain available. The native builder emits actual solid
snapshots; v2 construction metadata carries analytic sketch geometry and feature
ancestry. This is not a general sketch constraint solver, arbitrary surface fillet
implementation or DWG/STEP importer.

Numerical native checks and a separate bounded text-only semantic review precede
publication. The semantic review is an LLM opinion, recorded as reviewed, not a
proof of fit, strength or standard certification. It sees the request, selected
answers, recipe and measured bounds/volume/area, without images, history or a new
web search. At most two native attempts share a deadline. Curated tutorial notes
currently provide a small bridge; generated MCP discovery must replace duplicated
capability knowledge rather than add a second competing authority.

Acceptance for retrieval work must include complete required tool-set coverage,
unsupported-operation rejection, exact parameter/units fidelity, stale-catalog
rejection, bilingual queries, and total token/cost/latency measurements. Also test
end-to-end native success and user-dimensional fit: retrieval hit rate alone is
insufficient. No retrieval integration or successful live AI-authored curved wrench
is claimed by this decision record.

## Primary sources

- [MCP tools specification](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
  and [resources](https://modelcontextprotocol.io/specification/2025-11-25/server/resources):
  tool schemas versus application-provided context.
- [Anthropic advanced tool use, 2025-11-24](https://www.anthropic.com/research/advanced-tool-use):
  on-demand tool loading, examples and tradeoffs for small catalogs.
- [SCOUT, 2026-08-25](https://arxiv.org/abs/2608.23992): hybrid BM25/dense retrieval,
  RRF and selective exposure in a much larger tool inventory. Its savings are not
  a forecast for this project.
- [Completeness and Cost, 2026-10-30](https://arxiv.org/abs/2610.41905): multi-tool
  prerequisite coverage is a separate problem from single-tool retrieval quality.
  This is a recent preprint, not a universal guarantee for CAD.
- [CADSmith, 2026-03-27](https://arxiv.org/abs/2603.26512): execution repair,
  programmatic measurements and separate assessment of intended shape.
- [SolidWorks Instructor's Guide, Lesson 3, printed page 59](https://www.solidworks.com/sw/docs/Instructor_WB_2011_ENG.pdf#page=73):
  the wrench exercise uses multiple sketch-based features and edge finishing.
- [FreeCAD Part Design tutorial](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/Basic_Part_Design_Tutorial.md):
  incremental sketch-based additions and cuts.
