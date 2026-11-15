import { CAD_LIMITS, type CadCircleSketch, type CadCircularHolePattern, type CadStep, type SketchSegment, circleSketchGeometry, circularHoleCenters, encodeCadRecipe, validateCadRecipe } from "./cad-recipe.ts";
import { assert, equal, rejects } from "./test_support.ts";

const washer = {
  units: "mm",
  steps: [
    {
      type: "cylinder",
      id: "outer",
      title: "Külső palást",
      size: null,
      center: [0, 0, 0],
      radius: 8,
      height: 1.6,
      inputs: null,
    },
    {
      type: "cylinder",
      id: "bore",
      title: "Furat vágóhengere",
      size: null,
      center: [0, 0, -1],
      radius: 4.2,
      height: 3.6,
      inputs: null,
    },
    {
      type: "subtract",
      id: "washer",
      title: "Átmenő furat kivágása",
      size: null,
      center: null,
      radius: null,
      height: null,
      inputs: ["outer", "bore"],
    },
  ],
};

const plate: CadStep = {
  type: "box", id: "plate", title: "Plate", size: [80, 50, 8],
  center: [0, 0, 0], radius: null, height: null, inputs: null,
};
const hole: CadStep = {
  type: "through_hole", id: "hole", title: "Through hole", size: null,
  center: [2, -3, 0], radius: 3, height: null, inputs: ["plate"],
};

const profile: CadStep = {
  type: "extrude_profile", id: "outline", title: "Planar part",
  outer: [[-20, 0], [20, 0], [20, 200], [8, 200], [8, 176], [-8, 176], [-8, 200], [-20, 200]],
  height: 6, z: -12.5,
};

const sketch: Extract<CadStep, { type: "sketch" }> = {
  type: "sketch", id: "outline", title: "Curved outline", start: [-5, 0], z: -2,
  segments: [
    { type: "line", to: [5, 0] },
    { type: "arc", center: [5, 5], to: [5, 10], clockwise: false },
    { type: "line", to: [-5, 10] },
    { type: "arc", center: [-5, 5], to: [-5, 0], clockwise: false },
  ],
};
const extrusion: Extract<CadStep, { type: "extrude" }> = {
  type: "extrude", id: "body", title: "Extruded body", inputs: ["outline"], height: 8,
};
const pocketSketch: Extract<CadStep, { type: "sketch" }> = {
  type: "sketch", id: "recess", title: "Pocket sketch", start: [2, 4], z: 6,
  segments: [
    { type: "arc", center: [0, 4], to: [-2, 4], clockwise: false },
    { type: "arc", center: [0, 4], to: [2, 4], clockwise: false },
  ],
};
const pocket: Extract<CadStep, { type: "pocket" }> = {
  type: "pocket", id: "pocket", title: "Pocket", inputs: ["body", "recess"], depth: 3,
};
const fillet: Extract<CadStep, { type: "fillet" }> = {
  type: "fillet", id: "rounded", title: "Selected edge", inputs: ["pocket"], edges: ["top|s1"], radius: 0.5,
};
const encoded = (steps: CadStep[]) => new TextDecoder().decode(encodeCadRecipe(validateCadRecipe({ units: "mm", steps })));

const circle: CadCircleSketch = {
  type: "sketch_circle", id: "outline", title: "Circular outline", center: [3, -4], radius: 30, z: -2,
};
const pattern: CadCircularHolePattern = {
  type: "circular_hole_pattern", id: "holes", title: "Circular holes", inputs: ["body"],
  center: [3, -4], patternRadius: 20, radius: 2, count: 4, startAngle: 0,
};

Deno.test("CAD recipe: native full circles retain one rim and work as extrusion and pocket sketches", () => {
  const recess: CadCircleSketch = { ...circle, id: "recess", radius: 3, z: 6 };
  for (const depth of [3, null]) {
    const text = encoded([circle, extrusion, recess, { ...pocket, depth }, {
      ...fillet, edges: ["top|side"],
    }]);
    assert(text.startsWith("k3d 1\nlayer geo\noutline = circle plane=xy at=3,-4,-2 r=30\nbody = extrude outline h=8\n"));
    assert(text.includes("recess = circle plane=xy at=3,-4,6 r=3\nkrly_sketch_pocket = circle plane=xy at=3,-4,6.1 r=3\n"));
    assert(text.includes(`krly_cut_pocket = extrude krly_sketch_pocket h=${depth === null ? "-8.2" : "-3.1"}\n`));
    assert(text.endsWith("rounded = fillet pocket edges=top|side, r=0.5\n"));
  }
  equal(new TextDecoder().decode(encodeCadRecipe({ units: "mm", steps: [circle] })),
    "k3d 1\nlayer geo\noutline = circle plane=xy at=3,-4,-2 r=30\n");
  const guide = circleSketchGeometry(circle);
  equal(guide, { start: [33, -4], segments: [
    { type: "arc", center: [3, -4], to: [-27, -4], clockwise: false },
    { type: "arc", center: [3, -4], to: [33, -4], clockwise: false },
  ] });
  validateCadRecipe({ units: "mm", steps: [{ ...sketch, ...guide }, extrusion] });
  guide.start[0] = 0;
  (guide.segments[0] as Extract<SketchSegment, { type: "arc" }>).center[0] = 0;
  equal(circle.center, [3, -4]);
});

Deno.test("CAD recipe: circle validates its entire extent and closed field set", async () => {
  encoded([{ ...circle, center: [9990, -9990], radius: 10, z: 10_000 }, extrusion]);
  for (const patch of [
    { center: [9991, 0], radius: 10 }, { center: [0, -9991], radius: 10 },
    { center: [0, 0, 0] }, { center: null }, { center: [NaN, 0] },
    ...[0, -1, 1e-10, NaN, Infinity, -Infinity, 10_001, null, "3"].map((radius) => ({ radius })),
    ...[NaN, Infinity, -Infinity, 10_001, -10_001, null, "3"].map((z) => ({ z })),
    { z: undefined }, { radius: undefined }, { inputs: null }, { size: null },
    { start: [1, 0] }, { segments: [] }, { id: "krly_cut_circle" },
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [
      { ...circle, ...patch }, extrusion,
    ] })), "invalid_response");
  }
});

Deno.test("CAD recipe: pattern coordinates use degrees, CCW ordering and finite angle reduction", () => {
  equal(circularHoleCenters(pattern), [[23, -4], [3, 16], [-17, -4], [3, -24]]);
  equal(circularHoleCenters({ ...pattern, startAngle: -90 }), [[3, -24], [23, -4], [3, 16], [-17, -4]]);
  equal(circularHoleCenters({ ...pattern, startAngle: 720 }), circularHoleCenters(pattern));
  for (const startAngle of [37, -123.25, Number.MAX_VALUE]) {
    const centers = circularHoleCenters({ ...pattern, count: 7, startAngle });
    for (const [index, center] of centers.entries()) {
      assert(center.every(Number.isFinite));
      assert(Math.abs(Math.hypot(center[0] - 3, center[1] + 4) - 20) < 1e-12);
      const next = centers[(index + 1) % centers.length];
      assert(Math.abs(Math.hypot(center[0] - next[0], center[1] - next[1]) - 40 * Math.sin(Math.PI / 7)) < 1e-12);
    }
  }
});

Deno.test("CAD recipe: one pattern feature expands to deterministic bounded through-all subtractions", () => {
  const steps = [circle, extrusion, pattern];
  const text = encoded(steps);
  equal(text, "k3d 1\nlayer geo\n" +
    "outline = circle plane=xy at=3,-4,-2 r=30\nbody = extrude outline h=8\n" +
    "krly_cut_2_holes_0 = cylinder at=23,-4,-2.1 h=8.2 r=2\nkrly_cut_2_holes_0_result = subtract body krly_cut_2_holes_0\n" +
    "krly_cut_2_holes_1 = cylinder at=3,16,-2.1 h=8.2 r=2\nkrly_cut_2_holes_1_result = subtract krly_cut_2_holes_0_result krly_cut_2_holes_1\n" +
    "krly_cut_2_holes_2 = cylinder at=-17,-4,-2.1 h=8.2 r=2\nkrly_cut_2_holes_2_result = subtract krly_cut_2_holes_1_result krly_cut_2_holes_2\n" +
    "krly_cut_2_holes_3 = cylinder at=3,-24,-2.1 h=8.2 r=2\nholes = subtract krly_cut_2_holes_2_result krly_cut_2_holes_3\n");
  const valid = validateCadRecipe({ units: "mm", steps });
  equal(valid.steps.length, 3);
  equal(encodeCadRecipe(validateCadRecipe(JSON.parse(JSON.stringify(valid)))), encodeCadRecipe(valid));
  const source = structuredClone({ units: "mm", steps });
  const owned = validateCadRecipe(source);
  (source.steps[0] as CadCircleSketch).center[0] = 99;
  (source.steps[2] as CadCircularHolePattern).inputs[0] = "changed";
  equal(owned, valid);
  // Single-hole patterns keep the same parameter semantics and final public id.
  assert(encoded([circle, extrusion, { ...pattern, count: 1 }]).endsWith("holes = subtract body krly_cut_2_holes_0\n"));
  // A downstream through-hole inherits the original body's Z bounds.
  assert(encoded([...steps, { ...hole, inputs: ["holes"] }]).includes("krly_cut_hole = cylinder at=2,-3,-2.1 h=8.2 r=3\n"));
  assert(encoded([{ ...circle, z: 10_000 }, { ...extrusion, height: 10_000 }, pattern])
    .includes("krly_cut_2_holes_0 = cylinder at=23,-4,9900 h=10200 r=2\n"));
});

Deno.test("CAD recipe: pattern internal ids cannot shadow legacy cutters or other pattern expansions", () => {
  const text = encoded([
    circle, extrusion,
    { ...hole, id: "holes_0", inputs: ["body"] },
    { ...pattern, inputs: ["holes_0"] },
    { ...pattern, id: "holes_0_result", inputs: ["holes"], startAngle: 45 },
    { ...hole, id: "holes_0_result_0", inputs: ["holes_0_result"] },
  ]);
  const declared = new Set<string>();
  for (const line of text.split("\n").filter((line) => line.includes(" = "))) {
    const [id, operation] = line.split(" = ");
    assert(!declared.has(id), `Duplicate generated node: ${id}`);
    if (operation.startsWith("subtract ") || operation.startsWith("extrude ")) {
      const inputs = operation.split(" ").slice(1, operation.startsWith("subtract ") ? 3 : 2);
      assert(inputs.every((input) => declared.has(input)), `Non-backward generated reference: ${line}`);
    }
    declared.add(id);
  }
  assert(text.includes("krly_cut_holes_0 = cylinder"));
  assert(text.includes("krly_cut_3_holes_0 = cylinder"));
  assert(text.includes("krly_cut_4_holes_0_result_0 = cylinder"));
});

Deno.test("CAD recipe: pattern rejects invalid numbers, touching/overlapping holes and full hole extent overflow", async () => {
  for (const patch of [
    ...[0, -1, 1e-10, NaN, Infinity, -Infinity, 10_001, null, "1"].flatMap((n) => [{ radius: n }, { patternRadius: n }]),
    ...[0, -1, 1.5, 17, NaN, Infinity, null, "4"].map((count) => ({ count })),
    ...[NaN, Infinity, -Infinity, null, "90"].map((startAngle) => ({ startAngle })),
    { center: [0, 0, 0] }, { center: [Infinity, 0] }, { center: [0, -10_001] },
    { center: [9979, 0] }, { center: [0, -9979] }, // centers fit but cutter rims do not
    { patternRadius: 2, radius: 2, count: 2 }, // tangent
    { patternRadius: 2, radius: 2.1, count: 2 }, // overlapping
    { patternRadius: 2, radius: 2 - 1e-10, count: 2 }, // ambiguous contact
    { patternRadius: 0, count: 4 }, // coincident centers
    { inputs: [] }, { inputs: ["body", "body"] }, { inputs: null },
    { inputs: ["outline"] }, { inputs: ["holes"] }, { inputs: ["later"] },
    { inputs: [42] }, { size: null }, { height: null }, { depth: null },
    { z: 0 }, { centers: [] }, { count: undefined }, { startAngle: undefined },
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [
      circle, extrusion, { ...pattern, ...patch },
    ] })), "invalid_response");
  }
  // Exact bounds and positive separation pass, without unsupported host-fit checks.
  encoded([circle, extrusion, { ...pattern, center: [9978, 0] }]);
  encoded([circle, extrusion, { ...pattern, patternRadius: 2, radius: 2 - 1e-8, count: 2 }]);
  encoded([circle, extrusion, { ...pattern, count: 16 }]);
});

Deno.test("CAD recipe: circle/pattern preserve typed ancestry and independent node, feature and total-hole limits", async () => {
  const steps: CadStep[] = [circle, extrusion];
  for (let i = 0; i < 7; i++) {
    const previous = steps.at(-1)!.id;
    steps.push({ ...circle, id: `s${i}`, radius: 1, z: 6 });
    steps.push({ ...pocket, id: `p${i}`, inputs: [previous, `s${i}`] });
  }
  equal(steps.length, 16);
  encoded(steps);
  const twoPatterns: CadStep[] = [circle, extrusion, { ...pattern, count: 8 },
    { ...pattern, id: "more", inputs: ["holes"], count: 8 }];
  encoded(twoPatterns);
  const eightSolids: CadStep[] = [circle, extrusion, pattern];
  for (let i = 0; i < 6; i++) eightSolids.push({ ...hole, id: `h${i}`, inputs: [eightSolids.at(-1)!.id] });
  encoded(eightSolids);
  for (const invalidSteps of [
    [circle], [circle, extrusion, { ...circle, id: "unused" }],
    [circle, extrusion, { ...circle, id: "unused" }, pattern],
    [circle, { ...extrusion, inputs: ["missing"] }], [extrusion, circle],
    [circle, { ...pattern, inputs: ["outline"] }],
    [circle, extrusion, { ...extrusion, id: "other", inputs: ["body"] }],
    [...steps, { ...pattern, inputs: ["p6"] }],
    [...eightSolids, { ...hole, id: "ninth", inputs: ["h5"] }],
    [...twoPatterns.slice(0, -1), { ...pattern, id: "more", inputs: ["holes"], count: 9 }],
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: invalidSteps })), "invalid_response");
  }
});

Deno.test("CAD recipe: real curves encode sketch/extrude/pocket/fillet without sampled geometry", () => {
  const source = { units: "mm", steps: [sketch, extrusion, pocketSketch, pocket, fillet] };
  const recipe = validateCadRecipe(source);
  const text = new TextDecoder().decode(encodeCadRecipe(recipe));
  equal(text, 'k3d 1\nlayer geo\n' +
    'outline = profile plane=xy at=0,0,-2 path="M -5,0 L 5,0 A 5,5 5,10 ccw L -5,10 A -5,5 -5,0 ccw Z"\n' +
    'body = extrude outline h=8\n' +
    'recess = profile plane=xy at=0,0,6 path="M 2,4 A 0,4 -2,4 ccw A 0,4 2,4 ccw Z"\n' +
    'krly_sketch_pocket = profile plane=xy at=0,0,6.1 path="M 2,4 A 0,4 -2,4 ccw A 0,4 2,4 ccw Z"\n' +
    'krly_cut_pocket = extrude krly_sketch_pocket h=-3.1\n' +
    'pocket = subtract body krly_cut_pocket\n' +
    'rounded = fillet pocket edges=top|s1, r=0.5\n');
  equal(encodeCadRecipe(validateCadRecipe(JSON.parse(JSON.stringify(recipe)))), encodeCadRecipe(recipe));
  const copy = structuredClone(source);
  const owned = validateCadRecipe(copy);
  (copy.steps[0] as typeof sketch).segments[0].to[0] = 99;
  (copy.steps[4] as typeof fillet).edges[0] = "changed|edge";
  equal(owned, recipe);
  // Incomplete sketch-only prefixes remain encodable for native callers.
  assert(new TextDecoder().decode(encodeCadRecipe({ units: "mm", steps: [sketch] })).includes('A 5,5 5,10 ccw'));
});

Deno.test("CAD recipe: sketch supports tangent joins, either winding, major arcs, two-arc circles and implicit closure", () => {
  const boundaries: { start: [number, number]; segments: SketchSegment[] }[] = [
    sketch,
    { start: [5, 0], segments: [
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false },
      { type: "arc", center: [0, 0], to: [5, 0], clockwise: false },
    ] },
    { start: [5, 0], segments: [{ type: "arc", center: [0, 0], to: [-5, 0], clockwise: false }] },
    { start: [5, 0], segments: [{ type: "arc", center: [0, 0], to: [0, -5], clockwise: false }] },
    { start: [3, -4], segments: [
      { type: "arc", center: [0, 0], to: [3, 4], clockwise: false },
      { type: "arc", center: [6, 0], to: [3, -4], clockwise: false },
    ] },
    { start: [0, 0], segments: [{ type: "line", to: [10, 0] }, { type: "line", to: [0, 10] }] },
  ];
  for (const boundary of boundaries) {
    // Close before reversing; then the same exact curves run in reverse.
    const segments: SketchSegment[] = [...boundary.segments];
    if (JSON.stringify(segments.at(-1)!.to) !== JSON.stringify(boundary.start)) {
      segments.push({ type: "line", to: boundary.start });
    }
    const reversed = segments.map((s, i): SketchSegment => ({
      ...s, to: i === 0 ? boundary.start : segments[i - 1].to,
      ...(s.type === "arc" ? { clockwise: !s.clockwise } : {}),
    })).reverse();
    for (const variant of [boundary, { start: boundary.start, segments: reversed }]) {
      const text = encoded([{ ...sketch, ...variant }, extrusion]);
      assert(text.endsWith("body = extrude outline h=8\n"));
    }
  }
  // Translating/rotating an exact tangent construction must not turn roundoff
  // in the line/circle discriminant into a spurious extra intersection.
  for (const degrees of [15, 37, 89, 135, 179]) {
    const angle = degrees * Math.PI / 180;
    const rotate = ([x, y]: [number, number]): [number, number] => [
      1000 + x * Math.cos(angle) - y * Math.sin(angle),
      -2000 + x * Math.sin(angle) + y * Math.cos(angle),
    ];
    encoded([{ ...sketch, start: rotate(sketch.start), segments: sketch.segments.map((s): SketchSegment =>
      s.type === "line" ? { ...s, to: rotate(s.to) } : { ...s, to: rotate(s.to), center: rotate(s.center) }
    ) }, extrusion]);
  }
});

Deno.test("CAD recipe: sketch rejects malformed curves, unequal radii and unbounded arc extrema", async () => {
  for (const patch of [
    { start: null }, { start: [0, 0, 0] }, { start: [Infinity, 0] }, { start: [10_001, 0] },
    { segments: [] }, { segments: null }, { z: Infinity }, { z: 10_001 },
    { holes: [] }, { inputs: null }, { outer: [[0, 0]] }, { plane: "xz" },
    { id: "krly_sketch_private" }, { id: "krly_cut_private" },
    ...[
      null, { type: "line", to: [5, 0], center: [0, 0] },
      { type: "line", to: [5, NaN] }, { type: "line", to: [5, 0, 0] },
      { type: "arc", center: [0, 0], to: [5, 0] },
      { type: "arc", center: [0, 0], to: [5, 0], clockwise: "false" },
      { type: "arc", center: [0, 0], to: [6, 0], clockwise: false },
      { type: "arc", center: [-5, 0], to: [5, 0], clockwise: false },
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false }, // full-circle ambiguity
      { type: "arc", center: [NaN, 0], to: [5, 0], clockwise: false },
      { type: "arc", center: [0, 0], to: [5, 0], clockwise: false, radius: 5 },
    ].map((segment) => ({ segments: [segment] })),
    // Endpoints/centre fit but the clockwise semicircle bulges beyond X=10000.
    { start: [9999, -2], segments: [{ type: "arc", center: [9999, 0], to: [9999, 2], clockwise: false }] },
    { start: [10_000, 10_000], segments: [{ type: "arc", center: [0, 0], to: [-10_000, 10_000], clockwise: false }] },
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({
      units: "mm", steps: [{ ...sketch, ...patch }, extrusion],
    })), "invalid_response");
  }
});

Deno.test("CAD recipe: analytic sketch admission rejects line/arc crossings, tangencies and overlaps", async () => {
  const cases: { start: [number, number]; segments: SketchSegment[] }[] = [
    { start: [0, 0], segments: [{ type: "line", to: [10, 10] }, { type: "line", to: [0, 10] }, { type: "line", to: [10, 0] }] },
    { start: [0, 0], segments: [{ type: "line", to: [10, 0] }, { type: "line", to: [5, 0] }, { type: "line", to: [5, 5] }] },
    { start: [0, 0], segments: [{ type: "line", to: [5, 0] }] },
    { start: [0, 0], segments: [{ type: "line", to: [0, 0] }, { type: "line", to: [5, 5] }] },
    // A line crosses the interior of the upper semicircle.
    { start: [5, 0], segments: [
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false },
      { type: "line", to: [0, 6] }, { type: "line", to: [0, -2] },
    ] },
    // A remote horizontal edge touches the semicircle at its apex.
    { start: [5, 0], segments: [
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false },
      { type: "line", to: [-6, 5] }, { type: "line", to: [6, 5] },
    ] },
    // Same-circle arcs double back over a positive angular interval.
    { start: [5, 0], segments: [
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false },
      { type: "arc", center: [0, 0], to: [0, 5], clockwise: true },
    ] },
    // Two upper semicircles cross at x=1.5, y=sqrt(25-2.25).
    { start: [5, 0], segments: [
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false },
      { type: "line", to: [-5, -2] }, { type: "line", to: [8, -2] },
      { type: "line", to: [8, 0] },
      { type: "arc", center: [3, 0], to: [-2, 0], clockwise: false },
    ] },
    // Different circles touch at an interior point of both arcs, (0,5).
    { start: [5, 0], segments: [
      { type: "arc", center: [0, 0], to: [-5, 0], clockwise: false },
      { type: "line", to: [-6, 0] }, { type: "line", to: [-6, 10] },
      { type: "line", to: [-5, 10] },
      { type: "arc", center: [0, 10], to: [5, 10], clockwise: false },
    ] },
  ];
  for (const boundary of cases) {
    await rejects(() => Promise.resolve(validateCadRecipe({
      units: "mm", steps: [{ ...sketch, ...boundary }, extrusion],
    })), "invalid_response");
  }
});

Deno.test("CAD recipe: sketch/solid references are typed, earlier, connected and end in a solid", async () => {
  const boolean: CadStep = {
    type: "union", id: "union", title: "Union", size: null, center: null,
    radius: null, height: null, inputs: ["outline", "body"],
  };
  for (const steps of [
    [sketch], [sketch, extrusion, pocketSketch],
    [sketch, extrusion, { ...extrusion, id: "other", inputs: ["body"] }],
    [sketch, extrusion, { ...extrusion, id: "other", inputs: ["missing"] }],
    [sketch, { ...extrusion, inputs: ["outline", "outline"] }],
    [extrusion, sketch], [sketch, { ...extrusion, inputs: ["body"] }],
    [sketch, extrusion, { ...hole, inputs: ["outline"] }],
    [sketch, extrusion, boolean],
    [sketch, extrusion, { ...boolean, type: "subtract" }],
    [sketch, extrusion, pocketSketch, { ...pocket, inputs: ["recess", "body"] }],
    [sketch, extrusion, pocketSketch, { ...pocket, inputs: ["body", "body"] }],
    [sketch, extrusion, pocketSketch, { ...pocket, inputs: ["body"] }],
    [sketch, extrusion, { ...fillet, inputs: ["outline"] }],
    [sketch, extrusion, { ...fillet, inputs: ["body", "body"] }],
    [sketch, extrusion, { ...pocketSketch, id: "body" }, pocket],
    [sketch, { ...extrusion, center: null }],
    [sketch, { ...extrusion, height: 0 }],
    [sketch, { ...extrusion, height: Infinity }],
    [sketch, { ...extrusion, height: 10_001 }],
    [sketch, { ...extrusion, inputs: null }],
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps })), "invalid_response");
  }
});

Deno.test("CAD recipe: node, solid-feature and total curve-segment budgets are independent", async () => {
  const steps: CadStep[] = [sketch, extrusion];
  for (let i = 0; i < 7; i++) {
    const solid = steps.at(-1)!.id;
    steps.push({ ...pocketSketch, id: `s${i}` });
    steps.push({ ...pocket, id: `p${i}`, inputs: [solid, `s${i}`] });
  }
  equal(steps.length, CAD_LIMITS.steps);
  equal(steps.filter((s) => s.type !== "sketch").length, CAD_LIMITS.solidFeatures);
  validateCadRecipe({ units: "mm", steps });
  // 16 connected nodes and 8 solid features fit; a ninth feature does not.
  await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [
    ...steps, { ...fillet, inputs: [steps.at(-1)!.id] },
  ] })), "invalid_response");
  const solidOnly: CadStep[] = [plate];
  for (let i = 0; i < 8; i++) solidOnly.push({ ...hole, id: `h${i}`, inputs: [solidOnly.at(-1)!.id] });
  equal(solidOnly.length, 9);
  await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: solidOnly })), "invalid_response");
  const vertices: [number, number][] = Array.from({ length: 64 }, (_, i) =>
    [20 * Math.cos(i * 2 * Math.PI / 64), 20 * Math.sin(i * 2 * Math.PI / 64)]);
  const segments: SketchSegment[] = vertices.slice(1).map((to) => ({ type: "line", to }));
  encoded([{ ...sketch, start: vertices[0], segments }, extrusion]);
  encoded([{ ...sketch, start: vertices[0], segments: [...segments, { type: "line", to: vertices[0] }] }, extrusion]);
  // One extra vertex along a line creates 65 boundary segments including Z.
  segments.splice(0, 0, { type: "line", to: [
    (vertices[0][0] + vertices[1][0]) / 2, (vertices[0][1] + vertices[1][1]) / 2,
  ] });
  await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [
    { ...sketch, start: vertices[0], segments }, extrusion,
  ] })), "invalid_response");
});

Deno.test("CAD recipe: pockets preserve top references, exact blind floors and deterministic through-all margins", async () => {
  for (const z of [-20, 0, 10]) {
    for (const depth of [null, 3, 8]) {
      const steps: CadStep[] = [{ ...sketch, z }, extrusion, { ...pocketSketch, z: z + 8 }, { ...pocket, depth }];
      const text = encoded(steps);
      const lifted = text.match(/krly_sketch_pocket = profile plane=xy at=0,0,([^ ]+) /);
      const cutter = text.match(/krly_cut_pocket = extrude krly_sketch_pocket h=([^\n]+)/);
      assert(lifted && cutter);
      const top = Number(lifted[1]), height = Number(cutter[1]);
      assert(Math.abs(top - (z + 8.1)) < 1e-12);
      assert(Math.abs(top + height - (depth === null ? z - 0.1 : z + 8 - depth)) < 1e-12);
      const through = encoded([...steps, { ...fillet }, { ...hole, inputs: [fillet.id] }]);
      const bore = through.match(/krly_cut_hole = cylinder at=2,-3,([^ ]+) h=([^ ]+) r=3/);
      assert(bore);
      assert(Math.abs(Number(bore[1]) - (z - 0.1)) < 1e-12);
      assert(Math.abs(Number(bore[2]) - 8.2) < 1e-12);
    }
  }
  // Accept numerical coincidence, but encode the target top rather than drift.
  assert(encoded([sketch, extrusion, { ...pocketSketch, z: 6 + 5e-10 }, pocket]).includes("at=0,0,6.1 path="));
  for (const bad of [
    { ...pocketSketch, z: 6 + 1e-7 }, { ...pocketSketch, z: -2 },
  ]) await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [sketch, extrusion, bad, pocket] })), "invalid_response");
  for (const patch of [
    { depth: 0 }, { depth: -1 }, { depth: 8.01 }, { depth: 10_001 },
    { depth: Infinity }, { depth: NaN }, { depth: "3" }, { depth: undefined }, { radius: null },
  ]) await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [sketch, extrusion, pocketSketch, { ...pocket, ...patch }] })), "invalid_response");
});

Deno.test("CAD recipe: fillet selectors are bounded safe face pairs, never a claim of native capability", async () => {
  const target = { ...fillet, inputs: ["plate"] as [string] };
  assert(encoded([plate, { ...target, edges: ["front|top", "back|top"] }]).includes("edges=front|top,back|top, r=0.5"));
  assert(encoded([plate, { ...target, edges: Array.from({ length: 8 }, (_, i) => `top|s${i + 1}`) }]).includes("top|s8,"));
  for (const patch of [
    { edges: [] }, { edges: null }, { edges: [null] },
    { edges: Array.from({ length: 9 }, (_, i) => `top|s${i + 1}`) },
    ...["top", "top|top", "top|s1|s2", "top|s1,", "top|s1\n", "top|s1 h=9", "top|s1#2", 'top|s1"', "a".repeat(33) + "|top"].map((edge) => ({ edges: [edge] })),
    { edges: ["top|s1", "s1|top"] }, { edges: ["top|s1", "top|s1"] },
    { radius: 0 }, { radius: -1 }, { radius: Infinity }, { radius: 10_001 }, { radius: null },
    { center: null },
  ]) await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [plate, { ...target, ...patch }] })), "invalid_response");
});

Deno.test("CAD recipe: profile is a distinct closed contract with an implicit loop and owned vertices", () => {
  for (const outer of [profile.outer, profile.outer.toReversed()]) {
    const source = { units: "mm", steps: [{ ...profile, outer: structuredClone(outer) }] };
    const recipe = validateCadRecipe(source);
    const text = new TextDecoder().decode(encodeCadRecipe(recipe));
    equal(text, `k3d 1\nlayer geo\nkrly_sketch_outline = profile plane=xy at=0,0,-12.5 path="${outer.map((p, i) => `${i === 0 ? "M" : "L"} ${p.join(",")}`).join(" ")} Z"\noutline = extrude krly_sketch_outline h=6\n`);
    source.steps[0].outer[0][0] = 999;
    const checked = recipe.steps[0];
    assert(checked.type === "extrude_profile");
    equal(checked.outer, outer);
    equal(Object.keys(checked).sort(), ["height", "id", "outer", "title", "type", "z"]);
  }
});

Deno.test("CAD recipe: profile bounds admit 64 vertices and limit numeric and structural inputs", async () => {
  const circle = (count: number) => Array.from({ length: count }, (_, i) =>
    [100 * Math.cos(i * 2 * Math.PI / count), 100 * Math.sin(i * 2 * Math.PI / count)]);
  validateCadRecipe({ units: "mm", steps: [{ ...profile, outer: circle(64) }] });
  validateCadRecipe({ units: "mm", steps: [{
    ...profile, outer: [[-10_000, -10_000], [10_000, -10_000], [0, 10_000]],
    height: 10_000, z: 10_000,
  }] });
  for (const patch of [
    { outer: circle(65) }, { outer: null }, { outer: [] }, { outer: [[0, 0], [1, 1]] },
    { outer: [[0, 0, 0], [10, 0], [0, 10]] },
    ...[NaN, Infinity, -Infinity, 10_001, -10_001, "1", null].flatMap((n) => [
      { outer: [[n, 0], [10, 0], [0, 10]] },
      { outer: [[0, n], [10, 0], [0, 10]] }, { z: n },
    ]),
    ...[0, -1, NaN, Infinity, 10_001, "6", null].map((height) => ({ height })),
    { z: undefined }, { height: undefined }, { outer: undefined },
    { holes: [] }, { inputs: null }, { size: null }, { center: null }, { radius: null },
    { path: 'M 0,0 Z' }, { plane: "xy" }, { id: "krly_sketch_outline" },
    { id: "krly_cut_outline" }, { title: "bad\nline" },
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({
      units: "mm", steps: [{ ...profile, ...patch }],
    })), "invalid_response");
  }
  // The new shape does not loosen the existing primitive field set.
  for (const step of [
    { ...plate, outer: profile.outer }, { ...plate, z: 0 },
    { ...plate, id: "krly_sketch_outline" },
    { type: "box", id: "box", title: "Box", size: [1, 1, 1], center: [0, 0, 0] },
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps: [step] })), "invalid_response");
  }
});

Deno.test("CAD recipe: profile rejects crossing, touching, repeated and degenerate boundaries in either winding", async () => {
  const invalidPolygons = [
    [[0, 0], [10, 10], [0, 10], [10, 0]], // bow tie, zero signed area
    [[0, 0], [10, 10], [0, 8], [10, 0]], // crossing with nonzero signed area
    [[0, 0], [10, 0], [10, 10], [5, 0], [0, 10]], // vertex touches another edge
    [[0, 0], [10, 0], [10, 10], [0, 0]], // explicit closure
    [[0, 0], [10, 0], [10, 0], [0, 10]], // zero-length edge
    [[0, 0], [10, 0], [10, 10], [0, 10], [10, 0]], // pinch point
    [[0, 0], [5, 0], [10, 0]], // zero area
    [[0, 0], [5, 0], [10, 0], [0, 10]], // redundant collinear corner
    [[0, 0], [10, 0], [5, 0], [5, 10], [0, 10]], // adjacent overlap
    [[0, 0], [10, 0], [10, 10], [2, 10], [2, 0], [8, 0], [8, 8], [0, 8]], // nonadjacent overlap
    [[0, 0], [1e-10, 0], [0, 10]], // numerically collapsed edge
    [[0, 0], [10, 0], [10, 10], [5, 1e-10], [0, 10]], // ambiguous near contact
  ];
  for (const points of invalidPolygons) {
    for (const outer of [points, points.toReversed()]) {
      await rejects(() => Promise.resolve(validateCadRecipe({
        units: "mm", steps: [{ ...profile, outer }],
      })), "invalid_response");
    }
  }
});

Deno.test("CAD recipe: profiles participate in connected ancestry and retain the encoded byte budget", async () => {
  const combined: CadStep = {
    type: "union", id: "joined", title: "Joined", size: null, center: null,
    radius: null, height: null, inputs: [profile.id, plate.id],
  };
  validateCadRecipe({ units: "mm", steps: [profile, plate, combined] });
  for (const steps of [[profile, plate], [profile, { ...profile }], [combined, profile, plate]]) {
    await rejects(() => Promise.resolve(validateCadRecipe({ units: "mm", steps })), "invalid_response");
  }
  const outer: [number, number][] = Array.from({ length: 64 }, (_, i) =>
    [100 * Math.cos(i * 2 * Math.PI / 64), 100 * Math.sin(i * 2 * Math.PI / 64)]);
  const steps: CadStep[] = [{ ...profile, outer, id: "p0" }];
  for (let i = 1; i <= 3; i++) {
    steps.push({ ...profile, outer, id: `p${i}` });
    steps.push({ ...combined, id: `u${i}`, inputs: [i === 1 ? "p0" : `u${i - 1}`, `p${i}`] });
  }
  // Four full-precision 64-vertex profiles fit; eight still cannot bypass 16 KiB.
  assert(encodeCadRecipe(validateCadRecipe({ units: "mm", steps })).length <= CAD_LIMITS.recipeBytes);
  await rejects(() => Promise.resolve(encodeCadRecipe({
    units: "mm", steps: Array.from({ length: 8 }, (_, i) => ({ ...profile, outer, id: `p${i}` })),
  })), "invalid_response");
});

Deno.test("CAD recipe: through holes derive cutter depth from primitive and boolean ancestry", () => {
  const cylinder: CadStep = {
    type: "cylinder", id: "cylinder", title: "Cylinder", size: null,
    center: [0, 0, -20], radius: 4, height: 200, inputs: null,
  };
  const sphere: CadStep = {
    ...cylinder, type: "sphere", id: "sphere", center: [0, 0, -5], radius: 3, height: null,
  };
  const torus: CadStep = {
    ...cylinder, type: "torus", id: "torus", center: [0, 0, 5], radius: 6, height: 2,
  };
  const boolean: CadStep = {
    type: "union", id: "combined", title: "Combined", size: null,
    center: null, radius: null, height: null, inputs: ["plate", "cylinder"],
  };
  const cases: [CadStep[], number, number][] = [
    [[plate], 0, 8],
    [[{ ...plate, center: [0, 0, 10] }], 10, 18],
    [[{ ...plate, center: [0, 0, -10] }], -10, -2],
    [[cylinder], -20, 180],
    [[sphere], -8, -2],
    [[torus], 3, 7],
    [[profile], -12.5, -6.5],
    [[{ ...profile, z: 10_000, height: 10_000 }], 10_000, 20_000],
    [[profile, cylinder, { ...boolean, inputs: [profile.id, cylinder.id] }], -20, 180],
    [[profile, cylinder, { ...boolean, type: "subtract", inputs: [profile.id, cylinder.id] }], -12.5, -6.5],
    [[profile, { ...hole, id: "first", inputs: [profile.id] }], -12.5, -6.5],
    [[plate, cylinder, boolean], -20, 180],
    [[plate, cylinder, { ...boolean, type: "subtract" }], 0, 8],
    [[plate, { ...hole, id: "first" }], 0, 8],
    [[{ ...plate, center: [0, 0, 10_000], size: [80, 50, 10_000] }], 10_000, 20_000],
  ];
  for (const [steps, minZ, maxZ] of cases) {
    const target = steps.at(-1)!.id;
    const recipe = validateCadRecipe({ units: "mm", steps: [...steps, { ...hole, inputs: [target] }] });
    const text = new TextDecoder().decode(encodeCadRecipe(recipe));
    const match = text.match(/krly_cut_hole = cylinder at=2,-3,([^ ]+) h=([^ ]+) r=3\n/);
    assert(match, text);
    const base = Number(match[1]), height = Number(match[2]);
    const margin = Math.max(0.1, (maxZ - minZ) * 0.01);
    assert(Number.isFinite(base) && Number.isFinite(height));
    assert(Math.abs(base - (minZ - margin)) < 1e-9);
    assert(Math.abs(base + height - (maxZ + margin)) < 1e-9);
    assert(text.endsWith(`hole = subtract ${target} krly_cut_hole\n`));
    equal(recipe.steps.length, steps.length + 1);
  }
});

Deno.test("CAD recipe: through holes require one earlier target, XY location and reserved cutter ids", async () => {
  for (const patch of [
    { inputs: null }, { inputs: [] }, { inputs: ["plate", "plate"] },
    { inputs: ["missing"] }, { inputs: ["hole"] }, { inputs: [42] },
    { inputs: ["later"] }, { center: [0, 0, 4] }, { center: [0, 0] },
    { center: [Infinity, 0, 0] }, { center: [10_001, 0, 0] },
    { radius: 0 }, { radius: -1 }, { radius: Infinity }, { radius: 10_001 },
    { height: 10 }, { size: [1, 1, 1] }, { id: "krly_cut_hole" },
    { depth: 10 },
  ]) {
    await rejects(() => Promise.resolve(validateCadRecipe({
      units: "mm", steps: [plate, { ...hole, ...patch }],
    })), "invalid_response");
  }
  await rejects(() => Promise.resolve(validateCadRecipe({
    units: "mm", steps: [{ ...plate, id: "krly_cut_hole" }],
  })), "invalid_response");
  const source = { units: "mm", steps: [plate, { ...hole, inputs: ["plate"] }] };
  const checked = validateCadRecipe(source);
  source.steps[1].inputs![0] = "changed";
  assert("inputs" in checked.steps[1]);
  equal(checked.steps[1].inputs, ["plate"]);
});

Deno.test("CAD recipe: validates and serializes a reproducible ISO-style washer feature tree", () => {
  const recipe = validateCadRecipe(washer);
  const text = new TextDecoder().decode(encodeCadRecipe(recipe));
  if (
    !text.includes("outer = cylinder") ||
    !text.includes("washer = subtract outer bore") ||
    !text.startsWith("k3d 1\nlayer geo\n")
  ) {
    throw new Error("Native geometry recipe was not serialized as expected");
  }
});

Deno.test("CAD recipe: rejects backwards references, unsafe ids, excessive size, and unbounded steps", async () => {
  for (
    const value of [
      { ...washer, steps: [{ ...washer.steps[2], id: "result" }] },
      { ...washer, steps: [{ ...washer.steps[0], id: "../tmp" }] },
      { ...washer, steps: [{ ...washer.steps[0], radius: 0 }] },
      { ...washer, steps: [{ ...washer.steps[0], center: [0, 0, 10_001] }] },
      {
        ...washer,
        steps: Array.from({ length: 9 }, (_, index) => ({
          ...washer.steps[0],
          id: `c${index}`,
        })),
      },
    ]
  ) {
    await rejects(
      () => Promise.resolve(validateCadRecipe(value)),
      "invalid_response",
    );
  }
});

Deno.test("CAD recipe: rejects unused solids and self-intersecting tori; validation owns its data", async () => {
  await rejects(
    () =>
      Promise.resolve(
        validateCadRecipe({ ...washer, steps: washer.steps.slice(0, 2) }),
      ),
    "invalid_response",
  );
  await rejects(
    () =>
      Promise.resolve(
        validateCadRecipe({
          units: "mm",
          steps: [{ ...washer.steps[0], type: "torus", radius: 4, height: 4 }],
        }),
      ),
    "invalid_response",
  );
  const source = structuredClone(washer);
  const valid = validateCadRecipe(source);
  source.steps[0].center![0] = 900;
  assert("center" in valid.steps[0]);
  equal(valid.steps[0].center, [0, 0, 0]);
});
