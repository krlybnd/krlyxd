import { create } from "@bufbuild/protobuf";
import { CadRequestSchema } from "../gen/krly/studio/v1/studio_pb.ts";
import { CadDocuments } from "./cad.ts";
import { prepareCadDraft } from "./cad-draft.ts";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

Deno.test("typed CAD review keeps a checked Studio model across undo and redo", async () => {
  const science = Deno.env.get("KRLYSCI_BIN");
  if (!science) return; // The integration fixture needs the adjacent publication-as-code CLI.
  const root = await Deno.makeTempDir({ prefix: "studio-cad-test-" });
  const native = new URL("../../../../target/debug/krly3d-cad-check", import.meta.url).pathname;
  const base = await Deno.readTextFile(new URL("../../../../cad-knowledge/cases/solid-ring.typ", import.meta.url));
  const revision = await Deno.readTextFile(new URL("../../../../cad-knowledge/cases/solid-ring-fill-hole.typ", import.meta.url));
  const sessionId = crypto.randomUUID();
  const docs = new CadDocuments({ root, science, native });
  const signal = new AbortController().signal;
  try {
    const start = await docs.apply(create(CadRequestSchema, {
      sessionId, action: "start", source: base,
    }), signal);
    assert(start.revision === 0 && start.status === "tested", "base must pass native checks");
    assert(start.mesh?.indices.length && start.mesh.openEdges === 0, "base mesh is visible");
    const changed = await docs.apply(create(CadRequestSchema, {
      sessionId, action: "review", source: revision,
      modelId: start.modelId, baseRevision: start.revision,
    }), signal);
    assert(changed.revision === 1 && changed.reviewed, "revision must carry a validated review");
    assert(changed.changes.some((item) => item.kind === "geometry_replace"), "visible geometry delta");
    assert(changed.mesh?.volume && changed.mesh.volume > start.mesh.volume, "filled ring is larger");
    let stale = false;
    try {
      await docs.apply(create(CadRequestSchema, {
        sessionId, action: "review", source: revision,
        modelId: start.modelId, baseRevision: start.revision,
      }), signal);
    } catch { stale = true; }
    assert(stale, "stale review must be rejected");
    const reopened = new CadDocuments({ root, science, native });
    const shown = await reopened.apply(create(CadRequestSchema, { sessionId, action: "show" }), signal);
    assert(shown.revision === 1 && shown.mesh?.indices.length, "accepted model survives reopening");
    const previewFailure = `${root}/preview-failure.sh`;
    await Deno.writeTextFile(previewFailure,
      `#!/bin/sh\nif [ "$1" = "--mesh-history" ]; then echo 'preview unavailable' >&2; exit 1; fi\nexec "${native}" "$@"\n`);
    await Deno.chmod(previewFailure, 0o700);
    const withoutPreview = await new CadDocuments({ root, science, native: previewFailure })
      .apply(create(CadRequestSchema, { sessionId, action: "show" }), signal);
    assert(withoutPreview.revision === 1 && !withoutPreview.mesh && withoutPreview.previewIssue,
      "an accepted revision remains visible when meshing fails");
    const undone = await reopened.apply(create(CadRequestSchema, {
      sessionId, action: "undo", modelId: start.modelId, baseRevision: 1,
    }), signal);
    assert(undone.revision === 0 && !undone.reviewed, "undo returns to accepted base");
    const redone = await reopened.apply(create(CadRequestSchema, {
      sessionId, action: "redo", modelId: start.modelId, baseRevision: 0,
    }), signal);
    assert(redone.revision === 1 && redone.reviewed, "redo restores reviewed model");
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("typed target-only draft is bound to its spec and synthesized without a recipe", async () => {
  const science = Deno.env.get("KRLYSCI_BIN");
  if (!science) return;
  const original = await Deno.readTextFile(new URL("../../../../cad-knowledge/cases/solid-ring.typ", import.meta.url));
  const [spec, rest] = original.split("#cad_plan(");
  const target = `#cad_target(${rest.split("#cad_target(")[1]}`.replace(/spec_hash: "[0-9a-f]{64}"/, 'spec_hash: "__SPEC_HASH__"');
  const signal = new AbortController().signal;
  const source = await prepareCadDraft({ spec, target }, signal, science);
  assert(!source.includes("#cad_plan("), "the model must not provide a feature plan");
  assert(!source.includes("__SPEC_HASH__"), "the parser owns the semantic hash");
  const root = await Deno.makeTempDir({ prefix: "studio-cad-synth-test-" });
  try {
    const docs = new CadDocuments({ root, science });
    const state = await docs.apply(create(CadRequestSchema, {
      sessionId: crypto.randomUUID(), action: "start", source,
    }), signal);
    assert(state.status === "tested" && state.plan.length > 0, "native search found a plan");
    assert(state.mesh?.indices.length && state.mesh.openEdges === 0, "synthesized solid is visible");
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("duplicate requirement labels are repaired locally before typed synthesis", async () => {
  const science = Deno.env.get("KRLYSCI_BIN");
  if (!science) return;
  const original = await Deno.readTextFile(new URL("../../../../cad-knowledge/cases/solid-ring.typ", import.meta.url));
  const [rawSpec, rest] = original.split("#cad_plan(");
  const spec = rawSpec.replace('require("outer",', 'require("positive",')
    .replace('require("height",', 'require("positive",');
  const target = `#cad_target(${rest.split("#cad_target(")[1]}`
    .replace(/spec_hash: "[0-9a-f]{64}"/, 'spec_hash: "__SPEC_HASH__"');
  const source = await prepareCadDraft({ spec, target }, new AbortController().signal, science);
  assert(source.includes('require("positive",'), "first requirement retains its name");
  assert(source.includes('require("positive#2",'), "second requirement gets a stable unique name");
  assert(!source.includes("__SPEC_HASH__"), "target binds to the corrected spec hash");
  const root = await Deno.makeTempDir({ prefix: "studio-cad-duplicate-test-" });
  try {
    const state = await new CadDocuments({ root, science }).apply(create(CadRequestSchema, {
      sessionId: crypto.randomUUID(), action: "start", source,
    }), new AbortController().signal);
    assert(state.status === "tested" && state.mesh?.indices.length, "corrected requirements build a checked solid");
  } finally {
    await Deno.remove(root, { recursive: true });
  }
});

Deno.test("typed CAD parser errors do not expose a temporary source path", async () => {
  const science = Deno.env.get("KRLYSCI_BIN");
  if (!science) return;
  let message = "";
  try {
    await prepareCadDraft({
      spec: '#cad_spec("bad", parameters: (R: 15mm), requirements: (require("positive", R > )))',
      target: '#cad_target("bad", spec_hash: "__SPEC_HASH__", checks: (require_span("width", x, 2*R, 2*R)))',
    }, new AbortController().signal, science);
  } catch (error) {
    message = error instanceof Error ? error.message : String(error);
  }
  assert(message.length > 0 && !message.includes("/tmp/studio-cad-draft-"),
    "the parser error must be visible without the transient workspace path");
});
