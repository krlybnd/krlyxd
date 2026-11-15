import { Code, ConnectError } from "@connectrpc/connect";
import type { ExpertQuestion } from "./ai/expert-types.ts";
import type { WorkflowInput, WorkflowOutcome } from "./workflow.ts";

export type Question = ExpertQuestion & { id: string };
export type PendingQuestion = {
  question: Question;
  input: WorkflowInput;
  created: number;
};
export const QUESTION_TTL_MS = 30 * 60 * 1000;

export function suspend(
  input: WorkflowInput,
  outcome: WorkflowOutcome,
): PendingQuestion {
  if (!outcome.question) {
    throw new ConnectError("Hiányzó kérdés.", Code.Internal);
  }
  return {
    question: { ...outcome.question, id: crypto.randomUUID() },
    input: {
      ...input,
      history: input.history.map((item) => ({ ...item })),
      answers: [...(input.answers ?? [])],
      research: outcome.research,
      usage: [...outcome.usage],
    },
    created: Date.now(),
  };
}

export function resume(
  pending: PendingQuestion | undefined,
  id: string,
  answers: string[],
): WorkflowInput {
  if (
    !pending || pending.question.id !== id ||
    Date.now() - pending.created > QUESTION_TTL_MS
  ) {
    throw new ConnectError(
      "Ez a kérdés már lezárult vagy lejárt. Indíts új kérést.",
      Code.FailedPrecondition,
    );
  }
  if (
    !answers.length || answers.length > pending.question.options.length ||
    new Set(answers).size !== answers.length ||
    answers.some((id) =>
      !pending.question.options.some((option) => option.id === id)
    )
  ) {
    throw new ConnectError(
      "Válassz legalább egyet a felkínált válaszokból.",
      Code.InvalidArgument,
    );
  }
  const selected = pending.question.options.filter((option) =>
    answers.includes(option.id)
  )
    .map(({ id, label }) => ({ id, label }));
  return {
    ...pending.input,
    answers: [...(pending.input.answers ?? []), {
      question: pending.question.title,
      selected,
    }],
  };
}
