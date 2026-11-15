import { reviewCad, type CadReviewInput } from "./cad-review.ts";
import { assert, configureLiteLLM, equal, isolated, rejects } from "./test_support.ts";
import type { UsageCall } from "./expert-types.ts";

function input(onUsage: (call: UsageCall) => void = () => {}): CadReviewInput {
  return {
    text: "15 mm sugarú, 8 mm magas gyűrű, 6 mm belső sugárral",
    draft: {
      spec: '#cad_spec("ring", parameters: (R: 15mm, r: 6mm, h: 8mm), requirements: (require("outer", R > r)))',
      target: '#cad_target("ring", spec_hash: "__SPEC_HASH__", checks: (require_span("height", z, h, h)))',
    },
    plan: [{ id: "ring", operation: "annulus", inputs: [] }, { id: "body", operation: "extrude", inputs: ["ring"] }],
    measurements: {
      bounds: { min: [-0.015, -0.015, 0], max: [0.015, 0.015, 0.008] },
      volume: 5.9e-6,
      area: 0.001,
    },
    signal: new AbortController().signal,
    onUsage,
  };
}

function completion(accepted: boolean, reason: string) {
  return {
    choices: [{ finish_reason: "tool_calls", message: { content: null, tool_calls: [{
      type: "function", function: { name: "studio_cad_review", arguments: JSON.stringify({ accepted, reason }) },
    }] } }],
    usage: { prompt_tokens: 100, completion_tokens: 30, total_tokens: 130 },
  };
}

Deno.test("CAD review receives typed requirements, synthesized plan and measured bounds", () =>
  isolated(async () => {
    configureLiteLLM();
    const usage: UsageCall[] = [];
    const data = input((call) => usage.push(call));
    let calls = 0;
    globalThis.fetch = (_url, init) => {
      calls++;
      const body = JSON.parse(String(init?.body));
      equal(body.messages.length, 2);
      equal(JSON.parse(body.messages[1].content), {
        text: data.text, answers: [], draft: data.draft,
        plan: data.plan, measurements: data.measurements,
      });
      assert(body.messages[0].content.includes("silently omitted"));
      equal(body.tools[0].function.name, "studio_cad_review");
      return Promise.resolve(Response.json(completion(true, "A méret és a furat ellenőrizve.")));
    };
    equal(await reviewCad(data), { accepted: true, reason: "A méret és a furat ellenőrizve." });
    equal(calls, 1);
    equal(usage.map((call) => call.stage), ["cad_review"]);
  }));

Deno.test("CAD review rejects missing typed evidence before an AI call", () =>
  isolated(async () => {
    configureLiteLLM();
    let called = false;
    globalThis.fetch = () => { called = true; throw new Error("unexpected provider call"); };
    await rejects(() => reviewCad({ ...input(), plan: [] }), "invalid_input");
    await rejects(() => reviewCad({ ...input(), measurements: {
      ...input().measurements, volume: Number.NaN,
    } }), "invalid_input");
    assert(!called);
  }));

Deno.test("CAD review records usage even when provider response is malformed", () =>
  isolated(async () => {
    configureLiteLLM();
    const usage: UsageCall[] = [];
    globalThis.fetch = () => Promise.resolve(Response.json(completion(true, "")));
    await rejects(() => reviewCad(input((call) => usage.push(call))), "invalid_response");
    equal(usage.map((call) => call.stage), ["cad_review"]);
  }));
