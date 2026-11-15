import { Code, ConnectError } from "@connectrpc/connect";
import { QUESTION_TTL_MS, resume, suspend } from "./questions.ts";
import type { WorkflowInput, WorkflowOutcome } from "./workflow.ts";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const input: WorkflowInput = {
  mode: "chat",
  text: "Építs a rajzból",
  height: 8,
  pixelUnit: .025,
  tolerance: .05,
  history: [{ role: "user", content: "eredeti" }],
};
const outcome: WorkflowOutcome = {
  reply: "Pontosítsunk",
  usage: [],
  question: {
    title: "Milyen célra?",
    reason: "Ez határozza meg a formát.",
    options: [
      { id: "A", label: "Dísz", description: "A körvonal fontos" },
      { id: "B", label: "Szemléltetés", description: "Egyszerű modell" },
    ],
  },
};

Deno.test("pending question preserves settings and canonical multiselect answers", () => {
  const pending = suspend(input, outcome);
  const next = resume(pending, pending.question.id, ["B", "A"]);
  assert(next.height === 8 && next.pixelUnit === .025, "original settings");
  assert(
    next.answers?.[0].selected.map((x) => x.id).join() === "A,B",
    "canonical order",
  );
  assert(
    !pending.input.answers?.length,
    "resume must not consume retryable question",
  );
  assert(pending.input.history !== input.history, "history snapshot");
  const second = suspend(next, outcome);
  assert(second.question.id !== pending.question.id, "fresh question identity");
  assert(
    resume(second, second.question.id, ["A"]).answers?.length === 2,
    "sequential answers",
  );
});

Deno.test("unknown, duplicate, empty and expired answers cannot resume", () => {
  const pending = suspend(input, outcome);
  for (const ids of [[], ["A", "A"], ["C"]]) {
    let code;
    try {
      resume(pending, pending.question.id, ids);
    } catch (e) {
      code = ConnectError.from(e).code;
    }
    assert(code === Code.InvalidArgument, "invalid selection rejected");
  }
  for (
    const p of [undefined, {
      ...pending,
      created: Date.now() - QUESTION_TTL_MS - 1,
    }]
  ) {
    let code;
    try {
      resume(p, pending.question.id, ["A"]);
    } catch (e) {
      code = ConnectError.from(e).code;
    }
    assert(code === Code.FailedPrecondition, "stale question rejected");
  }
});
