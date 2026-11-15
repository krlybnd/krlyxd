/** Microphone capture only. The caller uploads the Blob through its own client. */
export interface Recording {
  /** Idempotent. Also retrieves a clip already stopped by the duration limit. */
  stop: () => Promise<Blob>;
  /** Discards the clip, releases the microphone, and makes stop reject AbortError. */
  cancel: () => void;
}

export interface RecordingOptions {
  /** Also cancels a pending microphone permission request; late streams are stopped. */
  signal?: AbortSignal;
  /** Defaults to 60 seconds; may only lower the hard limit. */
  maxDurationMs?: number;
  /** Defaults to 8 MiB, matching the server upload limit; may only lower it. */
  maxBytes?: number;
}

export const VOICE_LIMITS = Object.freeze({
  maxDurationMs: 60_000,
  maxBytes: 8 * 1024 * 1024,
  permissionTimeoutMs: 30_000,
  stopTimeoutMs: 3000,
});

const MIME_TYPES = [
  "audio/webm;codecs=opus",
  "audio/mp4;codecs=mp4a.40.2",
  "audio/ogg;codecs=opus",
  "audio/webm",
  "audio/mp4",
  "audio/ogg",
];
const ALLOWED_TYPES = new Set(["audio/webm", "audio/mp4", "audio/ogg"]);

// A new click supersedes a pending/active capture, including permission races.
let activeCancel: (() => void) | undefined;

function aborted(): DOMException {
  return new DOMException("A hangfelvétel megszakítva.", "AbortError");
}

function stopTracks(stream: MediaStream | undefined): void {
  for (const track of stream?.getTracks() ?? []) {
    try { track.stop(); } catch { /* Continue releasing the remaining tracks. */ }
  }
}

function userMessage(error: unknown): string {
  const name = error instanceof Error || error instanceof DOMException ? error.name : "";
  if (name === "NotAllowedError" || name === "SecurityError") return "A mikrofon használata nincs engedélyezve. Engedélyezd, majd próbáld újra.";
  if (name === "NotFoundError") return "Nem található mikrofon.";
  if (name === "NotReadableError") return "A mikrofon foglalt vagy nem érhető el.";
  return "Nem sikerült elindítani a hangfelvételt.";
}

/**
 * Call directly from a Record button click, before awaiting network work. Importing
 * this module never asks for permission. A fresh click cancels the previous capture.
 *
 * At maxDurationMs the mic stops and onError receives a notice that the completed
 * clip is ready; call stop() to retrieve it. No clip is uploaded automatically.
 * Other errors discard the recording and make stop() reject. Always cancel() on
 * component cleanup, and abort the caller's transcription request separately.
 */
export async function startRecording(
  onError?: (message: string) => void,
  options: RecordingOptions = {},
): Promise<Recording> {
  const notify = (message: string) => {
    try { onError?.(message); } catch { /* UI callbacks must not prevent cleanup. */ }
  };
  const duration = options.maxDurationMs ?? VOICE_LIMITS.maxDurationMs;
  const byteLimit = options.maxBytes ?? VOICE_LIMITS.maxBytes;
  if (!Number.isInteger(duration) || duration < 100 || duration > VOICE_LIMITS.maxDurationMs ||
    !Number.isInteger(byteLimit) || byteLimit < 1 || byteLimit > VOICE_LIMITS.maxBytes) {
    throw new RangeError("A felvételi korlátoknak a megengedett tartományba kell esniük.");
  }
  if (options.signal?.aborted) throw aborted();
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia ||
    typeof MediaRecorder === "undefined" || typeof MediaRecorder.isTypeSupported !== "function") {
    const message = "A hangfelvételhez támogatott böngésző és HTTPS vagy localhost szükséges.";
    notify(message);
    throw new Error(message);
  }
  if (navigator.userActivation && !navigator.userActivation.isActive) {
    const message = "A felvételt a mikrofon gombjával indítsd el.";
    notify(message);
    throw new Error(message);
  }
  let mime = "";
  try { mime = MIME_TYPES.find((type) => MediaRecorder.isTypeSupported(type)) ?? ""; } catch { /* Report below. */ }
  if (!mime) {
    const message = "Ez a böngésző nem tud támogatott hangformátumban rögzíteni.";
    notify(message);
    throw new Error(message);
  }

  activeCancel?.();
  let stream: MediaStream | undefined;
  let recorder: MediaRecorder | undefined;
  let chunks: Blob[] = [];
  let size = 0;
  let settled = false;
  let cancelled = false;
  let stopping = false;
  let durationTimer: ReturnType<typeof setTimeout> | undefined;
  let stopTimer: ReturnType<typeof setTimeout> | undefined;
  let permissionTimer: ReturnType<typeof setTimeout> | undefined;
  let resolveResult!: (blob: Blob) => void;
  let rejectResult!: (error: Error) => void;
  const result = new Promise<Blob>((resolve, reject) => { resolveResult = resolve; rejectResult = reject; });
  // Errors can happen before the caller clicks Stop; retain them without an unhandled rejection.
  void result.catch(() => {});
  const ended: { track: MediaStreamTrack; handler: () => void }[] = [];

  const cleanup = () => {
    clearTimeout(durationTimer);
    clearTimeout(stopTimer);
    clearTimeout(permissionTimer);
    options.signal?.removeEventListener("abort", cancel);
    for (const { track, handler } of ended) track.removeEventListener("ended", handler);
    ended.length = 0;
    if (recorder) {
      recorder.ondataavailable = null;
      recorder.onerror = null;
      recorder.onstop = null;
      try { if (recorder.state !== "inactive") recorder.stop(); } catch { /* Tracks still stop. */ }
    }
    stopTracks(stream);
    stream = undefined;
    chunks = [];
    if (activeCancel === cancel) activeCancel = undefined;
  };
  const fail = (error: Error, message?: string) => {
    if (settled) return;
    settled = true;
    cleanup();
    rejectResult(error);
    if (message) notify(message);
  };
  function cancel(): void {
    cancelled = true;
    fail(aborted());
    // cancel after a completed recording still invalidates stop() for a stale UI.
    cleanup();
  }
  const stop = (): Promise<Blob> => {
    if (cancelled || options.signal?.aborted) return Promise.reject(aborted());
    if (!settled && !stopping && recorder) {
      stopping = true;
      clearTimeout(durationTimer);
      stopTimer = setTimeout(() => {
        const message = "A böngésző nem fejezte be a hangfelvételt. Próbáld újra.";
        fail(new Error(message), message);
      }, VOICE_LIMITS.stopTimeoutMs);
      try {
        if (recorder.state !== "inactive") recorder.stop();
        // Release hardware immediately, even when final data/onstop is delayed.
        stopTracks(stream);
      } catch {
        const message = "Nem sikerült lezárni a hangfelvételt. Próbáld újra.";
        fail(new Error(message), message);
      }
    }
    return result;
  };
  activeCancel = cancel;
  options.signal?.addEventListener("abort", cancel, { once: true });

  permissionTimer = setTimeout(() => {
    const message = "Lejárt a mikrofon engedélyezésére várakozás. Indíts új felvételt.";
    fail(new Error(message), message);
  }, VOICE_LIMITS.permissionTimeoutMs);
  try {
    // The browser permission prompt cannot be programmatically dismissed. A late
    // grant is still observed and immediately released after cancel/timeout.
    const acquiring = navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, channelCount: 1 },
      video: false,
    }).then((acquired) => {
      if (settled || cancelled) { stopTracks(acquired); throw aborted(); }
      return acquired;
    });
    const failed = result.then(() => { throw aborted(); });
    stream = await Promise.race([acquiring, failed]);
    clearTimeout(permissionTimer);
    if (settled || cancelled) { stopTracks(stream); throw aborted(); }
    if (!stream.getAudioTracks().some((track) => track.readyState === "live")) {
      throw new Error("No live microphone track.");
    }
    recorder = new MediaRecorder(stream, { mimeType: mime, audioBitsPerSecond: 64_000 });
    const actualMime = recorder.mimeType || mime;
    if (!ALLOWED_TYPES.has(actualMime.split(";", 1)[0].trim().toLowerCase())) {
      throw new Error("Unsupported recording format.");
    }
    recorder.ondataavailable = (event: BlobEvent) => {
      if (settled || !event.data.size) return;
      size += event.data.size;
      if (size > byteLimit) {
        const message = "A hangfelvétel túl nagy. Próbálj rövidebb felvételt készíteni.";
        fail(new Error(message), message);
      } else {
        chunks.push(event.data);
      }
    };
    recorder.onerror = () => {
      const message = "A hangfelvétel sikertelen. Próbáld újra.";
      fail(new Error(message), message);
    };
    recorder.onstop = () => {
      if (settled) return;
      if (!size) {
        const message = "A hangfelvétel üres. Próbáld újra, és beszélj a leállítás előtt.";
        fail(new Error(message), message);
        return;
      }
      const blob = new Blob(chunks, { type: actualMime });
      settled = true;
      cleanup();
      resolveResult(blob);
    };
    for (const track of stream.getTracks()) {
      const handler = () => {
        if (!stopping && !settled) {
          const message = "Megszakadt a mikrofon kapcsolata. Indíts új felvételt.";
          fail(new Error(message), message);
        }
      };
      track.addEventListener("ended", handler, { once: true });
      ended.push({ track, handler });
    }
    recorder.start(250);
    durationTimer = setTimeout(() => {
      if (settled || stopping) return;
      void stop(); // result already has a rejection handler; retrieval remains via stop().
      notify(`A felvétel elérte a ${duration / 1000} másodperces korlátot. A rögzített hang használatához nyomd meg a Leállítás gombot.`);
    }, duration);
    return { stop, cancel };
  } catch (error) {
    if (!settled) {
      const message = userMessage(error);
      fail(new Error(message), message);
    }
    // Use only our sanitized error, never raw device/provider error details.
    await result;
    throw aborted();
  }
}
