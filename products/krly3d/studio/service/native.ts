import type { CadConstructionData } from "./native_construction.ts";
/** External application adapter. Rust retains its in-process shared-memory path.
 *
 * No caller paths, executable overrides, shell, or inherited environment. Artifacts
 * are copied into events before the private workspace is removed. A background
 * producer owns the child and workspace, including while the iterator is paused.
 * Errors reject iteration; completed is published only after a successful exit.
 */

export type NativeEvent = {
  kind: string;
  text?: string;
  mesh?: {
    positions: number[];
    indices: number[];
    volume: number;
    area: number;
    openEdges: number;
    source: string;
    units?: "mm" | "m";
  };
  artifact?: { name: string; mime: string; bytes: Uint8Array };
  details?: Record<string, unknown>;
  construction?: ConstructionData;
};

export type ConstructionData = ImageConstructionData | CadConstructionData;

export type ImageConstructionData = {
  version: 1;
  coordinateSystem: "right_handed_z_up";
  units: "model";
  sourceCenter: [number, number];
  sourceBounds: [number, number, number, number];
  sketchPlane: "XY";
  sketchOffset: number;
  sketchLoops: { pointsXy: number[]; hole: boolean }[];
  extrusionDirection: [number, number, number];
  extrusionDistance: number;
};

function parseConstruction(value: unknown): ImageConstructionData {
  const data = object(value, "construction");
  require(data.version === 1, "Unsupported construction version");
  require(data.coordinate_system === "right_handed_z_up", "Unsupported construction coordinate system");
  require(data.units === "model", "Unsupported construction units");
  const pair = (input: unknown, length: number, label: string) => {
    require(Array.isArray(input) && input.length === length, `${label} must have ${length} numbers`);
    return input.map((item, index) => finite(item, -NATIVE_LIMITS.coordinateMax, NATIVE_LIMITS.coordinateMax, `${label}[${index}]`));
  };
  const sourceCenter = pair(data.source_center, 2, "source_center") as [number, number];
  const sourceBounds = pair(data.source_bounds, 4, "source_bounds") as [number, number, number, number];
  require(sourceBounds[0] < sourceBounds[2] && sourceBounds[1] < sourceBounds[3], "Construction source bounds are degenerate");
  require(data.sketch_plane === "XY", "Unsupported sketch plane");
  const sketchOffset = finite(data.sketch_offset, -NATIVE_LIMITS.coordinateMax, NATIVE_LIMITS.coordinateMax, "sketch_offset");
  require(sketchOffset === 0, "The base sketch must lie on the XY origin plane");
  require(Array.isArray(data.sketch_loops) && data.sketch_loops.length >= 1 && data.sketch_loops.length <= 32, "Invalid construction sketch loops");
  let remainingPoints = 256;
  const sketchLoops = data.sketch_loops.map((raw, loopIndex) => {
    const loop = object(raw, `sketch_loops[${loopIndex}]`);
    require(typeof loop.hole === "boolean" && loop.hole === (loopIndex > 0), "Sketch loop order or hole marker is invalid");
    require(Array.isArray(loop.points_xy) && loop.points_xy.length >= 6 && loop.points_xy.length % 2 === 0, "Sketch loop needs at least three XY points");
    remainingPoints -= loop.points_xy.length / 2;
    require(remainingPoints >= 0, "Construction sketch exceeds its vertex limit");
    return { pointsXy: pair(loop.points_xy, loop.points_xy.length, `sketch_loops[${loopIndex}].points_xy`), hole: loop.hole };
  });
  const extrusionDirection = pair(data.extrusion_direction, 3, "extrusion_direction") as [number, number, number];
  require(extrusionDirection[0] === 0 && extrusionDirection[1] === 0 && extrusionDirection[2] === 1, "Extrusion must follow +Z");
  const extrusionDistance = finite(data.extrusion_distance, Number.MIN_VALUE, NATIVE_LIMITS.coordinateMax, "extrusion_distance");
  return { version: 1, coordinateSystem: "right_handed_z_up", units: "model", sourceCenter, sourceBounds, sketchPlane: "XY", sketchOffset, sketchLoops, extrusionDirection, extrusionDistance };
}

export type ReconstructionInput = {
  bytes: Uint8Array;
  format: "png" | "jpeg" | "pnm";
  height: number;
  pixelUnit: number;
  seed?: [number, number];
  tolerance?: number;
};

/** Studio contour simplification tolerance in ORIGINAL source pixels.
 * Rust simplifies the full-resolution contour with topology preservation. The
 * uploaded raster is never downsampled; pixelUnit remains model units per source
 * pixel. This is independent of the model-unit mesh tolerance supplied by callers.
 */
export const STUDIO_OUTLINE_TOLERANCE_PIXELS = 1;

/** Admission/transport bounds, not a Rust heap, CPU, or disk quota. */
export const NATIVE_LIMITS = Object.freeze({
  imageBytes: 16 * 1024 * 1024,
  imageSide: 8192,
  imagePixels: 16 * 1024 * 1024,
  scalarMax: 1e6,
  toleranceMin: 1e-8,
  coordinateMax: 1e9,
  vertices: 250_000,
  triangles: 500_000,
  artifactBytes: 16 * 1024 * 1024,
  totalArtifactBytes: 64 * 1024 * 1024,
  streamLineBytes: 256 * 1024,
  streamBytes: 1024 * 1024,
  meshLineBytes: 32 * 1024 * 1024,
  meshStreamBytes: 33 * 1024 * 1024,
  stderrBytes: 1024 * 1024,
  stderrRetainedBytes: 16 * 1024,
  events: 16,
  timeoutMs: 120_000,
  termGraceMs: 500,
  killWaitMs: 2000,
});

const ROOT = new URL("../../../../", import.meta.url);
const BINARIES = {
  reconstruct: new URL("target/debug/krly3d-reconstruct", ROOT),
  mesh: new URL("target/debug/krly3d-mesh", ROOT),
  field: new URL("target/debug/krly3d-field", ROOT),
  print: new URL("target/debug/krly3d-print", ROOT),
} as const;
type CharacterProfile = "release" | "debug";

/** Select one complete fixed pair. Never mix builds or retry after a process failure. */
export async function resolveCharacterProfile(): Promise<
  CharacterProfile | undefined
> {
  for (const profile of ["release", "debug"] as const) {
    const available = await Promise.all(["field", "print"].map(async (name) => {
      try {
        const info = await Deno.lstat(
          new URL(`target/${profile}/krly3d-${name}`, ROOT),
        );
        return info.isFile && !info.isSymlink;
      } catch {
        return false;
      }
    }));
    if (available.every(Boolean)) return profile;
  }
  return undefined;
}

const decoder = () => new TextDecoder("utf-8", { fatal: true });
const STAGES = [
  "started",
  "evidence",
  "coarse_preview",
  "editable_proposal",
  "preview",
  "validation",
  "completed",
] as const;

function require(condition: unknown, message: string): asserts condition {
  if (!condition) throw new TypeError(message);
}

function object(value: unknown, label: string): Record<string, unknown> {
  require(
    value !== null && typeof value === "object" && !Array.isArray(value),
    `${label} must be an object`,
  );
  return value as Record<string, unknown>;
}

function finite(
  value: unknown,
  min: number,
  max: number,
  label: string,
): number {
  require(
    typeof value === "number" && Number.isFinite(value) && value >= min &&
      value <= max,
    `${label} must be finite in [${min}, ${max}]`,
  );
  return value;
}

function integer(
  value: unknown,
  min: number,
  max: number,
  label: string,
): number {
  const n = finite(value, min, max, label);
  require(Number.isSafeInteger(n), `${label} must be a safe integer`);
  return n;
}

function text(value: unknown, label: string): string {
  require(
    typeof value === "string" && value.length > 0 && value.length <= 4096,
    `${label} must be a bounded nonempty string`,
  );
  return value;
}

function dimensions(
  bytes: Uint8Array,
  format: ReconstructionInput["format"],
): [number, number] {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (format === "png") {
    require(
      bytes.length >= 33 && [137, 80, 78, 71, 13, 10, 26, 10].every((v, i) =>
        bytes[i] === v
      ) &&
        view.getUint32(8) === 13 && view.getUint32(12) === 0x49484452,
      "Invalid PNG header",
    );
    return [view.getUint32(16), view.getUint32(20)];
  }
  if (format === "pnm") {
    // A bounded header scan; the native decoder validates samples and encoding.
    const header = new TextDecoder().decode(bytes.subarray(0, 64 * 1024));
    const tokens = header.replace(/#[^\r\n]*/g, " ").trimStart().split(
      /\s+/,
      4,
    );
    require(
      /^P[2356]$/.test(tokens[0]) && tokens.length === 4 &&
        tokens.slice(1).every((v) => /^\d{1,10}$/.test(v)),
      "Invalid or oversized PNM header",
    );
    integer(Number(tokens[3]), 1, 65535, "PNM max sample");
    return [Number(tokens[1]), Number(tokens[2])];
  }
  require(
    bytes.length >= 4 && bytes[0] === 255 && bytes[1] === 216,
    "Invalid JPEG header",
  );
  let at = 2;
  while (at < bytes.length) {
    require(bytes[at++] === 255, "Invalid JPEG marker");
    while (bytes[at] === 255) at++;
    const marker = bytes[at++];
    require(
      marker !== undefined && marker !== 0xda && marker !== 0xd9,
      "JPEG has no frame dimensions",
    );
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) continue;
    require(at + 2 <= bytes.length, "Truncated JPEG segment");
    const size = view.getUint16(at);
    require(
      size >= 2 && at + size <= bytes.length,
      "Invalid JPEG segment size",
    );
    if (
      marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)
    ) {
      require(size >= 8, "Invalid JPEG frame");
      return [view.getUint16(at + 5), view.getUint16(at + 3)];
    }
    at += size;
  }
  throw new TypeError("JPEG has no frame dimensions");
}

/** Pure admission validation, also useful before accepting an upload. */
export function validateInput(value: ReconstructionInput): ReconstructionInput {
  const input = object(value, "input");
  require(
    Object.keys(input).every((k) =>
      ["bytes", "format", "height", "pixelUnit", "seed", "tolerance"].includes(
        k,
      )
    ),
    "Unknown input field (paths are not accepted)",
  );
  require(
    input.bytes instanceof Uint8Array &&
      !(input.bytes.buffer instanceof SharedArrayBuffer) &&
      input.bytes.length > 0 && input.bytes.length <= NATIVE_LIMITS.imageBytes,
    "Image must contain 1..16777216 non-shared bytes",
  );
  require(
    input.format === "png" || input.format === "jpeg" || input.format === "pnm",
    "format must be png, jpeg, or pnm",
  );
  const [width, height] = dimensions(input.bytes, input.format);
  integer(width, 1, NATIVE_LIMITS.imageSide, "image width");
  integer(height, 1, NATIVE_LIMITS.imageSide, "image height");
  require(
    width * height <= NATIVE_LIMITS.imagePixels,
    "Image exceeds pixel limit",
  );
  finite(input.height, Number.MIN_VALUE, NATIVE_LIMITS.scalarMax, "height");
  finite(
    input.pixelUnit,
    Number.MIN_VALUE,
    NATIVE_LIMITS.scalarMax,
    "pixelUnit",
  );
  const tolerance = finite(
    input.tolerance === undefined ? 0.05 : input.tolerance,
    NATIVE_LIMITS.toleranceMin,
    NATIVE_LIMITS.scalarMax,
    "tolerance",
  );
  if (input.seed !== undefined) {
    require(
      Array.isArray(input.seed) && input.seed.length === 2,
      "seed must contain two pixel coordinates",
    );
    finite(input.seed[0], 0, width - Number.EPSILON, "seed x");
    finite(input.seed[1], 0, height - Number.EPSILON, "seed y");
    require(
      input.seed[0] < width && input.seed[1] < height,
      "seed must be inside the image",
    );
  }
  // Snapshot mutable caller-owned data before the first asynchronous operation.
  return {
    bytes: input.bytes.slice(),
    format: input.format,
    height: input.height as number,
    pixelUnit: input.pixelUnit as number,
    tolerance,
    seed: input.seed === undefined
      ? undefined
      : [...input.seed as [number, number]],
  };
}

/** Decode the actual mesh.data.points/triangles tuple encoding, preserving provenance. */
export function parseMesh(value: unknown): NonNullable<NativeEvent["mesh"]> {
  const result = object(value, "mesh result");
  const artifact = object(result.mesh, "mesh artifact");
  text(artifact.id, "mesh artifact id");
  integer(
    artifact.revision,
    1,
    Number.MAX_SAFE_INTEGER,
    "mesh artifact revision",
  );
  const data = object(artifact.data, "mesh data");
  require(
    Array.isArray(data.points) && Array.isArray(data.triangles),
    "Mesh requires points and triangles arrays",
  );
  const points = integer(result.points, 1, NATIVE_LIMITS.vertices, "points");
  const triangles = integer(
    result.triangles,
    1,
    NATIVE_LIMITS.triangles,
    "triangles",
  );
  require(
    data.points.length === points && data.triangles.length === triangles,
    "Mesh counts do not match geometry",
  );
  const positions: number[] = [];
  const indices: number[] = [];
  for (const point of data.points) {
    require(
      Array.isArray(point) && point.length === 3,
      "Mesh point must be a triple",
    );
    for (const v of point) {
      positions.push(
        finite(
          v,
          -NATIVE_LIMITS.coordinateMax,
          NATIVE_LIMITS.coordinateMax,
          "coordinate",
        ),
      );
    }
  }
  for (const triangle of data.triangles) {
    require(
      Array.isArray(triangle) && triangle.length === 3,
      "Mesh triangle must be a triple",
    );
    for (const v of triangle) {
      indices.push(integer(v, 0, points - 1, "triangle index"));
    }
  }
  return {
    positions,
    indices,
    volume: finite(
      result.volume,
      -Number.MAX_VALUE,
      Number.MAX_VALUE,
      "volume",
    ),
    area: finite(result.area, 0, Number.MAX_VALUE, "area"),
    openEdges: integer(result.open, 0, Number.MAX_SAFE_INTEGER, "open edges"),
    source: text(object(result.provenance, "provenance").source, "source"),
  };
}

type NdjsonLimits = { lineBytes: number; totalBytes: number; records: number };

/** Byte-bounded, strict UTF-8 NDJSON; unterminated tails and blank records fail. */
export async function* readNdjson(
  stream: ReadableStream<Uint8Array>,
  limits: NdjsonLimits,
  signal?: AbortSignal,
): AsyncGenerator<Record<string, unknown>> {
  integer(limits.lineBytes, 1, NATIVE_LIMITS.meshLineBytes, "line byte limit");
  integer(
    limits.totalBytes,
    1,
    NATIVE_LIMITS.meshStreamBytes,
    "total byte limit",
  );
  integer(limits.records, 1, NATIVE_LIMITS.events, "record limit");
  const reader = stream.getReader();
  const cancel = () => {
    void reader.cancel().catch(() => {});
  };
  signal?.addEventListener("abort", cancel, { once: true });
  let pending = new Uint8Array(Math.min(4096, limits.lineBytes));
  let used = 0, total = 0, records = 0;
  try {
    signal?.throwIfAborted();
    while (true) {
      const { value, done } = await reader.read();
      signal?.throwIfAborted();
      if (done) break;
      total += value.byteLength;
      require(total <= limits.totalBytes, "NDJSON exceeds total byte limit");
      let start = 0;
      for (let i = 0; i <= value.length; i++) {
        if (i !== value.length && value[i] !== 10) continue;
        const end = i;
        const size = used + end - start;
        require(size <= limits.lineBytes, "NDJSON exceeds line byte limit");
        if (size > pending.length) {
          const grown = new Uint8Array(
            Math.min(limits.lineBytes, Math.max(size, pending.length * 2)),
          );
          grown.set(pending.subarray(0, used));
          pending = grown;
        }
        pending.set(value.subarray(start, end), used);
        used = size;
        start = i + 1;
        if (i !== value.length) {
          require(++records <= limits.records, "NDJSON exceeds record limit");
          const line = decoder().decode(pending.subarray(0, used));
          require(line.trim().length > 0, "NDJSON contains a blank record");
          used = 0;
          yield object(JSON.parse(line), "NDJSON record");
        }
      }
    }
    require(used === 0, "NDJSON has an unterminated record");
  } finally {
    signal?.removeEventListener("abort", cancel);
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

function abortError(): DOMException {
  return new DOMException("Native operation aborted", "AbortError");
}

async function within<T>(
  promise: Promise<T>,
  milliseconds: number,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("Native process did not exit after SIGKILL")),
          milliseconds,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

type Scope = {
  control: AbortController;
  directory: string;
  artifactBytes: number;
  characterProfile?: CharacterProfile;
};
type Emit = (event: NativeEvent) => void;

async function boundedFile(
  path: string | URL,
  limit: number,
): Promise<Uint8Array> {
  const info = await Deno.lstat(path);
  require(
    info.isFile && !info.isSymlink && info.size > 0 && info.size <= limit,
    "Native artifact is not a bounded regular file",
  );
  const file = await Deno.open(path, { read: true });
  try {
    const bytes = new Uint8Array(info.size);
    let offset = 0;
    while (offset < bytes.length) {
      const count = await file.read(bytes.subarray(offset));
      require(count !== null && count > 0, "Native artifact was truncated");
      offset += count;
    }
    require(
      await file.read(new Uint8Array(1)) === null,
      "Native artifact grew beyond its bound",
    );
    return bytes;
  } finally {
    file.close();
  }
}

async function artifact(
  scope: Scope,
  name: string,
  mime: string,
): Promise<NonNullable<NativeEvent["artifact"]>> {
  scope.control.signal.throwIfAborted();
  const bytes = await boundedFile(
    `${scope.directory}/output/${name}`,
    NATIVE_LIMITS.artifactBytes,
  );
  scope.artifactBytes += bytes.length;
  require(
    scope.artifactBytes <= NATIVE_LIMITS.totalArtifactBytes,
    "Native artifacts exceed total byte limit",
  );
  scope.control.signal.throwIfAborted();
  return { name, mime, bytes };
}

async function runProcess(
  scope: Scope,
  binary: keyof typeof BINARIES,
  args: string[],
  onRecord: (record: Record<string, unknown>) => void | Promise<void>,
  acceptedCodes: number[] = [0],
): Promise<number> {
  const signal = scope.control.signal;
  signal.throwIfAborted();
  const executable = (binary === "field" || binary === "print") &&
      scope.characterProfile === "release"
    ? new URL(`target/release/krly3d-${binary}`, ROOT)
    : BINARIES[binary];
  const info = await Deno.lstat(executable);
  require(
    info.isFile && !info.isSymlink,
    `Native executable must be a regular file: ${executable.pathname}`,
  );
  signal.throwIfAborted();
  const child = new Deno.Command(executable, {
    args,
    cwd: ROOT,
    clearEnv: true,
    stdin: "null",
    stdout: "piped",
    stderr: "piped",
  }).spawn();
  let exited = false;
  let killTimer: ReturnType<typeof setTimeout> | undefined;
  const status = child.status.then((value) => {
    exited = true;
    clearTimeout(killTimer);
    return value;
  });
  const kill = (kind: Deno.Signal) => {
    if (!exited) {
      try {
        child.kill(kind);
      } catch (error) {
        if (!(error instanceof Deno.errors.NotFound)) throw error;
      }
    }
  };
  const stop = () => {
    if (exited || killTimer !== undefined) return;
    kill("SIGTERM");
    killTimer = setTimeout(() => kill("SIGKILL"), NATIVE_LIMITS.termGraceMs);
  };
  const io = new AbortController();
  let rejectAbort!: (reason: unknown) => void;
  const aborted = new Promise<never>((_, reject) => {
    rejectAbort = reject;
  });
  const abort = () => {
    stop();
    io.abort(signal.reason);
    rejectAbort(signal.reason);
  };
  signal.addEventListener("abort", abort, { once: true });
  const retained = new Uint8Array(NATIVE_LIMITS.stderrRetainedBytes);
  let retainedSize = 0;
  const errors = (async () => {
    const reader = child.stderr.getReader();
    const cancel = () => {
      void reader.cancel().catch(() => {});
    };
    io.signal.addEventListener("abort", cancel, { once: true });
    let total = 0;
    try {
      while (!io.signal.aborted) {
        const { value, done } = await reader.read();
        if (done) break;
        total += value.length;
        const part = value.subarray(0, retained.length - retainedSize);
        retained.set(part, retainedSize);
        retainedSize += part.length;
        require(
          total <= NATIVE_LIMITS.stderrBytes,
          "Native stderr exceeds byte limit",
        );
      }
    } finally {
      io.signal.removeEventListener("abort", cancel);
      await reader.cancel().catch(() => {});
      reader.releaseLock();
    }
  })();
  const output = (async () => {
    const limits = binary !== "mesh"
      ? {
        lineBytes: NATIVE_LIMITS.streamLineBytes,
        totalBytes: NATIVE_LIMITS.streamBytes,
        records: 8,
      }
      : {
        lineBytes: NATIVE_LIMITS.meshLineBytes,
        totalBytes: NATIVE_LIMITS.meshStreamBytes,
        records: 2,
      };
    for await (const record of readNdjson(child.stdout, limits, io.signal)) {
      signal.throwIfAborted();
      await onRecord(record);
    }
  })();
  try {
    if (signal.aborted) abort();
    const [exit] = await Promise.race([
      Promise.all([status, output, errors]),
      aborted,
    ]);
    signal.throwIfAborted();
    if (!acceptedCodes.includes(exit.code)) {
      const stderr = new TextDecoder().decode(
        retained.subarray(0, retainedSize),
      );
      throw new Error(
        `Native ${binary} exited ${exit.code}${stderr ? `: ${stderr}` : ""}`,
      );
    }
    return exit.code;
  } finally {
    signal.removeEventListener("abort", abort);
    stop();
    io.abort(abortError());
    try {
      await within(
        status,
        NATIVE_LIMITS.termGraceMs + NATIVE_LIMITS.killWaitMs,
      );
    } finally {
      clearTimeout(killTimer);
      // Both readers are cancelled above, independently of consumer backpressure.
      await Promise.allSettled([output, errors]);
    }
  }
}

async function* bridge(
  signal: AbortSignal,
  work: (scope: Scope, emit: Emit) => Promise<void>,
): AsyncGenerator<NativeEvent> {
  require(signal instanceof AbortSignal, "signal must be an AbortSignal");
  if (signal.aborted) throw abortError();
  const control = new AbortController();
  const queue: NativeEvent[] = [];
  let wake: (() => void) | undefined;
  let finished = false;
  let failure: unknown;
  const abort = () => control.abort(abortError());
  signal.addEventListener("abort", abort, { once: true });
  const discard = () => {
    queue.length = 0;
    wake?.();
  };
  control.signal.addEventListener("abort", discard, { once: true });
  const timer = setTimeout(
    () =>
      control.abort(
        new DOMException(
          "Native operation exceeded 120 seconds",
          "TimeoutError",
        ),
      ),
    NATIVE_LIMITS.timeoutMs,
  );
  const producer = (async () => {
    let scope: Scope | undefined;
    try {
      const directory = await Deno.makeTempDir({ prefix: "krly3d-studio-" });
      scope = { directory, control, artifactBytes: 0 };
      control.signal.throwIfAborted();
      await work(scope, (event) => {
        control.signal.throwIfAborted();
        require(
          queue.length < NATIVE_LIMITS.events,
          "Native event queue exceeded limit",
        );
        queue.push(event);
        wake?.();
      });
    } catch (error) {
      failure = control.signal.aborted ? control.signal.reason : error;
    } finally {
      try {
        if (scope) await Deno.remove(scope.directory, { recursive: true });
      } catch (error) {
        failure = new Error("Could not remove native temporary workspace", {
          cause: error,
        });
      }
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      control.signal.removeEventListener("abort", discard);
      finished = true;
      wake?.();
    }
  })();
  try {
    while (true) {
      if (signal.aborted) control.abort(abortError());
      if (control.signal.aborted) {
        await producer;
        throw failure ?? control.signal.reason;
      }
      const next = queue.shift();
      if (next) yield next;
      else if (finished) {
        if (failure !== undefined) throw failure;
        return;
      } else {await new Promise<void>((resolve) => {
          wake = resolve;
        });}
    }
  } finally {
    control.abort(abortError());
    await producer;
  }
}

/** Seven native stages plus `artifact` events carrying proposal.k3d and mesh.json.
 * Evidence, coarse mesh JSON, preview PNG, likeness JSON, and manifest bytes ride
 * their stage events. Errors reject; cancellation is AbortError, deadline TimeoutError.
 */
export async function* reconstruct(
  input: ReconstructionInput,
  signal: AbortSignal,
): AsyncGenerator<NativeEvent> {
  if (signal?.aborted) throw abortError();
  const checked = validateInput(input);
  yield* bridge(signal, async (scope, emit) => {
    const image = `${scope.directory}/input.${checked.format}`;
    await Deno.writeFile(image, checked.bytes);
    const args = [
      "--stream",
      "--image",
      image,
      "--output",
      `${scope.directory}/output`,
      "--height",
      String(checked.height),
      "--pixel-unit",
      String(checked.pixelUnit),
      "--outline-tolerance",
      String(STUDIO_OUTLINE_TOLERANCE_PIXELS),
      "--tolerance",
      String(checked.tolerance),
      "--timeout",
      "120",
    ];
    if (checked.seed) args.push("--seed", checked.seed.join(","));
    let sequence = 0;
    let flow: string | undefined;
    let elapsed = 0;
    let source: string | undefined;
    let completion: NativeEvent | undefined;
    await runProcess(scope, "reconstruct", args, async (record) => {
      require(
        record.protocol === "krly3d.shared-reconstruction/2",
        "Unsupported reconstruction protocol",
      );
      require(
        record.seq === sequence && record.revision === 1,
        "Invalid reconstruction sequence or revision",
      );
      const identity = text(record.flow_id, "flow id");
      flow ??= identity;
      require(identity === flow, "Reconstruction flow identity changed");
      elapsed = finite(
        record.elapsed_ms,
        elapsed,
        Number.MAX_VALUE,
        "elapsed_ms",
      );
      finite(record.stage_ms, 0, Number.MAX_VALUE, "stage_ms");
      if (record.kind === "failed" || record.kind === "cancelled") {
        throw new Error(
          `Native reconstruction ${record.kind}: ${
            text(record.message, "failure message")
          }`,
        );
      }
      require(
        sequence < STAGES.length && record.kind === STAGES[sequence++],
        "Unexpected reconstruction stage",
      );
      const event: NativeEvent = {
        kind: record.kind as string,
        details: record,
      };
      if (record.kind === "editable_proposal") {
        event.construction = parseConstruction(record.construction);
      }
      let meshArtifact: NativeEvent["artifact"];
      const load = async (name: string, mime = "application/json") => {
        require(record.artifact === name, "Unexpected native artifact name");
        return await artifact(scope, name, mime);
      };
      switch (record.kind) {
        case "evidence":
          event.artifact = await load("evidence.json");
          object(
            JSON.parse(decoder().decode(event.artifact.bytes)),
            "evidence",
          );
          break;
        case "coarse_preview":
          event.artifact = await load("coarse-mesh.json");
          event.mesh = parseMesh(
            JSON.parse(decoder().decode(event.artifact.bytes)),
          );
          break;
        case "editable_proposal": {
          require(record.mesh === "mesh.json", "Unexpected native mesh name");
          const mesh = await artifact(scope, "mesh.json", "application/json");
          meshArtifact = mesh;
          event.mesh = parseMesh(JSON.parse(decoder().decode(mesh.bytes)));
          source = event.mesh.source;
          event.artifact = await load(
            "proposal.k3d",
            "text/plain; charset=utf-8",
          );
          require(
            decoder().decode(event.artifact.bytes).startsWith("k3d 1\n"),
            "Invalid native k3d document",
          );
          break;
        }
        case "preview": {
          event.artifact = await load("preview.png", "image/png");
          const [width, height] = dimensions(event.artifact.bytes, "png");
          integer(width, 1, NATIVE_LIMITS.imageSide, "preview width");
          integer(height, 1, NATIVE_LIMITS.imageSide, "preview height");
          require(
            width * height <= NATIVE_LIMITS.imagePixels,
            "Preview exceeds pixel limit",
          );
          const identity = object(
            object(record.provenance, "render provenance").source,
            "render source",
          );
          require(
            source !== undefined && identity.source === source,
            "Render source does not match final B-rep",
          );
          break;
        }
        case "validation": {
          event.artifact = await load("likeness.json");
          const likeness = object(
            JSON.parse(decoder().decode(event.artifact.bytes)),
            "likeness",
          );
          require(
            record.matches === null || typeof record.matches === "boolean",
            "Invalid likeness verdict",
          );
          const identity = object(
            object(likeness.provenance, "likeness provenance").source,
            "likeness source",
          );
          require(
            likeness.matches === record.matches && source !== undefined &&
              identity.source === source,
            "Likeness verdict or B-rep source mismatch",
          );
          break;
        }
        case "completed": {
          object(record.manifest, "manifest");
          event.artifact = await artifact(
            scope,
            "manifest.json",
            "application/json",
          );
          const manifest = object(
            JSON.parse(decoder().decode(event.artifact.bytes)),
            "manifest artifact",
          );
          require(
            manifest.protocol === record.protocol &&
              manifest.flow_id === flow &&
              source !== undefined && manifest.brep_source === source,
            "Manifest identity mismatch",
          );
          completion = event;
          return;
        }
      }
      // The service routes mesh and artifact events independently. Publish the
      // editable document separately so a mesh-first consumer also receives it.
      if (event.kind === "editable_proposal") {
        const document = event.artifact;
        delete event.artifact;
        emit(event);
        emit({
          kind: "artifact",
          artifact: document,
          details: { stage: "editable_proposal" },
        });
        emit({
          kind: "artifact",
          artifact: meshArtifact,
          details: { stage: "editable_proposal" },
        });
      } else emit(event);
    });
    require(
      sequence === STAGES.length && completion !== undefined,
      "Native reconstruction ended before completed",
    );
    emit(completion);
  });
}

/** Fixed native documents/nodes only. Complex steps each run the real mesh CLI. */
export async function* demo(
  signal: AbortSignal,
  variant: "plate" | "complex" = "plate",
): AsyncGenerator<NativeEvent> {
  require(variant === "plate" || variant === "complex", "Unknown demo variant");
  const name = variant === "plate"
    ? "plate-with-hole.k3d"
    : "studio-bracket.k3d";
  const steps = variant === "plate"
    ? [{ node: "part", text: "Lyukas lemez" }]
    : [
      { node: "part@1", text: "Alaplemez" },
      { node: "part@5", text: "Négy átmenő rögzítőfurat" },
      { node: "part@9", text: "Négy kiemelt támasztóbak" },
      {
        node: "part@14",
        text: "Hátfal két kivágással és két merevítőbordával",
      },
    ];
  yield* bridge(signal, async (scope, emit) => {
    const bytes = await boundedFile(
      new URL(`models/${name}`, ROOT),
      NATIVE_LIMITS.artifactBytes,
    );
    const model = `${scope.directory}/${name}`;
    const request = `${scope.directory}/request.json`;
    await Deno.writeFile(model, bytes);
    let finalMesh: NativeEvent["mesh"];
    for (const [index, step] of steps.entries()) {
      scope.control.signal.throwIfAborted();
      await Deno.writeTextFile(
        request,
        JSON.stringify({
          request_id: "studio-demo",
          protocol: { major: 2, minor: 0 },
          api: { major: 2, minor: 0 },
          timeout_ms: NATIVE_LIMITS.timeoutMs,
          input: {
            node: step.node,
            triangles: true,
            tolerance: { value: 0.05, unit: "model" },
          },
        }),
      );
      let sequence = 0;
      let operation: string | undefined;
      let source: string | undefined;
      let mesh: NativeEvent["mesh"];
      await runProcess(
        scope,
        "mesh",
        ["--model", model, "--request", request],
        (record) => {
          for (const key of ["protocol", "api"]) {
            const version = object(record[key], key);
            require(
              version.major === 2 && version.minor === 0,
              "Unsupported mesh protocol/API",
            );
          }
          require(
            record.seq === sequence && record.request_id === "studio-demo",
            "Invalid mesh sequence/request",
          );
          const identity = text(record.operation_id, "operation id");
          operation ??= identity;
          require(identity === operation, "Mesh operation identity changed");
          const body = object(record.body, "mesh event body");
          if (sequence === 0) {
            require(body.kind === "started", "Mesh did not start");
            source = text(
              object(body.provenance, "mesh provenance").source,
              "mesh source",
            );
            if (index === 0) {
              emit({
                kind: "started",
                details: { ...record, demo: true, variant },
              });
            }
          } else {
            require(
              sequence === 1 && body.kind === "completed",
              `Native mesh did not complete: ${
                JSON.stringify(body).slice(0, 4096)
              }`,
            );
            mesh = parseMesh(body.result);
            require(mesh.source === source, "Completed mesh source changed");
          }
          sequence++;
        },
      );
      require(
        sequence === 2 && mesh !== undefined,
        "Native mesh ended before completed",
      );
      const final = index === steps.length - 1;
      emit({
        kind: final ? "editable_proposal" : "construction_step",
        mesh,
        text: step.text,
        details: {
          demo: true,
          variant,
          node: step.node,
          step: index + 1,
          steps: steps.length,
        },
      });
      finalMesh = mesh;
    }
    require(finalMesh !== undefined, "Demo has no final mesh");
    emit({
      kind: "artifact",
      artifact: {
        name,
        mime: "text/plain; charset=utf-8",
        bytes,
      },
      details: { demo: true, variant, stage: "editable_proposal" },
    });
    emit({
      kind: "completed",
      details: { demo: true, variant, source: finalMesh.source },
    });
  });
}

/** Versioned native report. Unknown geometric checks must remain explicit. */
export function parsePrintReport(value: unknown, mesh?: NativeEvent["mesh"]) {
  const report = object(value, "print report");
  require(
    report.format === "krly3d.mesh.print-report/1" && report.units === "mm",
    "Unsupported print report format/units",
  );
  require(
    typeof report.exported === "boolean" && report.print_ready === false,
    "Invalid print verdict",
  );
  const roundtrip = object(report.float32_roundtrip, "float32 roundtrip");
  require(
    ["passed", "failed", "not_run"].includes(String(roundtrip.status)),
    "Invalid float32 check",
  );
  if (!report.exported) {
    const error = object(report.error, "print error");
    return {
      report,
      summary: `STL-export sikertelen: ${
        text(error.message, "print error message")
      }. Nyomtathatóság nem igazolt.`,
    };
  }
  require(
    mesh && report.error === null && roundtrip.status === "passed" &&
      report.topology_valid === true && report.export_eligible === true,
    "Inconsistent successful print report",
  );
  require(
    report.points === mesh.positions.length / 3 &&
      report.triangles === mesh.indices.length / 3,
    "Print report mesh counts differ",
  );
  for (
    const key of [
      "unused_points",
      "coincident_points",
      "degenerate_triangles",
      "boundary_edges",
      "nonmanifold_edges",
      "inconsistent_edges",
      "nonmanifold_vertices",
    ]
  ) {
    integer(report[key], 0, Number.MAX_SAFE_INTEGER, key);
    if (key !== "unused_points") {
      require(report[key] === 0, `Successful print report has ${key}`);
    }
  }
  require(report.connected_components === 1, "Expected one connected figure");
  finite(
    report.signed_volume,
    Number.MIN_VALUE,
    Number.MAX_VALUE,
    "signed volume",
  );
  const checkLabels: Record<string, string> = {
    unknown: "ismeretlen",
    not_run: "nem futott",
    passed: "megfelelt a natív vizsgálatnak",
    failed: "hibát talált",
  };
  const checks: [unknown, string, boolean][] = [
    [report.self_intersections, "Önmetszés · eredeti mesh", true],
    [roundtrip.self_intersections, "Önmetszés · float32 STL", true],
    [report.wall_thickness, "Falvastagság", false],
  ];
  const checkSummaries = checks.map(([value, label, intersections]) => {
    const check = object(value, label);
    const status = text(check.status, `${label} status`);
    const reason = check.reason == null
      ? ""
      : text(check.reason, `${label} reason`);
    if (!intersections) {
      return `${label}: ${
        checkLabels[status] ?? `nem értelmezett natív státusz (${status})`
      }${reason ? ` (${reason})` : ""}.`;
    }
    require(
      status === "passed" &&
        check.method === "bounded_bvh_triangle_contacts/1" &&
        check.exact === false && check.first_pair === null &&
        check.reason === null,
      "Export requires passed numerical intersection checks for original and float32 meshes",
    );
    const pairs = integer(
      check.tested_pairs,
      0,
      Number.MAX_SAFE_INTEGER,
      `${label} tested pairs`,
    );
    const visits = integer(
      check.node_visits,
      0,
      Number.MAX_SAFE_INTEGER,
      `${label} node visits`,
    );
    const pairLimit = integer(
      check.candidate_limit,
      1,
      Number.MAX_SAFE_INTEGER,
      `${label} pair limit`,
    );
    const visitLimit = integer(
      check.node_visit_limit,
      1,
      Number.MAX_SAFE_INTEGER,
      `${label} visit limit`,
    );
    require(
      pairs <= pairLimit && visits <= visitLimit,
      "Intersection check exceeded declared work budget",
    );
    const tolerance = finite(
      check.tolerance_input_units,
      Number.MIN_VALUE,
      NATIVE_LIMITS.scalarMax,
      `${label} tolerance`,
    );
    const limitations = text(check.limitations, `${label} limitations`);
    return `${label}: megfelelt a korlátos numerikus vizsgálatnak. Vizsgált háromszögpárok: ${pairs} / ${pairLimit}; térfabejárások: ${visits} / ${visitLimit}; tolerancia: ${tolerance} mm. Módszer: ${check.method}. Közelítő vizsgálat, nem egzakt bizonyítás. Korlátok: ${limitations}.`;
  });
  require(
    Array.isArray(report.diagnostics) && report.diagnostics.length <= 64,
    "Invalid print diagnostics",
  );
  for (const diagnostic of report.diagnostics) {
    text(diagnostic, "print diagnostic");
  }
  return {
    report,
    summary:
      `Ellenőrzött: ${report.connected_components} összefüggő komponens; ${report.boundary_edges} nyitott él; ${report.nonmanifold_edges} nem-manifold él; ${report.nonmanifold_vertices} nem-manifold csúcs; ${report.inconsistent_edges} irányítási hiba; ${report.degenerate_triangles} degenerált háromszög; ${report.coincident_points} egybeeső csúcs; ${report.unused_points} nem használt csúcs. Pozitív térfogat és float32 STL-visszaolvasás: megfelelt. ${
        checkSummaries.join(" ")
      } Nyomtathatóság nem igazolt; szeletelőben ellenőrizd a méretet, támaszokat és gyártási beállításokat.`,
  };
}

/** Adapt native tuple data through the same bounded geometry validation as B-rep meshes.
 * Only a surface measurement is computed here; every vertex/triangle is native output.
 */
export function parseFieldMesh(
  value: unknown,
): NonNullable<NativeEvent["mesh"]> {
  const data = object(value, "field mesh");
  const mesh = parseMesh({
    mesh: { id: "field:pikachu", revision: 1, data },
    points: Array.isArray(data.points) ? data.points.length : 0,
    triangles: Array.isArray(data.triangles) ? data.triangles.length : 0,
    volume: 0,
    area: 0,
    open: 0,
    provenance: { source: "field:krly3d.field.native/1:pikachu.field.json" },
  });
  let area = 0;
  for (let i = 0; i < mesh.indices.length; i += 3) {
    const [a, b, c] = mesh.indices.slice(i, i + 3).map((index) => index * 3);
    const p = mesh.positions;
    const ux = p[b] - p[a], uy = p[b + 1] - p[a + 1], uz = p[b + 2] - p[a + 2];
    const vx = p[c] - p[a], vy = p[c + 1] - p[a + 1], vz = p[c + 2] - p[a + 2];
    area +=
      Math.hypot(uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx) / 2;
  }
  return {
    ...mesh,
    area: finite(area, 0, Number.MAX_VALUE, "field mesh area"),
    units: "mm",
  };
}

/** Fixed volumetric demo, with no image or caller-controlled recipe/executable paths. */
export async function* character(
  signal: AbortSignal,
): AsyncGenerator<NativeEvent> {
  yield* bridge(signal, async (scope, emit) => {
    scope.characterProfile = await resolveCharacterProfile();
    require(
      scope.characterProfile,
      "A complete native field/print binary pair is required (release or debug)",
    );
    scope.control.signal.throwIfAborted();
    const bytes = await boundedFile(
      new URL("models/pikachu.field.json", ROOT),
      NATIVE_LIMITS.artifactBytes,
    );
    const recipe = `${scope.directory}/pikachu.field.json`;
    const output = `${scope.directory}/output`;
    await Deno.writeFile(recipe, bytes);
    await Deno.mkdir(output);
    let sequence = 0;
    let completion: Record<string, unknown> | undefined;
    await runProcess(
      scope,
      "field",
      ["--recipe", recipe, "--output", output],
      (record) => {
        require(
          record.stage === ["sampling", "mesh", "completed"][sequence++],
          "Unexpected field stage",
        );
        if (record.stage === "completed") {
          require(
            record.units === "mm" && record.recipe_version === 1 &&
              record.engine_revision === "krly3d.field.native/1" &&
              record.exactness === "approximate",
            "Unsupported field completion",
          );
          finite(
            record.cell_size,
            Number.MIN_VALUE,
            NATIVE_LIMITS.scalarMax,
            "field cell size",
          );
          completion = record;
        } else {
          integer(record.grid_nodes, 1, Number.MAX_SAFE_INTEGER, "grid nodes");
          emit({
            kind: record.stage === "mesh" ? "field_mesh" : "sampling",
            details: record,
          });
        }
      },
    );
    require(sequence === 3 && completion, "Field ended before completed");
    const meshFile = await artifact(scope, "mesh.json", "application/json");
    const mesh = parseFieldMesh(JSON.parse(decoder().decode(meshFile.bytes)));
    require(
      completion.points === mesh.positions.length / 3 &&
        completion.triangles === mesh.indices.length / 3,
      "Field completion mesh counts differ",
    );
    emit({ kind: "printing" });
    const exit = await runProcess(
      scope,
      "print",
      [
        "--mesh",
        `${output}/mesh.json`,
        "--output",
        `${output}/pikachu.stl`,
        "--report",
        `${output}/print-report.json`,
        "--units",
        "mm",
      ],
      () => {},
      [0, 1],
    );
    const reportFile = await artifact(
      scope,
      "print-report.json",
      "application/json",
    );
    const { report, summary } = parsePrintReport(
      JSON.parse(decoder().decode(reportFile.bytes)),
      mesh,
    );
    require((exit === 0) === report.exported, "Print exit/report disagree");
    emit({ kind: "print_validation", text: summary });
    emit({ kind: "artifact", artifact: reportFile });
    if (!report.exported) throw new Error(summary);
    const stl = await artifact(scope, "pikachu.stl", "model/stl");
    require(
      stl.bytes.length >= 84 &&
        new DataView(stl.bytes.buffer, stl.bytes.byteOffset).getUint32(
            80,
            true,
          ) === mesh.indices.length / 3 &&
        stl.bytes.length === 84 + 50 * mesh.indices.length / 3,
      "Invalid binary STL triangle count/size",
    );
    emit({
      kind: "final_mesh",
      mesh: {
        ...mesh,
        volume: report.signed_volume as number,
        openEdges: report.boundary_edges as number,
      },
      text: "Stilizált Pikachu · térbeli mesh",
    });
    emit({ kind: "artifact", artifact: meshFile });
    emit({ kind: "artifact", artifact: stl });
    scope.artifactBytes += bytes.length;
    require(
      scope.artifactBytes <= NATIVE_LIMITS.totalArtifactBytes,
      "Native artifacts exceed total byte limit",
    );
    emit({
      kind: "artifact",
      artifact: { name: "pikachu.field.json", mime: "application/json", bytes },
    });
    emit({
      kind: "completed",
      details: { demo: true, variant: "character", units: "mm", ...completion },
    });
  });
}
