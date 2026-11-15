import {
  character,
  demo,
  NATIVE_LIMITS,
  type NativeEvent,
  parseFieldMesh,
  parseMesh,
  parsePrintReport,
  readNdjson,
  reconstruct,
  type ReconstructionInput,
  resolveCharacterProfile,
  STUDIO_OUTLINE_TOLERANCE_PIXELS,
  validateInput,
} from "./native.ts";

// No downloads or test dependencies. Run with --allow-read --allow-write
// --allow-run. Real integrations are ignored only when their binary is absent.
const ROOT = new URL("../../../../", import.meta.url);
const enc = new TextEncoder();

function assert(value: unknown, message = "Assertion failed"): asserts value {
  if (!value) throw new Error(message);
}

function equal(actual: unknown, expected: unknown) {
  assert(
    JSON.stringify(actual) === JSON.stringify(expected),
    `Expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
  );
}

function throws(fn: () => unknown, pattern?: RegExp) {
  try {
    fn();
  } catch (error) {
    assert(error instanceof Error);
    if (pattern) assert(pattern.test(error.message), error.message);
    return;
  }
  throw new Error("Expected an exception");
}

async function rejects(
  fn: () => Promise<unknown>,
  pattern?: RegExp,
  name?: string,
) {
  try {
    await fn();
  } catch (error) {
    assert(error instanceof Error);
    if (pattern) assert(pattern.test(error.message), error.message);
    if (name) equal(error.name, name);
    return;
  }
  throw new Error("Expected rejection");
}

async function collect<T>(source: AsyncIterable<T>): Promise<T[]> {
  const values: T[] = [];
  for await (const value of source) values.push(value);
  return values;
}

function stream(chunks: Uint8Array[]): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(chunk);
      controller.close();
    },
  });
}

function raster(): Uint8Array {
  const header = enc.encode("P5 32 24 255\n");
  const bytes = new Uint8Array(header.length + 32 * 24);
  bytes.set(header);
  for (let y = 0; y < 24; y++) {
    for (let x = 0; x < 32; x++) {
      const material = x >= 3 && x < 29 && y >= 3 && y < 21 &&
        !(x >= 9 && x < 22 && y >= 8 && y < 16);
      bytes[header.length + y * 32 + x] = material ? 0 : 255;
    }
  }
  return bytes;
}

function input(): ReconstructionInput {
  return {
    bytes: raster(),
    format: "pnm",
    height: 3,
    pixelUnit: 0.25,
    seed: [5, 5],
  };
}

function meshResult() {
  // A codec fixture only; production geometry always comes from Rust.
  return {
    points: 3,
    triangles: 1,
    volume: 1.5,
    area: 2,
    open: 3,
    provenance: { source: "brep-session:test:part@1:build" },
    mesh: {
      id: "mesh:test",
      revision: 1,
      data: {
        points: [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
        triangles: [[0, 1, 2]],
      },
    },
  };
}

Deno.test("native mesh decoder uses tuple data and real provenance", () => {
  equal(parseMesh(meshResult()), {
    positions: [0, 0, 0, 1, 0, 0, 0, 1, 0],
    indices: [0, 1, 2],
    volume: 1.5,
    area: 2,
    openEdges: 3,
    source: "brep-session:test:part@1:build",
  });
});

Deno.test("native mesh decoder rejects malformed, nonfinite, oversized and out-of-range geometry", () => {
  const cases: ((value: ReturnType<typeof meshResult>) => void)[] = [
    (v) => {
      v.mesh.data.points[0] = [0, 0];
    },
    (v) => {
      v.mesh.data.points[0][0] = NaN;
    },
    (v) => {
      v.mesh.data.points[0][0] = Infinity;
    },
    (v) => {
      v.mesh.data.points[0][0] = NATIVE_LIMITS.coordinateMax + 1;
    },
    (v) => {
      v.mesh.data.triangles[0][0] = 3;
    },
    (v) => {
      v.mesh.data.triangles[0][0] = -1;
    },
    (v) => {
      v.mesh.data.triangles[0][0] = 0.5;
    },
    (v) => {
      v.mesh.data.triangles[0].pop();
    },
    (v) => {
      v.points = 4;
    },
    (v) => {
      v.points = NATIVE_LIMITS.vertices + 1;
    },
    (v) => {
      v.triangles = NATIVE_LIMITS.triangles + 1;
    },
    (v) => {
      v.area = -1;
    },
    (v) => {
      v.volume = Infinity;
    },
    (v) => {
      v.open = Number.MAX_SAFE_INTEGER + 1;
    },
    (v) => {
      v.provenance.source = "";
    },
  ];
  for (const mutate of cases) {
    const value = meshResult();
    mutate(value);
    throws(() => parseMesh(value));
  }
  throws(() => parseMesh({ coordinates: [0, 0, 0], corners: [0, 0, 0] }));
  throws(() => parseMesh({ ...meshResult(), mesh: null }));
});

Deno.test("native input validation snapshots bytes and seed", () => {
  const value = input();
  const checked = validateInput(value);
  value.bytes.fill(0);
  value.seed![0] = 20;
  equal(checked.bytes, raster());
  equal(checked.seed, [5, 5]);
  equal(checked.tolerance, 0.05);
  const pnm = enc.encode("P2\n# comment\n2 1\n255\n0 255\n");
  validateInput({ ...input(), bytes: pnm, seed: [0, 0] });
});

Deno.test("native input validation rejects caller paths, bad scalars, bombs and wrong formats", async () => {
  const bad: unknown[] = [
    null,
    {},
    { ...input(), path: "/etc/passwd" },
    { ...input(), executable: "sh" },
    { ...input(), format: "svg" },
    { ...input(), format: "png" },
    { ...input(), bytes: new Uint8Array() },
    { ...input(), bytes: [] },
    { ...input(), bytes: new Uint8Array(NATIVE_LIMITS.imageBytes + 1) },
    { ...input(), bytes: new Uint8Array(new SharedArrayBuffer(32)) },
    ...[0, -1, NaN, Infinity, NATIVE_LIMITS.scalarMax + 1, "3"].flatMap((n) => [
      { ...input(), height: n },
      { ...input(), pixelUnit: n },
    ]),
    ...[0, NaN, Infinity, null, "0.05", NATIVE_LIMITS.toleranceMin / 2].map((
      tolerance,
    ) => ({ ...input(), tolerance })),
    ...[null, [0], [0, 0, 0], [-1, 0], [NaN, 0], [32, 0], [0, 24], ["5", 5]]
      .map((seed) => ({ ...input(), seed })),
    { ...input(), bytes: enc.encode("P5 8193 1 255\n") },
    { ...input(), bytes: enc.encode("P5 8192 8192 255\n") },
  ];
  for (const value of bad) {
    await rejects(() =>
      reconstruct(value as ReconstructionInput, new AbortController().signal)
        .next()
    );
  }
});

Deno.test("native dimensions validate PNG and JPEG headers before decoding", () => {
  const png = new Uint8Array(33);
  png.set([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82]);
  const view = new DataView(png.buffer);
  view.setUint32(16, 32);
  view.setUint32(20, 24);
  validateInput({ ...input(), format: "png", bytes: png });
  view.setUint32(16, 100_000);
  throws(() => validateInput({ ...input(), format: "png", bytes: png }));
  const jpeg = new Uint8Array([
    255,
    216,
    255,
    224,
    0,
    4,
    0,
    0,
    255,
    192,
    0,
    8,
    8,
    0,
    24,
    0,
    32,
    1,
  ]);
  validateInput({ ...input(), format: "jpeg", bytes: jpeg });
  for (
    const bytes of [
      jpeg.subarray(0, 10),
      enc.encode("bogus"),
      new Uint8Array([255, 216, 255, 218]),
    ]
  ) {
    throws(() => validateInput({ ...input(), format: "jpeg", bytes }));
  }
});

const SMALL = { lineBytes: 64, totalBytes: 256, records: 8 };
Deno.test("native NDJSON handles fragmented UTF-8, CRLF and multiple records per chunk", async () => {
  const bytes = enc.encode('{"name":"árvíz"}\r\n{"n":2}\n');
  const chunks = Array.from(bytes, (v) => new Uint8Array([v]));
  equal(await collect(readNdjson(stream(chunks), SMALL)), [{ name: "árvíz" }, {
    n: 2,
  }]);
  equal(await collect(readNdjson(stream([bytes]), SMALL)), [{ name: "árvíz" }, {
    n: 2,
  }]);
});

Deno.test("native NDJSON enforces bytes, count, UTF-8, JSON shape and terminated records", async () => {
  for (
    const value of ["{}", "{}\n{", "\n", " \n", "[]\n", "null\n", "{oops}\n"]
  ) {
    await rejects(() =>
      collect(readNdjson(stream([enc.encode(value)]), SMALL))
    );
  }
  await rejects(() =>
    collect(
      readNdjson(
        stream([new Uint8Array([123, 34, 120, 34, 58, 34, 255, 34, 125, 10])]),
        SMALL,
      ),
    )
  );
  await rejects(
    () =>
      collect(
        readNdjson(
          stream([enc.encode('{"x":"'), enc.encode("x".repeat(64))]),
          SMALL,
        ),
      ),
    /line byte/,
  );
  await rejects(
    () =>
      collect(
        readNdjson(stream([enc.encode("{}\n{}\n")]), {
          ...SMALL,
          totalBytes: 5,
        }),
      ),
    /total byte/,
  );
  await rejects(
    () =>
      collect(
        readNdjson(stream([enc.encode("{}\n{}\n")]), { ...SMALL, records: 1 }),
      ),
    /record limit/,
  );
});

Deno.test("native NDJSON cancellation interrupts a pending read and releases its stream", async () => {
  let cancelled = false;
  const control = new AbortController();
  const source = new ReadableStream<Uint8Array>({
    cancel() {
      cancelled = true;
    },
  });
  const pending = collect(readNdjson(source, SMALL, control.signal));
  control.abort(new DOMException("test", "AbortError"));
  await rejects(() => pending, undefined, "AbortError");
  assert(cancelled);
  assert(!source.locked);
});

async function absent(path: string) {
  try {
    await Deno.lstat(path);
    return false;
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) return true;
    throw error;
  }
}

async function eventually(
  check: () => boolean | Promise<boolean>,
  timeout = 4000,
) {
  const until = performance.now() + timeout;
  while (!await check()) {
    assert(performance.now() < until, "Timed out waiting for cleanup");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

type Fake = {
  signals: Deno.Signal[];
  directory: string;
  options: Deno.CommandOptions;
  executable: string | URL;
  out: (record: unknown) => void;
  raw: (bytes: Uint8Array) => void;
  err: (bytes: Uint8Array) => void;
  exit: (success?: boolean) => void;
  stderrCancelled: boolean;
};

/** Replace only the platform process boundary; no arbitrary executable API is exposed. */
async function fakeNative(
  ignoreTerm: boolean,
  body: (get: () => Fake) => Promise<void>,
  onSpawn?: (fake: Fake) => void,
) {
  const commandDescriptor = Object.getOwnPropertyDescriptor(Deno, "Command")!;
  const statDescriptor = Object.getOwnPropertyDescriptor(Deno, "lstat")!;
  const stat = Deno.lstat;
  let fake: Fake | undefined;
  class Command {
    constructor(
      private executable: string | URL,
      private options: Deno.CommandOptions,
    ) {}
    spawn() {
      let stdout!: ReadableStreamDefaultController<Uint8Array>;
      let stderr!: ReadableStreamDefaultController<Uint8Array>;
      let resolve!: (status: Deno.CommandStatus) => void;
      let done = false;
      const status = new Promise<Deno.CommandStatus>((r) => {
        resolve = r;
      });
      const output = new ReadableStream<Uint8Array>({
        start(c) {
          stdout = c;
        },
      });
      const errors = new ReadableStream<Uint8Array>({
        start(c) {
          stderr = c;
        },
        cancel() {
          fake!.stderrCancelled = true;
        },
      });
      const args = this.options.args!;
      const directory = args.includes("--recipe")
        ? args[args.indexOf("--recipe") + 1].replace(
          /\/pikachu\.field\.json$/,
          "",
        )
        : args.includes("--mesh")
        ? args[args.indexOf("--mesh") + 1].replace(/\/output\/mesh\.json$/, "")
        : args.includes("--model")
        ? args[args.indexOf("--model") + 1].replace(/\/[^/]+$/, "")
        : args[args.indexOf("--image") + 1].replace(/\/input\.[a-z]+$/, "");
      const exit = (success = false) => {
        if (done) return;
        done = true;
        try {
          stdout.close();
        } catch { /* cancelled reader */ }
        try {
          stderr.close();
        } catch { /* cancelled reader */ }
        resolve({
          success,
          code: success ? 0 : 1,
          signal: success ? null : "SIGTERM",
        });
      };
      fake = {
        directory,
        signals: [],
        options: this.options,
        executable: this.executable,
        out: (record) =>
          stdout.enqueue(enc.encode(`${JSON.stringify(record)}\n`)),
        raw: (bytes) => stdout.enqueue(bytes),
        err: (bytes) => stderr.enqueue(bytes),
        exit,
        stderrCancelled: false,
      };
      onSpawn?.(fake);
      return {
        status,
        stdout: output,
        stderr: errors,
        kill(signal: Deno.Signal) {
          fake!.signals.push(signal);
          if (!ignoreTerm || signal === "SIGKILL") exit();
        },
      };
    }
  }
  Object.defineProperty(Deno, "Command", {
    configurable: true,
    enumerable: commandDescriptor.enumerable,
    value: Command,
  });
  Object.defineProperty(Deno, "lstat", {
    ...statDescriptor,
    value: (path: string | URL) => {
      if (
        path instanceof URL &&
        /\/target\/(debug|release)\/krly3d-(mesh|reconstruct|field|print)$/
          .test(
            path.pathname,
          )
      ) {
        return Promise.resolve({ isFile: true, isSymlink: false });
      }
      return stat(path);
    },
  });
  try {
    await body(() => {
      assert(fake, "Process has not spawned");
      return fake;
    });
  } finally {
    fake?.exit();
    Object.defineProperty(Deno, "Command", commandDescriptor);
    Object.defineProperty(Deno, "lstat", statDescriptor);
  }
}

function stage(seq = 0, kind = "started", extra: Record<string, unknown> = {}) {
  return {
    protocol: "krly3d.shared-reconstruction/2",
    flow_id: "test-flow",
    seq,
    revision: 1,
    elapsed_ms: seq,
    stage_ms: 0,
    kind,
    ...extra,
  };
}

Deno.test("native paused iterator abort sends TERM then KILL, reaps and removes workspace without next()", async () => {
  await fakeNative(true, async (get) => {
    const control = new AbortController();
    const iterator = reconstruct(input(), control.signal);
    equal((await iterator.next()).value?.kind, "started");
    const fake = get();
    equal(
      String(fake.executable),
      String(new URL("target/debug/krly3d-reconstruct", ROOT)),
    );
    equal(String(fake.options.cwd), String(ROOT));
    assert(fake.options.clearEnv);
    equal(fake.options.stdin, "null");
    assert(fake.options.args!.includes("--stream"));
    const started = performance.now();
    control.abort();
    // Deliberately leave the consumer paused at yield throughout cleanup.
    await eventually(() => absent(fake.directory));
    equal(fake.signals, ["SIGTERM", "SIGKILL"]);
    assert(performance.now() - started >= NATIVE_LIMITS.termGraceMs - 20);
    assert(fake.stderrCancelled);
    await rejects(() => iterator.next(), undefined, "AbortError");
  }, (fake) => {
    fake.err(new Uint8Array(NATIVE_LIMITS.stderrRetainedBytes * 4).fill(120));
    fake.out(stage());
  });
});

Deno.test("native forwards one source-pixel outline tolerance without downsampling a 512px image", async () => {
  const header = enc.encode("P5 512 512 255\n");
  const bytes = new Uint8Array(header.length + 512 * 512).fill(255);
  bytes.set(header);
  for (let y = 32; y < 480; y++) {
    bytes.fill(0, header.length + y * 512 + 32, header.length + y * 512 + 480);
  }
  await fakeNative(false, async (get) => {
    const iterator = reconstruct({
      ...input(),
      bytes,
      pixelUnit: 0.025,
      seed: [64, 64],
    }, new AbortController().signal);
    try {
      equal((await iterator.next()).value?.kind, "started");
      const args = get().options.args!;
      const outlineAt = args.indexOf("--outline-tolerance");
      assert(outlineAt >= 0);
      equal(args.slice(outlineAt, outlineAt + 2), ["--outline-tolerance", "1"]);
      equal(STUDIO_OUTLINE_TOLERANCE_PIXELS, 1);
      equal(args[args.indexOf("--pixel-unit") + 1], "0.025");
      equal(await Deno.readFile(args[args.indexOf("--image") + 1]), bytes);
    } finally {
      await iterator.return(undefined);
    }
  }, (fake) => fake.out(stage()));
});

Deno.test("native iterator return cancels child and removes workspace", async () => {
  await fakeNative(false, async (get) => {
    const iterator = reconstruct(input(), new AbortController().signal);
    await iterator.next();
    const fake = get();
    await iterator.return(undefined);
    equal(fake.signals, ["SIGTERM"]);
    assert(await absent(fake.directory));
  }, (fake) => fake.out(stage()));
});

Deno.test("native failures reject iteration, terminate process and clean partial artifacts", async () => {
  const cases: ((fake: Fake) => void)[] = [
    (fake) =>
      fake.raw(enc.encode("{" + "x".repeat(NATIVE_LIMITS.streamLineBytes))),
    (fake) => fake.err(new Uint8Array(NATIVE_LIMITS.stderrBytes + 1)),
    (fake) => fake.out(stage(1)),
    (fake) => fake.out(stage(0, "completed")),
    (fake) => {
      fake.out(stage());
      fake.out(stage(1, "evidence", { artifact: "../../etc/passwd" }));
    },
    (fake) => {
      fake.out(stage());
      fake.out(stage(1, "failed", { message: "native failure" }));
    },
    (fake) => {
      fake.out(stage());
      fake.exit(true);
    },
  ];
  for (const onSpawn of cases) {
    await fakeNative(false, async (get) => {
      await rejects(() =>
        collect(reconstruct(input(), new AbortController().signal))
      );
      assert(await absent(get().directory));
    }, onSpawn);
  }
});

Deno.test("native pre-aborted calls fail without starting work", async () => {
  const control = new AbortController();
  control.abort("untrusted reason");
  await rejects(() => demo(control.signal).next(), undefined, "AbortError");
  await rejects(
    () => reconstruct(input(), control.signal).next(),
    undefined,
    "AbortError",
  );
});

Deno.test("native artifact reads reject oversized files and symlinks before exposing bytes", async () => {
  for (const mode of ["oversized", "symlink"]) {
    await fakeNative(false, async (get) => {
      const iterator = reconstruct(input(), new AbortController().signal);
      await iterator.next();
      const fake = get();
      await Deno.mkdir(`${fake.directory}/output`);
      const path = `${fake.directory}/output/evidence.json`;
      if (mode === "oversized") {
        const file = await Deno.open(path, { create: true, write: true });
        try {
          await file.truncate(NATIVE_LIMITS.artifactBytes + 1);
        } finally {
          file.close();
        }
      } else {
        await Deno.symlink(new URL("models/plate-with-hole.k3d", ROOT), path);
      }
      fake.out(stage(1, "evidence", { artifact: "evidence.json" }));
      await rejects(() => collect(iterator), /bounded regular file/);
      assert(await absent(fake.directory));
    }, (fake) => fake.out(stage()));
  }
});

async function binaryExists(name: string): Promise<boolean> {
  try {
    return (await Deno.stat(new URL(`target/debug/${name}`, ROOT))).isFile;
  } catch (error) {
    if (error instanceof Deno.errors.NotFound) return false;
    throw error;
  }
}

const hasReconstruct = await binaryExists("krly3d-reconstruct");
const hasMesh = await binaryExists("krly3d-mesh");

function byKind(events: NativeEvent[], kind: string): NativeEvent {
  const event = events.find((e) => e.kind === kind);
  assert(event, `Missing ${kind}`);
  return event;
}

Deno.test({
  name:
    "native real reconstruction streams evidence, coarse/final B-rep mesh, PNG, likeness and k3d",
  ignore: !hasReconstruct,
  async fn() {
    const events = await collect(
      reconstruct(input(), new AbortController().signal),
    );
    equal(events.map((event) => event.kind), [
      "started",
      "evidence",
      "coarse_preview",
      "editable_proposal",
      "artifact",
      "artifact",
      "preview",
      "validation",
      "completed",
    ]);
    const evidence = byKind(events, "evidence").artifact!;
    equal(evidence.name, "evidence.json");
    const parsed = JSON.parse(new TextDecoder().decode(evidence.bytes));
    equal(parsed.evidence.polygons.length, 2);
    const coarse = byKind(events, "coarse_preview");
    equal(coarse.details!.approximation, "bounding_prism");
    const final = byKind(events, "editable_proposal").mesh!;
    const construction = byKind(events, "editable_proposal").construction!;
    assert(construction.version === 1);
    equal(construction.coordinateSystem, "right_handed_z_up");
    equal(construction.sketchPlane, "XY");
    equal(construction.sketchOffset, 0);
    equal(construction.sourceCenter, [4, 3]);
    equal(construction.sourceBounds, [0.75, 0.75, 7.25, 5.25]);
    equal(construction.extrusionDirection, [0, 0, 1]);
    equal(construction.extrusionDistance, 3);
    equal(construction.sketchLoops.length, 2);
    equal(construction.sketchLoops.map((loop) => loop.hole), [false, true]);
    assert(final.positions.length > 0 && final.indices.length > 0);
    const extent = (axis: number) => final.positions.reduce((range, value, index) => {
      if (index % 3 !== axis) return range;
      return [Math.min(range[0], value), Math.max(range[1], value)] as [number, number];
    }, [Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY] as [number, number]);
    const [minX, maxX] = extent(0), [minY, maxY] = extent(1);
    assert(Math.abs(minX + maxX) < 1e-9, "Model X bounds should be centered at the UCS origin");
    assert(Math.abs(minY + maxY) < 1e-9, "Model Y bounds should be centered at the UCS origin");
    equal(extent(2), [0, 3]);
    assert(Math.abs(final.volume - 364 * 0.25 ** 2 * 3) < 1e-8);
    equal(final.openEdges, 0);
    assert(
      coarse.mesh!.volume > final.volume,
      "Coarse bounds must still fill the through hole",
    );
    const model = byKind(events, "artifact").artifact!;
    equal(model.name, "proposal.k3d");
    assert(new TextDecoder().decode(model.bytes).startsWith("k3d 1\n"));
    const meshArtifact = events.find((event) =>
      event.artifact?.name === "mesh.json"
    )!.artifact!;
    const downloadedMesh = parseMesh(
      JSON.parse(new TextDecoder().decode(meshArtifact.bytes)),
    );
    equal(downloadedMesh, final);
    const preview = byKind(events, "preview");
    equal(preview.artifact!.mime, "image/png");
    equal([...preview.artifact!.bytes.subarray(0, 8)], [
      137,
      80,
      78,
      71,
      13,
      10,
      26,
      10,
    ]);
    const validation = byKind(events, "validation");
    equal(validation.details!.matches, true);
    const likeness = JSON.parse(
      new TextDecoder().decode(validation.artifact!.bytes),
    );
    equal(likeness.provenance.source.source, final.source);
    const manifest = JSON.parse(
      new TextDecoder().decode(byKind(events, "completed").artifact!.bytes),
    );
    equal(manifest.brep_source, final.source);
    equal(manifest.execution, "in_process");
    // Bytes remain usable after the workspace has been removed.
    assert(await absent(parsed.provenance.source.replace(/\/input\.pnm$/, "")));
  },
});

Deno.test({
  name:
    "native real reconstruction abort cleans workspace while iterator is paused",
  ignore: !hasReconstruct,
  async fn() {
    const control = new AbortController();
    const iterator = reconstruct(input(), control.signal);
    try {
      equal((await iterator.next()).value?.kind, "started");
      const evidence = (await iterator.next()).value!.artifact!;
      const parsed = JSON.parse(new TextDecoder().decode(evidence.bytes));
      const directory = parsed.provenance.source.replace(/\/input\.pnm$/, "");
      control.abort();
      await eventually(() => absent(directory));
      await rejects(() => iterator.next(), undefined, "AbortError");
    } finally {
      control.abort();
      await iterator.return(undefined);
    }
  },
});

Deno.test({
  name:
    "native real demo uses the unchanged plate-with-hole model and native mesh v2",
  ignore: !hasMesh,
  async fn() {
    const events = await collect(demo(new AbortController().signal));
    equal(events.map((e) => e.kind), [
      "started",
      "editable_proposal",
      "artifact",
      "completed",
    ]);
    const nativeModel = await Deno.readFile(
      new URL("models/plate-with-hole.k3d", ROOT),
    );
    equal(byKind(events, "artifact").artifact!.bytes, nativeModel);
    const mesh = byKind(events, "editable_proposal").mesh!;
    assert(mesh.positions.length > 100 && mesh.indices.length > 100);
    equal(mesh.openEdges, 0);
    assert(mesh.volume > 23_000 && mesh.volume < 24_000);
    assert(mesh.source.includes("part@3"));
    equal(byKind(events, "completed").details!.source, mesh.source);
  },
});

Deno.test("native demo rejects unknown variants and caller paths", async () => {
  for (
    const variant of [
      "../plate-with-hole.k3d",
      "/etc/passwd",
      "pegboard",
      "",
      null,
    ]
  ) {
    await rejects(
      () => demo(new AbortController().signal, variant as "plate").next(),
      /Unknown demo variant/,
    );
  }
});

Deno.test({
  name:
    "native complex demo computes four distinct closed construction stages within the interactive budget",
  ignore: !hasMesh,
  async fn() {
    const started = performance.now();
    const events = await collect(demo(new AbortController().signal, "complex"));
    const elapsed = performance.now() - started;
    console.info(`Complex native demo: ${elapsed.toFixed(0)} ms`);
    assert(
      elapsed < 15_000,
      `Complex native demo took ${elapsed.toFixed(0)} ms`,
    );
    equal(events.map((event) => event.kind), [
      "started",
      "construction_step",
      "construction_step",
      "construction_step",
      "editable_proposal",
      "artifact",
      "completed",
    ]);
    const stages = events.filter((event) => event.mesh !== undefined);
    equal(stages.map((event) => event.details!.node), [
      "part@1",
      "part@5",
      "part@9",
      "part@14",
    ]);
    equal(stages.map((event) => event.details!.step), [1, 2, 3, 4]);
    equal(new Set(stages.map((event) => event.mesh!.volume)).size, 4);
    equal(new Set(stages.map((event) => event.mesh!.positions.length)).size, 4);
    equal(
      new Set(stages.map((event) => JSON.stringify(event.mesh!.positions)))
        .size,
      4,
    );
    for (const stage of stages) {
      equal(stage.mesh!.openEdges, 0);
      assert(stage.mesh!.volume > 0);
      assert(stage.text && stage.text.length > 3);
      assert(stage.mesh!.source.includes(`:${stage.details!.node}:`));
    }
    equal(stages[0].mesh!.volume, 84 * 54 * 6);
    assert(stages[1].mesh!.volume < stages[0].mesh!.volume);
    assert(stages[2].mesh!.volume > stages[1].mesh!.volume);
    const final = stages[3].mesh!;
    // Mesh volume approximates four radius-3 mounting holes, four radius-5
    // raised bosses and two radius-7 windows; allow native tessellation error.
    const expectedVolume = 49_872 - 500 * Math.PI;
    assert(
      Math.abs(final.volume - expectedVolume) < 15,
      `Unexpected bracket volume ${final.volume}`,
    );
    const bounds = [0, 1, 2].map((axis) => {
      const values = final.positions.filter((_, index) => index % 3 === axis);
      return [Math.min(...values), Math.max(...values)];
    });
    equal(bounds, [[-42, 42], [-27, 27], [0, 36]]);
    const document = byKind(events, "artifact").artifact!;
    equal(document.name, "studio-bracket.k3d");
    equal(
      document.bytes,
      await Deno.readFile(new URL("models/studio-bracket.k3d", ROOT)),
    );
    equal(byKind(events, "completed").details!.source, final.source);
  },
});

const fieldFixture = {
  points: [[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1]],
  triangles: [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
};
function intersectionFixture() {
  return {
    status: "passed",
    method: "bounded_bvh_triangle_contacts/1",
    exact: false,
    tested_pairs: 6,
    node_visits: 8,
    first_pair: null,
    tolerance_input_units: 1e-12,
    candidate_limit: 100,
    node_visit_limit: 200,
    reason: null,
    limitations:
      "floating-point contact test; not an exact proof or a wall-thickness/support test",
  };
}
function printFixture() {
  return {
    format: "krly3d.mesh.print-report/1",
    units: "mm",
    points: 4,
    triangles: 4,
    unused_points: 0,
    coincident_points: 0,
    degenerate_triangles: 0,
    boundary_edges: 0,
    nonmanifold_edges: 0,
    inconsistent_edges: 0,
    nonmanifold_vertices: 0,
    connected_components: 1,
    signed_volume: 1 / 6,
    topology_valid: true,
    export_eligible: true,
    exported: true,
    print_ready: false,
    error: null,
    float32_roundtrip: {
      status: "passed",
      self_intersections: intersectionFixture(),
    },
    self_intersections: intersectionFixture(),
    wall_thickness: { status: "unknown", reason: "not implemented" },
    diagnostics: [],
  };
}
Deno.test("field tuple adapter preserves native geometry and report refuses unsupported print claims", () => {
  const mesh = parseFieldMesh(fieldFixture);
  equal(mesh.positions, fieldFixture.points.flat());
  equal(mesh.indices, fieldFixture.triangles.flat());
  equal(mesh.units, "mm");
  assert(Math.abs(mesh.area - (1.5 + Math.sqrt(3) / 2)) < 1e-12);
  const valid = printFixture();
  const summary = parsePrintReport(valid, mesh).summary;
  assert(summary.includes("Önmetszés · eredeti mesh: megfelelt"));
  assert(summary.includes("Önmetszés · float32 STL: megfelelt"));
  assert(summary.includes("nem egzakt bizonyítás"));
  assert(summary.includes("Falvastagság: ismeretlen"));
  assert(
    parsePrintReport(valid, mesh).summary.includes(
      "Nyomtathatóság nem igazolt",
    ),
  );
  for (
    const report of [
      { ...valid, points: 5 },
      { ...valid, units: "m" },
      { ...valid, print_ready: true },
      { ...valid, signed_volume: NaN },
      { ...valid, boundary_edges: 1 },
      { ...valid, float32_roundtrip: { status: "not_run" } },
      { ...valid, self_intersections: { status: null } },
      {
        ...valid,
        self_intersections: { ...intersectionFixture(), status: "unknown" },
      },
      {
        ...valid,
        self_intersections: { ...intersectionFixture(), tested_pairs: 101 },
      },
      {
        ...valid,
        self_intersections: { ...intersectionFixture(), first_pair: [0, 1] },
      },
      {
        ...valid,
        float32_roundtrip: { status: "passed", self_intersections: "unknown" },
      },
      {
        ...valid,
        float32_roundtrip: {
          status: "passed",
          self_intersections: { ...intersectionFixture(), status: "failed" },
        },
      },
      {
        ...valid,
        float32_roundtrip: {
          status: "passed",
          self_intersections: { ...intersectionFixture(), status: "unknown" },
        },
      },
    ]
  ) throws(() => parsePrintReport(report, mesh));
  throws(() => parseFieldMesh({ ...fieldFixture, triangles: [[0, 1, 4]] }));
  throws(() => parseFieldMesh({ ...fieldFixture, points: [[Infinity, 0, 0]] }));
});

async function fieldOutput(fake: Fake) {
  equal(fake.options.clearEnv, true);
  equal(fake.options.stdin, "null");
  assert(!fake.options.env);
  fake.out({ stage: "sampling", grid_nodes: 100 });
  fake.out({ stage: "mesh", grid_nodes: 100 });
  await Deno.writeTextFile(
    `${fake.directory}/output/mesh.json`,
    JSON.stringify(fieldFixture),
  );
  fake.out({
    stage: "completed",
    points: 4,
    triangles: 4,
    units: "mm",
    recipe_version: 1,
    engine_revision: "krly3d.field.native/1",
    exactness: "approximate",
    cell_size: 1,
    grid_spacing: [1, 1, 1],
  });
  fake.exit(true);
}

Deno.test("character workflow is native only, uses isolated fixed paths, exposes mm mesh/STL/report/recipe after both exits", async () => {
  const { workflow } = await import("./workflow.ts");
  const fetch = globalThis.fetch;
  globalThis.fetch = () => {
    throw new Error("Character must never invoke AI");
  };
  const spawned: Fake[] = [];
  try {
    await fakeNative(false, async () => {
      const events: import("./workflow.ts").WorkflowEvent[] = [];
      const result = await workflow(
        {
          mode: "character",
          text: "ignored prompt",
          height: 8,
          pixelUnit: .025,
          tolerance: .05,
          history: [],
        },
        new AbortController().signal,
        async (event) => {
          events.push(event);
        },
      );
      equal(result.usage, []);
      equal(spawned.length, 2);
      assert(
        String(spawned[0].executable).endsWith("/target/release/krly3d-field"),
      );
      assert(
        String(spawned[1].executable).endsWith("/target/release/krly3d-print"),
      );
      equal(spawned[0].directory, spawned[1].directory);
      equal(spawned[1].options.args?.slice(-2), ["--units", "mm"]);
      assert(await absent(spawned[0].directory));
      assert(events.some((event) => event.phase === "sampling"));
      assert(events.some((event) => event.phase === "field_mesh"));
      assert(!events.some((event) => event.phase === "editable_proposal"));
      const mesh = events.find((event) => event.phase === "final_mesh")?.mesh;
      assert(
        mesh?.units === "mm" && mesh.volume === 1 / 6 && mesh.openEdges === 0,
      );
      equal(
        events.flatMap((event) => event.artifact ? [event.artifact.name] : [])
          .sort(),
        ["mesh.json", "pikachu.field.json", "pikachu.stl", "print-report.json"],
      );
    }, (fake) => {
      spawned.push(fake);
      void (async () => {
        if (String(fake.executable).endsWith("krly3d-field")) {
          await fieldOutput(fake);
        } else {
          equal(fake.options.clearEnv, true);
          equal(fake.options.stdin, "null");
          await Deno.writeTextFile(
            `${fake.directory}/output/print-report.json`,
            JSON.stringify(printFixture()),
          );
          const stl = new Uint8Array(84 + 50 * 4);
          new DataView(stl.buffer).setUint32(80, 4, true);
          await Deno.writeFile(`${fake.directory}/output/pikachu.stl`, stl);
          fake.exit(true);
        }
      })().catch((error) => {
        fake.err(enc.encode(String(error)));
        fake.exit();
      });
    });
  } finally {
    globalThis.fetch = fetch;
  }
});

Deno.test("character rejected print preserves failure report without final geometry or STL", async () => {
  const events: NativeEvent[] = [];
  await fakeNative(false, async () => {
    await rejects(async () => {
      for await (const event of character(new AbortController().signal)) {
        events.push(event);
      }
    }, /STL-export sikertelen/);
    assert(
      events.some((event) => event.artifact?.name === "print-report.json"),
    );
    assert(
      !events.some((event) =>
        event.mesh || event.kind === "completed" ||
        event.artifact?.name.endsWith(".stl")
      ),
    );
  }, (fake) => {
    void (async () => {
      if (String(fake.executable).endsWith("krly3d-field")) {
        await fieldOutput(fake);
      } else {
        await Deno.writeTextFile(
          `${fake.directory}/output/print-report.json`,
          JSON.stringify({
            ...printFixture(),
            exported: false,
            error: { code: "print_preflight", message: "boundary edges" },
            float32_roundtrip: { status: "not_run" },
          }),
        );
        fake.exit();
      }
    })().catch((error) => {
      fake.err(enc.encode(String(error)));
      fake.exit();
    });
  });
});

Deno.test("character abort while paused at sampling reaps child and removes workspace", async () => {
  await fakeNative(true, async (get) => {
    const control = new AbortController();
    const iterator = character(control.signal);
    equal((await iterator.next()).value?.kind, "sampling");
    const fake = get();
    control.abort();
    await eventually(() => absent(fake.directory));
    equal(fake.signals, ["SIGTERM", "SIGKILL"]);
    await rejects(() => iterator.next(), undefined, "AbortError");
  }, (fake) => fake.out({ stage: "sampling", grid_nodes: 100 }));
});

Deno.test("character selects a complete release pair, falls back together, rejects symlinks and never mixes profiles", async () => {
  const descriptor = Object.getOwnPropertyDescriptor(Deno, "lstat")!;
  const stat = Deno.lstat;
  let files: Record<string, "file" | "symlink"> = {};
  const inspected: string[] = [];
  Object.defineProperty(Deno, "lstat", {
    ...descriptor,
    value: (path: string | URL) => {
      const key = String(path).match(
        /target\/(release|debug)\/krly3d-(field|print)$/,
      )?.slice(1).join("/");
      if (!key) return stat(path);
      inspected.push(key);
      if (!files[key]) return Promise.reject(new Deno.errors.NotFound());
      return Promise.resolve({
        isFile: true,
        isSymlink: files[key] === "symlink",
      });
    },
  });
  try {
    const debug = { "debug/field": "file", "debug/print": "file" } as const;
    files = { ...debug, "release/field": "file", "release/print": "file" };
    equal(await resolveCharacterProfile(), "release");
    equal(inspected, ["release/field", "release/print"]);
    files = { ...debug, "release/field": "file" };
    equal(await resolveCharacterProfile(), "debug");
    files = { ...debug, "release/field": "file", "release/print": "symlink" };
    equal(await resolveCharacterProfile(), "debug");
    files = { "release/field": "file", "debug/print": "file" };
    equal(await resolveCharacterProfile(), undefined);
    files = {};
    equal(await resolveCharacterProfile(), undefined);
  } finally {
    Object.defineProperty(Deno, "lstat", descriptor);
  }
});
