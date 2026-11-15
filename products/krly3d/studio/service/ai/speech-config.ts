import { ProviderError } from "./errors.ts";

export type SpeechProvider = "azure" | "chromium" | "litellm";

/** Keys stay within server modules. Never return this object from an RPC. */
export function getSpeechConfig(): {
  provider: SpeechProvider;
  key: string;
  region: string;
  language: string;
  timeoutMs: number;
  languageField: "language" | "languages" | "none";
} {
  let selected: string;
  let azureKey: string;
  let chromiumKey: string;
  let region: string;
  let language: string;
  let rawTimeout: string;
  let languageField: string;
  try {
    selected = Deno.env.get("STUDIO_SPEECH_PROVIDER")?.trim() || "auto";
    azureKey = Deno.env.get("AZURE_SPEECH_KEY")?.trim() || "";
    chromiumKey = Deno.env.get("CHROMIUM_SPEECH_KEY")?.trim() || "";
    region = Deno.env.get("AZURE_SPEECH_REGION")?.trim() || "swedencentral";
    language = Deno.env.get("STUDIO_SPEECH_LANGUAGE")?.trim() || "hu-HU";
    rawTimeout = Deno.env.get("STUDIO_SPEECH_TIMEOUT_MS")?.trim() || "30000";
    languageField = Deno.env.get("LITELLM_SPEECH_LANGUAGE_FIELD")?.trim() ||
      "language";
  } catch {
    throw new ProviderError(
      "invalid_configuration",
      "Server environment access is unavailable.",
    );
  }
  if (!["auto", "azure", "chromium", "litellm"].includes(selected)) {
    throw new ProviderError(
      "invalid_configuration",
      "Select the azure, chromium, or litellm speech provider.",
    );
  }
  const provider =
    (selected === "auto"
      ? azureKey ? "azure" : chromiumKey ? "chromium" : "litellm"
      : selected) as SpeechProvider;
  const key = provider === "azure"
    ? azureKey
    : provider === "chromium"
    ? chromiumKey
    : "";
  if (
    provider === "litellm" &&
    !["language", "languages", "none"].includes(languageField)
  ) {
    throw new ProviderError(
      "invalid_configuration",
      "LiteLLM speech language field must be language, languages, or none.",
    );
  }
  const timeoutMs = Number(rawTimeout);
  if (
    !/^[a-zA-Z]{2,3}(?:-[a-zA-Z0-9]{2,8}){1,2}$/.test(language) ||
    (provider === "azure" && !/^[a-z][a-z0-9]{1,40}$/.test(region)) ||
    /[\r\n]/.test(key) || key.length > 512 ||
    !Number.isInteger(timeoutMs) || timeoutMs < 100 || timeoutMs > 120_000
  ) {
    throw new ProviderError(
      "invalid_configuration",
      "Speech locale, region, key format, or timeout is invalid.",
    );
  }
  return {
    provider,
    key,
    region,
    language,
    timeoutMs,
    languageField: languageField as "language" | "languages" | "none",
  };
}
