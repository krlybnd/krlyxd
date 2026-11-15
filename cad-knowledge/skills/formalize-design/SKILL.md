---
name: formalize-design
description: Formalizes CAD requirements and review corrections into typed DesignSpec or ReviewDelta proposals. Use when explicitly asked to formalize dimensions, geometry, evidence, edit intent or an ambiguous CAD correction.
disable-model-invocation: true
---

# Formalize a design or correction

Read the [theory contract](../../../docs/plans/optimal-cad/theory-contract.md) before producing a proposal. Use its types and semantics; do not invent a parallel schema or choose construction operations from an object name.

## Inputs and outputs

Input: user requirements, evidence with source locations/units, available capability and tolerance versions; for edits, the current spec/plan hashes, model revision, semantic selection and accepted intent.

Output: a proposed `DesignSpec` for new work or revision-bound `ReviewDelta` for edits, plus typed diagnostics and unresolved obligations. Identify the actual serializer/schema version if one exists. Otherwise label the output a contract-level proposal, not executable runtime input.

## Procedure

1. Separate observed facts, user requirements and uncertain interpretations. Preserve source hashes, locators and original units. Do not fill missing dimensions from object-family assumptions or average conflicting sources.
2. Assign stable parameter, datum, requirement and region IDs. Check quantity dimensions, angle/count distinctions, coordinate frames and explicit units. Record admissible domains separately from current values.
3. Express final geometry, relations and edit invariants without inventing a feature sequence or recipe. If the user explicitly asks for a construction method, record it as a typed process constraint and check backend support. A source drawing or mesh is evidence; it does not by itself establish a fully constrained target or intended feature history.
4. For edits, bind the selection to the current model/revision. Use `replace` for changed requirements and explicit parameter updates with expected old values. Retain unaffected requirements; record preserves and necessary allowed effects. Reject stale bases rather than silently rebinding topology.
5. Clarify only missing facts that affect meaning: unresolved selection, direction, unit, contradictory dimensions or competing interpretations. Reuse accepted decisions within their recorded scope. Do not ask again merely because the same operation recurs.
6. Validate the proposal against the available typed validator. Report input as `valid`, `invalid`, `unsupported` or `needs_clarification` with failing IDs and locations. If no matching validator exists, say validation is unavailable; do not certify a hand-written proposal as runtime-valid.

Example: “increase this height by 2 mm; keep the lower datum and opening diameter” requires a resolved height parameter and current value. Propose its typed increment, preserve the named datum/diameter and authorize the changed upper extent. Do not translate this request into an assumed extrusion command.

## Runtime boundary

The theory contract specifies proposed geometric types and review semantics. Existing krlysci equation evaluation does not implement those types or CAD synthesis. Successful `.typ` parsing, numeric evaluation or generated schema output cannot certify that a geometric predicate was interpreted. Unsupported constructs must remain visible diagnostics; never hide them in formula text that the current parser may only partially consume.
