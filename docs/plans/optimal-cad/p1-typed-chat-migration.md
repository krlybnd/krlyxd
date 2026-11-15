# P1/P2: Studio text-CAD migration to typed requirements

Date: 2026-11-15. Baseline: `b85f2647a8f3c4e535cf9ff7169f4f89281ea936` on `ontrack`.

The text-build expert now proposes a `#cad_spec` and `#cad_target`, without
`#cad_plan`. The host obtains the spec's semantic hash from `krlysci`, binds
the target to it and rejects a supplied plan. The existing native bounded
search finds a plan, checks it, and returns its actual mesh, plan and check
report. An independent intent review compares the original request against
the draft, discovered plan and measured bounds before publication. One
bounded correction is available for a rejected candidate. The downloadable
result contains `design.typ`, `design.validation.json` and
`design.sources.json`; no handcrafted recipe is exported.

The removed recipe interpreter and execution adapter were copied to
`.tmp/refactor/typed-cad/before/` with their original paths before deletion.
That directory is ignored; the baseline Git commit is the durable recovery
source. SHA-256 of the removed originals:

- `service/ai/cad-recipe.ts`: `7cdb31c1675305de4b9eb24b6d0e335ed73c7bf573ba408e5576629ca264e2ec` (784 lines)
- `service/cad-execution.ts`: `4fe20c314f7570ba5ca311ba2c5b8daa62feb938967f2b857eb81e85fb0d2033` (245 lines)
- `service/ai/cad-guidance.ts`: `21bb56a56ed31b3b07ecb60611a109f41d305781f40ab3015b38fe9bff1c67f5` (26 lines; no remaining callers)
- Recipe-only tests: `service/ai/cad-recipe_test.ts` (715), `service/cad-execution_test.ts` (438), `service/cad-parametric_test.ts` (723).

Measured with physical newline-delimited source lines at migration commit
`f733dcc`, comparing the baseline with the migrated files:
`cad-recipe.ts` 784→0,
`cad-execution.ts` 245→0, `cad-guidance.ts` 26→0,
`native.ts` 1491→1379, `expert.ts`
755→533, `cad-review.ts` 235→228, `workflow.ts` 823→841, and new
`cad-draft.ts` 0→105. Net production change in these files: **−1,273
lines**. The affected service test files, including the new typed tests,
change by **−3,298 lines**. These counts are separate: deleting old tests
does not itself simplify production. Presentation-only construction v2 code
remains, so this is not the entire cleanup promised by P5.

Follow-up: a live ring prompt produced two `require("positive", …)` predicates.
The strict reader rejected the duplicate name before native synthesis. Studio
now renames only duplicate literal requirement labels, keeps every predicate
and dimension unchanged, and reruns the strict reader before binding the
target hash. A malformed expression still fails. The expert prompt also asks
for distinct labels. The emitted `design.typ` contains the corrected labels,
and parser diagnostics no longer expose a transient `/tmp` path. The full
workflow test proves this case reaches a checked mesh with one AI call. This
follow-up adds 35 production lines; the net reduction in the same production
files is now **−1,238 lines** relative to the original baseline.

The migration intentionally narrows the conversational builder's *supported*
geometry to the currently verified native search grammar: centred disk,
rectangle or annulus XY region, +Z extrusion and at most one union/difference
of two solids using 1–6 positive length parameters. Old handwritten recipes
could describe several features outside this grammar. The expert is told to
reply unsupported for such requests; the intent reviewer must reject a
simplified substitute. This is a capability reduction, not a parity claim.
The existing image-outline and fixed demo paths remain separate.

QA: the typed target-only Studio integration checks semantic-hash binding,
plan synthesis, native checks and a visible closed mesh. A mocked text-build
checks the user path and `design.typ` artifact. Full Studio service suite:
108 passed; standalone construction metadata tests: 8 passed; web build and
DOM contract check passed. Browser E2E: 9/9 passed after fixing an integration
bug it exposed: the UI initially rejected the new `validated` final-mesh
phase. This failure was in the UI, not the native synthesis.

Remaining gates from [PLAN](PLAN.md):

1. Extend the typed grammar and native search with frames/transforms, general
   profiles, repeated operations, helical/sweep geometry and relations before
   claiming support for rods, threads or helical gears. Add withheld
   compositions and real geometry checks for each extension; report `unknown`
   honestly when the budget ends.
2. Solve parameter domains and verify edits across more than one sample;
   distinguish finite `tested` from proof and expose search coverage and cost.
3. Bind text review to selected model geometry, typed requirement deltas,
   accepted revision, preservation constraints, measured diff and undo in the
   Studio UI. The current chat repair changes the draft and retries once; it
   is not yet the full P1 review workflow.
4. Replace the remaining presentation-only construction-v2 bridge when native
   typed feature records can drive the timeline. Then audit its UI and tests
   for removable code. Preserve playback of existing stored sessions if needed.
5. Complete the Inventor adapter and broader multi-backend acceptance gates
   before making a general optimal-construction claim.
