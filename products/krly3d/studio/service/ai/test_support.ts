import { ProviderError } from "./errors.ts";

export function assert(
  value: unknown,
  message = "Assertion failed",
): asserts value {
  if (!value) throw new Error(message);
}

export function equal(actual: unknown, expected: unknown): void {
  assert(JSON.stringify(actual) === JSON.stringify(expected), "Values differ");
}

export async function rejects(
  action: () => Promise<unknown>,
  code: string,
): Promise<ProviderError> {
  try {
    await action();
  } catch (error) {
    assert(error instanceof ProviderError, "Expected a safe ProviderError");
    equal(error.code, code);
    return error;
  }
  throw new Error("Expected rejection");
}

const KEYS = [
  "LITELLM_BASE_URL",
  "LITELLM_API_KEY",
  "LITELLM_CHAT_MODEL",
  "LITELLM_RESEARCH_MODEL",
  "LITELLM_IMAGE_SEARCH_MODEL",
  "LITELLM_VISION_MODEL",
  "LITELLM_SPEECH_MODEL",
  "LITELLM_EMBEDDING_MODEL",
  "LITELLM_TIMEOUT_MS",
  "LITELLM_SPEECH_TIMEOUT_MS",
  "STUDIO_RESEARCH_ENABLED",
  "LITELLM_SPEECH_LANGUAGE_FIELD",
  "STUDIO_SPEECH_PROVIDER",
  "STUDIO_SPEECH_LANGUAGE",
  "STUDIO_SPEECH_TIMEOUT_MS",
  "AZURE_SPEECH_KEY",
  "AZURE_SPEECH_REGION",
  "CHROMIUM_SPEECH_KEY",
];

/** Isolate tests from all real user credentials without printing/copying them. */
export async function isolated(run: () => void | Promise<void>): Promise<void> {
  const prior = new Map(KEYS.map((key) => [key, Deno.env.get(key)]));
  const fetch = globalThis.fetch;
  try {
    for (const key of KEYS) Deno.env.delete(key);
    await run();
  } finally {
    globalThis.fetch = fetch;
    for (const [key, value] of prior) {
      if (value === undefined) Deno.env.delete(key);
      else Deno.env.set(key, value);
    }
  }
}

export function configureLiteLLM(): void {
  Deno.env.set(
    "LITELLM_BASE_URL",
    "https://gateway.example.invalid/prefix/v1/",
  );
  Deno.env.set("LITELLM_API_KEY", "unit-test-key");
}

export function wavFixture(seconds = 0.1): Uint8Array {
  const size = Math.floor(seconds * 16000) * 2;
  const bytes = new Uint8Array(44 + size);
  const view = new DataView(bytes.buffer);
  for (
    const [offset, text] of [[0, "RIFF"], [8, "WAVE"], [12, "fmt "], [
      36,
      "data",
    ]] as const
  ) {
    for (let i = 0; i < text.length; i++) {
      bytes[offset + i] = text.charCodeAt(i);
    }
  }
  view.setUint32(4, 36 + size, true);
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 16000, true);
  view.setUint32(28, 32000, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  view.setUint32(40, size, true);
  return bytes;
}
