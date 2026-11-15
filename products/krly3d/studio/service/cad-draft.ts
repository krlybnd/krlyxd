import { Code, ConnectError } from "@connectrpc/connect";
import { join } from "node:path";
import { create } from "@bufbuild/protobuf";
import { CadRequestSchema } from "../gen/krly/studio/v1/studio_pb.ts";
import { CadDocuments } from "./cad.ts";

/** A requirement proposal, deliberately without a feature plan. */
export type CadDraft = { spec: string; target: string };

const HASH_MARKER = "__SPEC_HASH__";
const MAX_SOURCE = 65_536;

function declaration(source: string, name: "cad_spec" | "cad_target"): boolean {
  const trimmed = source.trim();
  return trimmed.startsWith(`#${name}(`) && trimmed.length <= MAX_SOURCE &&
    (trimmed.match(/#cad_[a-z_]+\s*\(/g) ?? []).length === 1;
}

async function runScience(binary: string, source: string, signal: AbortSignal): Promise<unknown> {
  const directory = await Deno.makeTempDir({ prefix: "studio-cad-draft-" });
  try {
    const path = join(directory, "contract.typ");
    await Deno.writeTextFile(path, source);
    signal.throwIfAborted();
    let child: Deno.ChildProcess;
    try {
      child = new Deno.Command(binary, {
        args: ["cad-contract", path], stdout: "piped", stderr: "piped",
      }).spawn();
    } catch (error) {
      throw new ConnectError(`A típusos CAD-értelmező nem indítható: ${error}`, Code.Unavailable);
    }
    const stop = () => { try { child.kill("SIGTERM"); } catch { /* Already exited. */ } };
    signal.addEventListener("abort", stop, { once: true });
    try {
      const output = await child.output();
      signal.throwIfAborted();
      if (!output.success) {
        throw new ConnectError(
          new TextDecoder().decode(output.stderr).trim().slice(0, 1200) ||
            "A típusos CAD-leírás érvénytelen.",
          Code.FailedPrecondition,
        );
      }
      if (output.stdout.length > 262_144) {
        throw new ConnectError("A típusos CAD-válasz túl nagy.", Code.ResourceExhausted);
      }
      return JSON.parse(new TextDecoder().decode(output.stdout));
    } finally {
      signal.removeEventListener("abort", stop);
    }
  } finally {
    await Deno.remove(directory, { recursive: true });
  }
}

/** Bind the target to the parser's semantic hash, then validate the complete
 * contract. The language model never supplies a plan or chooses a hash.
 */
export async function prepareCadDraft(
  draft: CadDraft,
  signal: AbortSignal,
  science = Deno.env.get("KRLYSCI_BIN") ?? "krlysci",
): Promise<string> {
  if (!declaration(draft.spec, "cad_spec") ||
    !declaration(draft.target, "cad_target") ||
    draft.target.split(HASH_MARKER).length !== 2 ||
    draft.spec.length + draft.target.length > MAX_SOURCE) {
    throw new ConnectError("Egy specifikáció és egy célfeltétel szükséges, kész terv nélkül.", Code.InvalidArgument);
  }
  const first = await runScience(science, `${draft.spec.trim()}\n`, signal);
  if (!first || typeof first !== "object" || !("spec" in first) ||
    !first.spec || typeof first.spec !== "object" ||
    !("semantic_hash" in first.spec) ||
    typeof first.spec.semantic_hash !== "string" ||
    !/^[0-9a-f]{64}$/.test(first.spec.semantic_hash) ||
    !("plan" in first) || first.plan !== null ||
    !("target" in first) || first.target !== null) {
    throw new ConnectError("A típusos specifikáció nem tervfüggetlen.", Code.FailedPrecondition);
  }
  const source = `${draft.spec.trim()}\n${draft.target.replace(HASH_MARKER, first.spec.semantic_hash).trim()}\n`;
  const complete = await runScience(science, source, signal);
  if (!complete || typeof complete !== "object" ||
    !("plan" in complete) || complete.plan !== null ||
    !("target" in complete) || complete.target === null) {
    throw new ConnectError("A típusos cél nem teljes vagy kész tervet tartalmaz.", Code.FailedPrecondition);
  }
  return source;
}

/** Ephemeral conversational build. The typed CAD panel owns durable revisions;
 * this path returns a verified mesh and the reusable requirement source.
 */
export async function buildCadDraft(draft: CadDraft, signal: AbortSignal) {
  const source = await prepareCadDraft(draft, signal);
  const root = await Deno.makeTempDir({ prefix: "studio-cad-build-" });
  try {
    const response = await new CadDocuments({ root }).apply(create(CadRequestSchema, {
      sessionId: crypto.randomUUID(), action: "start", source,
    }), signal);
    return { source, response };
  } finally {
    await Deno.remove(root, { recursive: true });
  }
}
