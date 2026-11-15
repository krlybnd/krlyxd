// Plain application contracts; protobuf is only the outer transport adapter.
import type { PlanInput, PlanResult } from "./provider.ts";
import type { CadRecipe } from "./cad-recipe.ts";

export type UsageCall = {
  id: string;
  stage: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  totalTokens: number;
  costUsd: number;
  costKind: "reported" | "estimated" | "unknown";
  tokensKnown: boolean;
  searchCalls: number;
};
export type Source = { title: string; url: string };
export type ExpertQuestion = {
  title: string;
  reason: string;
  options: { id: string; label: string; description: string }[];
};
export type Answer = {
  question: string;
  selected: { id: string; label: string }[];
};
export type Research = { summary: string; sources: Source[] };
export type ExpertInput = PlanInput & {
  answers?: Answer[];
  research?: Research;
  researchAllowed: boolean;
  researchOnly?: boolean;
  /** Shared workflow budget: schema and native repairs consume the same one turn. */
  planRepairAllowed?: boolean;
  repair?: {
    recipe: CadRecipe;
    code:
      | "boolean_contact"
      | "unsupported_geometry"
      | "verification"
      | "intent";
    reason?: string;
  };
  onUsage: (call: UsageCall) => void;
};
export type ExpertResult = Omit<PlanResult, "action"> & {
  action: "reply" | "reconstruct" | "ask" | "research" | "design";
  question?: ExpertQuestion;
  searchQuery?: string;
  design?: CadRecipe;
};
