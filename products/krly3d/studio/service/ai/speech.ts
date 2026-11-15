/** Server-only STT: reference-compatible Chromium, Azure Speech, or LiteLLM. */
import { AI_LIMITS, transcribe as transcribeLiteLLM } from "./provider.ts";
import { ProviderError } from "./errors.ts";
import { getSpeechConfig } from "./speech-config.ts";

const RATE = 16_000;
const MAX_PCM = RATE * 2 * 60;
const RESPONSE_LIMIT = 128 * 1024;
const TYPES = new Set([
  "audio/webm",
  "audio/ogg",
  "audio/mp4",
  "audio/m4a",
  "audio/x-m4a",
  "audio/mpeg",
  "audio/mp3",
  "audio/wav",
  "audio/x-wav",
  "audio/flac",
  "audio/x-flac",
]);

function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

async function collect(
  stream: ReadableStream<Uint8Array>,
  limit: number,
  signal: AbortSignal,
  audio = false,
): Promise<Uint8Array> {
  const reader = stream.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  let done = false;
  const cancel = () => {
    void reader.cancel().catch(() => {});
  };
  signal.addEventListener("abort", cancel, { once: true });
  try {
    signal.throwIfAborted();
    while (true) {
      const part = await reader.read();
      signal.throwIfAborted();
      if (part.done) {
        done = true;
        break;
      }
      size += part.value.byteLength;
      if (size > limit) {
        throw new ProviderError(
          audio ? "invalid_audio" : "response_too_large",
          audio
            ? "Decoded recording exceeds the 60-second audio limit."
            : "Speech provider response exceeds the size limit.",
        );
      }
      chunks.push(part.value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    return bytes;
  } finally {
    signal.removeEventListener("abort", cancel);
    if (!done) cancel();
    reader.releaseLock();
  }
}

/** No shell, network protocols, playlist demuxers, files, or unbounded stderr. */
async function ffmpeg(
  bytes: Uint8Array,
  args: string[],
  limit: number,
  signal: AbortSignal,
): Promise<Uint8Array> {
  signal.throwIfAborted();
  let child: Deno.ChildProcess;
  try {
    child = new Deno.Command("ffmpeg", {
      args: [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-max_alloc",
        "33554432",
        "-threads",
        "1",
        "-filter_threads",
        "1",
        ...args,
      ],
      stdin: "piped",
      stdout: "piped",
      stderr: "null",
      clearEnv: true,
      env: { PATH: "/usr/bin:/bin" },
    }).spawn();
  } catch {
    throw new ProviderError(
      "decoder_unavailable",
      "Audio conversion needs ffmpeg and server permission to run it.",
    );
  }
  const kill = () => {
    try {
      child.kill("SIGKILL");
    } catch { /* Already exited. */ }
  };
  signal.addEventListener("abort", kill, { once: true });
  const writer = child.stdin.getWriter();
  const status = child.status;
  const writing = (async () => {
    try {
      await writer.write(bytes);
      await writer.close();
    } finally {
      writer.releaseLock();
    }
  })();
  const reading = collect(child.stdout, limit, signal, true);
  try {
    const [, output, exit] = await Promise.all([writing, reading, status]);
    if (!exit.success || !output.length) {
      throw new ProviderError(
        "invalid_audio",
        "The recording could not be decoded.",
      );
    }
    return output;
  } catch (error) {
    kill();
    await Promise.allSettled([writing, reading, status]);
    if (signal.aborted) signal.throwIfAborted();
    if (error instanceof ProviderError) throw error;
    throw new ProviderError(
      "invalid_audio",
      "The recording could not be decoded.",
    );
  } finally {
    signal.removeEventListener("abort", kill);
  }
}

async function decodePCM(
  bytes: Uint8Array,
  signal: AbortSignal,
): Promise<Uint8Array> {
  return await ffmpeg(
    bytes,
    [
      "-protocol_whitelist",
      "pipe",
      "-format_whitelist",
      "wav,matroska,webm,ogg,mov,mp4,m4a,mp3,flac,aac",
      "-i",
      "pipe:0",
      "-map",
      "0:a:0",
      "-vn",
      "-sn",
      "-dn",
      "-ac",
      "1",
      "-ar",
      String(RATE),
      "-c:a",
      "pcm_s16le",
      "-f",
      "s16le",
      "pipe:1",
    ],
    MAX_PCM,
    signal,
  );
}

function wav(pcm: Uint8Array): Uint8Array {
  const bytes = new Uint8Array(44 + pcm.length);
  const view = new DataView(bytes.buffer);
  const tag = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i++) {
      bytes[offset + i] = text.charCodeAt(i);
    }
  };
  tag(0, "RIFF");
  view.setUint32(4, 36 + pcm.length, true);
  tag(8, "WAVE");
  tag(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, RATE, true);
  view.setUint32(28, RATE * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  tag(36, "data");
  view.setUint32(40, pcm.length, true);
  bytes.set(pcm, 44);
  return bytes;
}

async function post(
  url: string,
  headers: HeadersInit,
  bytes: Uint8Array,
  signal: AbortSignal,
): Promise<string> {
  const response = await fetch(url, {
    method: "POST",
    headers,
    body: new Uint8Array(bytes),
    signal,
    redirect: "error",
  });
  if (!response.ok) {
    void response.body?.cancel().catch(() => {});
    throw new ProviderError(
      "http_error",
      `Speech request failed (HTTP ${response.status}).`,
      response.status,
    );
  }
  if (!response.body) {
    throw new ProviderError(
      "invalid_response",
      "Speech provider returned an empty response.",
    );
  }
  const declared = Number(response.headers.get("content-length"));
  if (declared > RESPONSE_LIMIT) {
    void response.body.cancel().catch(() => {});
    throw new ProviderError(
      "response_too_large",
      "Speech provider response exceeds the size limit.",
    );
  }
  return new TextDecoder().decode(
    await collect(response.body, RESPONSE_LIMIT, signal),
  );
}

/** Parse the reference's newline-delimited Chromium response without silent failures. */
function chromiumTranscript(body: string): string {
  let valid = false;
  for (const line of body.split(/\r?\n/)) {
    if (!line.trim()) continue;
    let payload: unknown;
    try {
      payload = JSON.parse(line);
    } catch {
      throw new ProviderError(
        "invalid_response",
        "Speech provider returned invalid JSON.",
      );
    }
    if (!object(payload) || !Array.isArray(payload.result)) {
      throw new ProviderError(
        "invalid_response",
        "Speech provider returned an invalid transcript.",
      );
    }
    valid = true;
    for (const result of payload.result) {
      if (!object(result) || !Array.isArray(result.alternative)) continue;
      for (const alternative of result.alternative) {
        if (
          object(alternative) && typeof alternative.transcript === "string" &&
          alternative.transcript.trim()
        ) {
          return transcript(alternative.transcript);
        }
      }
    }
  }
  if (!valid) {
    throw new ProviderError(
      "invalid_response",
      "Speech provider returned an invalid transcript.",
    );
  }
  throw new ProviderError(
    "no_speech",
    "No speech was detected. Try recording again.",
  );
}

function transcript(text: string): string {
  if (text.length > AI_LIMITS.textChars) {
    throw new ProviderError(
      "invalid_response",
      "Speech transcript exceeds the size limit.",
    );
  }
  if (!text.trim()) {
    throw new ProviderError(
      "no_speech",
      "No speech was detected. Try recording again.",
    );
  }
  return text.trim();
}

/** Same RPC contract for every provider; keys and transcoding stay server-side. */
export async function transcribe(
  bytes: Uint8Array,
  mime: string,
  signal: AbortSignal,
): Promise<string> {
  if (!(signal instanceof AbortSignal)) {
    throw new ProviderError("invalid_input", "An AbortSignal is required.");
  }
  if (signal.aborted) {
    throw new ProviderError("aborted", "Speech request was cancelled.");
  }
  if (
    !(bytes instanceof Uint8Array) || !bytes.length ||
    bytes.length > AI_LIMITS.audioBytes ||
    typeof mime !== "string" ||
    !TYPES.has(mime.split(";", 1)[0].trim().toLowerCase())
  ) {
    throw new ProviderError(
      "invalid_input",
      "A supported audio recording of at most 8 MiB is required.",
    );
  }
  const config = getSpeechConfig();
  if (config.provider === "litellm") {
    return await transcribeLiteLLM(bytes, mime, signal);
  }
  if (!config.key) {
    throw new ProviderError(
      "not_configured",
      "The selected speech provider needs a server-side key.",
    );
  }
  const controller = new AbortController();
  let timedOut = false;
  const onAbort = () => controller.abort();
  signal.addEventListener("abort", onAbort, { once: true });
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, config.timeoutMs);
  try {
    const pcm = await decodePCM(bytes, controller.signal);
    if (config.provider === "chromium") {
      const flac = await ffmpeg(
        pcm,
        [
          "-protocol_whitelist",
          "pipe",
          "-f",
          "s16le",
          "-ar",
          String(RATE),
          "-ac",
          "1",
          "-i",
          "pipe:0",
          "-c:a",
          "flac",
          "-f",
          "flac",
          "pipe:1",
        ],
        MAX_PCM + 128 * 1024,
        controller.signal,
      );
      // Compatibility with the existing working laptop path. Never log this URL:
      // this protocol requires its client key as a query parameter.
      const url = new URL("https://www.google.com/speech-api/v2/recognize");
      url.search = new URLSearchParams({
        client: "chromium",
        lang: config.language,
        key: config.key,
      }).toString();
      return chromiumTranscript(
        await post(
          url.href,
          { "content-type": "audio/x-flac; rate=16000" },
          flac,
          controller.signal,
        ),
      );
    }
    const url = new URL(
      `https://${config.region}.stt.speech.microsoft.com/speech/recognition/conversation/cognitiveservices/v1`,
    );
    url.search = new URLSearchParams({
      language: config.language,
      format: "simple",
    }).toString();
    const body = await post(
      url.href,
      {
        "Ocp-Apim-Subscription-Key": config.key,
        "content-type": "audio/wav; codecs=audio/pcm; samplerate=16000",
        accept: "application/json",
      },
      wav(pcm),
      controller.signal,
    );
    let result: unknown;
    try {
      result = JSON.parse(body);
    } catch {
      throw new ProviderError(
        "invalid_response",
        "Speech provider returned invalid JSON.",
      );
    }
    if (!object(result)) {
      throw new ProviderError(
        "invalid_response",
        "Speech provider returned an invalid transcript.",
      );
    }
    if (
      ["NoMatch", "InitialSilenceTimeout", "BabbleTimeout"].includes(
        result.RecognitionStatus as string,
      )
    ) {
      throw new ProviderError(
        "no_speech",
        "No speech was detected. Try recording again.",
      );
    }
    if (
      result.RecognitionStatus !== "Success" ||
      typeof result.DisplayText !== "string"
    ) {
      throw new ProviderError(
        "invalid_response",
        "Speech provider did not recognize the recording.",
      );
    }
    return transcript(result.DisplayText);
  } catch (error) {
    if (signal.aborted) {
      throw new ProviderError("aborted", "Speech request was cancelled.");
    }
    if (timedOut) {
      throw new ProviderError(
        "timeout",
        "Speech decoding or transcription timed out.",
      );
    }
    if (error instanceof ProviderError) throw error;
    throw new ProviderError(
      "network",
      "Could not complete speech transcription.",
    );
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", onAbort);
  }
}
