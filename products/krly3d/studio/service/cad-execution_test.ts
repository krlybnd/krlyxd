import { CAD_EXECUTION_LIMITS, executeCadAttempt } from "./cad-execution.ts";
import type { CadRecipe } from "./ai/cad-recipe.ts";
import type { buildCad, NativeEvent } from "./native.ts";

function assert(value: unknown, message = "Assertion failed"): asserts value {
  if (!value) throw new Error(message);
}

function equal(actual: unknown, expected: unknown) {
  assert(JSON.stringify(actual) === JSON.stringify(expected));
}

async function rejects(action: () => Promise<unknown>, expected?: unknown) {
  try {
    await action();
  } catch (error) {
    if (expected !== undefined) {
      assert(error === expected, "Error identity changed");
    }
    return error;
  }
  throw new Error("Expected rejection");
}

const recipe: CadRecipe = {
  units: "mm",
  steps: [{
    type: "box",
    id: "body",
    title: "Body",
    size: [1, 1, 1],
    center: [0, 0, 0],
    radius: null,
    height: null,
    inputs: null,
  }],
};

function mesh(kind = "editable_proposal"): NativeEvent {
  // Outward-oriented unit tetrahedron; its bounds have extent on all axes.
  return {
    kind,
    mesh: {
      positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
      indices: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
      volume: 1 / 6,
      area: (3 + Math.sqrt(3)) / 2,
      openEdges: 0,
      source: "native-test",
      units: "mm",
    },
  };
}

function artifact(bytes = new Uint8Array([1, 2, 3])): NativeEvent {
  return {
    kind: "artifact",
    artifact: { name: "design.k3d", mime: "text/plain", bytes },
  };
}

const completed: NativeEvent = { kind: "completed" };
const signal = () => new AbortController().signal;

function builder(events: NativeEvent[], error?: unknown): typeof buildCad {
  return async function* () {
    yield* events;
    if (error !== undefined) throw error;
  };
}

function nativeFailure(code: string, message: string) {
  return new TypeError(
    `Native text CAD mesh did not complete: ${
      JSON.stringify({
        kind: "failed",
        diagnostic: { code, message },
      })
    }`,
  );
}

Deno.test("CAD attempt publishes original events only after terminal exhaustion", async () => {
  const events = [
    { kind: "started" },
    mesh("construction_step"),
    mesh(),
    artifact(),
    completed,
  ];
  let exhausted = false;
  const inputSignal = signal();
  const build: typeof buildCad = async function* (inputRecipe, inputAbort) {
    assert(inputRecipe === recipe && inputAbort === inputSignal);
    yield* events;
    exhausted = true;
  };
  const result = await executeCadAttempt(recipe, inputSignal, build);
  assert(result.ok && exhausted);
  assert(result.events.every((event, i) => event === events[i]));
  equal(result.report, {
    units: "mm",
    bounds: { min: [0, 0, 0], max: [1, 1, 1] },
    volume: 1 / 6,
    area: (3 + Math.sqrt(3)) / 2,
    openEdges: 0,
    stageCount: 2,
    designIntent: "unknown",
    standards: "unknown",
    exactProof: false,
  });
});

Deno.test("CAD geometry failure after previews or completed exposes no buffered data", async () => {
  for (
    const events of [[mesh("construction_step")], [
      mesh(),
      artifact(),
      completed,
    ]]
  ) {
    const error = new Error(
      "Native mesh exited 1: krly3d-mesh: unsupported: edges of the two solids run along each other\n/private/secret\n",
    );
    // Structured diagnostics also cover failures during tessellation.
    for (
      const cause of [
        error,
        nativeFailure(
          "unsupported",
          "edges of the two solids run along each other",
        ),
      ]
    ) {
      const result = await executeCadAttempt(
        recipe,
        signal(),
        builder(events, cause),
      );
      assert(!result.ok && result.failure.code === "boolean_contact");
      equal(Object.keys(result).sort(), ["failure", "ok"]);
      assert(result.failure.message.length < 256);
      assert(!JSON.stringify(result).includes("secret"));
    }
  }
});

Deno.test("CAD unsupported native geometry and native closed-solid checks are safe failures", async () => {
  for (
    const [error, code] of [
      [
        nativeFailure("unsupported", "no section rule for `torus|sphere`"),
        "unsupported_geometry",
      ],
      [
        new TypeError("A text CAD feature did not produce a closed solid"),
        "verification",
      ],
      [
        nativeFailure("geometry", "the solids touch tangentially"),
        "boolean_contact",
      ],
    ] as const
  ) {
    const result = await executeCadAttempt(
      recipe,
      signal(),
      builder([], error),
    );
    assert(!result.ok && result.failure.code === code);
  }
});

Deno.test("CAD unknown, protocol, infrastructure, timeout and native capacity errors rethrow", async () => {
  const errors = [
    new Error("unknown /private/path"),
    new Error("unsupported: edges of the two solids run along each other"),
    new Error("Native mesh exited 137: krly3d-mesh: unsupported: bad geometry"),
    new TypeError("Unsupported mesh protocol/API"),
    nativeFailure("io", "edges of the two solids run along each other"),
    nativeFailure("deadline_exceeded", "expired"),
    nativeFailure("resource_limit", "capacity exhausted"),
    nativeFailure(
      "unsupported",
      "a mesh count exceeds the JSON-safe integer range",
    ),
    nativeFailure("unsupported", "no tool is registered as `subtract`"),
    nativeFailure("unsupported", "geometry memory limit exceeded"),
    nativeFailure("unsupported", "cannot read file /private/design.k3d"),
    nativeFailure("unsupported", "cannot open directory /private/workspace"),
    nativeFailure("unsupported", "profile tessellation timed out"),
    nativeFailure("geometry", "unexpected mathematics failure"),
    new DOMException("unsupported geometry", "TimeoutError"),
    new DOMException("unsupported geometry", "AbortError"),
    "unsupported",
  ];
  for (const error of errors) {
    await rejects(
      () =>
        executeCadAttempt(
          recipe,
          signal(),
          builder([mesh(), completed], error),
        ),
      error,
    );
  }
});

Deno.test("CAD profile geometry diagnostics do not match infrastructure keywords", async () => {
  for (
    const message of [
      "a loft section may hold only lines, circular arcs and ellipses: unsupported profile",
      "unsupported extrude_profile geometry",
      "cannot mesh this loft profile /private/secret.k3d",
    ]
  ) {
    for (
      const error of [
        nativeFailure("unsupported", message),
        new Error(
          `Native mesh exited 1: krly3d-mesh: unsupported: ${message}\n`,
        ),
      ]
    ) {
      const result = await executeCadAttempt(
        recipe,
        signal(),
        builder([], error),
      );
      assert(!result.ok && result.failure.code === "unsupported_geometry");
      assert(result.failure.message.length < 256);
      assert(!JSON.stringify(result).includes("secret"));
    }
  }
});

Deno.test("CAD cancellation before execution, during previews, and after completed always rethrows", async () => {
  for (const point of ["before", "preview", "terminal", "error"] as const) {
    const controller = new AbortController();
    const reason = new Error("request cancelled");
    let called = false;
    if (point === "before") controller.abort(reason);
    const build: typeof buildCad = async function* () {
      called = true;
      yield mesh("construction_step");
      if (point === "preview") controller.abort(reason);
      yield mesh();
      yield completed;
      controller.abort(reason);
      if (point === "error") {
        throw nativeFailure("unsupported", "unsupported geometry");
      }
    };
    await rejects(
      () => executeCadAttempt(recipe, controller.signal, build),
      reason,
    );
    assert(called === (point !== "before"));
  }
});

Deno.test("CAD rejects missing, duplicate, misplaced terminal events and premature artifacts", async () => {
  const invalid: NativeEvent[][] = [
    [],
    [mesh()],
    [completed],
    [mesh("construction_step"), completed],
    [mesh(), mesh(), completed],
    [mesh(), mesh("construction_step"), completed],
    [mesh(), completed, completed],
    [mesh(), completed, { kind: "progress" }],
    [artifact(), mesh(), completed],
    [mesh("construction_step"), artifact(), mesh(), completed],
    [{ kind: "editable_proposal" }, completed],
    [{ ...mesh(), artifact: artifact().artifact }, completed],
    [mesh(), { kind: "artifact" }, completed],
    [mesh(), { kind: "failed" }, completed],
  ];
  for (const events of invalid) {
    assert(
      await rejects(() =>
        executeCadAttempt(recipe, signal(), builder(events))
      ) instanceof TypeError,
    );
  }
});

Deno.test("CAD numerical mesh failures return verification only after a clean complete stream", async () => {
  const changes: Partial<NonNullable<NativeEvent["mesh"]>>[] = [
    { units: undefined },
    { volume: 0 },
    { volume: -1 },
    { volume: NaN },
    { area: 0 },
    { area: Infinity },
    { openEdges: 1 },
    { openEdges: NaN },
    { positions: [0, 0, 0] },
    { positions: new Array(12).fill(0) },
    { positions: [NaN, ...mesh().mesh!.positions.slice(1)] },
    { indices: [0, 1, 2] },
    { indices: new Array(12).fill(-1) },
    { indices: new Array(12).fill(4) },
    { indices: new Array(12).fill(0.5) },
  ];
  for (const change of changes) {
    const invalid = mesh();
    Object.assign(invalid.mesh!, change);
    const result = await executeCadAttempt(
      recipe,
      signal(),
      builder([invalid, completed]),
    );
    assert(!result.ok && result.failure.code === "verification");
  }
  const invalidPreview = mesh("construction_step");
  invalidPreview.mesh!.openEdges = 2;
  const result = await executeCadAttempt(
    recipe,
    signal(),
    builder([invalidPreview, mesh(), completed]),
  );
  assert(!result.ok && result.failure.code === "verification");
  const exitError = new Error("late native exit failure");
  await rejects(
    () =>
      executeCadAttempt(
        recipe,
        signal(),
        builder([invalidPreview, mesh(), completed], exitError),
      ),
    exitError,
  );
});

Deno.test("CAD accepts eight mesh stages and rejects nine, closing the producer", async () => {
  const previews = Array.from({ length: 7 }, () => mesh("construction_step"));
  const result = await executeCadAttempt(
    recipe,
    signal(),
    builder([...previews, mesh(), completed]),
  );
  assert(result.ok && result.report.stageCount === 8);
  let closed = false;
  const build: typeof buildCad = async function* () {
    try {
      yield* [...previews, mesh("construction_step"), mesh(), completed];
    } finally {
      closed = true;
    }
  };
  assert(
    await rejects(() => executeCadAttempt(recipe, signal(), build)) instanceof
      RangeError,
  );
  assert(closed);
});

Deno.test("CAD accepts exactly 32 events and rejects the next event", async () => {
  const progress = Array.from({ length: 30 }, () => ({ kind: "progress" }));
  assert(
    (await executeCadAttempt(
      recipe,
      signal(),
      builder([...progress, mesh(), completed]),
    )).ok,
  );
  assert(
    await rejects(() =>
      executeCadAttempt(
        recipe,
        signal(),
        builder([
          ...progress,
          { kind: "progress" },
          mesh(),
          completed,
        ]),
      )
    ) instanceof RangeError,
  );
});

Deno.test("CAD total memory budget counts mesh numbers, metadata and cumulative artifact bytes", async () => {
  const final = mesh();
  final.details = { step: 1 };
  const meshBytes = (12 + 12 + 3 + 1) * 8;
  const bytes = new Uint8Array(
    CAD_EXECUTION_LIMITS.approximateBytes - meshBytes,
  );
  const output = artifact(bytes);
  const result = await executeCadAttempt(
    recipe,
    signal(),
    builder([final, output, completed]),
  );
  assert(result.ok && result.events[1].artifact!.bytes === bytes);
  assert(
    await rejects(() =>
      executeCadAttempt(
        recipe,
        signal(),
        builder([
          final,
          output,
          artifact(new Uint8Array(1)),
          completed,
        ]),
      )
    ) instanceof RangeError,
  );
  // Reused payloads still count at every stage; there is no cloning/dedup bypass.
  const chunk = artifact(bytes.subarray(0, 17 * 1024 * 1024));
  assert(
    await rejects(() =>
      executeCadAttempt(
        recipe,
        signal(),
        builder([
          final,
          chunk,
          chunk,
          chunk,
          chunk,
          completed,
        ]),
      )
    ) instanceof RangeError,
  );
  const huge = mesh();
  huge.mesh!.positions = new Array(CAD_EXECUTION_LIMITS.approximateBytes / 8);
  assert(
    await rejects(() =>
      executeCadAttempt(recipe, signal(), builder([huge, completed]))
    ) instanceof RangeError,
  );
});
