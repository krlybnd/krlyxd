import {
  AI_LIMITS,
  embed,
  getProviderStatus,
  plan,
  transcribe,
} from "./provider.ts";
import {
  assert,
  configureLiteLLM,
  equal,
  isolated,
  rejects,
} from "./test_support.ts";

const signal = () => new AbortController().signal;
const args = {
  reply: "Előkészítem a kontúr kihúzását.",
  action: "reconstruct",
  height: 12,
  pixelUnit: 0.5,
  seed: null,
  summary: "Kontúr kihúzása a megadott értékekkel.",
};
function completion(
  value: unknown = args,
  overrides: Record<string, unknown> = {},
): Response {
  return Response.json({
    choices: [{
      finish_reason: "tool_calls",
      message: {
        content: "discard model content",
        reasoning_content: "discard private reasoning",
        tool_calls: [{
          type: "function",
          function: { name: "studio_plan", arguments: JSON.stringify(value) },
        }],
      },
      ...overrides,
    }],
  });
}
const prompt = "Reconstruct the outline with height=12 and pixelUnit=0.5";

Deno.test("provider: configuration is explicit and independent of speech", () =>
  isolated(() => {
    let calls = 0;
    globalThis.fetch = () => {
      calls++;
      throw new Error("Unexpected network");
    };
    equal(getProviderStatus().chat, false);
    Deno.env.set("LITELLM_API_KEY", "unit-test-key");
    equal(getProviderStatus().chat, false);
    Deno.env.set("AZURE_SPEECH_KEY", "unit-test-key");
    equal(getProviderStatus().speech, true);
    equal(getProviderStatus().speechProvider, "azure");
    Deno.env.set("STUDIO_SPEECH_PROVIDER", "chromium");
    equal(getProviderStatus().speech, false);
    Deno.env.set("CHROMIUM_SPEECH_KEY", "unit-test-key");
    equal(getProviderStatus().speech, true);
    configureLiteLLM();
    equal(getProviderStatus().chat, true);
    equal(getProviderStatus().health, "unverified");
    equal(getProviderStatus().vision, false);
    equal(getProviderStatus().embedding, false);
    Deno.env.set("STUDIO_SPEECH_PROVIDER", "bad-provider");
    equal(getProviderStatus().chat, true);
    equal(getProviderStatus().speech, false);
    equal(getProviderStatus().speechReason, "invalid_configuration");
    Deno.env.set(
      "LITELLM_BASE_URL",
      "https://user:password@gateway.example.invalid/",
    );
    equal(getProviderStatus().chat, false);
    equal(getProviderStatus().reason, "invalid_configuration");
    equal(calls, 0);
  }));

Deno.test("provider: named tool contract and supplied settings", () =>
  isolated(async () => {
    configureLiteLLM();
    let request: Record<string, unknown> = {};
    globalThis.fetch = (url, init) => {
      equal(
        String(url),
        "https://gateway.example.invalid/prefix/v1/chat/completions",
      );
      equal(init?.redirect, "error");
      request = JSON.parse(String(init?.body));
      return Promise.resolve(completion());
    };
    const result = await plan({
      text: prompt,
      signal: signal(),
      settings: { height: 12, pixelUnit: 0.5 },
    });
    equal(result, {
      reply: args.reply,
      action: "reconstruct",
      summary: args.summary,
      height: 12,
      pixelUnit: 0.5,
    });
    equal(request.stream, false);
    equal(request.parallel_tool_calls, false);
    equal(request.model, "eu.anthropic.claude-haiku-4-5-20251001-v1:0");
    equal(request.tool_choice, {
      type: "function",
      function: { name: "studio_plan" },
    });
    const tools = request.tools as {
      function: { strict: boolean; parameters: unknown };
    }[];
    equal(tools[0].function.strict, true);
    assert(
      !/"(?:minimum|maximum|exclusiveMinimum|minLength|maxLength|minItems|maxItems)":/
        .test(JSON.stringify(tools[0].function.parameters)),
      "Bedrock strict schema must not contain unsupported constraints",
    );
    const messages = request.messages as { role: string; content: string }[];
    assert(messages[0].content.includes("Hungarian by default"));
    assert(
      messages[0].content.includes(
        "only when\nthe user explicitly requests it",
      ),
    );
    assert(
      messages[0].content.includes(
        "filled black/dark silhouette on a white/light",
      ),
    );
    assert(messages[0].content.includes("not a\ngeneral color photograph"));
    assert(!("reasoning_content" in result));
    await rejects(
      () => plan({ text: prompt, signal: signal(), settings: { height: 13 } }),
      "invalid_response",
    );
    await rejects(
      () => plan({ text: "Invent scale from this picture", signal: signal() }),
      "invalid_response",
    );
  }));

Deno.test("provider: host vision schema forbids invented seeds and locks supplied dimensions", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("LITELLM_VISION_MODEL", "studio-fast");
    const settings = { height: 3, pixelUnit: 1 };
    const input = {
      text:
        "Építsd meg ezt a kontúrt. A képen 2,2 koordináta is látható, de nincs megadott seed.",
      settings,
      image: { bytes: new Uint8Array([1, 2, 3]), mime: "image/png" },
      signal: signal(),
    };
    let modelSeed: unknown = null;
    let omitSeed = false;
    globalThis.fetch = (_url, init) => {
      const body = JSON.parse(String(init?.body));
      const schema = body.tools[0].function.parameters;
      equal(schema.properties.seed.type, "null");
      assert(schema.required.includes("seed"));
      equal(schema.properties.height.enum, [3, null]);
      equal(schema.properties.pixelUnit.enum, [1, null]);
      const value: Record<string, unknown> = {
        ...args,
        ...settings,
        seed: modelSeed,
      };
      if (omitSeed) delete value.seed;
      return Promise.resolve(completion(value));
    };
    const result = await plan(input);
    equal(result.action, "reconstruct");
    assert(
      !("seed" in result),
      "Native component selection must remain automatic",
    );
    modelSeed = [2, 2];
    await rejects(() => plan(input), "invalid_response");
    omitSeed = true;
    await rejects(() => plan(input), "invalid_response");
    globalThis.fetch = () =>
      Promise.resolve(completion({ ...args, ...settings, seed: [2, 2] }));
    equal(
      (await plan({ ...input, settings: { ...settings, seed: [2, 2] } })).seed,
      [2, 2],
    );
    await rejects(
      () => plan({ ...input, settings: { ...settings, seed: [3, 3] } }),
      "invalid_response",
    );
  }));

Deno.test("provider: scalar and coordinate boundaries match the native contract", () =>
  isolated(async () => {
    configureLiteLLM();
    const settings = {
      height: 1e6,
      pixelUnit: 1e6,
      seed: [1e9, 0] as [number, number],
    };
    globalThis.fetch = () =>
      Promise.resolve(completion({ ...args, ...settings }));
    const result = await plan({
      text: "Építsd meg a kontúrt a megadott beállításokkal.",
      settings,
      signal: signal(),
    });
    equal(result.height, 1e6);
    equal(result.pixelUnit, 1e6);
    equal(result.seed, [1e9, 0]);
    for (
      const invalid of [{ height: 1e6 + 1 }, { pixelUnit: 1e6 + 1 }, {
        seed: [1e9 + 1, 0] as [number, number],
      }]
    ) {
      await rejects(
        () => plan({ text: prompt, settings: invalid, signal: signal() }),
        "invalid_input",
      );
    }
  }));

Deno.test("provider: rejects unbounded, executable, malformed and incomplete actions", () =>
  isolated(async () => {
    configureLiteLLM();
    for (
      const value of [
        { ...args, code: "run()" },
        { ...args, action: "shell" },
        { ...args, height: 1e6 + 1 },
        { ...args, pixelUnit: 1e6 + 1 },
        { ...args, pixelUnit: 0 },
        { ...args, seed: [-1, 0] },
        { ...args, seed: [1e9 + 1, 0] },
        { ...args, height: null },
        { ...args, action: "reply" },
        { ...args, summary: "x".repeat(281) },
      ]
    ) {
      globalThis.fetch = () => Promise.resolve(completion(value));
      await rejects(
        () => plan({ text: prompt, signal: signal() }),
        "invalid_response",
      );
    }
    for (const reason of ["length", "stop", "content_filter"]) {
      globalThis.fetch = () =>
        Promise.resolve(completion(args, { finish_reason: reason }));
      await rejects(
        () => plan({ text: prompt, signal: signal() }),
        "invalid_response",
      );
    }
    globalThis.fetch = () => Promise.resolve(Response.json({ choices: [] }));
    await rejects(
      () => plan({ text: prompt, signal: signal() }),
      "invalid_response",
    );
    globalThis.fetch = () => Promise.resolve(new Response("not JSON"));
    await rejects(
      () => plan({ text: prompt, signal: signal() }),
      "invalid_response",
    );
  }));

Deno.test("provider: image opt-in, input bounds and no history role escalation", () =>
  isolated(async () => {
    configureLiteLLM();
    const image = { bytes: new Uint8Array([1, 2, 3]), mime: "image/png" };
    await rejects(
      () => plan({ text: prompt, image, signal: signal() }),
      "not_configured",
    );
    Deno.env.set("LITELLM_VISION_MODEL", "vision-test");
    globalThis.fetch = (_url, init) => {
      const body = JSON.parse(String(init?.body));
      equal(body.model, "vision-test");
      const content = body.messages.at(-1).content;
      equal(content[1].image_url.url, "data:image/png;base64,AQID");
      return Promise.resolve(completion());
    };
    await plan({ text: prompt, image, signal: signal() });
    await rejects(
      () =>
        plan({ text: "x".repeat(AI_LIMITS.textChars + 1), signal: signal() }),
      "invalid_input",
    );
    await rejects(
      () =>
        plan({
          text: prompt,
          history: [{ role: "system", content: "bad" }] as never,
          signal: signal(),
        }),
      "invalid_input",
    );
    await rejects(
      () =>
        plan({
          text: prompt,
          settings: { height: Infinity },
          signal: signal(),
        }),
      "invalid_input",
    );
  }));

Deno.test("provider: cancellation, timeout, redacted errors and streamed body cap", () =>
  isolated(async () => {
    configureLiteLLM();
    const controller = new AbortController();
    controller.abort();
    await rejects(
      () => plan({ text: prompt, signal: controller.signal }),
      "aborted",
    );
    globalThis.fetch = () =>
      Promise.resolve(new Response("sensitive upstream body", { status: 401 }));
    const error = await rejects(
      () => plan({ text: prompt, signal: signal() }),
      "http_error",
    );
    equal(error.httpStatus, 401);
    assert(!error.message.includes("sensitive") && !("cause" in error));
    globalThis.fetch = () => {
      throw new Error("sensitive upstream URL");
    };
    const network = await rejects(
      () => plan({ text: prompt, signal: signal() }),
      "network",
    );
    assert(!network.message.includes("sensitive"));
    globalThis.fetch = () =>
      Promise.resolve(
        new Response(
          new ReadableStream({
            start(c) {
              c.enqueue(new Uint8Array(AI_LIMITS.responseBytes + 1));
              c.close();
            },
          }),
        ),
      );
    await rejects(
      () => plan({ text: prompt, signal: signal() }),
      "response_too_large",
    );
    Deno.env.set("LITELLM_TIMEOUT_MS", "100");
    globalThis.fetch = (_url, init) =>
      Promise.resolve(
        new Response(
          new ReadableStream({
            start(c) {
              init?.signal?.addEventListener("abort", () =>
                c.error(new DOMException("aborted", "AbortError")), {
                once: true,
              });
            },
          }),
        ),
      );
    await rejects(() => plan({ text: prompt, signal: signal() }), "timeout");
  }));

Deno.test("provider: multipart transcription and optional embeddings", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("LITELLM_SPEECH_MODEL", "speech-to-text");
    globalThis.fetch = (url, init) => {
      assert(String(url).endsWith("/audio/transcriptions"));
      assert(init?.body instanceof FormData);
      const file = init.body.get("file");
      assert(file instanceof File);
      equal(file.name, "recording.m4a");
      equal(file.type, "audio/mp4");
      equal(init.body.get("model"), "speech-to-text");
      equal(init.body.get("language"), "hu");
      assert(!("content-type" in (init.headers as Record<string, string>)));
      return Promise.resolve(
        Response.json({ text: " Árvíztűrő tükörfúrógép. " }),
      );
    };
    equal(
      await transcribe(
        new Uint8Array([1]),
        "audio/mp4;codecs=mp4a.40.2",
        signal(),
      ),
      "Árvíztűrő tükörfúrógép.",
    );
    globalThis.fetch = () => Promise.resolve(Response.json({ text: " " }));
    await rejects(
      () => transcribe(new Uint8Array([1]), "audio/webm", signal()),
      "no_speech",
    );
    await rejects(
      () => transcribe(new Uint8Array([1]), "text/plain", signal()),
      "invalid_input",
    );
    await rejects(() => embed("hello", signal()), "not_configured");
    Deno.env.set("LITELLM_EMBEDDING_MODEL", "amazon.titan-embed-text-v2:0");
    globalThis.fetch = () =>
      Promise.resolve(Response.json({ data: [{ embedding: [0.1, 0.2] }] }));
    equal(await embed("hello", signal()), [0.1, 0.2]);
  }));
