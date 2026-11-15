import { Code, ConnectError, type ConnectRouter } from "@connectrpc/connect";
import {
  type RunRequest,
  StudioService,
} from "../gen/krly/studio/v1/studio_pb.ts";
import { getProviderStatus } from "./ai/provider.ts";
import { transcribe } from "./ai/speech.ts";
import { BlobStore, imageFormat, SessionStore } from "./store.ts";
import { Channel } from "./channel.ts";
import { CadDocuments } from "./cad.ts";
import { providerError } from "./errors.ts";
import { resolveCharacterProfile } from "./native.ts";
import { resume, suspend } from "./questions.ts";
import {
  workflow,
  type WorkflowEvent,
  type WorkflowInput,
  type WorkflowOutcome,
} from "./workflow.ts";

export function validateRun(req: RunRequest) {
  if (req.questionId) {
    if (
      req.mode !== "chat" || req.questionId.length > 80 || req.text.trim() ||
      req.attachmentIds.length ||
      !req.answerIds.length || req.answerIds.length > 4 ||
      req.answerIds.some((id) => !/^[A-D]$/.test(id))
    ) {
      throw new ConnectError(
        "A pontosító kérdésre a válaszűrlappal válaszolj.",
        Code.InvalidArgument,
      );
    }
  } else if (req.answerIds.length) {
    throw new ConnectError(
      "A válaszhoz kérdésazonosító szükséges.",
      Code.InvalidArgument,
    );
  }
  if (
    !["chat", "build", "demo", "complex", "character"].includes(req.mode) ||
    req.text.length > 8000 ||
    req.attachmentIds.length > 1
  ) {
    throw new ConnectError(
      "Érvénytelen kérés. Egyszerre egy rajzot dolgozunk fel.",
      Code.InvalidArgument,
    );
  }
  if (
    ![req.height, req.pixelUnit, req.tolerance].every((v) =>
      Number.isFinite(v) && v > 0
    ) || req.height > 1e6 || req.pixelUnit > 1e6 || req.tolerance < 1e-8 ||
    req.tolerance > 1e6
  ) {
    throw new ConnectError(
      "Pozitív, véges magasság, méretarány és hálótolerancia szükséges.",
      Code.InvalidArgument,
    );
  }
  if (
    req.seed.length !== 0 &&
    (req.seed.length !== 2 ||
      !req.seed.every((v) => Number.isFinite(v) && v >= 0))
  ) {
    throw new ConnectError(
      "A kijelölés két nemnegatív pixelkoordinátából áll.",
      Code.InvalidArgument,
    );
  }
  if (
    req.mode === "chat" && !req.questionId && !req.text.trim() &&
    !req.attachmentIds.length
  ) {
    throw new ConnectError(
      "Írj üzenetet vagy csatolj képet.",
      Code.InvalidArgument,
    );
  }
}

export function routes(
  router: ConnectRouter,
  dependencies: { workflow?: typeof workflow; cad?: CadDocuments } = {},
) {
  const execute = dependencies.workflow ?? workflow;
  const cad = dependencies.cad ?? new CadDocuments();
  const blobs = new BlobStore();
  const sessions = new SessionStore();
  const cadQueues = new Map<string, Promise<void>>();
  let active = 0;
  router.service(StudioService, {
    async cad(req, ctx) {
      // A refresh can issue a show while the previous request is still releasing
      // its session. Keep CAD reads and writes in one order per document.
      const previous = cadQueues.get(req.sessionId) ?? Promise.resolve();
      let finish!: () => void;
      const current = new Promise<void>((resolve) => { finish = resolve; });
      const tail = previous.then(() => current);
      cadQueues.set(req.sessionId, tail);
      try {
        await previous;
        ctx.signal.throwIfAborted();
        const session = sessions.acquire(req.sessionId);
        try {
          return await cad.apply(req, ctx.signal);
        } finally {
          sessions.release(session);
        }
      } finally {
        finish();
        if (cadQueues.get(req.sessionId) === tail) cadQueues.delete(req.sessionId);
      }
    },
    async health() {
      const status = getProviderStatus();
      const binaries = ["krly3d-reconstruct", "krly3d-mesh"];
      const native = (await Promise.all(binaries.map(async (name) => {
        try {
          return (await Deno.stat(
            new URL(`../../../../target/debug/${name}`, import.meta.url),
          )).isFile;
        } catch {
          return false;
        }
      }))).every(Boolean);
      const characterProfile = await resolveCharacterProfile();
      let nativeCharacter = false;
      if (characterProfile) {
        try {
          const recipe = await Deno.lstat(
            new URL("../../../../models/pikachu.field.json", import.meta.url),
          );
          nativeCharacter = recipe.isFile && !recipe.isSymlink;
        } catch { /* Missing fixed recipe. */ }
      }
      return {
        ...status,
        nativeCharacter,
        native,
        message: !native
          ? "A natív motorhoz előbb cargo build --workspace szükséges."
          : status.chat
          ? "Natív motor és AI-kapcsolat konfigurálva."
          : "Natív motor kész. AI-kapcsolat még nincs konfigurálva.",
      };
    },
    upload(req) {
      const format = imageFormat(req.data);
      const mime = format === "pnm"
        ? "image/x-portable-anymap"
        : `image/${format}`;
      const entry = blobs.put(req.name, mime, req.data);
      return { id: entry.id, name: entry.name, mime };
    },
    async transcribe(req, ctx) {
      try {
        return { text: await transcribe(req.data, req.mime, ctx.signal) };
      } catch (error) {
        throw providerError(error);
      }
    },
    download(req) {
      const entry = blobs.get(req.id);
      return { data: entry.bytes, name: entry.name, mime: entry.mime };
    },
    async *run(req, ctx) {
      validateRun(req);
      if (active >= 2) {
        throw new ConnectError(
          "A két számítási hely foglalt. Próbáld újra az egyik befejezése után.",
          Code.ResourceExhausted,
        );
      }
      const session = sessions.acquire(req.sessionId);
      active++;
      const local = new AbortController();
      const deadline = AbortSignal.timeout(120_000);
      const signal = AbortSignal.any([ctx.signal, local.signal, deadline]);
      const channel = new Channel<WorkflowEvent>(2);
      let producer: Promise<void> | undefined;
      let released = false;
      const release = () => {
        if (released) return;
        released = true;
        signal.removeEventListener("abort", stop);
        sessions.release(session);
        active--;
      };
      // An RPC consumer may stop reading while this generator is paused at a
      // yield. Abort must reclaim admission even without another next/return,
      // but only after native/provider work has actually stopped.
      const stop = () => {
        channel.close(signal.reason);
        void (producer ?? Promise.resolve()).then(release, release);
      };
      signal.addEventListener("abort", stop, { once: true });
      const runId = crypto.randomUUID(), started = performance.now();
      let seq = 0;
      let outcome: WorkflowOutcome | undefined;
      try {
        signal.throwIfAborted();
        const id =
          req.questionId || ["demo", "complex", "character"].includes(req.mode)
            ? undefined
            : req.attachmentIds[0] ?? session.imageId;
        let image;
        try {
          image = id ? blobs.get(id) : undefined;
        } catch (error) {
          if (req.attachmentIds.length || req.mode !== "chat") throw error;
          session.imageId = undefined;
        }
        if (image) imageFormat(image.bytes);
        const input: WorkflowInput = req.questionId
          ? resume(session.pending, req.questionId, req.answerIds)
          : {
            text: req.text,
            mode: req.mode as WorkflowInput["mode"],
            height: req.height,
            pixelUnit: req.pixelUnit,
            tolerance: req.tolerance,
            seed: req.seed.length === 2
              ? [req.seed[0], req.seed[1]]
              : undefined,
            image,
            imageExplicit: req.attachmentIds.length > 0,
            history: session.history,
          };
        if (!req.questionId) session.pending = undefined;
        yield {
          runId,
          seq: seq++,
          kind: "status",
          phase: "accepted",
          text: "Kérés fogadva.",
          elapsedMs: 0,
        };
        producer = execute(input, signal, (event) => channel.send(event)).then(
          (result) => {
            signal.throwIfAborted();
            outcome = result;
            // Web-fetched references are transient workflow inputs, never a
            // session attachment that silently becomes the next request's image.
            if (input.image && input.imageExplicit !== false) {
              session.imageId = input.image.id;
            }
            if (result.question) session.pending = suspend(input, result);
            else {
              session.pending = undefined;
              session.history.push({
                role: "user",
                content: (input.text.trim() + (input.answers?.length
                  ? `\nDöntések: ${JSON.stringify(input.answers)}`
                  : "")).slice(0, 8000) ||
                  (req.mode === "character"
                    ? "Stilizált Pikachu · térbeli mesh (rögzített demó)"
                    : req.mode === "demo"
                    ? "Demó betöltése"
                    : "A csatolt rajz feldolgozása"),
              }, { role: "assistant", content: result.reply.slice(0, 8000) });
            }
            channel.close();
          },
        ).catch((error) => channel.close(error));
        for await (const event of channel) {
          signal.throwIfAborted();
          if (
            event.usage && req.questionId &&
            session.pending?.question.id === req.questionId
          ) {
            session.pending.input.usage = event.usage.calls;
          }
          const artifact = event.artifact
            ? blobs.put(
              event.artifact.name,
              event.artifact.mime,
              event.artifact.bytes,
            )
            : undefined;
          yield {
            runId,
            seq: seq++,
            kind: event.kind,
            phase: event.phase,
            text: event.text,
            mesh: event.mesh,
            construction: event.construction,
            usage: event.usage,
            sources: event.sources,
            artifact: artifact
              ? {
                id: artifact.id,
                name: artifact.name,
                mime: artifact.mime,
                size: artifact.bytes.length,
              }
              : undefined,
            elapsedMs: performance.now() - started,
          };
        }
        signal.throwIfAborted();
        if (outcome?.question && session.pending) {
          yield {
            runId,
            seq: seq++,
            kind: "question",
            phase: "question",
            text: outcome.reply,
            question: session.pending.question,
            usage: { calls: outcome.usage },
            elapsedMs: performance.now() - started,
          };
          return;
        }
        yield {
          runId,
          seq: seq++,
          kind: "completed",
          phase: "completed",
          text: "Kész.",
          elapsedMs: performance.now() - started,
        };
      } catch (error) {
        const publicError = providerError(error);
        if (!ctx.signal.aborted) {
          const text = signal.aborted
            ? "A művelet megszakadt vagy lejárt a határideje."
            : publicError instanceof Error
            ? publicError.message.slice(0, 1400)
            : "A feldolgozás sikertelen.";
          yield {
            runId,
            seq: seq++,
            kind: "error",
            phase: "failed",
            text,
            elapsedMs: performance.now() - started,
          };
        }
      } finally {
        local.abort();
        channel.close();
        signal.removeEventListener("abort", stop);
        await producer;
        release();
      }
    },
  });
}
