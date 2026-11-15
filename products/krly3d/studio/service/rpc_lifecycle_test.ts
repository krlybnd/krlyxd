import { create } from "@bufbuild/protobuf";
import {
  Code,
  ConnectError,
  createConnectRouter,
  createHandlerContext,
  type ServiceImpl,
} from "@connectrpc/connect";
import {
  RunRequestSchema,
  StudioService,
} from "../gen/krly/studio/v1/studio_pb.ts";
import { routes } from "./rpc.ts";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function service() {
  const router = createConnectRouter();
  let implementation: Partial<ServiceImpl<typeof StudioService>> = {};
  router.service = (_descriptor, impl) => {
    implementation = impl as Partial<ServiceImpl<typeof StudioService>>;
    return router;
  };
  routes(router);
  assert(implementation.run, "run registered");
  const run = implementation.run;
  return (
    id = crypto.randomUUID(),
    mode = "demo",
    attachmentIds: string[] = [],
  ) => {
    const context = createHandlerContext({
      service: StudioService,
      method: StudioService.method.run,
      protocolName: "test",
      requestMethod: "POST",
      url: "http://localhost/test",
    });
    const iterator = run(
      create(RunRequestSchema, {
        sessionId: id,
        mode,
        height: 3,
        pixelUnit: 1,
        tolerance: 0.05,
        attachmentIds,
      }),
      context,
    )[Symbol.asyncIterator]();
    return { context, iterator, id };
  };
}

for (const startProducer of [false, true]) {
  Deno.test(`aborted paused RPC releases admission without iterator return (producer=${startProducer})`, async () => {
    const start = service();
    const old = [start(), start()];
    const fresh: ReturnType<typeof start>[] = [];
    try {
      for (const call of old) {
        assert(
          (await call.iterator.next()).value?.phase === "accepted",
          "admitted",
        );
        if (startProducer) await call.iterator.next();
      }
      for (const call of old) call.context.abort();
      const until = performance.now() + 5000;
      for (const call of old) {
        for (;;) {
          const next = start(call.id);
          try {
            assert(
              (await next.iterator.next()).value?.phase === "accepted",
              "slot reclaimed",
            );
            fresh.push(next);
            break;
          } catch (error) {
            next.context.abort();
            const code = ConnectError.from(error).code;
            assert(
              [Code.ResourceExhausted, Code.AlreadyExists].includes(code),
              "only pending cleanup can block admission",
            );
            if (performance.now() >= until) {
              throw new Error("aborted streams leaked admission");
            }
            await new Promise((resolve) => setTimeout(resolve, 10));
          }
        }
      }
      // A late generator finally must not release a newer run's reservation.
      for (const call of old) await call.iterator.return?.();
      const excess = start();
      try {
        let code: Code | undefined;
        try {
          await excess.iterator.next();
        } catch (error) {
          code = ConnectError.from(error).code;
        }
        assert(
          code === Code.ResourceExhausted,
          "late cleanup must not decrement admission twice",
        );
      } finally {
        excess.context.abort();
      }
    } finally {
      for (const call of [...old, ...fresh]) {
        call.context.abort();
        await call.iterator.return?.();
      }
    }
  });
}

Deno.test("demo ignores missing or expired attachment identifiers", async () => {
  const start = service();
  for (const mode of ["demo", "complex"]) {
    const call = start(crypto.randomUUID(), mode, ["expired-image"]);
    try {
      assert(
        (await call.iterator.next()).value?.phase === "accepted",
        "demo accepted",
      );
      assert(
        (await call.iterator.next()).value?.phase === "geometry",
        "demo does not resolve irrelevant image IDs",
      );
    } finally {
      call.context.abort();
      await call.iterator.return?.();
    }
  }
});
