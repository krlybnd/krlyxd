import { create } from "@bufbuild/protobuf";
import { CadRequestSchema } from "../gen/krly/studio/v1/studio_pb.ts";
import { CadDocuments } from "./cad.ts";

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
