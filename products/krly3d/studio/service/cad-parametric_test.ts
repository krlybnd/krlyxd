import { buildCad, type NativeEvent } from "./native.ts";
import {
  type CadRecipe,
  type CadStep,
  encodeCadRecipe,
  validateCadRecipe,
} from "./ai/cad-recipe.ts";

// Real native integration only. No provider, mock geometry, or production
// object templates. Run with --allow-read --allow-write --allow-run --allow-env.
const ROOT = new URL("../../../../", import.meta.url);
const hasMesh = await Deno.stat(new URL("target/debug/krly3d-mesh", ROOT))
  .then((info) => info.isFile, (error) => {
    if (error instanceof Deno.errors.NotFound) return false;
    throw error;
  });

function assert(value: unknown, message = "Assertion failed"): asserts value {
  if (!value) throw new Error(message);
}
function equal(actual: unknown, expected: unknown) {
  assert(JSON.stringify(actual) === JSON.stringify(expected), "Values differ");
}
function near(actual: number, expected: number, tolerance = 1e-7) {
  assert(
    Math.abs(actual - expected) <= tolerance,
    `Expected ${expected} ± ${tolerance}, got ${actual}`,
  );
}
type Mesh = NonNullable<NativeEvent["mesh"]>;
type MeshEvent = NativeEvent & { mesh: Mesh };
const hasGeometry = (event: NativeEvent): event is MeshEvent => !!event.mesh;
type Sketch = Extract<CadStep, { type: "sketch" }>;

function block(z = 0): Extract<CadStep, { type: "box" }> {
  return {
    type: "box",
    id: "blank",
    title: "Blank",
    size: [40, 30, 8],
    center: [0, 0, z],
    radius: null,
    height: null,
    inputs: null,
  };
}
function rectangle(
  id: string,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  z: number,
): Sketch {
  return {
    type: "sketch",
    id,
    title: id,
    start: [x0, y0],
    z,
    segments: [
      { type: "line", to: [x1, y0] },
      { type: "line", to: [x1, y1] },
      { type: "line", to: [x0, y1] },
    ],
  };
}

function bounds(mesh: Mesh) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < mesh.positions.length; i++) {
    const axis = i % 3;
    min[axis] = Math.min(min[axis], mesh.positions[i]);
    max[axis] = Math.max(max[axis], mesh.positions[i]);
  }
  return { min, max };
}
function checkBounds(mesh: Mesh, min: number[], max: number[]) {
  const measured = bounds(mesh);
  for (let axis = 0; axis < 3; axis++) {
    near(measured.min[axis], min[axis]);
    near(measured.max[axis], max[axis]);
  }
}

/** Independently verify indexed edge incidence/orientation as well as the
 * native summary. This catches open or duplicated caps in pocket meshes.
 */
function closed(mesh: Mesh) {
  equal(mesh.units, "mm");
  equal(mesh.openEdges, 0);
  assert(mesh.volume > 0 && mesh.area > 0);
  assert(mesh.positions.every(Number.isFinite));
  const edges = new Map<string, { count: number; balance: number }>();
  for (let i = 0; i < mesh.indices.length; i += 3) {
    for (let j = 0; j < 3; j++) {
      const a = mesh.indices[i + j], b = mesh.indices[i + (j + 1) % 3];
      assert(a !== b && a >= 0 && a < mesh.positions.length / 3);
      const key = `${Math.min(a, b)},${Math.max(a, b)}`;
      const edge = edges.get(key) ?? { count: 0, balance: 0 };
      edge.count++;
      edge.balance += a < b ? 1 : -1;
      edges.set(key, edge);
    }
  }
  assert(
    [...edges.values()].every((edge) => edge.count === 2 && edge.balance === 0),
    "Mesh edges must have two oppositely oriented incident triangles",
  );
}

/** Vertical line/triangle intersections: inspect material floors/caps instead
 * of trusting volume alone. The sample points are strictly inside openings.
 */
function verticalHits(mesh: Mesh, x: number, y: number): number[] {
  const hits: number[] = [], p = mesh.positions;
  for (let i = 0; i < mesh.indices.length; i += 3) {
    const a = 3 * mesh.indices[i],
      b = 3 * mesh.indices[i + 1],
      c = 3 * mesh.indices[i + 2];
    const ux = p[b] - p[a], uy = p[b + 1] - p[a + 1];
    const vx = p[c] - p[a], vy = p[c + 1] - p[a + 1];
    const determinant = ux * vy - uy * vx;
    if (Math.abs(determinant) < 1e-10) continue;
    const dx = x - p[a], dy = y - p[a + 1];
    const u = (dx * vy - dy * vx) / determinant;
    const v = (ux * dy - uy * dx) / determinant;
    if (u < -1e-9 || v < -1e-9 || u + v > 1 + 1e-9) continue;
    const z = p[a + 2] + u * (p[b + 2] - p[a + 2]) + v * (p[c + 2] - p[a + 2]);
    if (!hits.some((hit) => Math.abs(hit - z) < 1e-7)) hits.push(z);
  }
  return hits.sort((a, b) => a - b);
}
function checkHits(mesh: Mesh, x: number, y: number, expected: number[]) {
  const hits = verticalHits(mesh, x, y);
  equal(hits.length, expected.length);
  hits.forEach((z, i) => near(z, expected[i]));
}

async function build(recipe: CadRecipe) {
  const events: NativeEvent[] = [];
  for await (const event of buildCad(recipe, new AbortController().signal)) {
    events.push(event);
  }
  const stages = events.filter(hasGeometry);
  const solids = recipe.steps.filter((step) =>
    step.type !== "sketch" && step.type !== "sketch_circle"
  );
  equal(stages.length, solids.length);
  equal(
    stages.map((event) => event.details?.node),
    solids.map((step) => step.id),
  );
  equal(
    stages.map((event) => event.kind),
    solids.map((_, i) =>
      i === solids.length - 1 ? "editable_proposal" : "construction_step"
    ),
  );
  equal(events.at(-1)?.kind, "completed");
  equal(events.filter((event) => event.kind === "completed").length, 1);
  for (const event of stages) {
    closed(event.mesh);
    // Discriminate construction v2 before inspecting CAD-specific data; image
    // reconstruction still uses the separate v1 contract.
    const construction = event.construction;
    assert(
      construction?.version === 2,
      "CAD solids must preserve v2 construction data",
    );
    equal(construction.units, "mm");
    equal(construction.coordinateSystem, "right_handed_z_up");
    equal(construction.activeFeatureId, event.details?.node);
    const completedIndex = recipe.steps.findIndex((step) =>
      step.id === construction.activeFeatureId
    );
    const prefix = recipe.steps.slice(0, completedIndex + 1);
    equal(
      construction.features.map((feature) => feature.id),
      prefix.map((step) => step.id),
    );
    for (const [index, step] of prefix.entries()) {
      const feature = construction.features[index];
      equal(feature.kind, step.type === "sketch_circle" ? "sketch" : step.type);
      equal(feature.title, step.title);
      equal(feature.inputs, "inputs" in step ? step.inputs ?? [] : []);
      if (step.type === "sketch") {
        assert(
          feature.sketch,
          "Sketch must survive as editable curves without a fake solid mesh",
        );
        equal(feature.sketch.plane, "XY");
        equal(feature.sketch.offset, step.z);
        equal(feature.sketch.start, step.start);
        equal(feature.sketch.curves.length, step.segments.length);
        for (const [i, segment] of step.segments.entries()) {
          const curve = feature.sketch.curves[i];
          equal(curve.kind, segment.type);
          equal(curve.to, segment.to);
          if (segment.type === "arc") {
            equal(curve.center, segment.center);
            equal(curve.clockwise, segment.clockwise);
          }
        }
      } else if (step.type === "sketch_circle") {
        assert(feature.sketch, "Circle must retain editable analytic curves");
        equal(feature.sketch.plane, "XY");
        equal(feature.sketch.offset, step.z);
        const [x, y] = step.center;
        equal(feature.sketch.start, [x + step.radius, y]);
        equal(feature.sketch.curves, [
          {
            kind: "arc",
            to: [x - step.radius, y],
            center: step.center,
            clockwise: false,
          },
          {
            kind: "arc",
            to: [x + step.radius, y],
            center: step.center,
            clockwise: false,
          },
        ]);
        for (const name of ["center", "radius", "z"] as const) {
          const parameter = feature.parameters.find((p) => p.name === name);
          assert(parameter, `Missing circle parameter ${name}`);
          equal(
            parameter.values,
            name === "center" ? step.center : [step[name]],
          );
          equal(parameter.unit, "mm");
        }
      } else if (step.type === "circular_hole_pattern") {
        for (
          const name of [
            "center",
            "patternRadius",
            "radius",
            "count",
            "startAngle",
          ] as const
        ) {
          const parameter = feature.parameters.find((p) => p.name === name);
          assert(parameter, `Missing pattern parameter ${name}`);
          equal(
            parameter.values,
            name === "center" ? step.center : [step[name]],
          );
          equal(
            parameter.unit,
            name === "count" ? "" : name === "startAngle" ? "deg" : "mm",
          );
        }
      } else if (step.type === "pocket") {
        const depth = feature.parameters.find((parameter) =>
          parameter.name === "depth"
        );
        assert(depth);
        if (step.depth === null) {
          equal(depth.text, "through_all");
          equal(depth.values, []);
        } else {
          equal(depth.values, [step.depth]);
          equal(depth.unit, "mm");
        }
      } else if (step.type === "fillet") {
        equal(feature.selections, step.edges);
        equal(
          feature.parameters.find((parameter) => parameter.name === "radius")
            ?.values,
          [step.radius],
        );
      } else if (step.type === "extrude") {
        equal(
          feature.parameters.find((parameter) => parameter.name === "height")
            ?.values,
          [step.height],
        );
      }
    }
  }
  return { events, stages, mesh: stages.at(-1)!.mesh };
}
async function replay(
  recipe: CadRecipe,
  original: Awaited<ReturnType<typeof build>>,
) {
  const artifact = (name: string) => {
    const value = original.events.find((event) => event.artifact?.name === name)
      ?.artifact;
    assert(value, `Missing ${name}`);
    return value.bytes;
  };
  const recipeBytes = artifact("design.recipe.json");
  const restored = validateCadRecipe(
    JSON.parse(new TextDecoder().decode(recipeBytes)),
  );
  equal(restored, recipe);
  equal(encodeCadRecipe(restored), artifact("design.k3d"));
  const rebuilt = await build(restored);
  for (const [i, stage] of original.stages.entries()) {
    equal(rebuilt.stages[i].construction, stage.construction);
    for (
      const key of [
        "positions",
        "indices",
        "volume",
        "area",
        "openEdges",
        "units",
      ] as const
    ) {
      equal(rebuilt.stages[i].mesh[key], stage.mesh[key]);
    }
  }
  for (const name of ["design.recipe.json", "design.k3d"]) {
    equal(
      rebuilt.events.find((event) => event.artifact?.name === name)?.artifact
        ?.bytes,
      artifact(name),
    );
  }
}

Deno.test({
  name:
    "parametric native: arc sketch extrudes with analytic rounded-rectangle volume, bounds and replay",
  ignore: !hasMesh,
  async fn() {
    const outline: Sketch = {
      type: "sketch",
      id: "outline",
      title: "Rounded rectangle",
      start: [-12, -10],
      z: 2,
      segments: [
        { type: "line", to: [12, -10] },
        { type: "arc", center: [12, -7], to: [15, -7], clockwise: false },
        { type: "line", to: [15, 7] },
        { type: "arc", center: [12, 7], to: [12, 10], clockwise: false },
        { type: "line", to: [-12, 10] },
        { type: "arc", center: [-12, 7], to: [-15, 7], clockwise: false },
        { type: "line", to: [-15, -7] },
        { type: "arc", center: [-12, -7], to: [-12, -10], clockwise: false },
      ],
    };
    const recipe: CadRecipe = {
      units: "mm",
      steps: [outline, {
        type: "extrude",
        id: "part",
        title: "Extrusion",
        inputs: ["outline"],
        height: 6,
      }],
    };
    const result = await build(recipe);
    // At native 0.05 mm chord tolerance, four radius-3 corners differ by about
    // 1.1 mm³ from their analytic volume; a sharp rectangle differs by 46 mm³.
    near(result.mesh.volume, (600 - (4 - Math.PI) * 9) * 6, 2);
    checkBounds(result.mesh, [-15, -10, 2], [15, 10, 8]);
    checkHits(result.mesh, 0, 0, [2, 8]);
    checkHits(result.mesh, 14.9, 9.9, []);
    await replay(recipe, result);
  },
});

for (const compact of [false, true]) {
  Deno.test({
    name: `parametric native: DN50-like ${
      compact ? "four-node circle/pattern" : "two-arc"
    } OD165 flange, thickness20, bore61.5 and four18 holes on PCD125 at45 replay`,
    ignore: !hasMesh,
    async fn() {
      // Test-only dimensions, expressed entirely as ordinary CAD operations.
      // Both the six-solid baseline and three-solid compact recipe stay below
      // the eight-solid limit. Pattern cutters must not become user features.
      const outerRadius = 165 / 2, boreRadius = 61.5 / 2;
      const boltRadius = 18 / 2, pitchRadius = 125 / 2, height = 20;
      const steps: CadStep[] = [
        compact
          ? {
            type: "sketch_circle",
            id: "outline",
            title: "Circular outline",
            center: [0, 0],
            radius: outerRadius,
            z: 0,
          }
          : {
            type: "sketch",
            id: "outline",
            title: "Circular outline",
            start: [outerRadius, 0],
            z: 0,
            segments: [
              {
                type: "arc",
                center: [0, 0],
                to: [-outerRadius, 0],
                clockwise: false,
              },
              {
                type: "arc",
                center: [0, 0],
                to: [outerRadius, 0],
                clockwise: false,
              },
            ],
          },
        {
          type: "extrude",
          id: "blank",
          title: "Circular blank",
          inputs: ["outline"],
          height,
        },
        {
          type: "through_hole",
          id: "bore",
          title: "Central bore",
          inputs: ["blank"],
          center: [0, 0, 0],
          radius: boreRadius,
          size: null,
          height: null,
        },
      ];
      if (compact) {
        steps.push({
          type: "circular_hole_pattern",
          id: "bolts",
          title: "Bolt hole pattern",
          inputs: ["bore"],
          center: [0, 0],
          patternRadius: pitchRadius,
          radius: boltRadius,
          count: 4,
          startAngle: 45,
        });
      } else {
        for (let i = 0; i < 4; i++) {
          const angle = (45 + 90 * i) * Math.PI / 180;
          steps.push({
            type: "through_hole",
            id: `bolt${i + 1}`,
            title: `Bolt hole ${i + 1}`,
            inputs: [i === 0 ? "bore" : `bolt${i}`],
            center: [
              pitchRadius * Math.cos(angle),
              pitchRadius * Math.sin(angle),
              0,
            ],
            radius: boltRadius,
            size: null,
            height: null,
          });
        }
      }
      const recipe = validateCadRecipe({ units: "mm", steps });
      const result = await build(recipe);
      equal(recipe.steps.length, compact ? 4 : 7);
      equal(result.stages.length, compact ? 3 : 6);

      // Check each native stage against analytic volume. A conservative error
      // budget is boundary perimeter * 0.05 mm chord tolerance * thickness;
      // include inner boundaries too, without relying on error cancellation.
      const chordTolerance = 0.05;
      for (const [i, stage] of result.stages.entries()) {
        const drilledHoles = compact && i === 2 ? 5 : i;
        const drilledBolts = Math.max(0, drilledHoles - 1);
        const removedArea = (i > 0 ? boreRadius ** 2 : 0) +
          drilledBolts * boltRadius ** 2;
        const boundaryRadii = outerRadius + (i > 0 ? boreRadius : 0) +
          drilledBolts * boltRadius;
        near(
          stage.mesh.volume,
          Math.PI * (outerRadius ** 2 - removedArea) * height,
          2 * Math.PI * boundaryRadii * chordTolerance * height,
        );
        const measured = bounds(stage.mesh);
        for (const axis of [0, 1]) {
          near(measured.min[axis], -outerRadius, chordTolerance);
          near(measured.max[axis], outerRadius, chordTolerance);
        }
        near(measured.min[2], 0);
        near(measured.max[2], height);
        if (i > 0) assert(stage.mesh.volume < result.stages[i - 1].mesh.volume);

        // Independent Cartesian locations verify the 45-degree pattern on the
        // 125 mm pitch circle, including material before each hole is drilled.
        const d = pitchRadius / Math.sqrt(2);
        const openings = [
          [0, 0, boreRadius],
          [d, d, boltRadius],
          [-d, d, boltRadius],
          [-d, -d, boltRadius],
          [d, -d, boltRadius],
        ];
        for (const [holeIndex, [x, y, radius]] of openings.entries()) {
          const expected = drilledHoles > holeIndex ? [] : [0, height];
          checkHits(stage.mesh, x, y, expected);
          // Samples straddle each nominal radius by more than the chord error:
          // catch wrong diameters, shifted holes, retained floors and extra cuts.
          for (const [dx, dy] of [[1, 0], [0, 1], [-1, 0], [0, -1]]) {
            checkHits(
              stage.mesh,
              x + dx * (radius - 0.2),
              y + dy * (radius - 0.2),
              expected,
            );
            checkHits(
              stage.mesh,
              x + dx * (radius + 0.2),
              y + dy * (radius + 0.2),
              [0, height],
            );
          }
        }
        // Cardinal PCD positions stay solid: a zero-degree pattern is wrong.
        for (
          const [x, y] of [
            [pitchRadius, 0],
            [0, pitchRadius],
            [-pitchRadius, 0],
            [0, -pitchRadius],
          ]
        ) {
          checkHits(stage.mesh, x, y, [0, height]);
        }
      }
      await replay(recipe, result);
    },
  });
}

for (const baseZ of [-7, 10]) {
  for (const depth of [3, null]) {
    Deno.test({
      name: `parametric native: ${
        depth === null
          ? "through-all pocket has no floor"
          : "blind pocket keeps its floor"
      } at Z=${baseZ}, mm volume and replay`,
      ignore: !hasMesh,
      async fn() {
        const recipe: CadRecipe = {
          units: "mm",
          steps: [
            block(baseZ),
            rectangle("cut", -5, -4, 5, 4, baseZ + 8),
            {
              type: "pocket",
              id: "part",
              title: "Pocket",
              inputs: ["blank", "cut"],
              depth,
            },
          ],
        };
        const result = await build(recipe);
        near(result.mesh.volume, 9600 - 80 * (depth ?? 8));
        checkBounds(result.mesh, [-20, -15, baseZ], [20, 15, baseZ + 8]);
        for (const [x, y] of [[0.3, 0.7], [-3, 2], [4, -2]]) {
          checkHits(
            result.mesh,
            x,
            y,
            depth === null ? [] : [baseZ, baseZ + 5],
          );
        }
        checkHits(result.mesh, 10, 9, [baseZ, baseZ + 8]);
        await replay(recipe, result);
      },
    });
  }
}

for (const radius of [1, 3]) {
  Deno.test({
    name:
      `parametric native: plain box vertical-corner fillet radius=${radius} removes analytic volume and replays`,
    ignore: !hasMesh,
    async fn() {
      const recipe: CadRecipe = {
        units: "mm",
        steps: [block(), {
          type: "fillet",
          id: "part",
          title: "Corner round",
          inputs: ["blank"],
          edges: ["front|left"],
          radius,
        }],
      };
      const result = await build(recipe);
      const removed = radius * radius * (1 - Math.PI / 4) * 8;
      near(result.mesh.volume, 9600 - removed, 0.6);
      assert(result.mesh.volume < result.stages[0].mesh.volume);
      checkBounds(result.mesh, [-20, -15, 0], [20, 15, 8]);
      const cx = -20 + radius, cy = -15 + radius;
      let interiorArcPoints = 0;
      for (let i = 0; i < result.mesh.positions.length; i += 3) {
        const [x, y] = result.mesh.positions.slice(i, i + 2);
        if (x <= cx + 1e-8 && y <= cy + 1e-8) {
          near(Math.hypot(x - cx, y - cy), radius);
          if (x < cx - 1e-8 && y < cy - 1e-8) interiorArcPoints++;
        }
      }
      assert(
        interiorArcPoints >= 2,
        "Fillet must retain a curved surface, not a chamfer",
      );
      await replay(recipe, result);
    },
  });
}

Deno.test({
  name:
    "parametric native: custom curved 200 mm wrench, 16 mm open jaw pocket and blind handle recess preserve features on replay",
  ignore: !hasMesh,
  async fn() {
    // Test-only custom proportions, not a manufacturer contour or standard.
    const outline: Sketch = {
      type: "sketch",
      id: "outline",
      title: "Curved head and handle",
      start: [-8, 8],
      z: 0,
      segments: [
        { type: "arc", center: [0, 8], to: [8, 8], clockwise: false },
        { type: "line", to: [8, 150] },
        { type: "line", to: [20, 168] },
        { type: "line", to: [20, 192] },
        { type: "arc", center: [12, 192], to: [12, 200], clockwise: false },
        { type: "line", to: [-12, 200] },
        { type: "arc", center: [-12, 192], to: [-20, 192], clockwise: false },
        { type: "line", to: [-20, 168] },
        { type: "line", to: [-8, 150] },
      ],
    };
    const recess: Sketch = {
      type: "sketch",
      id: "recess",
      title: "Handle recess",
      start: [-3, 40],
      z: 6,
      segments: [
        { type: "arc", center: [0, 40], to: [3, 40], clockwise: false },
        { type: "line", to: [3, 130] },
        { type: "arc", center: [0, 130], to: [-3, 130], clockwise: false },
      ],
    };
    const recipe: CadRecipe = {
      units: "mm",
      steps: [
        outline,
        {
          type: "extrude",
          id: "blank",
          title: "Flat blank",
          inputs: ["outline"],
          height: 6,
        },
        rectangle("mouth", -8, 180, 8, 210, 6),
        {
          type: "pocket",
          id: "jaw",
          title: "Open jaw",
          inputs: ["blank", "mouth"],
          depth: null,
        },
        recess,
        {
          type: "pocket",
          id: "part",
          title: "Handle relief",
          inputs: ["jaw", "recess"],
          depth: 1.5,
        },
      ],
    };
    const result = await build(recipe);
    const blankArea = Math.PI * 32 + 16 * 142 + 28 * 18 + 40 * 24 +
      40 * 8 - 2 * (64 - Math.PI * 16);
    near(result.stages[0].mesh.volume, blankArea * 6, 3);
    near(
      result.stages[0].mesh.volume - result.stages[1].mesh.volume,
      16 * 20 * 6,
    );
    near(
      result.stages[1].mesh.volume - result.mesh.volume,
      (6 * 90 + Math.PI * 9) * 1.5,
      1,
    );
    checkBounds(result.mesh, [-20, 0, 0], [20, 200, 6]);
    // Open mouth, solid jaws, narrow handle, and the blind recess's retained floor.
    for (const y of [181, 190, 199]) {
      checkHits(result.mesh, 0, y, []);
      checkHits(result.mesh, 10, y, [0, 6]);
      checkHits(result.mesh, -10, y, [0, 6]);
    }
    checkHits(result.mesh, 10, 80, []);
    checkHits(result.mesh, 6, 80, [0, 6]);
    checkHits(result.mesh, 0, 80, [0, 4.5]);
    // Parallel jaw planes are 16 mm apart and each spans the full 6 mm thickness.
    for (const x of [-8, 8]) {
      for (const y of [180, 200]) {
        for (const z of [0, 6]) {
          assert(result.mesh.positions.some((v, i, p) =>
            i % 3 === 0 &&
            Math.abs(v - x) < 1e-7 && Math.abs(p[i + 1] - y) < 1e-7 &&
            Math.abs(p[i + 2] - z) < 1e-7
          ));
        }
      }
    }
    await replay(recipe, result);
  },
});
