import { create } from "@bufbuild/protobuf";
import {
  createConnectRouter,
  createHandlerContext,
  type ServiceImpl,
} from "@connectrpc/connect";
import {
  RunRequestSchema,
  StudioService,
} from "../gen/krly/studio/v1/studio_pb.ts";
import { routes } from "./rpc.ts";
import type { workflow, WorkflowInput, WorkflowOutcome } from "./workflow.ts";
import type { UsageCall } from "./ai/expert-types.ts";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const charge = (id: string): UsageCall => ({
  id,
  stage: "expert",
  model: "test",
  inputTokens: 20,
  outputTokens: 10,
  cachedTokens: 0,
  totalTokens: 30,
  tokensKnown: true,
  costUsd: .000024,
  costKind: "reported",
  searchCalls: 0,
});
const question: WorkflowOutcome["question"] = {
  title: "Milyen célra?",
  reason: "Pontosítás",
  options: [{ id: "A", label: "Dísz", description: "Körvonal" }, {
    id: "B",
    label: "Szemléltetés",
    description: "Bemutató",
  }],
};

function harness(execute: typeof workflow) {
  const router = createConnectRouter();
  let impl: Partial<ServiceImpl<typeof StudioService>> = {};
  router.service = (_descriptor, service) => {
    impl = service as typeof impl;
    return router;
  };
  routes(router, { workflow: execute });
  return async (
    sessionId: string,
    questionId = "",
    answerIds: string[] = [],
  ) => {
    const ctx = createHandlerContext({
      service: StudioService,
      method: StudioService.method.run,
      protocolName: "test",
      requestMethod: "POST",
      url: "http://localhost/test",
    });
    const events = [];
    try {
      for await (
        const event of impl.run!(
          create(RunRequestSchema, {
            sessionId,
            questionId,
            answerIds,
            text: questionId ? "" : "Építs a rajzból",
            mode: "chat",
            height: questionId ? 99 : 8,
            pixelUnit: .025,
            tolerance: .05,
          }),
          ctx,
        )
      ) events.push(event);
    } finally {
      ctx.abort();
    }
    return events;
  };
}

Deno.test("HITL is sequential, session-bound and preserves original input and cumulative usage", async () => {
  const seen: WorkflowInput[] = [];
  const run = harness(async (input, _signal, emit) => {
    seen.push(input);
    const usage = [...(input.usage ?? []), charge(String(seen.length))];
    await emit({
      kind: "usage",
      phase: "usage",
      text: "usage",
      usage: { calls: usage },
    });
    return {
      reply: "Rendben",
      usage,
      question: (input.answers?.length ?? 0) < 2 ? question : undefined,
    };
  });
  const first = await run("original");
  const q1 = first.at(-1)?.question;
  assert(
    q1?.id && first.at(-1)?.kind === "question",
    "question suspends instead of completes",
  );
  assert(!first.some((e) => e.kind === "completed"), "no false completion");
  assert(
    (await run("intruder", q1.id, ["A"])).at(-1)?.kind === "error",
    "cross-session denied",
  );
  const second = await run("original", q1.id, ["B", "A"]);
  const q2 = second.at(-1)?.question;
  assert(q2?.id && q2.id !== q1.id, "next question has fresh id");
  assert(seen[1].height === 8, "resume ignores changed request settings");
  assert(seen[1].answers?.[0].selected.length === 2, "both choices retained");
  assert(
    (await run("original", q1.id, ["A"])).at(-1)?.kind === "error",
    "stale answer denied",
  );
  const final = await run("original", q2.id, ["A"]);
  assert(final.at(-1)?.kind === "completed", "answered sequence completes");
  assert(seen[2].answers?.length === 2, "full answers retained");
  assert(
    final.findLast((e) => e.usage)?.usage?.calls?.length === 3,
    "three calls, no duplicate accumulation",
  );
  assert(
    (await run("original", q2.id, ["A"])).at(-1)?.kind === "error",
    "completed question consumed",
  );
});

Deno.test("failed resume is retryable with known charges; a new request supersedes the question", async () => {
  let fail = true;
  const run = harness(async (input, _signal, emit) => {
    const usage = [...(input.usage ?? []), charge(crypto.randomUUID())];
    await emit({
      kind: "usage",
      phase: "usage",
      text: "usage",
      usage: { calls: usage },
    });
    if (input.answers?.length && fail) {
      fail = false;
      throw new Error("provider unavailable");
    }
    return {
      reply: "Rendben",
      usage,
      question: input.answers?.length ? undefined : question,
    };
  });
  const first = await run("retry-session");
  const id = first.at(-1)?.question?.id;
  assert(id, "question present");
  assert(
    (await run("retry-session", id, ["A"])).at(-1)?.kind === "error",
    "failed resume",
  );
  const retried = await run("retry-session", id, ["A"]);
  assert(retried.at(-1)?.kind === "completed", "retry completes");
  assert(
    retried.findLast((e) => e.usage)?.usage?.calls?.length === 3,
    "failed call charge preserved",
  );
  const old = (await run("replace-session")).at(-1)?.question?.id;
  assert(old, "old question");
  const fresh = (await run("replace-session")).at(-1)?.question?.id;
  assert(fresh && fresh !== old, "new request replaces pending question");
  assert(
    (await run("replace-session", old, ["A"])).at(-1)?.kind === "error",
    "old form cannot resume",
  );
});
