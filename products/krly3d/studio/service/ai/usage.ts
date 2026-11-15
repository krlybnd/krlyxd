/** Usage is captured at the HTTP boundary, before semantic response validation. */
import type { UsageCall } from "./expert-types.ts";
export type { UsageCall } from "./expert-types.ts";

export interface UsageContext {
  stage: "expert" | "cad_review" | "cad_repair" | "research" | "image_search";
  model: string;
  onUsage: (call: UsageCall) => void;
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}
const count = (n: unknown): n is number =>
  typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
const rate = (n: unknown): n is number =>
  typeof n === "number" && Number.isFinite(n) && n >= 0;

/** Unknown numeric fields are transport placeholders, NEVER evidence of zero cost. */
export function collectUsage(
  context: UsageContext,
  headers?: Headers,
  payload?: unknown,
): UsageCall {
  const body = record(payload);
  const usage = record(body.usage);
  const input = usage.input_tokens !== undefined
    ? usage.input_tokens
    : usage.prompt_tokens;
  const output = usage.output_tokens !== undefined
    ? usage.output_tokens
    : usage.completion_tokens;
  const details = usage.input_tokens_details !== undefined
    ? usage.input_tokens_details
    : usage.prompt_tokens_details;
  const detailsValid = details === undefined ||
    (!!details && typeof details === "object" && !Array.isArray(details));
  const cachedValue = record(details).cached_tokens;
  const cached = cachedValue === undefined ? 0 : cachedValue;
  const total = usage.total_tokens !== undefined
    ? usage.total_tokens
    : (count(input) && count(output) ? input + output : undefined);
  const tokensKnown = detailsValid && count(input) && count(output) &&
    count(cached) &&
    count(total) && cached <= input && total === input + output;
  const call: UsageCall = {
    id: crypto.randomUUID(),
    stage: context.stage,
    model: context.model,
    inputTokens: count(input) ? input : 0,
    outputTokens: count(output) ? output : 0,
    cachedTokens: count(cached) ? cached : 0,
    totalTokens: count(total) ? total : 0,
    tokensKnown,
    costUsd: 0,
    costKind: "unknown",
    searchCalls: Array.isArray(body.output)
      ? body.output.filter((item) => record(item).type === "web_search_call")
        .length
      : 0,
  };
  const reported = headers?.get("x-litellm-response-cost")?.trim();
  if (reported && /^(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(reported)) {
    const cost = Number(reported);
    if (rate(cost)) return { ...call, costUsd: cost, costKind: "reported" };
  }
  // Search content/tool charges vary by route and may be outside token usage.
  // Never present a token-only estimate as the total research cost.
  if (!tokensKnown || context.stage === "research" || call.searchCalls) {
    return call;
  }
  try {
    const configured = JSON.parse(
      Deno.env.get("LITELLM_USAGE_RATES_JSON") || "{}",
    );
    const rates = record(record(configured)[context.model]);
    if (
      rates.verified !== true || !rate(rates.inputUsdPerMillion) ||
      !rate(rates.outputUsdPerMillion) ||
      (cached > 0 && !rate(rates.cachedInputUsdPerMillion))
    ) return call;
    const cost = ((input - cached) * rates.inputUsdPerMillion +
      output * rates.outputUsdPerMillion +
      cached *
        (rate(rates.cachedInputUsdPerMillion)
          ? rates.cachedInputUsdPerMillion
          : 0)) / 1e6;
    if (rate(cost)) return { ...call, costUsd: cost, costKind: "estimated" };
  } catch {
    /* Absent/malformed/unverified tariffs mean unknown, never free. */
  }
  return call;
}
