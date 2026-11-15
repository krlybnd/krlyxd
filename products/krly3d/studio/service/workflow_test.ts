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

Deno.test("text build sends requirements through typed synthesis, not a feature recipe", () =>
  isolated(async () => {
    if (!Deno.env.get("KRLYSCI_BIN")) return;
    configureLiteLLM();
    const original = await Deno.readTextFile(new URL("../../../../cad-knowledge/cases/solid-ring.typ", import.meta.url));
    const spec = original.split("#cad_plan(")[0];
    const target = `#cad_target(${original.split("#cad_target(")[1]}`
      .replace(/spec_hash: "[0-9a-f]{64}"/, 'spec_hash: "__SPEC_HASH__"');
    globalThis.fetch = () => Promise.resolve(Response.json({
      choices: [{ finish_reason: "tool_calls", message: { tool_calls: [{
        type: "function", function: { name: "studio_expert", arguments: JSON.stringify({
          reply: "A gyűrű specifikációja elkészült.", summary: "Típusos cél keresése.",
          decision: { action: "design", height: null, pixelUnit: null, seed: null,
            question: null, searchQuery: null, design: { spec, target } },
        }) },
      }] } }],
      usage: { prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 },
    }, { headers: { "x-litellm-response-cost": "0.000024" } }));
    const events: WorkflowEvent[] = [];
    const result = await workflow({ ...input, mode: "build", text: "15 mm outer, 6 mm inner, 8 mm tall ring" },
      new AbortController().signal, async (event) => { events.push(event); });
    assert(result.reply.includes("típusos követelményekből"));
    assert(events.some((event) => event.kind === "mesh" && event.mesh?.indices.length));
    assert(events.some((event) => event.artifact?.name === "design.typ"));
    assert(!events.some((event) => event.artifact?.name === "design.recipe.json"));
  }));
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
