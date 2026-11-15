import { decodeResearch, research, researchEnabled } from "./research.ts";
import type { UsageCall } from "./expert-types.ts";
import {
  assert,
  configureLiteLLM,
  equal,
  isolated,
  rejects,
} from "./test_support.ts";

function response() {
  return {
    id: "private-response-id",
    status: "completed",
    error: null,
    incomplete_details: null,
    usage: {
      input_tokens: 300,
      output_tokens: 60,
      total_tokens: 360,
      input_tokens_details: { cached_tokens: 100 },
    },
    output: [
      {
        type: "web_search_call",
        status: "completed",
        action: { type: "search", query: "steel density" },
      },
      {
        type: "message",
        role: "assistant",
        status: "completed",
        content: [{
          type: "output_text",
          text: "A forrás szerint az érték az anyagminőségtől függ.",
          annotations: [{
            type: "url_citation",
            start_index: 0,
            end_index: 8,
            title: "Anyagadatlap",
            url: "https://example.org/material",
          }],
        }],
      },
    ],
  };
}
async function run(test: () => Promise<void>) {
  const keys = [
    "STUDIO_RESEARCH_ENABLED",
    "LITELLM_RESEARCH_MODEL",
    "LITELLM_USAGE_RATES_JSON",
  ];
  const prior = keys.map((k) => Deno.env.get(k));
  try {
    keys.forEach((k) => Deno.env.delete(k));
    await isolated(async () => {
      configureLiteLLM();
      await test();
    });
  } finally {
    keys.forEach((k, i) => {
      if (prior[i] === undefined) Deno.env.delete(k);
      else Deno.env.set(k, prior[i]!);
    });
  }
}
const signal = () => new AbortController().signal;

Deno.test("research: explicit opt-in, alias fallback, bounded authenticated Responses search", () =>
  run(async () => {
    let calls = 0;
    const usage: UsageCall[] = [];
    globalThis.fetch = (_url, _init) => {
      throw new Error("availability must not fetch");
    };
    equal(researchEnabled(), false);
    await rejects(
      () => research("steel density", signal(), (u) => usage.push(u)),
      "not_configured",
    );
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    Deno.env.set("LITELLM_CHAT_MODEL", "chat-alias");
    equal(researchEnabled(), true);
    let expectedModel = "chat-alias";
    globalThis.fetch = (url, init) => {
      calls++;
      equal(String(url), "https://gateway.example.invalid/prefix/v1/responses");
      equal(init?.redirect, "error");
      equal(
        (init?.headers as Record<string, string>).authorization,
        "Bearer unit-test-key",
      );
      const body = JSON.parse(String(init?.body));
      equal(body.model, expectedModel);
      equal(body.input, "steel density");
      equal(body.tools, [{ type: "web_search", search_context_size: "low" }]);
      equal(body.max_tool_calls, 1);
      equal(body.max_output_tokens, 600);
      equal(body.tool_choice, "required");
      equal(body.store, false);
      assert(!("messages" in body) && !("previous_response_id" in body));
      return Promise.resolve(
        Response.json(response(), {
          headers: { "x-litellm-response-cost": "0.012" },
        }),
      );
    };
    const result = await research(
      "steel density",
      signal(),
      (u) => usage.push(u),
    );
    equal(result.sources, [{
      title: "Anyagadatlap",
      url: "https://example.org/material",
    }]);
    equal(usage[0].searchCalls, 1);
    equal(usage[0].cachedTokens, 100);
    equal(usage[0].costUsd, 0.012);
    equal(usage[0].costKind, "reported");
    assert(!JSON.stringify(usage).includes("private-response-id"));
    Deno.env.set("LITELLM_RESEARCH_MODEL", "research-alias");
    expectedModel = "research-alias";
    await research("steel density", signal(), (u) => usage.push(u));
    equal(calls, 2);
    Deno.env.set("LITELLM_RESEARCH_MODEL", "bad alias");
    equal(researchEnabled(), false);
  }));

Deno.test("research: only real annotation citations, deduplicated and bounded", async () => {
  const body = response();
  const part = body.output[1].content![0];
  const completeFact = "A forrás szerint az érték az anyagminőségtől függ.";
  part.text = completeFact + " " + "X".repeat(3000) + ".";
  part.annotations = Array.from(
    { length: 10 },
    (_, i) => ({
      type: "url_citation",
      start_index: 0,
      end_index: 8,
      title: `Source ${i}`,
      url: `https://example.org/${i}`,
    }),
  );
  part.annotations.unshift(part.annotations[0]);
  part.annotations.unshift({
    ...part.annotations[0],
    url: "javascript:alert(1)",
  });
  part.annotations.unshift({
    ...part.annotations[1],
    url: "https://user:password@example.org/",
  });
  const result = decodeResearch(body);
  equal(result.summary, completeFact);
  equal(result.sources.length, 6);
  equal(result.sources[0].url, "https://example.org/0");
  for (
    const mutation of [
      "no_annotations",
      "fake_text_urls",
      "no_search",
      "two_searches",
      "failed",
      "refusal",
      "bad_span",
      "bad_url",
      "chat_schema",
    ]
  ) {
    const candidate = response();
    const output = candidate.output[1].content![0];
    if (mutation === "no_annotations" || mutation === "fake_text_urls") {
      output.annotations = [];
      output.text = "A made-up source https://example.org/fake";
    }
    if (mutation === "no_search") candidate.output.shift();
    if (mutation === "two_searches") candidate.output.push(candidate.output[0]);
    if (mutation === "failed") candidate.status = "failed";
    if (mutation === "refusal") output.type = "refusal";
    if (mutation === "bad_span") output.annotations[0].end_index = 9999;
    if (mutation === "bad_url") output.annotations[0].url = "file:///secret";
    if (mutation === "chat_schema") {
      Object.assign(output.annotations[0], {
        url: undefined,
        url_citation: output.annotations[0],
      });
    }
    await rejects(
      () => Promise.resolve(decodeResearch(candidate)),
      "invalid_response",
    );
  }
});

Deno.test("research: bounded public query, no retry on failure and cancellation accounting", () =>
  run(async () => {
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let calls = 0;
    const usage: UsageCall[] = [];
    globalThis.fetch = () => {
      calls++;
      return Promise.resolve(
        Response.json({ status: "failed" }, {
          headers: { "x-litellm-response-cost": "0.01" },
        }),
      );
    };
    for (
      const query of [
        "",
        "x".repeat(241),
        "secret=abc",
        "see https://private.example",
        "a@private.example",
        "first\nsecond",
        "read /home/private/project",
      ]
    ) {
      await rejects(
        () => research(query, signal(), (u) => usage.push(u)),
        "invalid_input",
      );
    }
    equal(calls, 0);
    await rejects(
      () => research("steel density", signal(), (u) => usage.push(u)),
      "invalid_response",
    );
    equal(calls, 1);
    equal(usage[0].costKind, "reported");
    const controller = new AbortController();
    controller.abort();
    await rejects(
      () => research("steel density", controller.signal, (u) => usage.push(u)),
      "aborted",
    );
    equal(calls, 1);
    globalThis.fetch = () => Promise.resolve(Response.json(response()));
    await research("steel density", signal(), (u) => usage.push(u));
    equal(usage.at(-1)?.costKind, "unknown");
  }));

Deno.test("research: overlong summaries keep complete sentences and respect both limits", async () => {
  const body = response();
  const part = body.output[1].content![0];
  const fact =
    "A forrás csak a megadott feltételek mellett igazolja az eredményt.";
  part.text = fact + " " + "További feltétel ".repeat(80) + ".";
  equal(decodeResearch(body).summary, fact);
  // A word-heavy response can exceed 180 words while still below 900 characters.
  const sentence = "A ".repeat(59) + "B.";
  part.text = Array(4).fill(sentence).join(" ");
  const summary = decodeResearch(body).summary;
  equal(summary, Array(3).fill(sentence).join(" "));
  equal(summary.split(/\s+/).length, 180);
  assert(summary.length <= 900);
  part.text = "x".repeat(901) + ".";
  await rejects(
    () => Promise.resolve(decodeResearch(body)),
    "invalid_response",
  );
});

Deno.test("research: concise evidence instructions and actual billed tokens survive over-budget output", () =>
  run(async () => {
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    const calls: UsageCall[] = [];
    globalThis.fetch = (_url, init) => {
      const request = JSON.parse(String(init?.body));
      assert(request.instructions.includes("900 characters AND 180 words"));
      assert(
        request.instructions.includes("3 concise, directly relevant facts"),
      );
      assert(request.instructions.includes("no headings"));
      assert(
        request.instructions.includes(
          "only what the retrieved sources actually support",
        ),
      );
      assert(
        request.instructions.includes("does not establish that alternatives"),
      );
      const body = response();
      body.usage.input_tokens = 8586;
      body.usage.output_tokens = 619;
      body.usage.total_tokens = 9205;
      body.output[1].content![0].text += " " +
        "Túl hosszú folytatás ".repeat(100) + ".";
      return Promise.resolve(
        Response.json(body, {
          headers: { "x-litellm-response-cost": "0.0294248" },
        }),
      );
    };
    const result = await research(
      "anyagadatlap feltételei",
      signal(),
      (call) => calls.push(call),
    );
    equal(result.summary, response().output[1].content![0].text);
    equal(calls.length, 1);
    equal(calls[0].outputTokens, 619);
    equal(calls[0].costUsd, 0.0294248);
    equal(calls[0].costKind, "reported");
  }));
