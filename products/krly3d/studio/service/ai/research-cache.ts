import { createHash } from "node:crypto";
import type { Research, UsageCall } from "./expert-types.ts";
import { configuredModel, ProviderError, requireConfig } from "./provider.ts";
import { research, researchEnabled, validResearchQuery } from "./research.ts";

export interface ResearchCacheConfig {
  gateway: string;
  model: string;
  credential: string;
}

function currentConfig(): ResearchCacheConfig | null {
  if (!researchEnabled()) return null;
  const c = requireConfig();
  return {
    gateway: c.base,
    model: configuredModel("LITELLM_RESEARCH_MODEL", c.model),
    credential: c.key,
  };
}

function partition(config: ResearchCacheConfig | null): string | null {
  if (!config) return null;
  return createHash("sha256").update(JSON.stringify([
    config.gateway,
    config.model,
    config.credential,
  ])).digest("hex");
}

function checkCancellation(signal: AbortSignal): void {
  if (signal.aborted) {
    throw new ProviderError("aborted", "Provider request was cancelled.");
  }
}

/** Process-local LRU of completed public research; no in-flight sharing or disk IO. */
export function createResearchCache(options: {
  now?: () => number;
  research?: typeof research;
  config?: () => ResearchCacheConfig | null;
} = {}) {
  const now = options.now ?? Date.now;
  const run = options.research ?? research;
  const config = options.config ?? currentConfig;
  const entries = new Map<string, { expires: number; value: Research }>();
  const ttl = 60 * 60 * 1000;

  return async function cachedResearch(
    query: string,
    signal: AbortSignal,
    onUsage: (call: UsageCall) => void,
    onHit?: () => void,
  ): Promise<Research> {
    // Validate before normalization so whitespace cannot hide forbidden input.
    if (
      !validResearchQuery(query) || !(signal instanceof AbortSignal) ||
      typeof onUsage !== "function"
    ) {
      throw new ProviderError(
        "invalid_input",
        "One bounded public research query and a usage callback are required.",
      );
    }
    checkCancellation(signal);
    const scope = partition(config());
    if (!scope) {
      throw new ProviderError(
        "not_configured",
        "Web research is not configured.",
      );
    }
    const normalized = query.trim().replace(/\s+/gu, " ");
    const key = JSON.stringify([scope, normalized]);
    const time = now();
    for (const [key, entry] of entries) {
      if (entry.expires <= time) entries.delete(key);
    }
    const hit = entries.get(key);
    if (hit) {
      entries.delete(key);
      entries.set(key, hit);
      const result = structuredClone(hit.value);
      onHit?.();
      checkCancellation(signal);
      return result;
    }

    const result = await run(normalized, signal, onUsage);
    checkCancellation(signal);
    // Configuration may rotate while a request is pending. Do not cache it then.
    let unchanged = false;
    try {
      unchanged = partition(config()) === scope;
    } catch {
      // A successful result is still usable if configuration became unavailable.
    }
    if (unchanged) {
      const entry = { expires: now() + ttl, value: structuredClone(result) };
      entries.delete(key);
      entries.set(key, entry);
      while (entries.size > 32) entries.delete(entries.keys().next().value!);
    }
    return structuredClone(result);
  };
}

export const cachedResearch = createResearchCache();
