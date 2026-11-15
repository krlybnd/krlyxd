/** Presentation metadata only. Never creates, changes or tessellates native solids.
 * Input is the already validated recipe prefix whose solid mesh succeeded.
 */
export type CadCurveData = {
  kind: "line" | "arc";
  to: number[];
  center: number[];
  clockwise: boolean;
};
export type CadSketchData = {
  plane: "XY";
  offset: number;
  start: number[];
  curves: CadCurveData[];
};
export type CadParameterData = {
  name: string;
  values: number[];
  unit: string;
  text: string;
};
export type CadFeatureData = {
  id: string;
  title: string;
  kind: string;
  inputs: string[];
  parameters: CadParameterData[];
  sketch?: CadSketchData;
  selections: string[];
};
export type CadConstructionData = {
  version: 2;
  coordinateSystem: "right_handed_z_up";
  units: "mm";
  features: CadFeatureData[];
  activeFeatureId: string;
};
const kinds = new Set([
  "sketch",
  "sketch_circle",
  "extrude",
  "pocket",
  "fillet",
  "box",
  "cylinder",
  "sphere",
  "torus",
  "union",
  "subtract",
  "through_hole",
  "circular_hole_pattern",
  "extrude_profile",
]);
function require(value: unknown, message: string): asserts value {
  if (!value) throw new TypeError(`CAD construction: ${message}`);
}
function object(value: unknown): Record<string, unknown> {
  require(
    value !== null && typeof value === "object" && !Array.isArray(value),
    "expected object",
  );
  return value as Record<string, unknown>;
}
function boundedText(value: unknown, max = 80): string {
  require(
    typeof value === "string" && value.length > 0 && value.length <= max,
    "invalid text",
  );
  return value;
}
function number(value: unknown, bound = 10_000): number {
  require(
    typeof value === "number" && Number.isFinite(value) &&
      Math.abs(value) <= bound,
    "invalid dimension",
  );
  return value;
}
function vector(value: unknown, length: number): number[] {
  require(
    Array.isArray(value) && value.length === length,
    "invalid coordinate tuple",
  );
  return value.map((item) => number(item));
}
function id(value: unknown): string {
  const result = boundedText(value, 32);
  require(/^[a-z][a-z0-9_-]{0,31}$/.test(result), "invalid feature ID");
  return result;
}
function sketchOf(step: Record<string, unknown>): CadSketchData {
  const start = vector(step.start, 2);
  require(
    Array.isArray(step.segments) && step.segments.length >= 1 &&
      step.segments.length <= 64,
    "invalid curve count",
  );
  const curves = step.segments.map((raw) => {
    const curve = object(raw);
    require(curve.type === "line" || curve.type === "arc", "unsupported curve");
    if (curve.type === "arc") {
      require(typeof curve.clockwise === "boolean", "invalid arc direction");
    }
    return {
      kind: curve.type,
      to: vector(curve.to, 2),
      center: curve.type === "arc" ? vector(curve.center, 2) : [],
      clockwise: curve.type === "arc" ? curve.clockwise as boolean : false,
    } as CadCurveData;
  });
  return { plane: "XY", offset: number(step.z), start, curves };
}

/** Exact circle representation in the existing analytic sketch wire format. */
function circleSketchOf(step: Record<string, unknown>): CadSketchData {
  const center = vector(step.center, 2);
  const radius = number(step.radius);
  require(radius > 0, "invalid circle radius");
  const start = vector([center[0] + radius, center[1]], 2);
  const opposite = vector([center[0] - radius, center[1]], 2);
  require(
    Math.abs(center[1]) + radius <= 10_000,
    "circle exceeds coordinate bounds",
  );
  return {
    plane: "XY",
    offset: number(step.z),
    start,
    curves: [opposite, [...start]].map((to) => ({
      kind: "arc",
      to,
      center: [...center],
      clockwise: false,
    })),
  };
}

/** Snapshot only the native-validated prefix, including profiles but no fake sketch meshes. */
export function buildCadConstruction(
  recipe: { units: string; steps: readonly unknown[] },
  index: number,
): CadConstructionData {
  require(
    recipe.units === "mm" && recipe.steps.length >= 1 &&
      recipe.steps.length <= 16,
    "invalid recipe bounds/units",
  );
  require(
    Number.isInteger(index) && index >= 0 && index < recipe.steps.length,
    "invalid completed step index",
  );
  const seen = new Set<string>();
  const solidIds = new Set<string>();
  let solids = 0;
  const features = recipe.steps.slice(0, index + 1).map((raw) => {
    const step = object(raw);
    const type = boundedText(step.type, 32);
    require(kinds.has(type), "unsupported feature kind");
    const kind = type === "sketch_circle" ? "sketch" : type;
    if (kind !== "sketch") require(++solids <= 8, "solid stage limit exceeded");
    const featureId = id(step.id);
    require(!seen.has(featureId), "duplicate feature ID");
    require(
      step.inputs == null ||
        (Array.isArray(step.inputs) && step.inputs.length <= 2),
      "invalid dependencies",
    );
    const inputs = step.inputs == null
      ? []
      : (step.inputs as unknown[]).map(id);
    require(
      inputs.every((input) => seen.has(input)),
      "dependency is not an earlier feature",
    );
    if (type === "sketch_circle") {
      require(inputs.length === 0, "circle has dependencies");
    }
    if (kind === "circular_hole_pattern") {
      require(
        inputs.length === 1 && solidIds.has(inputs[0]),
        "pattern requires an earlier solid",
      );
      vector(step.center, 2);
      require(
        number(step.radius) > 0 && number(step.patternRadius) > 0,
        "invalid pattern radius",
      );
      require(
        Number.isInteger(step.count) && number(step.count) >= 1 &&
          number(step.count) <= 16,
        "invalid pattern count",
      );
      number(step.startAngle, Infinity);
    }
    seen.add(featureId);
    if (kind !== "sketch") solidIds.add(featureId);
    const parameters: CadParameterData[] = [];
    const keys = ["size", "center", "radius", "height", "depth", "z"];
    if (kind === "circular_hole_pattern") {
      keys.push("patternRadius", "count", "startAngle");
    }
    for (const key of keys) {
      const value = step[key];
      if (value != null) {
        parameters.push({
          name: key,
          values: Array.isArray(value)
            ? vector(
              value,
              key === "center" &&
                (type === "sketch_circle" || kind === "circular_hole_pattern")
                ? 2
                : 3,
            )
            : [number(value, key === "startAngle" ? Infinity : 10_000)],
          unit: key === "count" ? "" : key === "startAngle" ? "deg" : "mm",
          text: "",
        });
      }
    }
    if (kind === "pocket" && step.depth === null) {
      parameters.push({
        name: "depth",
        values: [],
        unit: "",
        text: "through_all",
      });
    }
    require(
      step.edges === undefined ||
        (Array.isArray(step.edges) && step.edges.length <= 32),
      "invalid edge selections",
    );
    const feature: CadFeatureData = {
      id: featureId,
      title: boundedText(step.title),
      kind,
      inputs,
      parameters,
      selections: step.edges === undefined
        ? []
        : (step.edges as unknown[]).map((value) => boundedText(value, 120)),
    };
    if (kind === "sketch") {
      feature.sketch = type === "sketch_circle"
        ? circleSketchOf(step)
        : sketchOf(step);
    }
    // Legacy combined profile/extrusion still retains its actual polygon; it is
    // displayed as the feature's profile, never invented as a separate recipe node.
    if (kind === "extrude_profile") {
      require(
        Array.isArray(step.outer) && step.outer.length >= 3 &&
          step.outer.length <= 64,
        "invalid polygon",
      );
      const points = step.outer.map((point) => vector(point, 2));
      feature.sketch = {
        plane: "XY",
        offset: number(step.z),
        start: points[0],
        curves: points.slice(1).map((to) => ({
          kind: "line",
          to,
          center: [],
          clockwise: false,
        })),
      };
    }
    return feature;
  });
  const active = features.at(-1)!;
  require(active.kind !== "sketch", "a sketch is not a solid mesh stage");
  return {
    version: 2,
    coordinateSystem: "right_handed_z_up",
    units: "mm",
    features,
    activeFeatureId: active.id,
  };
}
