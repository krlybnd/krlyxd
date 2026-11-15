---
name: synthesize-and-verify
description: Runs available bounded CAD synthesis and verifies candidate plans against typed design requirements. Use when explicitly asked to synthesize, compare or verify a CAD construction or review revision.
disable-model-invocation: true
---

# Synthesize and verify

Read the [theory contract](../../../docs/plans/optimal-cad/theory-contract.md). Apply this workflow to actual repository capabilities; the contract alone does not provide a solver.

## Inputs and outputs

Input: a validated `DesignSpec`, versioned grammar/capabilities/tolerances/objective, finite search bounds and optional current `FeaturePlan` plus accepted `ReviewDelta`.

Output: candidate `FeaturePlan` when available, requirement-level verification evidence, search coverage/termination, objective comparison and revision-bound artifact references when execution succeeds. Report missing runtime capabilities explicitly instead of presenting a manually selected recipe as synthesized.

## Procedure

1. Locate the real validator, synthesis entry point, kernel adapter and verifier. Check their accepted versions and operation subset. If synthesis is absent or the spec uses unsupported predicates, stop that dependent path with a concrete `unsupported` diagnostic; retain any useful input checks already performed.
2. Validate input, references and operation preconditions before kernel work. Use the same generic entry point for every object. Never dispatch by fixture name, family label or a fixed sequence hidden in a skill.
3. Run deterministic bounded search using fixed grammar, derivation rules, budgets and profiling snapshot. Record candidate counts, unresolved/skipped candidates and termination reason. An LLM may interpret diagnostics; it must not choose a construction in place of the declared mathematical search.
4. Verify hard requirements before scoring. Check material geometry, required topology, dimensions, clearance and preserved intent using supported methods. Mass properties are rejection filters, not shape-equivalence proof. Record exact samples and error bounds; sampled tests do not prove interval guarantees.
5. Among feasible candidates, compare lexicographically: unauthorized semantic change, semantic edit distance, versioned modeling cost, stable canonical key. Preserves remain hard gates. Flag any remaining unrequested change for review; feature-count savings cannot compensate for a failed requirement.
6. Before publishing, require the configured verification gate and current revision/hash match. Preserve the accepted predecessor for undo. Cancellation, stale results and kernel failures cannot publish a successful replacement or become cached geometric impossibility claims.

## Report statuses independently

- Input: `valid`, `invalid`, `unsupported`, `needs_clarification`.
- Verification: `proven` with method/scope, `tested` with samples, `failed` with counterexample, or `unknown` with unresolved obligations.
- Search: `feasible_found`, `infeasible_within_bounds`, or `unknown`. Bounded infeasibility requires exhaustive coverage and decisive checks.
- Optimality: `best_observed`, `proven_within_bounds`, or `not_established`. State exact grammar/bounds and coverage; an unresolved cheaper candidate prevents an optimum proof.
- Lifecycle: one terminal outcome after execution: `completed`, `failed`, `cancelled`, or `deadline_exceeded`. Completion does not imply geometric correctness.

A deadline with a verified candidate can report `best_observed`. A deadline without one means search `unknown`, not infeasibility. Attach input/plan hashes, policy versions and kernel/capability versions so results can be replayed.

## Runtime boundary

Existing krlysci supports checked declarations and limited numeric/vector/matrix/curve equation execution; it is not the proposed dimensional CAD evaluator or general program synthesizer. Verify newly added adapters from their code and tests before using them. If only a finite candidate comparison exists, report that scope faithfully; do not claim open-ended synthesis, robust topology naming or universal optimality.
