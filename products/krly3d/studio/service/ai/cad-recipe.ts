import { ProviderError } from "./errors.ts";

export type SketchSegment =
  | { type: "line"; to: [number, number] }
  | {
    type: "arc";
    center: [number, number];
    to: [number, number];
    clockwise: boolean;
  };

export type CadStep =
  | {
    type: "sketch_circle";
    id: string;
    title: string;
    center: [number, number];
    radius: number;
    z: number;
  }
  | {
    type: "circular_hole_pattern";
    id: string;
    title: string;
    inputs: [string];
    center: [number, number];
    patternRadius: number;
    radius: number;
    count: number;
    /** Degrees counterclockwise from +X; all lengths are mm. */
    startAngle: number;
  }
  | {
    type: "sketch";
    id: string;
    title: string;
    /** Absolute XY points at world Z=z. Z closes any remaining straight edge. */
    start: [number, number];
    segments: SketchSegment[];
    z: number;
  }
  | {
    type: "extrude";
    id: string;
    title: string;
    inputs: [string];
    height: number;
  }
  | {
    type: "pocket";
    id: string;
    title: string;
    /** Earlier solid, then an earlier sketch on its upper Z bound. */
    inputs: [string, string];
    /** Downward depth from the top; null means through-all. */
    depth: number | null;
  }
  | {
    type: "fillet";
    id: string;
    title: string;
    inputs: [string];
    /** Native face-pair selectors. Syntactic validity is not kernel support. */
    edges: string[];
    radius: number;
  }
  | {
    type: "extrude_profile";
    id: string;
    title: string;
    /** Simple XY polygon, implicitly closed, in either winding order. */
    outer: [number, number][];
    height: number;
    /** Base plane; extrusion follows +Z. */
    z: number;
  }
  | {
    type: "through_hole";
    id: string;
    title: string;
    size: null;
    center: [number, number, number];
    radius: number;
    height: null;
    inputs: [string];
  }
  | {
    type: "box";
    id: string;
    title: string;
    size: [number, number, number];
    center: [number, number, number];
    radius: null;
    height: null;
    inputs: null;
  }
  | {
    type: "cylinder";
    id: string;
    title: string;
    size: null;
    center: [number, number, number];
    radius: number;
    height: number;
    inputs: null;
  }
  | {
    type: "sphere";
    id: string;
    title: string;
    size: null;
    center: [number, number, number];
    radius: number;
    height: null;
    inputs: null;
  }
  | {
    type: "torus";
    id: string;
    title: string;
    size: null;
    center: [number, number, number];
    radius: number;
    height: number;
    inputs: null;
  }
  | {
    type: "union" | "subtract";
    id: string;
    title: string;
    size: null;
    center: null;
    radius: null;
    height: null;
    inputs: [string, string];
  };

export type CadRecipe = { units: "mm"; steps: CadStep[] };
export type CadCircleSketch = Extract<CadStep, { type: "sketch_circle" }>;
export type CadCircularHolePattern = Extract<CadStep, { type: "circular_hole_pattern" }>;
export type CadSketch = Extract<CadStep, { type: "sketch" | "sketch_circle" }>;

export const CAD_LIMITS = Object.freeze({
  steps: 16,
  solidFeatures: 8,
  recipeBytes: 16 * 1024,
  coordinateMm: 10_000,
  sizeMm: 10_000,
  titleChars: 80,
  profileVertices: 64,
  sketchSegments: 64,
  filletEdges: 8,
  patternHoles: 16,
  profileToleranceMm: 1e-9,
});

function invalid(): never {
  throw new ProviderError(
    "invalid_response",
    "A szakértő érvénytelen vagy nem támogatott CAD-műveleti tervet adott.",
  );
}

function exact(value: Record<string, unknown>, keys: string[]): boolean {
  return Object.keys(value).length === keys.length &&
    keys.every((key) => Object.hasOwn(value, key));
}

function tuple(
  value: unknown,
  bound: number,
  positive: boolean,
): value is [number, number, number] {
  return Array.isArray(value) && value.length === 3 &&
    value.every((item) =>
      typeof item === "number" && Number.isFinite(item) &&
      (positive ? item > 0 && item <= bound : Math.abs(item) <= bound)
    );
}

function scalar(value: unknown, positive = false): value is number {
  return typeof value === "number" && Number.isFinite(value) &&
    (positive ? value > 0 && value <= CAD_LIMITS.sizeMm : true);
}

/** Bounded admission checks, not a second geometry engine. The Rust 2D Region
 * API is structural only and has no synchronous TS topology adapter. Native
 * profile/extrude still own construction and tessellation. Reject numerically
 * ambiguous contacts and redundant collinear corners instead of repairing them.
 */
function polygon(value: unknown): value is [number, number][] {
  if (
    !Array.isArray(value) || value.length < 3 ||
    value.length > CAD_LIMITS.profileVertices
  ) return false;
  const points: [number, number][] = [];
  for (const point of value) {
    if (
      !Array.isArray(point) || point.length !== 2 ||
      !point.every((n) => scalar(n) && Math.abs(n) <= CAD_LIMITS.coordinateMm)
    ) return false;
    points.push(point as [number, number]);
  }
  const epsilon = CAD_LIMITS.profileToleranceMm;
  type Point = [number, number];
  const cross = (a: Point, b: Point, c: Point) =>
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
  const side = (a: Point, b: Point, c: Point) => {
    const ux = b[0] - a[0], uy = b[1] - a[1];
    const vx = c[0] - a[0], vy = c[1] - a[1];
    const tolerance = epsilon * Math.hypot(ux, uy) +
      8 * Number.EPSILON * (Math.abs(ux * vy) + Math.abs(uy * vx));
    const determinant = cross(a, b, c);
    return Math.abs(determinant) <= tolerance ? 0 : Math.sign(determinant);
  };
  const onSegment = (a: Point, b: Point, p: Point) =>
    p.every((n, axis) => n >= Math.min(a[axis], b[axis]) - epsilon &&
      n <= Math.max(a[axis], b[axis]) + epsilon);
  let twiceArea = 0, perimeter = 0;
  for (let i = 0; i < points.length; i++) {
    const a = points[i], b = points[(i + 1) % points.length];
    if (side(a, b, points[(i + 2) % points.length]) === 0) return false;
    twiceArea += cross(points[0], a, b);
    perimeter += Math.hypot(b[0] - a[0], b[1] - a[1]);
    for (let j = i + 1; j < points.length; j++) {
      const c = points[j], d = points[(j + 1) % points.length];
      // Includes a repeated closing vertex, zero-length edges and pinch points.
      if (Math.hypot(c[0] - a[0], c[1] - a[1]) <= epsilon) return false;
      if (j === i + 1 || (i === 0 && j === points.length - 1)) continue;
      const abC = side(a, b, c), abD = side(a, b, d);
      const cdA = side(c, d, a), cdB = side(c, d, b);
      if (
        (abC * abD < 0 && cdA * cdB < 0) ||
        (abC === 0 && onSegment(a, b, c)) ||
        (abD === 0 && onSegment(a, b, d)) ||
        (cdA === 0 && onSegment(c, d, a)) ||
        (cdB === 0 && onSegment(c, d, b))
      ) return false;
    }
  }
  return Math.abs(twiceArea) > epsilon * perimeter;
}

type Point2 = [number, number];
type Boundary = { from: Point2; to: Point2 } & (
  | { type: "line" }
  | { type: "arc"; center: Point2; radius: number; angle: number; sweep: number }
);
const EPS = CAD_LIMITS.profileToleranceMm;
const TAU = 2 * Math.PI;
const difference = (a: Point2, b: Point2): Point2 => [a[0] - b[0], a[1] - b[1]];
const cross2 = (a: Point2, b: Point2) => a[0] * b[1] - a[1] * b[0];
const dot2 = (a: Point2, b: Point2) => a[0] * b[0] + a[1] * b[1];
const distance = (a: Point2, b: Point2) => Math.hypot(...difference(a, b));
const positiveAngle = (angle: number) => ((angle % TAU) + TAU) % TAU;
function point2(value: unknown): value is Point2 {
  return Array.isArray(value) && value.length === 2 &&
    scalar(value[0]) && scalar(value[1]) &&
    Math.abs(value[0]) <= CAD_LIMITS.coordinateMm &&
    Math.abs(value[1]) <= CAD_LIMITS.coordinateMm;
}

/** Exact analytic guide for a validated circle; native encoding uses one edge. */
export function circleSketchGeometry(
  step: Pick<CadCircleSketch, "center" | "radius">,
): { start: Point2; segments: SketchSegment[] } {
  const [x, y] = step.center;
  return {
    start: [x + step.radius, y],
    segments: [
      { type: "arc", center: [x, y], to: [x - step.radius, y], clockwise: false },
      { type: "arc", center: [x, y], to: [x + step.radius, y], clockwise: false },
    ],
  };
}

/** Host-computed XY positions for a validated pattern, in cutting order.
 * Reduce degrees before conversion to avoid overflow for finite large angles.
 * Exact cardinal directions avoid artificial offsets on axis-aligned patterns.
 */
export function circularHoleCenters(
  step: Pick<CadCircularHolePattern, "center" | "patternRadius" | "count" | "startAngle">,
): Point2[] {
  return Array.from({ length: step.count }, (_, index) => {
    const degrees = (((step.startAngle % 360) + 360) % 360 + index * 360 / step.count) % 360;
    const angle = degrees * Math.PI / 180;
    const [x, y] = degrees === 0 ? [1, 0] : degrees === 90 ? [0, 1] :
      degrees === 180 ? [-1, 0] : degrees === 270 ? [0, -1] : [Math.cos(angle), Math.sin(angle)];
    return [step.center[0] + step.patternRadius * x, step.center[1] + step.patternRadius * y];
  });
}

function boundedCircle(center: unknown, radius: unknown): boolean {
  return point2(center) && scalar(radius, true) && radius > EPS &&
    center.every((value) => Math.abs(value) + radius <= CAD_LIMITS.coordinateMm);
}

function onBoundary(edge: Boundary, p: Point2): boolean {
  if (distance(edge.from, p) <= EPS || distance(edge.to, p) <= EPS) return true;
  if (edge.type === "line") {
    const v = difference(edge.to, edge.from), q = difference(p, edge.from);
    const length = Math.hypot(...v), along = dot2(q, v) / length;
    return Math.abs(cross2(v, q)) <= EPS * length &&
      along >= -EPS && along <= length + EPS;
  }
  const angle = Math.atan2(p[1] - edge.center[1], p[0] - edge.center[0]);
  const travel = positiveAngle((angle - edge.angle) * Math.sign(edge.sweep));
  return Math.abs(distance(edge.center, p) - edge.radius) <= 4 * EPS &&
    travel <= Math.abs(edge.sweep) + EPS / edge.radius;
}

/** Analytic line/line, line/circle and circle/circle contacts, trimmed to arcs.
 * null means overlapping or numerically unresolved boundaries. No polyline
 * sampling is used for admission or emitted geometry. Work is bounded O(n²).
 */
function contacts(a: Boundary, b: Boundary): Point2[] | null {
  if (a.type === "line" && b.type === "line") {
    const u = difference(a.to, a.from), v = difference(b.to, b.from);
    const q = difference(b.from, a.from), denominator = cross2(u, v);
    const roundoff = 16 * Number.EPSILON *
      (Math.abs(u[0] * v[1]) + Math.abs(u[1] * v[0]));
    if (Math.abs(denominator) <= roundoff) {
      const length = Math.hypot(...u);
      if (Math.abs(cross2(u, q)) > EPS * length) return [];
      const first = dot2(q, u) / length;
      const last = dot2(difference(b.to, a.from), u) / length;
      const lo = Math.max(0, Math.min(first, last));
      const hi = Math.min(length, Math.max(first, last));
      if (hi < lo - EPS) return [];
      if (hi > lo + EPS) return null;
      return [[a.from[0] + lo * u[0] / length, a.from[1] + lo * u[1] / length]];
    }
    const t = cross2(q, v) / denominator;
    const p: Point2 = [a.from[0] + t * u[0], a.from[1] + t * u[1]];
    return onBoundary(a, p) && onBoundary(b, p) ? [p] : [];
  }
  if (a.type === "arc" && b.type === "line") return contacts(b, a);
  if (a.type === "line" && b.type === "arc") {
    const v = difference(a.to, a.from), length = Math.hypot(...v);
    const unit: Point2 = [v[0] / length, v[1] / length];
    const offset = difference(b.center, a.from), along = dot2(offset, unit);
    const perpendicular = cross2(unit, offset);
    const square = b.radius ** 2 - perpendicular ** 2;
    const tolerance = 4 * EPS * b.radius + 32 * Number.EPSILON * b.radius ** 2;
    if (square < -tolerance) return [];
    const delta = Math.abs(square) <= tolerance ? 0 : Math.sqrt(square);
    return [along - delta, along + delta].flatMap((t) => {
      const p: Point2 = [a.from[0] + t * unit[0], a.from[1] + t * unit[1]];
      return t >= -EPS && t <= length + EPS && onBoundary(b, p) ? [p] : [];
    });
  }
  if (a.type !== "arc" || b.type !== "arc") return invalid();
  const delta = difference(b.center, a.center), separation = Math.hypot(...delta);
  if (separation <= EPS) {
    if (Math.abs(a.radius - b.radius) > EPS) return [];
    // On the same circle, endpoints partition angular intervals. An interior
    // endpoint or midpoint belonging to the other interval proves overlap.
    const interior = (edge: Boundary, p: Point2) => onBoundary(edge, p) &&
      distance(edge.from, p) > EPS && distance(edge.to, p) > EPS;
    const middle = (edge: Extract<Boundary, { type: "arc" }>): Point2 => [
      edge.center[0] + edge.radius * Math.cos(edge.angle + edge.sweep / 2),
      edge.center[1] + edge.radius * Math.sin(edge.angle + edge.sweep / 2),
    ];
    if (
      [a.from, a.to, middle(a)].some((p) => interior(b, p)) ||
      [b.from, b.to, middle(b)].some((p) => interior(a, p))
    ) return null;
    return [a.from, a.to].filter((p) => onBoundary(b, p));
  }
  if (
    separation > a.radius + b.radius + EPS ||
    separation < Math.abs(a.radius - b.radius) - EPS
  ) return [];
  const along = (a.radius ** 2 - b.radius ** 2 + separation ** 2) / (2 * separation);
  const square = a.radius ** 2 - along ** 2;
  const tolerance = 4 * EPS * a.radius + 32 * Number.EPSILON * a.radius ** 2;
  if (square < -tolerance) return [];
  const height = Math.abs(square) <= tolerance ? 0 : Math.sqrt(square);
  const ux = delta[0] / separation, uy = delta[1] / separation;
  return [-1, 1].flatMap((sign) => {
    const p: Point2 = [a.center[0] + along * ux - sign * height * uy,
      a.center[1] + along * uy + sign * height * ux];
    return onBoundary(a, p) && onBoundary(b, p) ? [p] : [];
  });
}

function sketchBoundary(start: unknown, segments: unknown): boolean {
  if (
    !point2(start) || !Array.isArray(segments) || segments.length < 1 ||
    segments.length > CAD_LIMITS.sketchSegments
  ) return false;
  const edges: Boundary[] = [];
  let from = start;
  for (const raw of segments) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return false;
    const segment = raw as Record<string, unknown>;
    if (!point2(segment.to) || distance(from, segment.to) <= EPS) return false;
    if (segment.type === "line" && exact(segment, ["type", "to"])) {
      edges.push({ type: "line", from, to: segment.to });
    } else if (
      segment.type === "arc" && exact(segment, ["type", "center", "to", "clockwise"]) &&
      point2(segment.center) && typeof segment.clockwise === "boolean"
    ) {
      const radius = distance(from, segment.center);
      if (
        radius <= EPS || radius > CAD_LIMITS.sizeMm ||
        Math.abs(radius - distance(segment.to, segment.center)) > EPS
      ) return false;
      const angle = Math.atan2(from[1] - segment.center[1], from[0] - segment.center[0]);
      const end = Math.atan2(segment.to[1] - segment.center[1], segment.to[0] - segment.center[0]);
      const sense = segment.clockwise ? -1 : 1;
      const sweep = sense * positiveAngle(sense * (end - angle));
      if (Math.abs(sweep) * radius <= EPS) return false;
      const edge: Boundary = { type: "arc", from, to: segment.to, center: segment.center, radius, angle, sweep };
      // Endpoints alone do not bound a major arc; include cardinal extrema.
      for (const [dx, dy] of [[1, 0], [0, 1], [-1, 0], [0, -1]]) {
        const p: Point2 = [segment.center[0] + radius * dx, segment.center[1] + radius * dy];
        if (onBoundary(edge, p) && !point2(p)) return false;
      }
      edges.push(edge);
    } else return false;
    from = segment.to;
  }
  if (distance(from, start) > EPS) edges.push({ type: "line", from, to: start });
  if (edges.length < 2 || edges.length > CAD_LIMITS.sketchSegments) return false;
  let twiceArea = 0, perimeter = 0;
  for (const [i, a] of edges.entries()) {
    if (a.type === "line") {
      twiceArea += cross2(difference(a.from, start), difference(a.to, start));
      perimeter += distance(a.from, a.to);
    } else {
      twiceArea += cross2(difference(a.center, start), difference(a.to, a.from)) +
        a.radius ** 2 * a.sweep;
      perimeter += a.radius * Math.abs(a.sweep);
    }
    for (let j = 0; j < i; j++) {
      const b = edges[j], hits = contacts(a, b);
      if (hits === null) return false;
      const adjacent = i === j + 1 || (j === 0 && i === edges.length - 1);
      // Adjacent edges may share endpoints, including both ends of a two-arc
      // circle. Any other crossing, tangent touch or overlap is rejected.
      if (hits.some((p) => !adjacent ||
        ![a.from, a.to].some((end) => distance(end, p) <= 4 * EPS) ||
        ![b.from, b.to].some((end) => distance(end, p) <= 4 * EPS))) return false;
    }
  }
  return Math.abs(twiceArea) > Math.max(2 * EPS, EPS * perimeter);
}

type ZBounds = [number, number];
function stepBounds(step: CadStep, get: (id: string) => ZBounds): ZBounds {
  switch (step.type) {
    case "sketch":
    case "sketch_circle": return [step.z, step.z];
    case "extrude_profile": return [step.z, step.z + step.height];
    case "extrude": return [get(step.inputs[0])[0], get(step.inputs[0])[0] + step.height];
    case "box": return [step.center[2], step.center[2] + step.size[2]];
    case "cylinder": return [step.center[2], step.center[2] + step.height];
    case "sphere": return [step.center[2] - step.radius, step.center[2] + step.radius];
    case "torus": return [step.center[2] - step.height, step.center[2] + step.height];
    case "union": {
      const a = get(step.inputs[0]), b = get(step.inputs[1]);
      return [Math.min(a[0], b[0]), Math.max(a[1], b[1])];
    }
    default: return get(step.inputs[0]);
  }
}

function filletEdges(value: unknown): value is string[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > CAD_LIMITS.filletEdges) return false;
  const seen = new Set<string>();
  for (const edge of value) {
    if (typeof edge !== "string" || !/^[a-z][a-z0-9_-]{0,31}\|[a-z][a-z0-9_-]{0,31}$/.test(edge)) return false;
    const faces = edge.split("|").sort(), key = faces.join("|");
    if (faces[0] === faces[1] || seen.has(key)) return false;
    seen.add(key);
  }
  return true;
}

/** Validate a small, closed CAD recipe before translating it to the native DSL. */
export function validateCadRecipe(value: unknown): CadRecipe {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return invalid();
  }
  const root = value as Record<string, unknown>;
  if (
    !exact(root, ["units", "steps"]) || root.units !== "mm" ||
    !Array.isArray(root.steps) || root.steps.length < 1 ||
    root.steps.length > CAD_LIMITS.steps
  ) return invalid();

  const ids = new Set<string>();
  const sketches = new Set<string>();
  const bounds = new Map<string, ZBounds>();
  let solidFeatures = 0;
  let patternHoles = 0;
  const steps: CadStep[] = [];
  for (const raw of root.steps) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) return invalid();
    const step = raw as Record<string, unknown>;
    if (
      !exact(step, step.type === "sketch_circle" ? [
        "type", "id", "title", "center", "radius", "z",
      ] : step.type === "circular_hole_pattern" ? [
        "type", "id", "title", "inputs", "center", "patternRadius", "radius", "count", "startAngle",
      ] : step.type === "sketch" ? [
        "type", "id", "title", "start", "segments", "z",
      ] : step.type === "extrude" ? [
        "type", "id", "title", "inputs", "height",
      ] : step.type === "pocket" ? [
        "type", "id", "title", "inputs", "depth",
      ] : step.type === "fillet" ? [
        "type", "id", "title", "inputs", "edges", "radius",
      ] : step.type === "extrude_profile" ? [
        "type", "id", "title", "outer", "height", "z",
      ] : [
        "type",
        "id",
        "title",
        "size",
        "center",
        "radius",
        "height",
        "inputs",
      ]) ||
      typeof step.id !== "string" || !/^[a-z][a-z0-9_-]{0,31}$/.test(step.id) ||
      step.id.startsWith("krly_cut_") ||
      step.id.startsWith("krly_sketch_") ||
      ids.has(step.id) || typeof step.title !== "string" ||
      step.title.trim().length < 1 ||
      step.title.length > CAD_LIMITS.titleChars ||
      /[\u0000-\u001f\u007f]/.test(step.title)
    ) return invalid();

    const references = (names: unknown, kinds: ("sketch" | "solid")[]): boolean =>
      Array.isArray(names) && names.length === kinds.length &&
      kinds.every((kind, index) => typeof names[index] === "string" &&
        ids.has(names[index]) && sketches.has(names[index]) === (kind === "sketch"));
    switch (step.type) {
      case "sketch_circle":
        if (!boundedCircle(step.center, step.radius) || !scalar(step.z) ||
          Math.abs(step.z) > CAD_LIMITS.coordinateMm) return invalid();
        sketches.add(step.id);
        steps.push(step as unknown as CadStep);
        break;
      case "circular_hole_pattern": {
        if (!references(step.inputs, ["solid"]) || !point2(step.center) ||
          !scalar(step.patternRadius, true) || step.patternRadius <= EPS ||
          !scalar(step.radius, true) || step.radius <= EPS || !scalar(step.startAngle) ||
          !scalar(step.count) || !Number.isInteger(step.count) || step.count < 1 ||
          step.count > CAD_LIMITS.patternHoles) return invalid();
        patternHoles += step.count;
        if (patternHoles > CAD_LIMITS.patternHoles) return invalid();
        const centers = circularHoleCenters(step as unknown as CadCircularHolePattern);
        for (const [index, center] of centers.entries()) {
          if (!boundedCircle(center, step.radius) || centers.slice(0, index).some((other) =>
            distance(center, other) <= 2 * (step.radius as number) + EPS)) return invalid();
        }
        steps.push(step as unknown as CadStep);
        break;
      }
      case "sketch":
        if (
          !sketchBoundary(step.start, step.segments) || !scalar(step.z) ||
          Math.abs(step.z) > CAD_LIMITS.coordinateMm
        ) return invalid();
        sketches.add(step.id);
        steps.push(step as unknown as CadStep);
        break;
      case "extrude":
        if (!references(step.inputs, ["sketch"]) || !scalar(step.height, true)) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "pocket": {
        if (!references(step.inputs, ["solid", "sketch"]) ||
          (step.depth !== null && !scalar(step.depth, true))) return invalid();
        const [solid, sketch] = step.inputs as [string, string];
        const [bottom, top] = bounds.get(solid)!;
        if (Math.abs(bounds.get(sketch)![0] - top) > EPS ||
          (step.depth !== null && (step.depth as number) > top - bottom)) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      }
      case "fillet":
        if (!references(step.inputs, ["solid"]) || !scalar(step.radius, true) ||
          !filletEdges(step.edges)) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "extrude_profile":
        if (
          !polygon(step.outer) || !scalar(step.height, true) ||
          !scalar(step.z) || Math.abs(step.z) > CAD_LIMITS.coordinateMm
        ) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "through_hole":
        if (
          step.size !== null ||
          !tuple(step.center, CAD_LIMITS.coordinateMm, false) ||
          step.center[2] !== 0 || !scalar(step.radius, true) ||
          step.height !== null || !references(step.inputs, ["solid"])
        ) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "box":
        if (
          !tuple(step.size, CAD_LIMITS.sizeMm, true) ||
          !tuple(step.center, CAD_LIMITS.coordinateMm, false) ||
          step.radius !== null || step.height !== null || step.inputs !== null
        ) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "cylinder":
      case "torus":
        if (
          step.size !== null ||
          !tuple(step.center, CAD_LIMITS.coordinateMm, false) ||
          !scalar(step.radius, true) || !scalar(step.height, true) ||
          step.inputs !== null
        ) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "sphere":
        if (
          step.size !== null ||
          !tuple(step.center, CAD_LIMITS.coordinateMm, false) ||
          !scalar(step.radius, true) || step.height !== null ||
          step.inputs !== null
        ) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      case "union":
      case "subtract":
        if (
          step.size !== null || step.center !== null || step.radius !== null ||
          step.height !== null || !references(step.inputs, ["solid", "solid"])
        ) return invalid();
        steps.push(step as unknown as CadStep);
        break;
      default:
        return invalid();
    }
    if (
      step.type === "torus" &&
      (step.height as number) >= (step.radius as number)
    ) return invalid();
    if (!sketches.has(step.id) && ++solidFeatures > CAD_LIMITS.solidFeatures) return invalid();
    const z = stepBounds(steps.at(-1)!, (id) => bounds.get(id) ?? invalid());
    if (!z.every(Number.isFinite) || z[0] > z[1]) return invalid();
    bounds.set(step.id, z);
    ids.add(step.id);
  }
  if (sketches.has(steps.at(-1)!.id)) return invalid();
  // No unused/disconnected features: the last node is the replayable result.
  const byId = new Map(steps.map((step) => [step.id, step]));
  const used = new Set<string>();
  const visit = (id: string) => {
    if (used.has(id)) return;
    used.add(id);
    const step = byId.get(id)!;
    if ("inputs" in step && step.inputs) step.inputs.forEach(visit);
  };
  visit(steps.at(-1)!.id);
  if (used.size !== steps.length) return invalid();
  return structuredClone({ units: "mm", steps });
}

function number(value: number): string {
  return Object.is(value, -0) ? "0" : String(value);
}

function sketchPath(step: Extract<CadStep, { type: "sketch" }>): string {
  const pair = (p: Point2) => p.map(number).join(",");
  return [`M ${pair(step.start)}`, ...step.segments.map((segment) =>
    segment.type === "line" ? `L ${pair(segment.to)}` :
      `A ${pair(segment.center)} ${pair(segment.to)} ${segment.clockwise ? "cw" : "ccw"}`
  ), "Z"].join(" ");
}

function sketchLine(id: string, step: CadSketch, z = step.z): string {
  return step.type === "sketch_circle"
    ? `${id} = circle plane=xy at=${number(step.center[0])},${number(step.center[1])},${number(z)} r=${number(step.radius)}`
    : `${id} = profile plane=xy at=0,0,${number(z)} path="${sketchPath(step)}"`;
}

/** Emit only the bounded geometry-only K3D syntax understood by the kernel. */
export function encodeCadRecipe(recipe: CadRecipe): Uint8Array {
  const lines = ["k3d 1", "layer geo"];
  // Conservative Z bounds follow ancestry, including in incomplete prefixes.
  // Box/cylinder centers are bases; sphere/torus centers are midplanes.
  const bounds = new Map<string, [number, number]>();
  const sketches = new Map<string, CadSketch>();
  const targetBounds = (id: string): [number, number] => bounds.get(id) ?? invalid();
  for (const [stepIndex, step] of recipe.steps.entries()) {
    const z = stepBounds(step, targetBounds);
    if (step.type === "sketch" || step.type === "sketch_circle") {
      sketches.set(step.id, step);
      lines.push(sketchLine(step.id, step));
    } else if (step.type === "extrude") {
      lines.push(`${step.id} = extrude ${step.inputs[0]} h=${number(step.height)}`);
    } else if (step.type === "pocket") {
      const sketch = sketches.get(step.inputs[1]) ?? invalid();
      const margin = Math.max(0.1, (z[1] - z[0]) * 0.01);
      const depth = step.depth ?? (z[1] - z[0] + margin);
      const cutter = `krly_cut_${step.id}`, lifted = `krly_sketch_${step.id}`;
      // Negative native extrusion keeps an exact blind floor at top-depth;
      // through-all additionally crosses the lower bound by the same margin.
      lines.push(
        sketchLine(lifted, sketch, z[1] + margin),
        `${cutter} = extrude ${lifted} h=${number(-(depth + margin))}`,
        `${step.id} = subtract ${step.inputs[0]} ${cutter}`,
      );
    } else if (step.type === "fillet") {
      // K3D needs a comma even for one item to decode this as Kind::Texts.
      lines.push(`${step.id} = fillet ${step.inputs[0]} edges=${step.edges.join(",")}, r=${number(step.radius)}`);
    } else if (step.type === "extrude_profile") {
      const sketch = `krly_sketch_${step.id}`;
      const path = step.outer.map((point, index) =>
        `${index === 0 ? "M" : "L"} ${point.map(number).join(",")}`
      ).join(" ");
      lines.push(
        `${sketch} = profile plane=xy at=0,0,${number(step.z)} path="${path} Z"`,
        `${step.id} = extrude ${sketch} h=${number(step.height)}`,
      );
    } else if (step.type === "box") {
      lines.push(
        `${step.id} = box at=${step.center.map(number).join(",")} w=${
          number(step.size[0])
        } d=${number(step.size[1])} h=${number(step.size[2])}`,
      );
    } else if (step.type === "cylinder") {
      lines.push(
        `${step.id} = cylinder at=${step.center.map(number).join(",")} h=${
          number(step.height)
        } r=${number(step.radius)}`,
      );
    } else if (step.type === "sphere") {
      lines.push(
        `${step.id} = sphere at=${step.center.map(number).join(",")} r=${
          number(step.radius)
        }`,
      );
    } else if (step.type === "torus") {
      lines.push(
        `${step.id} = torus at=${step.center.map(number).join(",")} ring=${
          number(step.radius)
        } tube=${number(step.height)}`,
      );
    } else if (step.type === "circular_hole_pattern") {
      const span = z[1] - z[0];
      const margin = Math.max(0.1, span * 0.01);
      let host = step.inputs[0];
      for (const [index, center] of circularHoleCenters(step).entries()) {
        // A numeric namespace cannot collide with legacy krly_cut_<user-id>
        // cutters: user ids must start with a letter. The recipe index also
        // separates patterns whose public ids happen to end in numeric suffixes.
        const cutter = `krly_cut_${stepIndex}_${step.id}_${index}`;
        const result = index === step.count - 1 ? step.id : `${cutter}_result`;
        lines.push(
          `${cutter} = cylinder at=${number(center[0])},${number(center[1])},${number(z[0] - margin)} h=${number(span + 2 * margin)} r=${number(step.radius)}`,
          `${result} = subtract ${host} ${cutter}`,
        );
        host = result;
      }
    } else if (step.type === "through_hole") {
      const span = z[1] - z[0];
      const margin = Math.max(0.1, span * 0.01);
      const cutter = `krly_cut_${step.id}`;
      lines.push(
        `${cutter} = cylinder at=${number(step.center[0])},${number(step.center[1])},${number(z[0] - margin)} h=${number(span + 2 * margin)} r=${number(step.radius)}`,
        `${step.id} = subtract ${step.inputs[0]} ${cutter}`,
      );
    } else {
      lines.push(`${step.id} = ${step.type} ${step.inputs.join(" ")}`);
    }
    if (!z.every(Number.isFinite) || z[0] > z[1]) return invalid();
    bounds.set(step.id, z);
  }
  const text = `${lines.join("\n")}\n`;
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > CAD_LIMITS.recipeBytes) return invalid();
  return bytes;
}
