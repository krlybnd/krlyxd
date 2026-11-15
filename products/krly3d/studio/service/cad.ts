import { Code, ConnectError } from "@connectrpc/connect";
import { create } from "@bufbuild/protobuf";
import { join } from "node:path";
import { CadResponseSchema, MeshSchema, type CadRequest, type CadResponse } from "../gen/krly/studio/v1/studio_pb.ts";

type RecordValue = Record<string, unknown>;
const object = (value: unknown): value is RecordValue =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const text = (value: unknown): string => typeof value === "string" ? value : "";
const index = (value: unknown): number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : -1;

/** The .typ parser and the native checker remain separate, explicit process ports. */
export class CadDocuments {
  private readonly root: string;
  private readonly science: string;
  private readonly native: string;

  constructor(options: { root?: string; science?: string; native?: string } = {}) {
    this.root = options.root ?? Deno.env.get("STUDIO_CAD_DIR") ??
      join(Deno.env.get("HOME") ?? ".", ".local", "share", "krly3d-studio", "cad");
    this.science = options.science ?? Deno.env.get("KRLYSCI_BIN") ?? "krlysci";
    this.native = options.native ?? Deno.env.get("KRLY3D_CAD_BIN") ??
      new URL("../../../../target/debug/krly3d-cad-check", import.meta.url).pathname;
  }

  async apply(req: CadRequest, signal: AbortSignal): Promise<CadResponse> {
    if (!/^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(req.sessionId)) {
      throw new ConnectError("Érvénytelen CAD-munkamenet.", Code.InvalidArgument);
    }
    if (!["start", "review", "show", "undo", "redo"].includes(req.action) ||
      req.source.length > 65_536 ||
      (["start", "review"].includes(req.action) && !req.source.trim()) ||
      (!["start", "review"].includes(req.action) && req.source.length)) {
      throw new ConnectError("Érvénytelen típusos CAD-kérés.", Code.InvalidArgument);
    }
    const directory = join(this.root, req.sessionId);
    const history = join(directory, "history.json");
    if (req.action === "start") {
      if (req.modelId || req.baseRevision) {
        throw new ConnectError("Az alapmodellhez még nincs bázisrevízió.", Code.InvalidArgument);
      }
      await Deno.mkdir(this.root, { recursive: true });
      try {
        await Deno.mkdir(directory, { mode: 0o700 });
      } catch (error) {
        if (error instanceof Deno.errors.AlreadyExists) {
          throw new ConnectError("Ebben a munkamenetben már van CAD-modell.", Code.AlreadyExists);
        }
        throw error;
      }
      try {
        const source = join(directory, "base.typ");
        const contract = join(directory, "base.json");
        await Deno.writeTextFile(source, req.source);
        await Deno.writeTextFile(contract, await this.run(this.science, ["cad-contract", source], signal));
        await this.run(this.native, ["--accept", history, contract], signal);
      } catch (error) {
        await Deno.remove(directory, { recursive: true });
        throw error;
      }
      return await this.state(history, signal);
    }
    try {
      await Deno.stat(history);
    } catch (error) {
      if (error instanceof Deno.errors.NotFound) {
        throw new ConnectError("Ehhez a munkamenethez még nincs CAD-modell.", Code.NotFound);
      }
      throw error;
    }
    const current = await this.summary(history, signal);
    if (req.action !== "show" &&
      (req.modelId !== current.model_id || req.baseRevision !== current.revision)) {
      throw new ConnectError("Elavult vagy másik modellhez tartozó CAD-kérés.", Code.FailedPrecondition);
    }
    if (req.action === "review") {
      const scratch = await Deno.makeTempDir({ prefix: "studio-cad-review-" });
      try {
        const stored = await this.history(history);
        const active = stored.entries[current.active_entry];
        const base = current.active_entry === 0 ? join(directory, "base.typ") : join(scratch, "base.json");
        if (current.active_entry !== 0) {
          await Deno.writeTextFile(base, JSON.stringify(active.review));
        }
        const reviewSource = join(scratch, "review.typ");
        const reviewState = join(scratch, "review.json");
        await Deno.writeTextFile(reviewSource, req.source);
        await Deno.writeTextFile(reviewState,
          await this.run(this.science, ["cad-review", base, reviewSource], signal));
        await this.run(this.native,
          ["--accept-reviewed", history, reviewState, this.science], signal);
      } finally {
        await Deno.remove(scratch, { recursive: true });
      }
    } else if (req.action === "undo" || req.action === "redo") {
      await this.run(this.native, [`--${req.action}`, history], signal);
    }
    return await this.state(history, signal);
  }

  private async run(binary: string, args: string[], signal: AbortSignal): Promise<string> {
    signal.throwIfAborted();
    let child: Deno.ChildProcess;
    try {
      child = new Deno.Command(binary, { args, stdout: "piped", stderr: "piped" }).spawn();
    } catch (error) {
      throw new ConnectError(`CAD-eszköz nem indítható: ${error}`, Code.Unavailable);
    }
    const stop = () => { try { child.kill("SIGTERM"); } catch { /* Already exited. */ } };
    signal.addEventListener("abort", stop, { once: true });
    try {
      const output = await child.output();
      signal.throwIfAborted();
      const result = new TextDecoder().decode(output.stdout);
      if (!output.success) {
        const reason = new TextDecoder().decode(output.stderr).trim().slice(0, 1200);
        throw new ConnectError(reason || "A CAD-ellenőrzés sikertelen.", Code.FailedPrecondition);
      }
      if (result.length > 8 * 1024 * 1024) {
        throw new ConnectError("A CAD-válasz túl nagy.", Code.ResourceExhausted);
      }
      return result;
    } finally {
      signal.removeEventListener("abort", stop);
    }
  }

  private async summary(history: string, signal: AbortSignal) {
    try {
      const value: unknown = JSON.parse(await this.run(this.native, ["--show", history], signal));
      if (!object(value) || !text(value.model_id) || index(value.revision) < 0 ||
        index(value.active_entry) < 0 || value.status !== "tested") throw new Error("CAD history shape");
      return { model_id: text(value.model_id), revision: index(value.revision),
        active_entry: index(value.active_entry), reviewed: value.reviewed === true };
    } catch (error) {
      if (error instanceof ConnectError) throw error;
      throw new ConnectError(`Érvénytelen CAD-előzmény: ${error}`, Code.DataLoss);
    }
  }

  private async history(path: string): Promise<{ active: number; entries: RecordValue[] }> {
    const value: unknown = JSON.parse(await Deno.readTextFile(path));
    if (!object(value) || index(value.active) < 0 || !Array.isArray(value.entries) ||
      !value.entries.every(object) || !value.entries[index(value.active)]) {
      throw new ConnectError("Sérült CAD-előzmény.", Code.DataLoss);
    }
    return { active: index(value.active), entries: value.entries };
  }

  private async state(history: string, signal: AbortSignal): Promise<CadResponse> {
    const summary = await this.summary(history, signal);
    const stored = await this.history(history);
    const entry = stored.entries[summary.active_entry];
    const contract = entry.contract;
    const target = object(contract) && object(contract.target) ? contract.target : {};
    const plan = object(contract) && object(contract.plan) ? contract.plan : {};
    const nodes = object(plan.nodes) ? plan.nodes : {};
    const report = object(entry.report) ? entry.report : {};
    const review = object(entry.review) && object(entry.review.revision) ? entry.review.revision : {};
    const changes = [
      ["parameter", "updated_parameters"], ["requirement_replace", "replaced_requirements"],
      ["requirement_add", "added_requirements"], ["requirement_retract", "retracted_requirements"],
      ["geometry_replace", "replaced_geometry_checks"], ["geometry_add", "added_geometry_checks"],
      ["geometry_retract", "retracted_geometry_checks"],
    ].flatMap(([kind, key]) => Array.isArray(review[key])
      ? review[key].filter((id): id is string => typeof id === "string").map((id) => ({ kind, id }))
      : []);
    let mesh: CadResponse["mesh"];
    let previewIssue = "";
    try {
      const meshValue: unknown = JSON.parse(await this.run(this.native, ["--mesh-history", history], signal));
      if (!object(meshValue) || !Array.isArray(meshValue.positions) ||
        !meshValue.positions.every((n) => typeof n === "number" && Number.isFinite(n)) ||
        !Array.isArray(meshValue.indices) ||
        !meshValue.indices.every((n) => Number.isSafeInteger(n) && n >= 0) ||
        meshValue.positions.length % 3 || meshValue.indices.length % 3) {
        throw new ConnectError("A CAD-háló érvénytelen.", Code.DataLoss);
      }
      mesh = create(MeshSchema, {
        positions: meshValue.positions as number[], indices: meshValue.indices as number[],
        volume: Number(meshValue.volume), area: Number(meshValue.area),
        openEdges: Number(meshValue.open_edges), source: text(meshValue.source), units: "m",
      });
    } catch (error) {
      signal.throwIfAborted();
      previewIssue = error instanceof Error ? error.message : "A CAD-háló nem elérhető.";
    }
    return create(CadResponseSchema, {
      modelId: summary.model_id,
      revision: summary.revision,
      activeEntry: summary.active_entry,
      acceptedEntries: stored.entries.length,
      status: "tested",
      reviewed: summary.reviewed,
      geometryVerified: false,
      planId: text(plan.id),
      targetId: text(target.id),
      changes,
      plan: Object.entries(nodes).flatMap(([id, value]) => object(value) ? [{
        id, operation: text(value.operation),
        inputs: Array.isArray(value.inputs) ? value.inputs.filter((v: unknown): v is string => typeof v === "string") : [],
      }] : []),
      checks: Array.isArray(report.checks) ? report.checks.filter(object).map((check) => ({
        id: text(check.id), status: text(check.status),
        observed: check.observed === undefined ? "" : JSON.stringify(check.observed),
      })) : [],
      mesh,
      previewIssue,
    });
  }
}
