import {
  workflow as runWorkflow,
  type WorkflowEvent,
  type WorkflowInput,
} from "./workflow.ts";
import {
  assert,
  configureLiteLLM,
  equal,
  isolated,
} from "./ai/test_support.ts";
import type { ImageSearchResult } from "./ai/image_search.ts";

import { research } from "./ai/research.ts";

// Each workflow test controls its own provider calls. Cache behavior is tested
// separately with an isolated clock/config; production uses the shared cache.
const workflow: typeof runWorkflow = (input, signal, emit, dependencies = {}) =>
  runWorkflow(input, signal, emit, {
    research,
    // Provider decision tests isolate semantic review; dedicated tests below
    // exercise its gating, feedback and failure behavior.
    reviewCad: async () => ({
      accepted: true,
      reason: "Test reviewer accepted.",
    }),
    ...dependencies,
  });

const input: WorkflowInput = {
  mode: "chat",
  text: "Nézz utána, majd kérdezz",
  height: 8,
  pixelUnit: .025,
  tolerance: .05,
  history: [],
};
const question = {
  title: "Melyik cél?",
  reason: "A forma célja nem ismert.",
  options: [{ id: "A", label: "Dísz", description: "Körvonal" }, {
    id: "B",
    label: "Bemutató",
    description: "Szemléltető",
  }],
};
function decision(action: string) {
  return Response.json({
    choices: [{
      finish_reason: "tool_calls",
      message: {
        tool_calls: [{
          type: "function",
          function: {
            name: "studio_expert",
            arguments: JSON.stringify({
              reply: "Pontosítsunk.",
              summary: "Az alkalmazás célját tisztázom.",
              decision: {
                action,
                height: action === "reconstruct" ? 8 : null,
                pixelUnit: action === "reconstruct" ? .025 : null,
                seed: null,
                question: action === "ask" ? question : null,
                searchQuery: action === "research"
                  ? "gear silhouette construction"
                  : null,
                design: null,
              },
            }),
          },
        }],
      },
    }],
    usage: { prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 },
  }, { headers: { "x-litellm-response-cost": "0.000024" } });
}

function groundedText() {
  const text =
    "A körvonalas alkatrészeknél a felülnézeti kontúr határozza meg az extrudált profilt.";
  return Response.json({
    status: "completed",
    output: [
      { type: "web_search_call", status: "completed" },
      {
        type: "message",
        role: "assistant",
        status: "completed",
        content: [{
          type: "output_text",
          text,
          annotations: [{
            type: "url_citation",
            title: "Outline extrusion guide",
            url: "https://example.org/outline-extrusion",
            start_index: 0,
            end_index: text.length,
          }],
        }],
      },
    ],
    usage: { input_tokens: 12, output_tokens: 18, total_tokens: 30 },
  }, { headers: { "x-litellm-response-cost": "0.00003" } });
}

Deno.test("graph research failure is bounded, then HITL resumes without repeating research", () =>
  isolated(async () => {
    configureLiteLLM();
    const previous = Deno.env.get("STUDIO_RESEARCH_ENABLED");
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let expertCalls = 0, researchCalls = 0;
    globalThis.fetch = (url, init) => {
      if (String(url).endsWith("/responses")) {
        researchCalls++;
        return Promise.resolve(
          Response.json({ error: "unavailable" }, { status: 503 }),
        );
      }
      expertCalls++;
      const request = JSON.parse(String(init?.body));
      const actions = request.tools[0].function.parameters.properties.decision
        .anyOf
        .map((branch: { properties: { action: { enum: string[] } } }) =>
          branch.properties.action.enum[0]
        );
      equal(actions.includes("research"), expertCalls === 1);
      return Promise.resolve(
        decision(
          expertCalls === 1 ? "research" : expertCalls === 2 ? "ask" : "reply",
        ),
      );
    };
    try {
      const events: WorkflowEvent[] = [];
      const result = await workflow(
        input,
        new AbortController().signal,
        (event) => {
          events.push(event);
          return Promise.resolve();
        },
      );
      assert(
        result.question && result.research &&
          result.research.sources.length === 0,
      );
      equal(result.usage.length, 3);
      equal(result.usage[1].costKind, "unknown");
      assert(!events.some((e) => e.phase === "geometry"));
      const next = await workflow(
        {
          ...input,
          research: result.research,
          usage: result.usage,
          answers: [{
            question: question.title,
            selected: [{ id: "A", label: "Dísz" }],
          }],
        },
        new AbortController().signal,
        async () => {},
      );
      assert(!next.question);
      equal(researchCalls, 1);
      equal(next.usage.length, 4);
    } finally {
      if (previous === undefined) Deno.env.delete("STUDIO_RESEARCH_ENABLED");
      else Deno.env.set("STUDIO_RESEARCH_ENABLED", previous);
    }
  }));

Deno.test("retained native-only image cannot poison text chat; explicit attachment still validates", () =>
  isolated(async () => {
    configureLiteLLM();
    let calls = 0;
    globalThis.fetch = (_url, init) => {
      calls++;
      const body = JSON.parse(String(init?.body));
      assert(
        typeof body.messages.at(-1).content === "string",
        "no unsupported vision payload",
      );
      return Promise.resolve(decision("reply"));
    };
    const image = {
      id: "retained",
      name: "shape.pgm",
      mime: "image/x-portable-anymap",
      bytes: new TextEncoder().encode("P2 1 1 255 0"),
      created: Date.now(),
    };
    await workflow(
      { ...input, image, imageExplicit: false },
      new AbortController().signal,
      async () => {},
    );
    let rejected = false;
    try {
      await workflow(
        { ...input, image, imageExplicit: true },
        new AbortController().signal,
        async () => {},
      );
    } catch {
      rejected = true;
    }
    assert(
      rejected && calls === 1,
      "explicit unsupported image rejected before billing",
    );
  }));

Deno.test("text-only build grounds with text before last-resort image search", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("LITELLM_VISION_MODEL", "vision-model");
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let expertCalls = 0;
    let textSearches = 0;
    let searches = 0;
    const events: WorkflowEvent[] = [];
    const source = {
      title: "Public silhouette reference",
      url: "https://example.org/shape",
    };
    const searchResult: ImageSearchResult = {
      sources: [source],
      candidates: [{
        imageURL: "https://images.example.org/shape.png",
        caption: "dark silhouette",
        source,
      }],
    };
    globalThis.fetch = (url, init) => {
      if (String(url).endsWith("/responses")) {
        textSearches++;
        return Promise.resolve(groundedText());
      }
      expertCalls++;
      const body = JSON.parse(String(init?.body));
      if (expertCalls === 1) {
        return Promise.resolve(decision("research"));
      }
      if (expertCalls === 2) return Promise.resolve(decision("reconstruct"));
      equal(body.model, "vision-model");
      const imageContent = body.messages.find((message: { content: unknown }) =>
        Array.isArray(message.content)
      )?.content;
      assert(
        Array.isArray(imageContent) &&
          imageContent.some((
            part: { type?: string; image_url?: { url?: string } },
          ) =>
            part.type === "image_url" &&
            part.image_url?.url?.startsWith("data:image/png;base64,")
          ),
      );
      return Promise.resolve(decision("reply"));
    };
    const result = await workflow(
      {
        ...input,
        mode: "build",
        text: "Készíts egy egyszerű, zárt fogaskerék-sziluettet.",
      },
      new AbortController().signal,
      async (event) => {
        events.push(event);
      },
      {
        searchImages: async (_query, _signal, onUsage) => {
          searches++;
          onUsage({
            id: "image-search",
            stage: "image_search",
            model: "search",
            inputTokens: 5,
            outputTokens: 2,
            cachedTokens: 0,
            totalTokens: 7,
            costUsd: 0,
            costKind: "unknown",
            tokensKnown: true,
            searchCalls: 1,
          });
          return searchResult;
        },
        fetchImageCandidate: async () => ({
          bytes: new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]),
          mime: "image/png",
        }),
      },
    );
    equal(expertCalls, 3);
    equal(textSearches, 1);
    equal(searches, 1);
    assert(result.reply);
    assert(
      events.some((event) =>
        event.phase === "image_search" && event.sources?.[0]?.url === source.url
      ),
    );
    assert(
      events.findIndex((event) => event.phase === "research") <
        events.findIndex((event) => event.phase === "image_search"),
      "text grounding must finish before image search starts",
    );
    assert(!events.some((event) => event.phase === "geometry"));
    equal(
      result.usage.filter((call) => call.stage === "image_search").length,
      1,
    );
  }));

Deno.test("text-only contour build degrades to a clear upload request when web search is disabled", () =>
  isolated(async () => {
    configureLiteLLM();
    let expertCalls = 0;
    globalThis.fetch = () => {
      expertCalls++;
      return Promise.resolve(decision("reconstruct"));
    };
    const events: WorkflowEvent[] = [];
    const result = await workflow(
      { ...input, mode: "build", text: "Készíts egy sötét, zárt sziluettet." },
      new AbortController().signal,
      async (event) => {
        events.push(event);
      },
    );
    equal(expertCalls, 1);
    assert(result.reply.includes("felülnézeti rajz"));
    assert(!events.some((event) => event.phase === "geometry"));
  }));

Deno.test("a direct text-only reconstruct decision is gated by text grounding before image search", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let expertCalls = 0;
    let textSearches = 0;
    let imageSearches = 0;
    const events: WorkflowEvent[] = [];
    globalThis.fetch = (url, init) => {
      if (String(url).endsWith("/responses")) {
        textSearches++;
        return Promise.resolve(groundedText());
      }
      expertCalls++;
      if (expertCalls === 1) return Promise.resolve(decision("reconstruct"));
      if (expertCalls === 2) {
        const body = JSON.parse(String(init?.body));
        const actions = body.tools[0].function.parameters.properties.decision
          .anyOf
          .map((branch: { properties: { action: { enum: string[] } } }) =>
            branch.properties.action.enum[0]
          );
        equal(actions, ["research"]);
        return Promise.resolve(decision("research"));
      }
      return Promise.resolve(decision("reconstruct"));
    };
    const result = await workflow(
      { ...input, mode: "build", text: "Készíts egy sötét, zárt sziluettet." },
      new AbortController().signal,
      async (event) => {
        events.push(event);
      },
      {
        searchImages: async (_query, _signal) => {
          imageSearches++;
          return { sources: [], candidates: [] };
        },
      },
    );
    equal(expertCalls, 3);
    equal(textSearches, 1);
    equal(imageSearches, 1);
    assert(
      result.reply.includes(
        "A keresés nem adott ellenőrizhető felülnézeti rajzot",
      ),
      `unexpected reply: ${result.reply}`,
    );
    assert(
      events.findIndex((event) => event.phase === "research") <
        events.findIndex((event) => event.phase === "image_search"),
      "text grounding must precede image search even after a direct reconstruct decision",
    );
  }));

Deno.test("text-only standard CAD researches dimensions then builds native feature meshes without image search", () =>
  isolated(async () => {
    configureLiteLLM();
    const previous = Deno.env.get("STUDIO_RESEARCH_ENABLED");
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    const order: string[] = [];
    const recipe = {
      units: "mm",
      steps: [
        {
          type: "cylinder",
          id: "outer",
          title: "Külső palást",
          size: null,
          center: [0, 0, 0],
          radius: 8,
          height: 1.6,
          inputs: null,
        },
        {
          type: "cylinder",
          id: "bore",
          title: "Furat vágóhengere",
          size: null,
          center: [0, 0, -1],
          radius: 4.2,
          height: 3.6,
          inputs: null,
        },
        {
          type: "subtract",
          id: "washer",
          title: "Átmenő furat kivágása",
          size: null,
          center: null,
          radius: null,
          height: null,
          inputs: ["outer", "bore"],
        },
      ],
    };
    const summary =
      "Az M8 alátét méretei 8,4 mm furat, 16 mm külső átmérő és 1,6 mm vastagság.";
    const source = Response.json({
      status: "completed",
      output: [
        { type: "web_search_call", status: "completed" },
        {
          type: "message",
          role: "assistant",
          status: "completed",
          content: [{
            type: "output_text",
            text: summary,
            annotations: [{
              type: "url_citation",
              title: "M8 alátét méretei",
              url: "https://example.org/iso-7089-m8",
              start_index: 0,
              end_index: summary.length,
            }],
          }],
        },
      ],
      usage: { input_tokens: 12, output_tokens: 18, total_tokens: 30 },
    });
    const modelDecision = Response.json({
      choices: [{
        finish_reason: "tool_calls",
        message: {
          tool_calls: [{
            type: "function",
            function: {
              name: "studio_expert",
              arguments: JSON.stringify({
                reply:
                  "A forrásból ellenőrzött méretekkel építem fel a natív feature-lépéseket.",
                summary: "ISO 7089 M8 alátét · ellenőrzött mm méretek.",
                decision: {
                  action: "design",
                  height: null,
                  pixelUnit: null,
                  seed: null,
                  question: null,
                  searchQuery: null,
                  design: recipe,
                },
              }),
            },
          }],
        },
      }],
      usage: { prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 },
    });
    globalThis.fetch = (url) => {
      if (String(url).endsWith("/responses")) {
        order.push("research");
        return Promise.resolve(source);
      }
      order.push("expert");
      return Promise.resolve(modelDecision);
    };
    try {
      const events: WorkflowEvent[] = [];
      const result = await workflow(
        {
          ...input,
          mode: "build",
          text: "Készíts szabványos ISO 7089 M8 alátétet milliméterben.",
        },
        new AbortController().signal,
        async (event) => {
          events.push(event);
        },
      );
      equal(order, ["research", "expert"]);
      assert(
        result.reply.includes("Kép nélkül"),
        `unexpected final reply: ${result.reply}`,
      );
      assert(result.research?.sources.length === 1);
      assert(
        events.some((event) =>
          event.phase === "construction_step" &&
          event.text.includes("Külső palást")
        ),
      );
      assert(
        events.some((event) =>
          event.phase === "editable_proposal" && event.mesh?.units === "mm"
        ),
      );
      assert(events.some((event) => event.artifact?.name === "design.k3d"));
      assert(!events.some((event) => event.phase === "image_search"));
    } finally {
      if (previous === undefined) Deno.env.delete("STUDIO_RESEARCH_ENABLED");
      else Deno.env.set("STUDIO_RESEARCH_ENABLED", previous);
    }
  }));

const explicitCylinder = {
  units: "mm",
  steps: [{
    type: "cylinder",
    id: "body",
    title: "Henger",
    center: [0, 0, 0],
    radius: 10,
    height: 20,
    size: null,
    inputs: null,
  }],
};
function designResponse(recipe = explicitCylinder) {
  return Response.json({
    choices: [{
      finish_reason: "tool_calls",
      message: {
        tool_calls: [{
          type: "function",
          function: {
            name: "studio_expert",
            arguments: JSON.stringify({
              reply: "A megadott méretekkel építek.",
              summary: "Henger milliméterben.",
              decision: {
                action: "design",
                height: null,
                pixelUnit: null,
                seed: null,
                question: null,
                searchQuery: null,
                design: recipe,
              },
            }),
          },
        }],
      },
    }],
  });
}

Deno.test("explicit text dimensions use one call despite a retained PNG and unrelated raster settings", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("LITELLM_VISION_MODEL", "vision-model");
    let calls = 0;
    globalThis.fetch = (_url, init) => {
      calls++;
      const body = JSON.parse(String(init?.body));
      assert(body.model !== "vision-model");
      assert(
        body.messages.every((m: { content: unknown }) =>
          typeof m.content === "string"
        ),
      );
      return Promise.resolve(designResponse());
    };
    const events: WorkflowEvent[] = [];
    await workflow(
      {
        ...input,
        mode: "build",
        text: "Henger: sugár 10 mm, magasság 20 mm.",
        height: 999,
        imageExplicit: false,
        image: {
          id: "old",
          name: "old.png",
          mime: "image/png",
          bytes: new Uint8Array([137, 80, 78, 71]),
          created: 0,
        },
      },
      new AbortController().signal,
      async (e) => {
        events.push(e);
      },
    );
    equal(calls, 1);
    const mesh = events.find((e) => e.mesh)?.mesh;
    assert(mesh && mesh.volume > 0 && mesh.openEdges === 0);
    // Tessellated volume is approximate; check dimensions to catch raster height leakage.
    const z = mesh.positions.filter((_, index) => index % 3 === 2);
    equal([Math.min(...z), Math.max(...z)], [0, 20]);
    const xy = mesh.positions.filter((_, index) => index % 3 !== 2);
    assert(Math.abs(Math.max(...xy) - 10) < 1e-8);
    assert(Math.abs(Math.min(...xy) + 10) < 1e-8);
    assert(
      !events.some((e) => e.phase === "research" || e.phase === "image_search"),
    );
    assert(events.some((e) => e.artifact?.name === "design.sources.json"));
  }));

Deno.test("standard design from model memory triggers one public search; failed evidence never publishes geometry", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let searches = 0;
    let experts = 0;
    globalThis.fetch = (url, init) => {
      if (String(url).endsWith("/responses")) {
        searches++;
        const body = JSON.parse(String(init?.body));
        equal(
          body.input,
          "ISO 7089 M8 nominal dimensions mm manufacturer dimension table",
        );
        assert(!body.input.includes("private-project"));
        return Promise.resolve(Response.json({ status: "failed" }));
      }
      experts++;
      return Promise.resolve(designResponse());
    };
    const events: WorkflowEvent[] = [];
    const result = await workflow(
      { ...input, text: "private-project: ISO 7089 M8 alátét" },
      new AbortController().signal,
      async (e) => {
        events.push(e);
      },
    );
    equal(searches, 1);
    equal(experts, 2);
    assert(result.reply.includes("ellenőrzése nem sikerült"));
    assert(!events.some((e) => e.mesh || e.artifact));
  }));

const repairMesh = {
  positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
  indices: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
  volume: 1 / 6,
  area: 2.36,
  openEdges: 0,
  units: "mm" as const,
  source: "test-verified",
};
const unsupportedContact = () =>
  new Error(
    "Native mesh exited 1: krly3d-mesh: unsupported: secret-node: edges of the two solids run along each other",
  );

Deno.test("CAD graph: standard build intent in chat grounds DN/PN before any planning call", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let experts = 0, searches = 0;
    globalThis.fetch = () => {
      experts++;
      return Promise.resolve(designResponse());
    };
    const events: WorkflowEvent[] = [];
    await workflow(
      {
        ...input,
        mode: "chat",
        text: "Készíts EN 1092-1 DN50 PN16 karimát. private-project",
      },
      new AbortController().signal,
      async (e) => {
        events.push(e);
      },
      {
        research: async (query) => {
          searches++;
          equal(experts, 0);
          equal(
            query,
            "EN 1092-1 DN50 PN16 nominal dimensions mm manufacturer dimension table",
          );
          return { summary: "Nincs ellenőrizhető adat.", sources: [] };
        },
      },
    );
    equal([searches, experts], [1, 1]);
    assert(!events.some((e) => e.mesh || e.artifact));
  }));

Deno.test("CAD graph: admission repair and native repair share one budget", () =>
  isolated(async () => {
    configureLiteLLM();
    let calls = 0, attempts = 0;
    globalThis.fetch = () => {
      calls++;
      return Promise.resolve(
        designResponse(
          calls === 1 ? { units: "mm", steps: [] } : explicitCylinder,
        ),
      );
    };
    const events: WorkflowEvent[] = [];
    const result = await workflow(
      { ...input, text: "Építs hengert" },
      new AbortController().signal,
      async (e) => {
        events.push(e);
      },
      {
        buildCad: async function* () {
          attempts++;
          yield { kind: "started" };
          throw unsupportedContact();
        },
      },
    );
    equal([calls, attempts], [2, 1]);
    equal(result.usage.filter((c) => c.stage === "cad_repair").length, 1);
    assert(!events.some((e) => e.mesh || e.artifact));
  }));

Deno.test("CAD graph: one bounded repair, no failed previews or artifacts, and no extra research", () =>
  isolated(async () => {
    configureLiteLLM();
    let calls = 0, attempts = 0;
    globalThis.fetch = (_url, init) => {
      calls++;
      const body = JSON.parse(String(init?.body));
      if (calls === 2) {
        assert(!JSON.stringify(body).includes("secret-node"));
        assert(!JSON.stringify(body).includes("old-history"));
        assert(
          body.messages.some((m: { content: string }) =>
            m.content.includes("executionFeedback")
          ),
        );
        assert(body.messages[1].content.includes("researchAllowed=false"));
        equal(
          body.tools[0].function.parameters.properties.decision.anyOf.map((
            v: { properties: { action: { enum: string[] } } },
          ) => v.properties.action.enum[0]),
          ["reply", "ask", "design"],
        );
      }
      return Promise.resolve(
        designResponse(
          calls === 1 ? explicitCylinder : {
            ...explicitCylinder,
            steps: [{ ...explicitCylinder.steps[0], id: "repaired" }],
          },
        ),
      );
    };
    const events: WorkflowEvent[] = [];
    const result = await workflow(
      {
        ...input,
        text: "Építs hengert",
        history: [{ role: "assistant", content: "old-history" }],
      },
      new AbortController().signal,
      async (e) => {
        events.push(e);
      },
      {
        buildCad: async function* () {
          attempts++;
          yield { kind: "started" };
          yield {
            kind: "construction_step",
            text: attempts === 1 ? "FAILED PREVIEW" : "validated stage",
            mesh: repairMesh,
          };
          if (attempts === 1) throw unsupportedContact();
          yield {
            kind: "editable_proposal",
            text: "validated final",
            mesh: repairMesh,
          };
          yield { kind: "completed" };
        },
      },
    );
    equal([calls, attempts], [2, 2]);
    assert(result.reply.includes("Kép nélkül"));
    assert(events.some((e) => e.phase === "cad_repair"));
    assert(!events.some((e) => e.text === "FAILED PREVIEW"));
    equal(events.filter((e) => e.mesh).length, 2);
    assert(events.some((e) => e.artifact?.name === "design.validation.json"));
  }));

Deno.test("CAD graph: second geometric failure stops; same plan is never re-executed", () =>
  isolated(async () => {
    configureLiteLLM();
    for (const unchanged of [false, true]) {
      let calls = 0, attempts = 0;
      globalThis.fetch = () => {
        calls++;
        return Promise.resolve(
          designResponse(
            calls === 1 || unchanged ? explicitCylinder : {
              ...explicitCylinder,
              steps: [{ ...explicitCylinder.steps[0], id: "repaired" }],
            },
          ),
        );
      };
      const events: WorkflowEvent[] = [];
      const result = await workflow(
        { ...input, text: "Építs hengert" },
        new AbortController().signal,
        async (e) => {
          events.push(e);
        },
        {
          buildCad: async function* () {
            attempts++;
            yield { kind: "started" };
            throw unsupportedContact();
          },
        },
      );
      equal(calls, 2);
      equal(attempts, unchanged ? 1 : 2);
      assert(!events.some((e) => e.mesh || e.artifact));
      assert(
        result.reply.includes(unchanged ? "változatlan" : "nem készült el"),
      );
    }
  }));

Deno.test("CAD graph: infrastructure errors and cancellation never spend a repair call", () =>
  isolated(async () => {
    configureLiteLLM();
    for (const abort of [false, true]) {
      let calls = 0;
      const controller = new AbortController();
      globalThis.fetch = () => {
        calls++;
        return Promise.resolve(designResponse());
      };
      let caught = false;
      try {
        await workflow(
          { ...input, text: "Építs hengert" },
          controller.signal,
          async () => {},
          {
            buildCad: async function* () {
              yield { kind: "started" };
              if (abort) controller.abort();
              throw abort
                ? unsupportedContact()
                : new Error("Native executable missing");
            },
          },
        );
      } catch {
        caught = true;
      }
      assert(caught);
      equal(calls, 1);
    }
  }));

Deno.test("CAD graph: valid mesh with wrong intent is withheld, repaired once and reviewed again", () =>
  isolated(async () => {
    configureLiteLLM();
    let calls = 0, reviews = 0, attempts = 0;
    globalThis.fetch = (_url, init) => {
      calls++;
      if (calls === 2) {
        const body = JSON.parse(String(init?.body));
        assert(JSON.stringify(body).includes("Wrong overall length"));
        assert(
          JSON.stringify(body).includes('"intent"') ||
            JSON.stringify(body).includes("intent"),
        );
      }
      return Promise.resolve(
        designResponse({
          ...explicitCylinder,
          steps: [{
            ...explicitCylinder.steps[0],
            id: calls === 1 ? "wrong" : "correct",
          }],
        }),
      );
    };
    const events: WorkflowEvent[] = [];
    await workflow(
      { ...input, text: "Henger 20 mm hosszúsággal" },
      new AbortController().signal,
      async (e) => {
        events.push(e);
      },
      {
        buildCad: async function* () {
          attempts++;
          yield {
            kind: "editable_proposal",
            text: attempts === 1 ? "wrong shape" : "correct shape",
            mesh: repairMesh,
          };
          yield { kind: "completed" };
        },
        reviewCad: async (request) => {
          reviews++;
          equal(request.measurements.bounds, {
            min: [0, 0, 0],
            max: [1, 1, 1],
          });
          return {
            accepted: reviews === 2,
            reason: reviews === 1
              ? "Wrong overall length"
              : "Accepted by independent reviewer",
          };
        },
      },
    );
    equal([calls, attempts, reviews], [2, 2, 2]);
    assert(!events.some((e) => e.text === "wrong shape"));
    assert(events.some((e) => e.text === "correct shape"));
    const report = events.find((e) =>
      e.artifact?.name === "design.validation.json"
    )!.artifact!;
    const data = JSON.parse(new TextDecoder().decode(report.bytes));
    equal(data.designIntent, "reviewed");
    equal(data.intentReview.method, "language_model");
    equal(data.exactProof, false);
  }));

Deno.test("CAD graph: semantic second rejection or reviewer outage never publishes geometry", () =>
  isolated(async () => {
    configureLiteLLM();
    for (const outage of [false, true]) {
      let calls = 0, reviews = 0;
      globalThis.fetch = () => {
        calls++;
        return Promise.resolve(
          designResponse({
            ...explicitCylinder,
            steps: [{ ...explicitCylinder.steps[0], id: `attempt${calls}` }],
          }),
        );
      };
      const events: WorkflowEvent[] = [];
      let threw = false;
      try {
        await workflow(
          { ...input, text: "Henger 20 mm hosszúsággal" },
          new AbortController().signal,
          async (e) => {
            events.push(e);
          },
          {
            buildCad: async function* () {
              yield { kind: "editable_proposal", mesh: repairMesh };
              yield { kind: "completed" };
            },
            reviewCad: async () => {
              reviews++;
              if (outage) throw new Error("Reviewer unavailable");
              return { accepted: false, reason: "Wrong overall length" };
            },
          },
        );
      } catch {
        threw = true;
      }
      equal(threw, outage);
      equal([calls, reviews], outage ? [1, 1] : [2, 2]);
      assert(!events.some((e) => e.mesh || e.artifact));
    }
  }));
