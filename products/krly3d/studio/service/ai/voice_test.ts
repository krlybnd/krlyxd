import { assert, equal } from "./test_support.ts";

// Runtime-import browser code: its DOM types are checked separately by web/tsconfig.
const browserModule = "../../web/src/voice.ts";
const { startRecording } = await import(browserModule) as {
  startRecording: (onError?: (message: string) => void, options?: {
    signal?: AbortSignal;
    maxDurationMs?: number;
    maxBytes?: number;
  }) => Promise<{ stop: () => Promise<Blob>; cancel: () => void }>;
};

class Track extends EventTarget {
  readyState = "live";
  stops = 0;
  stop(): void {
    this.stops++;
    this.readyState = "ended";
  }
}
class Stream {
  tracks = [new Track(), new Track()];
  getTracks(): Track[] {
    return this.tracks;
  }
  getAudioTracks(): Track[] {
    return this.tracks;
  }
}
class Recorder {
  static instances: Recorder[] = [];
  static throwConstructor = false;
  static format = "audio/webm;codecs=opus";
  static isTypeSupported(type: string): boolean {
    return type === Recorder.format;
  }
  state = "inactive";
  mimeType: string;
  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  onerror: (() => void) | null = null;
  onstop: (() => void) | null = null;
  stopCalls = 0;
  emitFinal = true;
  constructor(_stream: Stream, options: { mimeType: string }) {
    if (Recorder.throwConstructor) throw new Error("private device details");
    this.mimeType = options.mimeType;
    Recorder.instances.push(this);
  }
  start(): void {
    this.state = "recording";
  }
  stop(): void {
    this.stopCalls++;
    this.state = "inactive";
    if (this.emitFinal) {
      queueMicrotask(() => {
        this.ondataavailable?.({ data: new Blob(["recorded audio"]) });
        this.onstop?.();
      });
    }
  }
}

async function expectError(
  action: () => Promise<unknown>,
  name?: string,
): Promise<void> {
  try {
    await action();
  } catch (error) {
    assert(error instanceof Error);
    if (name) equal(error.name, name);
    assert(!error.message.includes("private"));
    return;
  }
  throw new Error("Expected recording rejection");
}

async function fixture(
  run: (
    setup: {
      stream: Stream;
      setAcquire: (fn: () => Promise<Stream>) => void;
      calls: () => number;
    },
  ) => Promise<void>,
): Promise<void> {
  const originalNavigator = Object.getOwnPropertyDescriptor(
    globalThis,
    "navigator",
  );
  const originalRecorder = Object.getOwnPropertyDescriptor(
    globalThis,
    "MediaRecorder",
  );
  const stream = new Stream();
  let acquire = () => Promise.resolve(stream);
  let count = 0;
  Recorder.instances = [];
  Recorder.throwConstructor = false;
  Recorder.format = "audio/webm;codecs=opus";
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: {
      userActivation: { isActive: true },
      mediaDevices: {
        getUserMedia: () => {
          count++;
          return acquire();
        },
      },
    },
  });
  Object.defineProperty(globalThis, "MediaRecorder", {
    configurable: true,
    value: Recorder,
  });
  try {
    await run({
      stream,
      setAcquire: (fn) => {
        acquire = fn;
      },
      calls: () => count,
    });
  } finally {
    if (originalNavigator) {
      Object.defineProperty(globalThis, "navigator", originalNavigator);
    } else Reflect.deleteProperty(globalThis, "navigator");
    if (originalRecorder) {
      Object.defineProperty(globalThis, "MediaRecorder", originalRecorder);
    } else Reflect.deleteProperty(globalThis, "MediaRecorder");
  }
}

Deno.test("voice: starts only on request, negotiates MIME, idempotent stop releases all tracks immediately", () =>
  fixture(async ({ stream, calls }) => {
    equal(calls(), 0);
    Recorder.format = "audio/mp4;codecs=mp4a.40.2";
    const capture = await startRecording();
    equal(calls(), 1);
    const first = capture.stop();
    const second = capture.stop();
    assert(first === second);
    assert(stream.tracks.every((t) => t.readyState === "ended"));
    const blob = await first;
    equal(blob.type, Recorder.format);
    equal(await blob.text(), "recorded audio");
    equal(Recorder.instances[0].stopCalls, 1);
    capture.cancel();
    await expectError(() => capture.stop(), "AbortError");
  }));

Deno.test("voice: abort pending permission rejects promptly and releases a late grant", () =>
  fixture(async ({ stream, setAcquire }) => {
    let grant!: (stream: Stream) => void;
    setAcquire(() =>
      new Promise((resolve) => {
        grant = resolve;
      })
    );
    const controller = new AbortController();
    const starting = startRecording(undefined, { signal: controller.signal });
    const rejected = expectError(() => starting, "AbortError");
    controller.abort();
    await rejected;
    grant(stream);
    await Promise.resolve();
    await Promise.resolve();
    assert(stream.tracks.every((t) => t.readyState === "ended"));
    equal(Recorder.instances.length, 0);
  }));

Deno.test("voice: a new click supersedes a pending capture and stale streams cannot win", () =>
  fixture(async ({ stream, setAcquire }) => {
    let grant!: (stream: Stream) => void;
    setAcquire(() =>
      new Promise((resolve) => {
        grant = resolve;
      })
    );
    const first = startRecording();
    const rejected = expectError(() => first, "AbortError");
    const secondStream = new Stream();
    setAcquire(() => Promise.resolve(secondStream));
    const second = await startRecording();
    await rejected;
    grant(stream);
    await Promise.resolve();
    await Promise.resolve();
    assert(stream.tracks.every((t) => t.readyState === "ended"));
    assert(secondStream.tracks.every((t) => t.readyState === "live"));
    second.cancel();
    assert(secondStream.tracks.every((t) => t.readyState === "ended"));
    await expectError(() => second.stop(), "AbortError");
  }));

Deno.test("voice: max duration releases the microphone and preserves the completed clip", () =>
  fixture(async ({ stream }) => {
    const notices: string[] = [];
    let notified!: () => void;
    const notice = new Promise<void>((resolve) => {
      notified = resolve;
    });
    const recording = await startRecording((message) => {
      notices.push(message);
      notified();
    }, { maxDurationMs: 100 });
    await notice;
    assert(stream.tracks.every((t) => t.readyState === "ended"));
    const blob = await recording.stop();
    assert(blob.size > 0);
    equal(notices.length, 1);
    assert(notices[0].includes("korlátot"));
  }));

Deno.test("voice: constructor failure and device loss release tracks with safe errors", () =>
  fixture(async ({ stream, setAcquire }) => {
    Recorder.throwConstructor = true;
    await expectError(() => startRecording());
    assert(stream.tracks.every((t) => t.readyState === "ended"));
    Recorder.throwConstructor = false;
    const next = new Stream();
    setAcquire(() => Promise.resolve(next));
    const recording = await startRecording(() => {
      throw new Error("callback failure");
    });
    next.tracks[0].dispatchEvent(new Event("ended"));
    await expectError(() => recording.stop());
    assert(next.tracks.every((t) => t.readyState === "ended"));
  }));

Deno.test("voice: oversize final chunks discard the recording", () =>
  fixture(async ({ stream }) => {
    const recording = await startRecording(undefined, { maxBytes: 2 });
    await expectError(() => recording.stop());
    assert(stream.tracks.every((t) => t.readyState === "ended"));
  }));

Deno.test("voice: hung finalization is bounded and never leaves the microphone open", () =>
  fixture(async ({ stream }) => {
    const recording = await startRecording();
    Recorder.instances[0].emitFinal = false;
    const stopping = recording.stop();
    assert(stream.tracks.every((t) => t.readyState === "ended"));
    await expectError(() => stopping);
  }));
