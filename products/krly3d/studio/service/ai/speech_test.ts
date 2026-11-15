import { transcribe } from "./speech.ts";
import { getProviderStatus } from "./provider.ts";
import {
  assert,
  configureLiteLLM,
  equal,
  isolated,
  rejects,
  wavFixture,
} from "./test_support.ts";

const signal = () => new AbortController().signal;
function configure(provider: "chromium" | "azure"): void {
  Deno.env.set("STUDIO_SPEECH_PROVIDER", provider);
  Deno.env.set(
    provider === "azure" ? "AZURE_SPEECH_KEY" : "CHROMIUM_SPEECH_KEY",
    "unit-test-key",
  );
}

Deno.test("speech: reference-compatible FLAC request and Hungarian NDJSON transcript", () =>
  isolated(async () => {
    configure("chromium");
    equal(getProviderStatus().speech, true);
    equal(getProviderStatus().chat, false);
    globalThis.fetch = (url, init) => {
      const endpoint = new URL(String(url));
      equal(
        endpoint.origin + endpoint.pathname,
        "https://www.google.com/speech-api/v2/recognize",
      );
      equal(endpoint.searchParams.get("client"), "chromium");
      equal(endpoint.searchParams.get("lang"), "hu-HU");
      equal(endpoint.searchParams.get("key"), "unit-test-key");
      equal(
        new Headers(init?.headers).get("content-type"),
        "audio/x-flac; rate=16000",
      );
      assert(init?.body instanceof Uint8Array);
      equal(new TextDecoder().decode(init.body.subarray(0, 4)), "fLaC");
      equal(init?.redirect, "error");
      return Promise.resolve(
        new Response(
          '{"result":[]}\n{"result":[{"alternative":[{"transcript":"Legyen a magasság tizenkét egység."}],"final":true}]}\n',
        ),
      );
    };
    equal(
      await transcribe(wavFixture(), "audio/wav", signal()),
      "Legyen a magasság tizenkét egység.",
    );
  }));

Deno.test("speech: real ffmpeg handles MediaRecorder WebM, MP4 and Ogg formats", () =>
  isolated(async () => {
    configure("chromium");
    globalThis.fetch = () =>
      Promise.resolve(
        new Response(
          '{"result":[{"alternative":[{"transcript":"Magyar hang."}]}]}',
        ),
      );
    for (
      const [format, codec, mime, extra] of [
        ["webm", "libopus", "audio/webm;codecs=opus", []],
        ["ogg", "libopus", "audio/ogg;codecs=opus", []],
        ["mp4", "aac", "audio/mp4;codecs=mp4a.40.2", [
          "-movflags",
          "frag_keyframe+empty_moov",
        ]],
      ] as const
    ) {
      const child = new Deno.Command("ffmpeg", {
        args: [
          "-hide_banner",
          "-loglevel",
          "error",
          "-i",
          "pipe:0",
          "-c:a",
          codec,
          ...extra,
          "-f",
          format,
          "pipe:1",
        ],
        stdin: "piped",
        stdout: "piped",
        stderr: "null",
      }).spawn();
      const writer = child.stdin.getWriter();
      await writer.write(wavFixture());
      await writer.close();
      writer.releaseLock();
      const result = await child.output();
      assert(result.success, "Audio fixture conversion failed");
      equal(await transcribe(result.stdout, mime, signal()), "Magyar hang.");
    }
  }));

Deno.test("speech: Azure default uses regional REST and valid 16 kHz WAV", () =>
  isolated(async () => {
    Deno.env.set("AZURE_SPEECH_KEY", "unit-test-key");
    Deno.env.set("AZURE_SPEECH_REGION", "swedencentral");
    equal(getProviderStatus().speechProvider, "azure");
    globalThis.fetch = (url, init) => {
      const endpoint = new URL(String(url));
      equal(endpoint.hostname, "swedencentral.stt.speech.microsoft.com");
      equal(endpoint.searchParams.get("language"), "hu-HU");
      const headers = new Headers(init?.headers);
      equal(headers.get("Ocp-Apim-Subscription-Key"), "unit-test-key");
      equal(
        headers.get("content-type"),
        "audio/wav; codecs=audio/pcm; samplerate=16000",
      );
      assert(init?.body instanceof Uint8Array);
      equal(new TextDecoder().decode(init.body.subarray(0, 4)), "RIFF");
      const view = new DataView(
        init.body.buffer,
        init.body.byteOffset,
        init.body.byteLength,
      );
      equal(view.getUint32(24, true), 16000);
      equal(view.getUint16(22, true), 1);
      equal(view.getUint32(40, true), init.body.length - 44);
      return Promise.resolve(
        Response.json({
          RecognitionStatus: "Success",
          DisplayText: "Készíts egy modellt.",
        }),
      );
    };
    equal(
      await transcribe(wavFixture(), "audio/wav", signal()),
      "Készíts egy modellt.",
    );
  }));

Deno.test("speech: explicit typed failures for silence, malformed responses and invalid audio", () =>
  isolated(async () => {
    configure("chromium");
    for (
      const [body, code] of [
        ['{"result":[]}', "no_speech"],
        ["not JSON", "invalid_response"],
        ['{"error":"upstream details"}', "invalid_response"],
      ]
    ) {
      globalThis.fetch = () => Promise.resolve(new Response(body));
      await rejects(
        () => transcribe(wavFixture(), "audio/wav", signal()),
        code,
      );
    }
    globalThis.fetch = () =>
      Promise.resolve(new Response("secret error body", { status: 403 }));
    const error = await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "http_error",
    );
    assert(!error.message.includes("secret") && !("cause" in error));
    await rejects(
      () => transcribe(new Uint8Array([1, 2, 3]), "audio/webm", signal()),
      "invalid_audio",
    );
    await rejects(
      () => transcribe(wavFixture(), "text/plain", signal()),
      "invalid_input",
    );
    // More than 60 seconds must fail instead of silently truncating the request.
    await rejects(
      () => transcribe(wavFixture(60.1), "audio/wav", signal()),
      "invalid_audio",
    );
  }));

Deno.test("speech: cancellation and timeout include decoding and response consumption", () =>
  isolated(async () => {
    configure("chromium");
    const cancelled = new AbortController();
    cancelled.abort();
    await rejects(
      () => transcribe(wavFixture(), "audio/wav", cancelled.signal),
      "aborted",
    );
    Deno.env.set("STUDIO_SPEECH_TIMEOUT_MS", "100");
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
    await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "timeout",
    );
  }));

Deno.test("speech: explicit LiteLLM option retains the existing multipart contract", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("STUDIO_SPEECH_PROVIDER", "litellm");
    Deno.env.set("LITELLM_SPEECH_MODEL", "speech-to-text");
    Deno.env.set("AZURE_SPEECH_KEY", "unit-test-key");
    equal(getProviderStatus().speechProvider, "litellm");
    globalThis.fetch = (url, init) => {
      assert(String(url).endsWith("/audio/transcriptions"));
      assert(init?.body instanceof FormData);
      equal(init.body.get("language"), "hu");
      return Promise.resolve(Response.json({ text: "Maradjon a LiteLLM." }));
    };
    equal(
      await transcribe(wavFixture(), "audio/wav", signal()),
      "Maradjon a LiteLLM.",
    );
  }));

Deno.test("speech: LiteLLM language hints support both legacy and current aliases", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("STUDIO_SPEECH_PROVIDER", "litellm");
    Deno.env.set("LITELLM_SPEECH_MODEL", "opaque-speech-alias");
    for (
      const [locale, field, name, expected] of [
        ["hu-HU", "language", "language", "hu"],
        ["hu-HU", "languages", "languages[]", "hu"],
        ["en-GB", "languages", "languages[]", "en"],
        ["hu-HU", "none", "", ""],
      ]
    ) {
      Deno.env.set("STUDIO_SPEECH_LANGUAGE", locale);
      Deno.env.set("LITELLM_SPEECH_LANGUAGE_FIELD", field);
      let calls = 0;
      globalThis.fetch = (_url, init) => {
        calls++;
        assert(init?.body instanceof FormData);
        equal(init.body.get("language"), name === "language" ? expected : null);
        equal(
          init.body.getAll("languages[]"),
          name === "languages[]" ? [expected] : [],
        );
        assert(
          !init.body.has("languages"),
          "Multipart lists must use bracketed field names",
        );
        return Promise.resolve(Response.json({ text: "Magyar hang." }));
      };
      equal(
        await transcribe(wavFixture(), "audio/wav", signal()),
        "Magyar hang.",
      );
      equal(calls, 1);
    }
    Deno.env.set("LITELLM_SPEECH_LANGUAGE_FIELD", "unsupported");
    await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "invalid_configuration",
    );
    equal(getProviderStatus().chat, true);
    equal(getProviderStatus().speech, false);
  }));

Deno.test("speech: abort during active conversion reaps the process before returning", () =>
  isolated(async () => {
    configure("chromium");
    let calls = 0;
    globalThis.fetch = () => {
      calls++;
      throw new Error("Unexpected provider request");
    };
    const controller = new AbortController();
    // transcribe starts ffmpeg synchronously before yielding at its first await.
    // Cancelling here exercises a real child process with active pipes.
    const pending = transcribe(wavFixture(30), "audio/wav", controller.signal);
    controller.abort();
    await rejects(() => pending, "aborted");
    equal(calls, 0);
  }));

Deno.test("speech: streamed response caps and network errors never reveal a keyed URL", () =>
  isolated(async () => {
    configure("chromium");
    globalThis.fetch = () =>
      Promise.resolve(
        new Response(
          new ReadableStream({
            start(c) {
              c.enqueue(new Uint8Array(128 * 1024 + 1));
              c.close();
            },
          }),
        ),
      );
    await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "response_too_large",
    );
    globalThis.fetch = (url) => {
      throw new Error(`Upstream failure at ${String(url)}`);
    };
    const error = await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "network",
    );
    assert(!error.message.includes("unit-test-key"));
    assert(!error.message.includes("google.com"));
    assert(!("cause" in error));
  }));

Deno.test("speech: pinned Azure errors and silence do not trigger another provider", () =>
  isolated(async () => {
    configure("azure");
    Deno.env.set("CHROMIUM_SPEECH_KEY", "unit-test-key");
    configureLiteLLM();
    Deno.env.set("LITELLM_SPEECH_MODEL", "speech-to-text");
    let calls = 0;
    globalThis.fetch = (url) => {
      calls++;
      assert(
        new URL(String(url)).hostname.endsWith(".stt.speech.microsoft.com"),
      );
      return Promise.resolve(Response.json({ RecognitionStatus: "NoMatch" }));
    };
    await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "no_speech",
    );
    equal(calls, 1);
    globalThis.fetch = () => {
      calls++;
      return Promise.resolve(
        new Response("upstream credentials", { status: 401 }),
      );
    };
    const error = await rejects(
      () => transcribe(wavFixture(), "audio/wav", signal()),
      "http_error",
    );
    equal(error.httpStatus, 401);
    equal(calls, 2);
    assert(!error.message.includes("credentials"));
  }));
