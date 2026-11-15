import {
  cachedResearch,
  createResearchCache,
  type ResearchCacheConfig,
} from "./research-cache.ts";
import type { Research, UsageCall } from "./expert-types.ts";
import { ProviderError } from "./provider.ts";
import {
  assert,
  configureLiteLLM,
  equal,
  isolated,
  rejects,
} from "./test_support.ts";

const config = (): ResearchCacheConfig => ({
  gateway: "https://gateway.example.invalid/v1",
  model: "research-model",
  credential: "test-credential",
});
const result = (): Research => ({
  summary: "A cited public fact.",
  sources: [{ title: "Reference", url: "https://example.org/reference" }],
});
const signal = () => new AbortController().signal;
const noop = () => {};

Deno.test("research cache: whitespace keys preserve case and clone every boundary", async () => {
  let calls = 0;
  let hits = 0;
  const original = result();
  const cache = createResearchCache({
    config,
    research: (query) => {
      calls++;
      equal(query, calls === 1 ? "Pa density" : "pa density");
      return Promise.resolve(original);
    },
  });
  const first = await cache("  Pa   density  ", signal(), noop);
  first.sources[0].title = "poison";
  original.summary = "mutated provider object";
  const second = await cache("Pa density", signal(), () => {
    throw new Error("A cache hit must not report usage");
  }, () => hits++);
  equal(second, result());
  second.sources.push({ title: "poison", url: "https://example.org/poison" });
  equal(await cache("Pa  density", signal(), noop), result());
  equal(calls, 1);
  equal(hits, 1);
  await cache("pa density", signal(), noop);
  equal(calls, 2);
});

Deno.test("research cache: one-hour TTL starts at completion and hits do not extend it", async () => {
  let time = 0;
  let calls = 0;
  const cache = createResearchCache({
    config,
    now: () => time,
    research: () => {
      calls++;
      time += 100;
      return Promise.resolve(result());
    },
  });
  await cache("density", signal(), noop);
  time = 3_600_099;
  await cache("density", signal(), noop);
  equal(calls, 1);
  time = 3_600_100;
  await cache("density", signal(), noop);
  equal(calls, 2);
});

Deno.test("research cache: at most 32 entries across partitions with LRU eviction", async () => {
  let calls = 0;
  let current = config();
  const cache = createResearchCache({
    config: () => current,
    research: () => {
      calls++;
      return Promise.resolve(result());
    },
  });
  for (let i = 0; i < 32; i++) await cache(`query ${i}`, signal(), noop);
  await cache("query 0", signal(), noop);
  equal(calls, 32);
  current = { ...current, credential: "rotated-test-credential" };
  await cache("query 32", signal(), noop);
  current = config();
  await cache("query 0", signal(), noop);
  equal(calls, 33);
  await cache("query 1", signal(), noop);
  equal(calls, 34);
});

Deno.test("research cache: gateway, model, credential partitions and disabled config", async () => {
  let current: ResearchCacheConfig | null = config();
  let calls = 0;
  const cache = createResearchCache({
    config: () => current,
    research: () => {
      calls++;
      return Promise.resolve(result());
    },
  });
  await cache("density", signal(), noop);
  for (const field of ["gateway", "model", "credential"] as const) {
    current = { ...config(), [field]: "changed" };
    await cache("density", signal(), noop);
  }
  equal(calls, 4);
  current = config();
  await cache("density", signal(), noop);
  equal(calls, 4);
  current = null;
  await rejects(() => cache("density", signal(), noop), "not_configured");
});

Deno.test("research cache: validates public input and cancellation even on hits", async () => {
  let calls = 0;
  const cache = createResearchCache({
    config,
    research: () => {
      calls++;
      return Promise.resolve(result());
    },
  });
  await cache("steel density", signal(), noop);
  for (
    const query of [
      "",
      "x".repeat(241),
      "secret=abc",
      "https://private.example",
      "a@private.example",
      "steel\ndensity",
      "steel\tdensity",
      "/home/private",
    ]
  ) {
    await rejects(() => cache(query, signal(), noop), "invalid_input");
  }
  await rejects(
    () => cache("steel density", signal(), null as unknown as typeof noop),
    "invalid_input",
  );
  const controller = new AbortController();
  controller.abort();
  await rejects(
    () => cache("steel density", controller.signal, noop),
    "aborted",
  );
  equal(calls, 1);
});

Deno.test("research cache: failures and late cancelled successes are not cached", async () => {
  let calls = 0;
  const controller = new AbortController();
  const cache = createResearchCache({
    config,
    research: () => {
      calls++;
      if (calls === 1) throw new ProviderError("invalid_response", "Failed.");
      if (calls === 2) controller.abort();
      return Promise.resolve(result());
    },
  });
  await rejects(() => cache("density", signal(), noop), "invalid_response");
  await rejects(() => cache("density", controller.signal, noop), "aborted");
  await cache("density", signal(), noop);
  await cache("density", signal(), noop);
  equal(calls, 3);
});

Deno.test("research cache: concurrent identical requests have independent cancellation", async () => {
  const pending: { signal: AbortSignal; resolve: (r: Research) => void }[] = [];
  const cache = createResearchCache({
    config,
    research: (_query, signal) =>
      new Promise<Research>((resolve, reject) => {
        pending.push({ signal, resolve });
        signal.addEventListener("abort", () => {
          reject(new ProviderError("aborted", "Cancelled."));
        }, { once: true });
      }),
  });
  const controller = new AbortController();
  const first = cache("density", controller.signal, noop);
  const secondSignal = signal();
  const second = cache("density", secondSignal, noop);
  equal(pending.length, 2);
  assert(pending[0].signal === controller.signal);
  assert(pending[1].signal === secondSignal);
  controller.abort();
  await rejects(() => first, "aborted");
  pending[1].resolve(result());
  equal(await second, result());
  equal(await cache("density", signal(), noop), result());
  equal(pending.length, 2);
});

Deno.test("research cache: configuration rotation during research prevents insertion", async () => {
  let current = config();
  let calls = 0;
  const cache = createResearchCache({
    config: () => current,
    research: () => {
      calls++;
      current = { ...config(), credential: "rotated" };
      return Promise.resolve(result());
    },
  });
  await cache("density", signal(), noop);
  current = config();
  await cache("density", signal(), noop);
  equal(calls, 2);
});

Deno.test("research cache: exported wrapper uses real research and real usage only", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    Deno.env.set("LITELLM_CHAT_MODEL", "fallback-model");
    let requests = 0;
    let hits = 0;
    let expectedModel = "fallback-model";
    const usage: UsageCall[] = [];
    globalThis.fetch = (_url, init) => {
      requests++;
      const body = JSON.parse(String(init?.body));
      equal(body.model, expectedModel);
      equal(body.input, "steel density");
      return Promise.resolve(Response.json({
        status: "completed",
        output: [
          { type: "web_search_call", status: "completed" },
          {
            type: "message",
            role: "assistant",
            status: "completed",
            content: [{
              type: "output_text",
              text: "Public fact.",
              annotations: [{
                type: "url_citation",
                title: "Reference",
                url: "https://example.org/reference",
                start_index: 0,
                end_index: 6,
              }],
            }],
          },
        ],
      }));
    };
    const run = () =>
      cachedResearch(
        " steel  density ",
        signal(),
        (u) => usage.push(u),
        () => hits++,
      );
    await run();
    await run();
    equal(requests, 1);
    equal(usage.length, 1);
    equal(hits, 1);
    for (
      const [name, value] of [
        ["LITELLM_BASE_URL", "https://other.example.invalid/v1"],
        ["LITELLM_API_KEY", "rotated-unit-test-key"],
        ["LITELLM_CHAT_MODEL", "changed-fallback"],
        ["LITELLM_RESEARCH_MODEL", "explicit-research"],
      ]
    ) {
      Deno.env.set(name, value);
      if (name.endsWith("MODEL")) expectedModel = value;
      await run();
    }
    equal(requests, 5);
    equal(usage.length, 5);
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "false");
    await rejects(run, "not_configured");
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    Deno.env.set("LITELLM_RESEARCH_MODEL", "invalid alias");
    await rejects(run, "not_configured");
    equal(requests, 5);
  }));
