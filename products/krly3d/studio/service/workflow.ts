import { Annotation, END, START, StateGraph } from "@langchain/langgraph";
import { Code, ConnectError } from "@connectrpc/connect";
import { getProviderStatus } from "./ai/provider.ts";
import { expert } from "./ai/expert.ts";
import { cachedResearch } from "./ai/research-cache.ts";
import { researchEnabled, validResearchQuery } from "./ai/research.ts";
import {
  fetchImageCandidate,
  imageSearchEnabled,
  searchImages,
} from "./ai/image_search.ts";
import type {
  Answer,
  ExpertQuestion,
  Research,
  Source,
  UsageCall,
} from "./ai/expert-types.ts";
import { buildCadDraft, type CadDraft } from "./cad-draft.ts";
import { reviewCad } from "./ai/cad-review.ts";
import {
  character,
  type ConstructionData,
  demo,
  type NativeEvent,
  reconstruct,
} from "./native.ts";
import { type BlobEntry, type History, imageFormat } from "./store.ts";

export type WorkflowInput = {
  text: string;
  mode: "chat" | "build" | "demo" | "complex" | "character";
  height: number;
  pixelUnit: number;
  tolerance: number;
  seed?: [number, number];
  image?: BlobEntry;
  history: History;
  imageExplicit?: boolean;
  answers?: Answer[];
  research?: Research;
  usage?: UsageCall[];
};
export type WorkflowEvent = {
  kind: "status" | "message" | "mesh" | "artifact" | "usage";
  phase: string;
  text: string;
  mesh?: NativeEvent["mesh"];
  construction?: ConstructionData;
  artifact?: NativeEvent["artifact"];
  usage?: { calls: UsageCall[] };
  sources?: Source[];
};
export type WorkflowOutcome = {
  reply: string;
  question?: ExpertQuestion;
  research?: Research;
  usage: UsageCall[];
};
export type WorkflowDependencies = {
  research?: typeof cachedResearch;
  buildCadDraft?: typeof buildCadDraft;
  reviewCad?: typeof reviewCad;
  searchImages?: typeof searchImages;
  fetchImageCandidate?: typeof fetchImageCandidate;
};
const State = Annotation.Root({
  action: Annotation<
    | "reply"
    | "reconstruct"
    | "design"
    | "demo"
    | "complex"
    | "character"
    | "ask"
    | "research"
  >(),
  question: Annotation<ExpertQuestion | undefined>(),
  searchQuery: Annotation<string | undefined>(),
  reply: Annotation<string>(),
  height: Annotation<number>(),
  pixelUnit: Annotation<number>(),
  seed: Annotation<[number, number] | undefined>(),
  design: Annotation<CadDraft | undefined>(),
  cadAttempts: Annotation<number>(),
  cadFailure: Annotation<
    {
      code:
        | "boolean_contact"
        | "unsupported_geometry"
        | "verification"
        | "intent";
      message: string;
    } | undefined
  >(),
});

function meshBounds(positions: number[]): { min: [number, number, number]; max: [number, number, number] } {
  const min: [number, number, number] = [Infinity, Infinity, Infinity];
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i++) {
    const axis = i % 3;
    min[axis] = Math.min(min[axis], positions[i]);
    max[axis] = Math.max(max[axis], positions[i]);
  }
  if (positions.length < 9 || positions.length % 3 ||
    min.some((value, axis) => !Number.isFinite(value) || value >= max[axis])) {
    throw new ConnectError("A CAD-háló befoglaló mérete érvénytelen.", Code.DataLoss);
  }
  return { min, max };
}

export async function workflow(
  input: WorkflowInput,
  signal: AbortSignal,
  emit: (event: WorkflowEvent) => Promise<void>,
  dependencies: WorkflowDependencies = {},
): Promise<WorkflowOutcome> {
  const calls = [...(input.usage ?? [])];
  const directBuildImage = input.mode === "build" && Boolean(input.image) &&
    (input.imageExplicit !== false || !input.text.trim());
  let found = input.research;
  let checkingWebImage = false;
  // Only a public standard designation and nominal size may enter this fallback
  // query; never forward the request, history or a private project identifier.
  const standard = input.text.match(
    /\b(?:ISO|DIN|ANSI|ASME|JIS|EN)\s*[-:]?\s*\d{2,6}(?:[-:]\d{1,4})?\b/i,
  )?.[0];
  const nominal = [
    ...input.text.matchAll(/\b(?:M|DN|PN)\s*\d{1,3}(?:[.,]\d{1,2})?\b/gi),
  ]
    .slice(0, 3).map((match) => match[0].toUpperCase()).join(" ");
  const buildIntent =
    /(?:^|\s)(?:készíts|rajzolj|modellezz|építs|create|build|draw)\b/iu.test(
      input.text,
    );
  const standardQuery = standard
    ? `${standard.toUpperCase()}${
      nominal ? ` ${nominal.toUpperCase()}` : ""
    } nominal dimensions mm manufacturer dimension table`
    : undefined;
  let currentDraft: CadDraft | undefined;
  let intentReview: Awaited<ReturnType<typeof reviewCad>> | undefined;
  let cadSignal: AbortSignal | undefined;
  let verifiedCad: Awaited<ReturnType<typeof buildCadDraft>> | undefined;
  let flushed = -1;
  let planRepairUsed = calls.some((call) => call.stage === "cad_repair");
  const flushUsage = async () => {
    if (flushed === calls.length || signal.aborted) return;
    await emit({
      kind: "usage",
      phase: "usage",
      text: "Tokenhasználat és költség",
      usage: { calls: [...calls] },
    });
    flushed = calls.length;
  };
  const onUsage = (call: UsageCall) => {
    if (call.stage === "cad_repair") planRepairUsed = true;
    if (!calls.some((item) => item.id === call.id) && calls.length < 32) {
      calls.push(call);
    }
  };
  const graph = new StateGraph(State)
    .addNode("expert", async () => {
      signal.throwIfAborted();
      if (
        input.mode === "demo" || input.mode === "complex" ||
        input.mode === "character"
      ) {
        return { action: input.mode };
      }
      if (directBuildImage) {
        return { action: "reconstruct" as const };
      }
      const provider = getProviderStatus();
      if (!provider.chat) {
        throw new ConnectError(
          "A szöveges építéshez és a referencia-kép ellenőrzéséhez AI-kapcsolat szükséges. Feltöltött rajzból, a demóban és a rögzített figuránál AI nélkül is dolgozhatsz.",
          Code.FailedPrecondition,
        );
      }
      await emit({
        kind: "status",
        phase: "expert",
        text: found
          ? "A szaki összeveti a forrásokat a kéréssel…"
          : "A szaki ellenőrzi a formát, a szokásos megoldásokat és a bizonytalanságokat…",
      });
      // A retained native-only drawing must not prevent unrelated text chat.
      // An explicit attachment still gets the provider's normal input checks.
      const canSendImage = input.image &&
        (input.imageExplicit !== false || checkingWebImage);
      let result;
      try {
        result = await expert({
          text: input.text ||
            "Nézd meg a csatolt rajzot, és segíts kiválasztani a megfelelő 3D megoldást.",
          history: input.history,
          settings: {
            height: input.height,
            pixelUnit: input.pixelUnit,
            seed: input.seed,
          },
          image: canSendImage && input.image
            ? { bytes: input.image.bytes, mime: input.image.mime }
            : undefined,
          answers: input.answers,
          research: found,
          researchAllowed: !found && researchEnabled(),
          planRepairAllowed: !planRepairUsed,
          onUsage,
          signal,
        });
      } finally {
        await flushUsage();
      }
      signal.throwIfAborted();
      // A model can skip its research instruction. Never accept a named standard
      // from model memory alone: one bounded search, then re-plan with evidence.
      if (result.action === "design" && standardQuery && !found) {
        if (researchEnabled()) {
          return { action: "research" as const, searchQuery: standardQuery };
        }
        return {
          action: "reply" as const,
          reply:
            "A megnevezett szabvány méreteihez most nincs elérhető forrásellenőrzés. Szabványmegfelelőséget nem állítok; saját alkatrészként, a teljes méretsorral kép nélkül is építhetsz.",
        };
      }
      if (
        result.action === "design" && standardQuery && found &&
        !found.sources.length
      ) {
        return {
          action: "reply" as const,
          reply:
            "A szabványméretek ellenőrzése nem sikerült. Nem helyettesítem őket emlékezetből vett adatokkal; adj meg egy teljes saját méretsort, vagy próbáld újra a forrásellenőrzést.",
        };
      }
      if (result.summary) {
        await emit({
          kind: "status",
          phase: "expert",
          text: result.action === "reconstruct"
            ? input.image
              ? "A szaki kontúrkihúzást javasol a megadott beállításokkal; most indul a natív építés."
              : "A szaki kontúrépítést javasol. Nincs csatolt kép; felülnézeti referenciát keresek, majd ellenőrzöm, mielőtt építenék."
            : result.summary,
        });
      }
      if (result.action === "ask" && (input.answers?.length ?? 0) >= 6) {
        return {
          action: "reply" as const,
          reply:
            "Hat pontosítás után még maradt bizonytalanság. Foglald össze egy új kérésben a kiválasztott irányt; addig nem indítok építést.",
        };
      }
      if (result.action === "research" && (found || !researchEnabled())) {
        return {
          action: "reply" as const,
          reply:
            "Ehhez további internetes ellenőrzés szükséges. Ellenőrzött forrás nélkül nem indítok építést.",
        };
      }
      return {
        action: result.action,
        reply: result.reply,
        question: result.question,
        searchQuery: result.searchQuery,
        height: result.height ?? input.height,
        pixelUnit: result.pixelUnit ?? input.pixelUnit,
        seed: result.seed ?? input.seed,
        design: result.design,
      };
    })
    .addNode("research", async (state) => {
      signal.throwIfAborted();
      if (!state.searchQuery || found) {
        throw new ConnectError("Érvénytelen kutatási lépés.", Code.Internal);
      }
      await emit({
        kind: "status",
        phase: "research",
        text: `Rövid utánanézés: ${state.searchQuery}`,
      });
      try {
        let fromCache = false;
        found = await (dependencies.research ?? cachedResearch)(
          state.searchQuery,
          signal,
          onUsage,
          () => {
            fromCache = true;
          },
        );
        if (fromCache) {
          await emit({
            kind: "status",
            phase: "research",
            text:
              "Korábban ellenőrzött források újrafelhasználva (1 órán belül); új keresési díj nincs.",
          });
        }
        await emit({
          kind: "status",
          phase: "research",
          text: found.summary,
          sources: found.sources,
        });
      } catch (error) {
        if (signal.aborted) throw error;
        found = {
          summary:
            "Az internetes ellenőrzés nem sikerült. A külső tényeket tekintsd ellenőrizetlennek; fontos bizonytalanságnál kérdezz vissza.",
          sources: [],
        };
        await emit({
          kind: "status",
          phase: "research",
          text:
            "Az utánanézés most nem sikerült; a szaki ezt bizonytalanságként kezeli.",
        });
      } finally {
        await flushUsage();
      }
      return {};
    })
    .addNode("grounding_query", async () => {
      signal.throwIfAborted();
      await emit({
        kind: "status",
        phase: "research",
        text: "A képkeresés előtt rövid szöveges támpontokat ellenőrzök…",
      });
      let result;
      try {
        result = await expert({
          text: input.text,
          history: input.history,
          settings: {
            height: input.height,
            pixelUnit: input.pixelUnit,
            seed: input.seed,
          },
          answers: input.answers,
          researchAllowed: true,
          researchOnly: true,
          onUsage,
          signal,
        });
      } finally {
        await flushUsage();
      }
      if (result.action !== "research" || !result.searchQuery) {
        throw new ConnectError(
          "Nem sikerült biztonságos szöveges keresést készíteni a képkeresés előtt.",
          Code.FailedPrecondition,
        );
      }
      return { action: "research" as const, searchQuery: result.searchQuery };
    })
    .addNode("find_image", async (state) => {
      signal.throwIfAborted();
      if (!imageSearchEnabled()) {
        return {
          action: "reply" as const,
          reply:
            "Ehhez a kontúrépítéshez felülnézeti rajz szükséges. A webes képkeresés nincs beállítva; csatolj egy világos háttéren lévő sötét, kitöltött sziluettet, vagy válassz egy kép nélküli demót.",
        };
      }
      const query = (state.searchQuery ?? input.text)
        .trim().slice(0, 240);
      if (!validResearchQuery(query)) {
        return {
          action: "reply" as const,
          reply:
            "Nem tudtam biztonságos, rövid nyilvános képkeresést készíteni ebből a kérésből. Csatolj egy felülnézeti rajzot, vagy írd le röviden a keresett formát.",
        };
      }
      await emit({
        kind: "status",
        phase: "image_search",
        text: "Nyilvános felülnézeti referencia keresése…",
      });
      let result;
      try {
        result = await (dependencies.searchImages ?? searchImages)(
          query,
          signal,
          onUsage,
        );
      } catch (error) {
        if (signal.aborted) throw error;
        await flushUsage();
        return {
          action: "reply" as const,
          reply:
            "Most nem találtam biztonságosan felhasználható referencia-képet. Csatolj egy felülnézeti sziluettet, és abból azonnal elindítom a natív építést.",
        };
      } finally {
        await flushUsage();
      }
      await emit({
        kind: "status",
        phase: "image_search",
        text: result.candidates.length
          ? `Találtam ${result.candidates.length} nyilvános képjelöltet; ellenőrzöm, használható-e a kontúrépítéshez.`
          : "A keresés nem adott felhasználható képjelöltet.",
        sources: result.sources,
      });
      if (!getProviderStatus().vision) {
        return {
          action: "reply" as const,
          reply: result.sources.length
            ? "Találtam nyilvános képforrásokat, de a beállított AI nem tudja ellenőrizni, hogy a kép megfelelő felülnézeti sziluett-e. Válassz egy forrást, majd töltsd fel a rajzot; addig nem készítek belőle geometriát."
            : "A keresés nem adott ellenőrizhető felülnézeti rajzot. Csatolj egy világos háttéren lévő sötét, kitöltött sziluettet.",
        };
      }
      for (const candidate of result.candidates) {
        signal.throwIfAborted();
        try {
          const downloaded =
            await (dependencies.fetchImageCandidate ?? fetchImageCandidate)(
              candidate,
              signal,
            );
          input.image = {
            id: `web-reference-${crypto.randomUUID()}`,
            name: downloaded.mime === "image/png"
              ? "web-reference.png"
              : "web-reference.jpg",
            mime: downloaded.mime,
            bytes: downloaded.bytes,
            created: Date.now(),
          };
          input.imageExplicit = false;
          checkingWebImage = true;
          found = {
            summary:
              `Nyilvános képkeresési jelölt, nem igazolt azonosítás: ${candidate.caption}. Forrás: ${candidate.source.title}. Csak akkor építs, ha a csatolt kép valóban megfelel a felhasználó által kért formának és világos háttéren sötét, kitöltött felülnézeti sziluettet mutat.`,
            sources: result.sources,
          };
          await emit({
            kind: "status",
            phase: "image_search",
            text:
              "A referencia betöltődött. A szakértő most ellenőrzi, hogy a kép valóban megfelel-e a kérésnek és a kontúrépítés feltételeinek.",
            sources: result.sources,
          });
          return {};
        } catch (error) {
          if (signal.aborted) throw error;
        }
      }
      return {
        action: "reply" as const,
        reply: result.sources.length
          ? "A források megtalálhatók, de egyik kép sem volt biztonságosan betölthető PNG/JPEG rajzként. Töltsd fel a kiválasztott felülnézeti sziluettet, hogy folytathassam."
          : "Nem találtam használható felülnézeti képet. Csatolj egy rajzot, vagy válassz kép nélküli demót.",
      };
    })
    .addNode("clarify", (state) => {
      if (!state.question) {
        throw new ConnectError("Hiányzó pontosító kérdés.", Code.Internal);
      }
      return {};
    })
    .addNode("cad_compile", (state) => {
      signal.throwIfAborted();
      // One deadline spans both native attempts and the optional model repair.
      cadSignal ??= AbortSignal.any([signal, AbortSignal.timeout(120_000)]);
      cadSignal.throwIfAborted();
      currentDraft = state.design;
      verifiedCad = undefined;
      intentReview = undefined;
      return { design: currentDraft };
    })
    .addNode("cad_execute", async (state) => {
      const control = cadSignal!;
      control.throwIfAborted();
      await emit({
        kind: "status",
        phase: "cad_validate",
        text: state.cadAttempts
          ? "A javított típusos cél natív keresése és geometriai ellenőrzése…"
          : "A típusos célból natív tervet keresek és ellenőrzök…",
      });
      try {
        verifiedCad = await (dependencies.buildCadDraft ?? buildCadDraft)(state.design!, control);
      } catch (error) {
        if (!(error instanceof ConnectError) ||
          ![Code.InvalidArgument, Code.FailedPrecondition].includes(error.code)) throw error;
        return {
          cadFailure: { code: "verification" as const, message: error.rawMessage.slice(0, 600) },
          cadAttempts: state.cadAttempts + 1,
        };
      }
      return { cadFailure: undefined, cadAttempts: state.cadAttempts + 1 };
    })
    .addNode("cad_review", async () => {
      const control = cadSignal!;
      control.throwIfAborted();
      const checked = verifiedCad!;
      const mesh = checked.response.mesh;
      if (!mesh) {
        throw new ConnectError(checked.response.previewIssue || "A natív CAD-háló nem elérhető.", Code.Unavailable);
      }
      await emit({
        kind: "status",
        phase: "cad_review",
        text:
          "A kérés, a típusos cél és a mért méretek külön ellenőrzése; kép és webkeresés nélkül…",
      });
      let review;
      try {
        review = await (dependencies.reviewCad ?? reviewCad)({
          text: input.text,
          answers: input.answers,
          draft: currentDraft!,
          plan: checked.response.plan.map((node) => ({
            id: node.id, operation: node.operation, inputs: node.inputs,
          })),
          measurements: {
            bounds: meshBounds(mesh.positions),
            volume: mesh.volume,
            area: mesh.area,
          },
          signal: control,
          onUsage,
        });
      } finally {
        await flushUsage();
      }
      control.throwIfAborted();
      if (!review.accepted) {
        verifiedCad = undefined;
        return {
          cadFailure: { code: "intent" as const, message: review.reason },
        };
      }
      intentReview = review;
      return { cadFailure: undefined };
    })
    .addNode("cad_repair", async (state) => {
      const control = cadSignal!;
      control.throwIfAborted();
      await emit({
        kind: "status",
        phase: "cad_repair",
        text:
          "Az ellenőrzés eltérést talált. Egy célzott javító kör indul; új keresés és képelemzés nélkül.",
      });
      let result;
      try {
        result = await expert({
          text: input.text,
          history: [],
          answers: input.answers,
          research: found,
          settings: {
            height: input.height,
            pixelUnit: input.pixelUnit,
            seed: input.seed,
          },
          researchAllowed: false,
          repair: {
            draft: state.design!,
            code: state.cadFailure!.code,
            reason: state.cadFailure!.message,
          },
          signal: control,
          onUsage,
        });
      } finally {
        await flushUsage();
      }
      control.throwIfAborted();
      if (result.action === "design") {
        if (JSON.stringify(result.design) === JSON.stringify(state.design)) {
          return {
            action: "reply" as const,
            reply:
              "A javító kör változatlan követelményeket adott, ezért nem futtatom újra ugyanazt a hibát. Nem készült új modell.",
          };
        }
        await emit({
          kind: "status",
          phase: "cad_repair",
          text: result.summary ||
            "A javított típusos cél elkészült; ismét natív ellenőrzés következik.",
        });
        return { design: result.design };
      }
      return {
        action: result.action,
        reply: result.reply,
        question: result.question,
      };
    })
    .addNode("cad_stop", async (state) => {
      await emit({
        kind: "status",
        phase: "cad_failed",
        text:
          "A típusos cél nem ment át minden ellenőrzésen. A korábbi modell megmarad.",
      });
      return {
        action: "reply" as const,
        reply: `A modell nem készült el: ${
          state.cadFailure!.message
        } Nem indítok további fizetős próbálkozást. A korábbi elkészült modell megmaradt.`,
      };
    })
    .addNode("cad_publish", async () => {
      cadSignal!.throwIfAborted();
      const checked = verifiedCad!;
      await emit({
        kind: "status",
        phase: "validation",
        text:
          "A natív geometriai vizsgálat és a külön szöveges tervellenőrzés sikeres. A mért méretek a jelentésben vannak.",
      });
      const mesh = checked.response.mesh;
      if (!mesh) throw new ConnectError("A CAD-háló nem elérhető.", Code.Unavailable);
      await emit({
        kind: "mesh",
        phase: "validated",
        text: "A típusos célból szintetizált, natívan ellenőrzött modell.",
        mesh: { ...mesh, units: "m" },
      });
      await emit({
        kind: "artifact",
        phase: "artifact",
        text: "design.typ",
        artifact: {
          name: "design.typ",
          mime: "text/plain; charset=utf-8",
          bytes: new TextEncoder().encode(checked.source),
        },
      });
      for (
        const [name, value] of [
          ["design.validation.json", {
            status: checked.response.status,
            modelId: checked.response.modelId,
            revision: checked.response.revision,
            plan: checked.response.plan,
            checks: checked.response.checks,
            geometryVerified: checked.response.geometryVerified,
            intentReview: { method: "language_model", ...intentReview },
          }],
          ["design.sources.json", {
            version: 1,
            units: "mm",
            standardsCertified: false,
            evidence: found ?? null,
            synthesis: "bounded_typed_search",
          }],
        ] as const
      ) {
        await emit({
          kind: "artifact",
          phase: "artifact",
          text: name,
          artifact: {
            name,
            mime: "application/json",
            bytes: new TextEncoder().encode(JSON.stringify(value, null, 2)),
          },
        });
      }
      verifiedCad = undefined;
      return {
        reply:
          "Kép nélkül elkészült a típusos követelményekből keresett natív CAD-modell. A .typ forrás, a mért próbák és a forrásadatok letölthetők. A véges próbák nem bizonyítják a teljes alakazonosságot vagy a szabványmegfelelőséget.",
      };
    })
    .addNode("geometry", async (state) => {
      signal.throwIfAborted();
      if (
        !["demo", "complex", "character"].includes(state.action) &&
        !input.image
      ) {
        throw new ConnectError(
          "Előbb tölts fel egy felülnézeti rajzot a kontúr építéséhez.",
          Code.InvalidArgument,
        );
      }
      await emit({
        kind: "status",
        phase: "geometry",
        text: state.action === "character"
          ? "Rögzített paraméteres Pikachu-demó: natív térfogati mező → mesh → STL, milliméterben. AI és képfeldolgozás nélkül."
          : state.action === "complex"
          ? "Az összetett konzol valódi építési lépéseinek hálózása…"
          : state.action === "demo"
          ? "A natív lyukas lemez betöltése és hálózása…"
          : "krlyvis → kontúr → B-rep → mesh. A korai háló rögtön megjelenik.",
      });
      const events = state.action === "character"
        ? character(signal)
        : ["demo", "complex"].includes(state.action)
        ? demo(signal, state.action === "complex" ? "complex" : "plate")
        : reconstruct({
          bytes: input.image!.bytes,
          format: imageFormat(input.image!.bytes),
          height: state.height,
          pixelUnit: state.pixelUnit,
          seed: state.seed,
          tolerance: input.tolerance,
        }, signal);
      const labels: Record<string, string> = {
        started: "A natív számítás elindult.",
        sampling: "A natív térfogati mező mintavételezése elindult.",
        field_mesh: "A natív térfogati mintákból háromszögháló készül.",
        printing: "A natív mesh nyomtatási ellenőrzése és STL-exportja…",
        final_mesh: "Stilizált Pikachu · térbeli mesh",
        evidence: "A rajz kontúrja elkészült.",
        coarse_preview: "Korai befoglaló háló elkészült.",
        editable_proposal: "A szerkeszthető B-rep modell elkészült.",
        preview: "Előnézet elkészült.",
        validation: "Geometriai ellenőrzés elkészült.",
        completed: "A natív feldolgozás befejeződött.",
      };
      for await (const event of events) {
        signal.throwIfAborted();
        if (event.mesh) {
          await emit({
            kind: "mesh",
            phase: event.kind,
            text: labels[event.kind] ?? event.text ?? "Háló frissítve.",
            mesh: event.mesh,
            construction: event.construction,
          });
        } else if (event.kind !== "artifact") {
          await emit({
            kind: "status",
            phase: event.kind,
            text: labels[event.kind] ?? event.text ?? "Natív feldolgozás…",
          });
        }
        if (event.artifact) {
          await emit({
            kind: "artifact",
            phase: "artifact",
            text: event.artifact.name,
            artifact: event.artifact,
          });
        }
      }
      return {
        reply: state.action === "character"
          ? "Elkészült a stilizált Pikachu térbeli hálója és milliméteres STL-je a rögzített paraméteres receptből. Ez térfogati figura, nem képsziluett-kihúzás. A recept, a mesh és az ellenőrzési jelentés letölthető. A teljes nyomtathatóság nincs igazolva; nézd meg a jelentés ismeretlen vizsgálatait és a szeletelő előnézetét."
          : state.action === "complex"
          ? "Elkészült az összetett konzol. A kirajzolást újra lejátszhatod; a bemutatott lépések valódi natív hálók."
          : state.action === "demo"
          ? "Elkészült a lyukas lemez natív B-rep modellje és háromszöghálója. Forgasd, válts dróthálóra, vagy tölts fel saját rajzot."
          : "Elkészült a kontúrból épített modell. A magasság és a méretarány megadott feltételezés; a képből rejtett felületeket nem következtettem ki.",
      };
    })
    .addNode("respond", async (state) => {
      signal.throwIfAborted();
      await emit({ kind: "message", phase: "answer", text: state.reply });
      return {};
    })
    .addConditionalEdges(
      START,
      () =>
        (input.mode === "build" || buildIntent) && !directBuildImage &&
          standardQuery &&
          !found && researchEnabled()
          ? "research"
          : "expert",
    )
    .addConditionalEdges("expert", (state) => {
      if (state.action === "reply") return "respond";
      if (state.action === "ask") return "clarify";
      if (state.action === "design") return "cad_compile";
      if (state.action === "research") {
        return "research";
      }
      if (state.action === "reconstruct" && !input.image) {
        return !found && researchEnabled() ? "grounding_query" : "find_image";
      }
      return "geometry";
    })
    .addEdge("grounding_query", "research")
    .addEdge("research", "expert")
    .addConditionalEdges(
      "find_image",
      () => input.image ? "expert" : "respond",
    )
    .addEdge("cad_compile", "cad_execute")
    .addConditionalEdges(
      "cad_execute",
      (state) =>
        state.cadFailure
          ? state.cadAttempts < 2 && !planRepairUsed ? "cad_repair" : "cad_stop"
          : "cad_review",
    )
    .addConditionalEdges(
      "cad_review",
      (state) =>
        state.cadFailure
          ? state.cadAttempts < 2 && !planRepairUsed ? "cad_repair" : "cad_stop"
          : "cad_publish",
    )
    .addConditionalEdges(
      "cad_repair",
      (state) =>
        state.action === "design"
          ? "cad_compile"
          : state.action === "ask"
          ? "clarify"
          : "respond",
    )
    .addEdge("cad_stop", "respond")
    .addEdge("cad_publish", "respond")
    .addEdge("clarify", END)
    .addEdge("geometry", "respond").addEdge("respond", END).compile();
  try {
    const result = await graph.invoke({
      action: "reply",
      reply: "",
      cadAttempts: 0,
      height: input.height,
      pixelUnit: input.pixelUnit,
      seed: input.seed,
      searchQuery: standardQuery,
    }, { signal, recursionLimit: 18 });
    return {
      reply: result.reply,
      question: result.action === "ask" ? result.question : undefined,
      research: found,
      usage: [...calls],
    };
  } finally {
    await flushUsage();
  }
}
