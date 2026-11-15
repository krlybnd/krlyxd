# Mathematical CAD contract, revision 1

Date: 2026-11-15. Normative design contract for the first bounded synthesis slice; not a claim that the existing runtime implements it. The parent [plan](PLAN.md) supplies product scope and acceptance gates. This document specializes its mathematical and review contracts. `MUST` denotes an implementation acceptance condition. No object name, catalogue family or fixture identifier may select an engine algorithm.

## 1. Separate requirements from construction

A `DesignSpec` describes admissible results and edit intent. A `FeaturePlan` is one candidate program satisfying those requirements. They have distinct validators and content hashes. By default, a spec carries neither a prescribed feature sequence nor a recipe identifier or object-family dispatch key. If the user explicitly requires a construction method (for example, a revolved sketch), that is a typed process constraint with provenance and capability checks; it is not permission to substitute a hidden, object-named recipe. Human labels are metadata excluded from semantic dispatch. An imported existing plan is review context, never a hidden requirement unless its preserved intent is explicitly recorded.

The following full algebraic types are a proposed language-neutral IR. Only the bounded quantity/requirement and static-plan fragment described in the [P0 implementation ledger](p0-typed-contract.md) is currently accepted by krlysci/Typst; the complete type structure below is not yet runtime input:

```text
DesignSpec = {
  schemaVersion, specId, revision, parentHash?,
  parameters: Map<ParamId, Parameter>, datums: Map<DatumId, Datum>,
  derived: Map<SymbolId, TypedExpression>,
  domain: Predicate, requirements: Map<RequirementId, Requirement>,
  evidence: Map<EvidenceId, Evidence>, editIntent: EditIntent,
  tolerancePolicyHash, grammarVersion, capabilitySnapshotHash,
  objectivePolicyHash
}
Parameter = {type, value, admissibleDomain, provenance}
Requirement = {predicate, scope, provenance, verificationObligation}
Evidence = {sourceHash, locator, interpretation, trustState}
FeaturePlan = {
  planId, designSpecHash, grammarVersion, capabilitySnapshotHash,
  nodes: Map<NodeId, TypedOperation>, output: NodeId,
  parameterBindings, requirementWitnesses, referenceProvenance
}
```

`TypedExpression` is a finite, typed AST; it is never an evaluated arbitrary source string. References use immutable IDs. Duplicate IDs, unknown fields, cycles, free symbols, unbound parameters and non-finite values are validation errors. A requirement witness identifies how to inspect an obligation; it is not itself proof of satisfaction. Evidence can be incomplete or conflicting. Merely loading it does not create a hard requirement or certify target geometry.

A plan is a rooted dependency DAG. Every retained node MUST contribute to the output or an explicit verification witness. Every operation has typed inputs, output kind and decidable precondition checks where available. The output is one nonempty, bounded, connected regular closed solid for this slice. A DAG cycle, dangling input, wrong output kind or unsupported operation invalidates the plan before kernel execution.

## 2. Quantities, coordinates and references

`Quantity<d>` has a dimension vector over SI bases, an exact decimal/rational canonical value where possible and an explicit display unit. Length is canonically metres; angles have a distinct `Angle` type, canonically radians, even though SI treats angles as dimensionless. `Count` is a nonnegative integer, distinct from `Scalar`. The first slice accepts finite values only. Unit conversion happens once at the input boundary and once at a backend boundary; provenance retains original text and units.

Typing rules include:

```text
add, subtract : Quantity<d> × Quantity<d> → Quantity<d>
multiply      : Quantity<a> × Quantity<b> → Quantity<a+b>
divide        : Quantity<a> × Quantity<b> → Quantity<a-b>  [divisor ≠ 0]
sin, cos      : Angle → Scalar
sqrt          : Quantity<2d> → Quantity<d>                [value ≥ 0]
Point<F> - Point<F> → Vector<Length,F>
Point<F> + Vector<Length,F> → Point<F>
```

There is no implicit point addition, angle-to-length conversion, count rounding, or coordinate-frame coercion. A direction is a dimensionless unit vector with a checked nonzero normalization input. `RigidTransform<F,G>` explicitly moves a point or direction between frames. A datum plane or axis is defined in a named frame with orientation; “inward” has no intrinsic meaning without a selected region and datum direction.

Geometry types distinguish `Region2<F>`, `Profile<F>` (validated closed boundaries), `Solid<F>`, and `Selection<ModelId,Revision,Kind>`. An edge or face reference also records producing node and semantic selector. Matching a new edge by transient index is forbidden. A reference to another model/revision yields `stale_reference`; rebinding requires an explicit, audited mapping with unique resolution. Topological naming across edits is an obligation, not an assumed kernel capability.

Tolerance policy is versioned and dimensioned: separate absolute length, angle, area and volume bounds plus relative scalar bounds and kernel numerical tolerances. Manufacturing limits remain requirements, separate from numerical tolerance. Numerical closeness uses `|a-b| ≤ absTol[d] + relTol*max(|a|,|b|)` with the operands' dimension. Exact parameter equalities and symbolic identities do not acquire arbitrary epsilon. Tolerance closeness is not transitive and MUST NOT justify unrestricted equivalence-class merging.

## 3. Constraint meaning and verification

For parameters `θ` in declared domain `D`, a program denotes `[[p]](θ)` when evaluation is defined. Feasibility requires typing, valid references and capabilities, operation preconditions and every hard requirement:

```text
∀ θ ∈ D : Defined([[p]](θ)) ∧ C(θ, p, [[p]](θ), evidence)
```

Requirements may describe geometry (membership, bounds, coaxiality, clearance), topology (connectedness, through cavities), parameter relations, edit behavior or an explicitly requested construction method. Process constraints are checked on the plan and the backend capability; they do not arise from a part label. `distance(A,B) ≥ c` requires defined nonempty referenced sets. A missing set is an invalid obligation, never a vacuous pass. `coaxial` checks axis directions and line distance with angle and length tolerances. A minimum wall relation must state the measured regions and distance convention; a global bounding box cannot establish it. A through void needs connectivity to both specified boundary patches, not just a cylindrical-looking mesh.

A geometric target predicate defines the entire material set. Equality to it is a shape requirement and does not prescribe a feature history. Regularized difference means `closure(interior(A \ B))`; nonmanifold or zero-thickness residues violate the output contract. Target boundary comparisons use the declared tolerance policy and verification method. Volume, centroid and inertia are useful rejection filters but cannot prove shape equality. A mesh distance without certified sampling error is an estimate.

`verificationObligation` records one of: exact symbolic proof on a supported fragment; certified bounded numerical check with error bound; or explicit finite test set. A finite test report states the actual parameter samples. It cannot discharge the universal statement over an interval. For this initial runnable slice, fixture `D` is a singleton; separate edit snapshots are separate singleton specs. Interval-valued design guarantees remain proposed until a sound verifier exists.

## 4. Bounded, object-independent synthesis

The proposed first grammar admits datum-aligned planar regions composed from disks and simple polygons with finite inner loops; straight extrusion; revolution of valid meridian profiles about a datum axis; rigid transforms; regularized union/difference; and finite repeated transforms of a solid expression. No fillets, freeform surfaces, arbitrary imported solids as unit-cost leaves, unconstrained sketch solving or inferred persistent edge binding are required. Each backend advertises a versioned supported subset; this list is a proposal, not a claim that all listed operations already execute.

Inputs supply finite scalar/point/angle expressions drawn from typed spec symbols, constants and a versioned derivation rule set. The synthesizer may derive profiles from target predicates in the supported spatial fragment; that extraction is generic AST analysis, not a fixture-name lookup. A target outside the fragment returns `unsupported` with the rejected construct. The bounds include maximum expression size, program nodes, transform cardinality, derivation depth and deterministic candidate count. Coordinates cannot range over arbitrary reals. A time deadline may interrupt but is not a proof boundary.

At a fixed bound, canonical typed enumeration can be exhaustive over this finite language. Candidate pruning needs a sound exclusion reason; a kernel exception excludes only that evaluated candidate with `unknown` geometry, not an entire class as impossible. A beam search can use the same grammar but cannot claim exhaustive optimality. The implementation MUST disclose enumerator strategy, exact grammar/bounds hash, candidate counts, skipped/unknown counts and termination reason. A grammar version MUST NOT introduce an opaque “build fixture X” leaf.

The distinction matters: “no feasible program in this bounded grammar” does not mean the part is impossible. Exhaustive enumeration with complete verification proves a bounded statement only. Discovery of a strategy outside the grammar remains future work.

## 5. Review as a revision-bound delta

```text
ReviewDelta = {
  deltaId, baseSpecHash, basePlanHash, modelId, modelRevision,
  selection: Selection?, interpretationProvenance,
  add: Map<RequirementId, Requirement>,
  replace: Map<RequirementId, {expectedOldHash, newRequirement}>,
  retract: Map<RequirementId, expectedOldHash>,
  parameterUpdates: Map<ParamId, {expectedOldValue, newValue}>,
  preserve: Set<RequirementId | ParamId | SemanticRegionId>,
  allowedEffects: Set<TypedEffect>, scope
}
```

Application is atomic and compare-and-swap against the base hashes. Operation sets must be disjoint; preserved and modified identifiers cannot overlap. A changed requirement uses `replace`; adding contradictory old/new equalities is not a dimension change. Retraction removes a named requirement, not its historical record. Parameter updates preserve their declared types and admissible domains. Any parameter that is also fixed by an explicit requirement needs that requirement replaced in the same transaction. Revalidation detects downstream contradictions before execution.

An accepted delta produces a new immutable spec revision plus a reversible revision record. Unchanged hard requirements remain hard. `allowedEffects` explains necessary consequences, such as translation of a selected void's boundary when its centre changes. It cannot silently permit a different outer contour, diameter or datum. Selection identifies semantic regions/requirements before a new plan exists. A stale or ambiguous selection fails without rebinding. Missing direction information produces `needs_clarification`; prior accepted direction within the same scope resolves it without another question.

The natural-language interpreter may propose this delta. The deterministic validator applies and checks it; an LLM cannot mark a failing predicate as satisfied. Only an actually accepted result enters local intent memory, keyed by model, scope, requirement provenance and policy version. A rejected proposal is not a learned global rule. Undo restores the prior accepted spec and plan atomically; a late result from an older revision cannot publish over a newer one.

## 6. Feasibility before lexicographic preference

All hard requirements, including explicitly preserved intent, are gates. An invalid candidate has no compensating feature-count score. For feasible review candidates minimize lexicographically:

```text
(U(p,p₀,delta), E(p,p₀), J(p), canonicalPlanKey(p))
J(p) = Σᵢ wᵢ φᵢ(p)/scaleᵢ
φ = (featureCount, dependencyDepth, expectedEditCost, profiledRebuildCost)
```

`U` measures weighted semantic changes outside the authorized effect closure, using stable parameter/datum/region identifiers and a versioned finite comparison set. Explicit preserves are hard gates; unprotected effects still have a preference cost. A candidate with `U>0` is flagged for review, never silently declared intention-preserving. `E` is weighted semantic graph edit distance on named parameters, datums, dependency edges and operation roles; it is not source-line edit distance. Feature renaming alone does not count. The policy defines all weights, role correspondence and distance terms before search. New construction has `U=E=0`.

`featureCount` counts reachable emitted semantic features under the versioned grammar, including a pattern as one feature plus its seed construction; helper geometry is accounted for by a fixed policy. `dependencyDepth` is longest emitted-feature path. Expected edit cost is the sum over declared scenarios of probability times cost of the affected DAG closure, each node counted once per scenario. Probabilities are explicit assumptions until supported by data. Scales are strictly positive; weights are nonnegative and sum to one. Rebuild cost comes from a hashed profiling snapshot, never live noisy timing during deterministic selection. The final canonical key breaks exact ties reproducibly but does not prove one construction professionally preferable.

## 7. Result status is a product, not a success boolean

A result carries distinct axes:

- Input: `valid`, `invalid`, `unsupported`, `needs_clarification` with typed diagnostics and source locations.
- Candidate verification: `proven` (method and scope), `tested` (explicit samples/checks), `failed` (counterexample), `unknown` (unresolved obligation). `tested` is not `proven`.
- Search: `feasible_found`, `infeasible_within_bounds`, `unknown`; only exhaustive enumeration and decisive checks for every candidate justify bounded infeasibility. Kernel failures, unsupported verifiers and budget exhaustion leave `unknown` when no feasible candidate is found.
- Optimality: `best_observed`, `proven_within_bounds`, `not_established`. Proof requires complete coverage, decisive verification and fixed deterministic objective over exactly the stated bounds. An unresolved candidate prevents the proof unless a certified cost lower bound shows it cannot beat the winner.
- Execution lifecycle: `pending`, `running`, then exactly one of `completed`, `failed`, `cancelled`, `deadline_exceeded`. Completion describes the computation, not geometric truth. Artifact publication additionally requires the configured verification gate and current revision match.

A timed-out run can retain a previously verified best candidate as `best_observed`; it cannot relabel the deadline as infeasibility. No candidate, failed candidate and unsupported input are different reports. Every report binds spec, plan if any, kernel/capability/tolerance/objective/grammar hashes, budgets and evidence. Replay uses those versions. There is no `optimal:true` field without its bounded proof scope.

## 8. Three declarative specifications

These are normative fixture data in mathematical notation, not executable `.typ` files. Coordinates below use mm consistently; squared quantities therefore use mm². Each names the final material set and independent acceptance facts, with no preassigned features or build order. All share frame `F`, planar datum `z=0`, positive orientation `+z`, singleton initial parameter domain and `singleConnectedRegularSolid`. Values can change only by a new valid spec revision. Labels are deliberately irrelevant to solving.

### A. Axially symmetric hollow spacer

Parameters: outer radius `R=15 mm`, inner radius `r=6 mm`, height `h=8 mm`. Admissible updates satisfy `R>r>0`, `h>0`, `R-r≥2 mm`.

```text
Material(x,y,z) ↔ r² ≤ x²+y² ≤ R² ∧ 0 ≤ z ≤ h
```

Requirements: exact dimensions in `F`; cylindrical boundaries coaxial with `z`; one axial through void; minimum radial wall `2 mm`; the lower datum stays fixed under height edits. Independent facts: bounding box `[-15,15]×[-15,15]×[0,8] mm`; volume `1512π mm³`; centroid `(0,0,4) mm`. These scalar facts supplement, not replace, material-set checking.

Review: “make the height 10 mm” updates `h`, preserves `R`, `r`, axis and lower datum, and authorizes only the changed top/boundary extent. Expected new volume `1890π mm³`, centroid height `5 mm`. Negative: `r=16 mm` violates domain; `h=10°` is a type error; sealing the void fails topology even if another cavity preserves volume.

### B. Asymmetric planar plate with one opening

Parameters: thickness `t=4 mm`; polygon vertices in `F.xy` are `(0,0),(60,0),(60,20),(35,20),(35,40),(0,40) mm`; hole centre `c=(15,15) mm`, radius `a=4 mm`. Let `P` denote the closed simple polygon's interior and boundary. Admissible updates retain polygon simplicity, positive thickness and clearance from the disk to the exterior of at least `2 mm`.

```text
Material(x,y,z) ↔ (x,y) ∈ P ∧ ||(x,y)-c||² ≥ a² ∧ 0 ≤ z ≤ t
```

Requirements: the entire polygonal outer contour; one circular through opening parallel to `z`; bottom datum fixed; minimum wall `2 mm`. Independent facts: area `1900-16π mm²`; volume `7600-64π mm³`; bounding box `[0,60]×[0,40]×[0,4] mm`. No rotational symmetry is asserted.

Review: “move this opening 2 mm along +x, keep its diameter and the outline” uses selection of its semantic void region, updates `c=(17,15) mm`, preserves `a`, `P`, `t` and datum, authorizes that void's translation. Volume is invariant; centre coordinates and clearance are rechecked. Negative: unqualified “inward” with no accepted direction is ambiguous; `c=(1,15) mm` breaks clearance/containment; moving the outer edge to keep the hole centre fixed violates preserve.

### C. Two-level stepped solid with a repeated void set

Parameters: `L=80 mm`, `W=30 mm`, lower height `b=5 mm`, central width `s=20 mm`, extra height `q=7 mm`, void radius `a=3 mm`, pitch `d=25 mm`, count `n=3`. Define `X={10 mm + i*d | i∈{0,…,n-1}}`, `C={(x,15 mm)|x∈X}`. `n` is `Count`; singleton initial domain fixes it to three. Region extents are anchored at `x=y=z=0`.

```text
Envelope(x,y,z) ↔ 0≤x≤L ∧ 0≤y≤W ∧
                  (0≤z≤b ∨ (30 mm≤x≤30 mm+s ∧ b≤z≤b+q))
Material(x,y,z) ↔ Envelope(x,y,z) ∧
                  ∀c∈C : ||(x,y)-c||² ≥ a²
```

Requirements: full envelope; voids pass through all local material parallel to `z`; minimum wall `2 mm`; equal radii; centres have the declared finite translational relation; one connected solid. Admissible updates keep the raised region within the base, all disks inside the footprint with required clearance, disks mutually separated by at least `2 mm`, and each disk wholly inside or outside the raised footprint with `2 mm` clearance (no ambiguous transition case in this fixture domain).

Independent facts: centres are `(10,15),(35,15),(60,15) mm`; only the middle disk lies in the raised region, with nearest raised edge clearance `2 mm`. Volume is `16200-198π mm³` (base `12000`, raised `4200`, removed `9π*(5+12+5)`). Bounding box `[0,80]×[0,30]×[0,12] mm`.

Review: “raise the central region by 2 mm, keep all openings and the base” updates `q=9 mm`, preserves footprint, `b`, void centres/radii and through condition. The central void necessarily becomes longer; that is authorized. New volume `17400-216π mm³`. Negative: preserving its old void depth would conflict with through condition; `n=3.5` is a type error; changing pitch to `20 mm` violates the declared transition clearance.

The `d=20 mm` negative places void centres on the raised footprint boundaries. The same generic clearance and domain predicates reject it; no named-object exception is needed.

## 9. QA obligations and present parser boundary

For each of A/B/C, the harness accepts a spec with no FeaturePlan and invokes the same public synthesis entry point. No expected operation order is supplied. It checks material-set requirements with the declared method, scalar facts above, edit deltas, preserve constraints and replay. Relabeling every spec and reordering its declarations MUST yield the same semantic input hash and candidate choice. A fourth withheld composition using the same language must run without new engine branches; successful execution of the three training fixtures alone is insufficient evidence of generality.

Required negatives also include: length-plus-angle, mixed coordinate frames, missing units, NaN/infinity, unknown predicate, unsatisfied precondition, zero-area/self-intersecting profile, negative extrusion extent if grammar disallows it, dangling and cyclic plan inputs, output of profile kind, stale references, stale delta hashes, conflicting replace/preserve, unsupported backend operation, contradictory exact requirements, verifier `unknown`, candidate-limit exhaustion, equal-cost deterministic tie, and cancel/late-revision publication. Counterexamples carry the failing requirement ID and concrete parameter assignment. Unknown parser input MUST fail closed; no successful parse of a prefix may certify a spec.

Read-only source inspection on 2026-11-15 distinguishes reusable mechanisms from proposed capabilities:

- [elaborate.rs](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/elaborate.rs) uses Typst's concrete syntax tree to collect `#structure` declarations and `#eq` notes with source spans. Its own comment states elaboration does not evaluate. `Note` stores formula text and structure references; this is not the dimensional/geometry AST defined here.
- [lib.rs](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/lib.rs) exposes `open`, schema projection, measurement and `run`; `open` elaborates then checks declarations and notes. These existing APIs are not evidence of CAD synthesis.
- [eval.rs](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/eval.rs) defines runtime `Value` as number, numeric vector, matrix or sampled curve. Its `Expr` supports numeric/degree literals, variables, negation, addition, subtraction, multiplication, calls/application, tuples and matrices. There is no Solid/Profile/Quantity/ReviewDelta value or candidate-program search in that evaluator. The checked AST is not a general symbolic constraint solver.
- `parse_shape` still treats certain equalities as binding/identity/claim using token heuristics. The concrete trailing-token, negative-residual and non-finite failures were [fixed](p0-parser-boundary.md), but that does not make arbitrary inequalities, quantifiers, units or CAD constraints meaningful to the original `#eq` evaluator. A separate, deliberately bounded [typed `.typ` contract reader](p0-typed-contract.md) now validates quantities, simple requirements and plan structure; it does not synthesize or prove geometry.

This contract proposes geometric predicates, bounded program enumeration, review-delta validation and proof-scoped reporting. The first dimensional and static-plan-validation fragment is linked above; the remaining capabilities are not claimed implemented. Reusing Typst syntax handling and checked declarations is reasonable; silently embedding unsupported CAD predicates into current equations is not.
