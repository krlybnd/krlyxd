import { collectUsage, type UsageContext } from "./usage.ts";
import { assert, equal } from "./test_support.ts";

const context: UsageContext = {
  stage: "expert",
  model: "chat-alias",
  onUsage: () => {},
};
const payload = {
  usage: {
    prompt_tokens: 100,
    completion_tokens: 20,
    total_tokens: 120,
    prompt_tokens_details: { cached_tokens: 40 },
  },
};
async function rates(value: string, run: () => void | Promise<void>) {
  const prior = Deno.env.get("LITELLM_USAGE_RATES_JSON");
  try {
    Deno.env.set("LITELLM_USAGE_RATES_JSON", value);
    await run();
  } finally {
    if (prior === undefined) Deno.env.delete("LITELLM_USAGE_RATES_JSON");
    else Deno.env.set("LITELLM_USAGE_RATES_JSON", prior);
  }
}

Deno.test("usage: chat/Responses tokens, cache, fresh public UUID and cost header", () =>
  rates("", () => {
    const headers = new Headers({
      "x-litellm-response-cost": "0.003",
      "x-request-id": "private",
    });
    const call = collectUsage(context, headers, payload);
    equal(call.inputTokens, 100);
    equal(call.outputTokens, 20);
    equal(call.cachedTokens, 40);
    equal(call.totalTokens, 120);
    equal(call.tokensKnown, true);
    equal(call.costKind, "reported");
    equal(call.costUsd, 0.003);
    assert(/^[\da-f-]{36}$/.test(call.id));
    assert(!JSON.stringify(call).includes("private"));
    const responses = collectUsage(
      { ...context, stage: "research" },
      undefined,
      {
        usage: {
          input_tokens: 50,
          output_tokens: 5,
          input_tokens_details: { cached_tokens: 10 },
        },
        output: [{ type: "web_search_call" }],
      },
    );
    equal(responses.totalTokens, 55);
    equal(responses.tokensKnown, true);
    equal(responses.searchCalls, 1);
    equal(responses.costKind, "unknown");
    assert(responses.id !== call.id);
    equal(
      collectUsage(context, new Headers({ "x-litellm-response-cost": "0" }))
        .costKind,
      "reported",
    );
  }));

Deno.test("usage: missing/malformed usage and costs stay explicitly unknown", () =>
  rates("", () => {
    for (
      const value of [
        undefined,
        null,
        {},
        { usage: null },
        { usage: { prompt_tokens: "100", completion_tokens: 2 } },
        { usage: { input_tokens: -1, output_tokens: 3 } },
        { usage: { input_tokens: 1.5, output_tokens: 3 } },
        {
          usage: {
            input_tokens: Number.MAX_SAFE_INTEGER + 1,
            output_tokens: 3,
          },
        },
        { usage: { input_tokens: 10, output_tokens: 3, total_tokens: 999 } },
        { usage: { input_tokens: 10, output_tokens: 3, total_tokens: null } },
        {
          usage: {
            input_tokens: 10,
            output_tokens: 3,
            input_tokens_details: "invalid",
          },
        },
        {
          usage: {
            input_tokens: 10,
            output_tokens: 3,
            input_tokens_details: { cached_tokens: null },
          },
        },
        {
          usage: {
            input_tokens: 10,
            output_tokens: 3,
            input_tokens_details: { cached_tokens: 11 },
          },
        },
        {
          usage: {
            input_tokens: 10,
            output_tokens: 3,
            input_tokens_details: { cached_tokens: "2" },
          },
        },
      ]
    ) {
      const call = collectUsage(context, undefined, value);
      equal(call.tokensKnown, false);
      equal(call.costKind, "unknown");
    }
    for (
      const value of [
        "",
        " ",
        "NaN",
        "Infinity",
        "-1",
        "null",
        "0x10",
        "0.1 dollars",
        "1e999",
      ]
    ) {
      equal(
        collectUsage(
          context,
          new Headers({ "x-litellm-response-cost": value }),
          payload,
        ).costKind,
        "unknown",
      );
    }
  }));

Deno.test("usage: only verified alias tariffs estimate; cached input not double billed; research never token-only", async () => {
  const tariff = {
    verified: true,
    inputUsdPerMillion: 2,
    outputUsdPerMillion: 8,
    cachedInputUsdPerMillion: 0.5,
  };
  await rates(JSON.stringify({ "chat-alias": tariff }), () => {
    const call = collectUsage(context, undefined, payload);
    equal(call.costKind, "estimated");
    equal(call.costUsd, (60 * 2 + 20 * 8 + 40 * 0.5) / 1e6);
    equal(
      collectUsage(
        context,
        new Headers({ "x-litellm-response-cost": "0.1" }),
        payload,
      ).costKind,
      "reported",
    );
    equal(
      collectUsage({ ...context, stage: "research" }, undefined, payload)
        .costKind,
      "unknown",
    );
    equal(
      collectUsage({ ...context, model: "another-alias" }, undefined, payload)
        .costKind,
      "unknown",
    );
  });
  for (
    const configured of [
      "{",
      "null",
      JSON.stringify({ "chat-alias": { ...tariff, verified: false } }),
      JSON.stringify({
        "chat-alias": { ...tariff, cachedInputUsdPerMillion: undefined },
      }),
      JSON.stringify({ "chat-alias": { ...tariff, inputUsdPerMillion: -1 } }),
    ]
  ) {
    await rates(
      configured,
      () =>
        equal(collectUsage(context, undefined, payload).costKind, "unknown"),
    );
  }
});
