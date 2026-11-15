import type { CadRecipe } from "./ai/cad-recipe.ts";
import { buildCad, type NativeEvent } from "./native.ts";

export type CadFailure = {
  code: "boolean_contact" | "unsupported_geometry" | "verification";
  /** Fixed, bounded text suitable for a repair prompt; never native diagnostics. */
  message: string;
};

export type CadVerificationReport = {
  units: "mm";
  bounds: { min: [number, number, number]; max: [number, number, number] };
  /** Native numerical measurements, in mm³ and mm² respectively. */
  volume: number;
  area: number;
  openEdges: 0;
  stageCount: number;
  designIntent: "unknown";
  standards: "unknown";
  exactProof: false;
};

export type CadAttemptResult =
  | { ok: true; events: NativeEvent[]; report: CadVerificationReport }
  | { ok: false; failure: CadFailure };

export const CAD_EXECUTION_LIMITS = Object.freeze({
  meshStages: 8,
  events: 32,
  approximateBytes: 64 * 1024 * 1024,
});

const FAILURE_MESSAGES: Record<CadFailure["code"], string> = {
  boolean_contact:
    "The native boolean operation rejected coincident edges or tangential contact. Adjust the recipe geometry to avoid ambiguous contact.",
  unsupported_geometry:
    "The native geometry kernel cannot construct or mesh this geometry. Use supported geometry operations in the recipe.",
  verification:
    "The native CAD geometry failed numerical mesh verification. A finite, positive solid in millimetres with zero open edges is required.",
};

function failure(code: CadFailure["code"]): CadAttemptResult {
  return { ok: false, failure: { code, message: FAILURE_MESSAGES[code] } };
}

/** Only the adapter's known native diagnostic envelopes are repair candidates.
 * Unsupported protocol, resource exhaustion, and arbitrary exception text are
 * not geometry failures. Do not copy any part of a diagnostic into the result.
 */
function geometryFailure(error: unknown): CadFailure["code"] | undefined {
  if (
    !(error instanceof Error) || error.name === "AbortError" ||
    error.name === "TimeoutError" || error.message.length > 20 * 1024
  ) {
    return undefined;
  }
  if (error.message === "A text CAD feature did not produce a closed solid") {
    return "verification";
  }
  let code: unknown;
  let message: unknown;
  const prefix = "Native text CAD mesh did not complete: ";
  if (error.message.startsWith(prefix)) {
    try {
      const body = JSON.parse(error.message.slice(prefix.length));
      if (body?.kind !== "failed") return undefined;
      code = body.diagnostic?.code;
      message = body.diagnostic?.message;
    } catch {
      return undefined;
    }
  } else {
    // The mesh CLI reports model-construction failures before emitting events.
    const match =
      /^Native mesh exited 1: krly3d-mesh: (unsupported|geometry|topology): ([\s\S]+)$/
        .exec(error.message);
    if (!match) return undefined;
    [, code, message] = match;
  }
  if (
    typeof message !== "string" ||
    !["unsupported", "geometry", "topology"].includes(String(code))
  ) {
    return undefined;
  }
  // Native "unsupported" also covers capacity and missing-plugin failures.
  if (
    /\b(?:limit\w*|exceed\w*|too (?:many|large)|overflow\w*|exhaust\w*|range|allocat\w*|memory|budget\w*|timeout\w*|timed out|deadline\w*|cancel\w*|interrupt\w*|protocol\w*|api|version\w*|registered|plugins?|io|permissions?|files?|paths?|director(?:y|ies)|spawn\w*|binar(?:y|ies))\b/i
      .test(message)
  ) {
    return undefined;
  }
  if (
    message.includes("edges of the two solids run along each other") ||
    message.includes("the solids touch tangentially")
  ) {
    return "boolean_contact";
  }
  return code === "unsupported" ? "unsupported_geometry" : undefined;
}

function protocol(condition: unknown): asserts condition {
  if (!condition) throw new TypeError("Invalid native CAD event sequence");
}

function limit(condition: unknown): asserts condition {
  if (!condition) {
    throw new RangeError("Native CAD attempt exceeds its resource limit");
  }
}

/** Counts numerical payloads and artifact bytes without copying arrays. This is
 * an approximate retained-payload budget, not a JavaScript heap quota. Native
 * transport separately bounds strings, nesting, and individual mesh sizes.
 */
function numericBytes(value: unknown): number {
  if (typeof value === "number") return 8;
  if (value === null || typeof value !== "object") return 0;
  if (value instanceof Uint8Array) return value.byteLength;
  let bytes = 0;
  for (const key in value) {
    if (!Object.hasOwn(value, key)) continue;
    bytes += numericBytes((value as Record<string, unknown>)[key]);
    limit(bytes <= CAD_EXECUTION_LIMITS.approximateBytes);
  }
  return bytes;
}

function checkMesh(
  mesh: NonNullable<NativeEvent["mesh"]>,
): CadVerificationReport["bounds"] | undefined {
  const { positions, indices } = mesh;
  if (
    mesh.units !== "mm" || !Number.isFinite(mesh.volume) || mesh.volume <= 0 ||
    !Number.isFinite(mesh.area) || mesh.area <= 0 || mesh.openEdges !== 0 ||
    !Array.isArray(positions) || !Array.isArray(indices) ||
    positions.length < 12 || positions.length % 3 !== 0 ||
    indices.length < 12 || indices.length % 3 !== 0
  ) return undefined;
  const min: [number, number, number] = [Infinity, Infinity, Infinity];
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i++) {
    const value = positions[i];
    if (!Number.isFinite(value)) return undefined;
    const axis = i % 3;
    min[axis] = Math.min(min[axis], value);
    max[axis] = Math.max(max[axis], value);
  }
  if (
    !min.every((value, axis) =>
      max[axis] > value && Number.isFinite(max[axis] - value)
    )
  ) return undefined;
  for (const index of indices) {
    if (
      !Number.isSafeInteger(index) || index < 0 || index >= positions.length / 3
    ) {
      return undefined;
    }
  }
  return { min, max };
}

/** Atomic publication boundary: a preview never escapes a failed attempt.
 * Consume past completed to observe generator cleanup/process-exit failures.
 * Reports check native numerical mesh data, not intent, standards, or exactness.
 */
export async function executeCadAttempt(
  recipe: CadRecipe,
  signal: AbortSignal,
  build: typeof buildCad = buildCad,
): Promise<CadAttemptResult> {
  signal.throwIfAborted();
  const events: NativeEvent[] = [];
  let bytes = 0;
  let stageCount = 0;
  let completed = false;
  let finalMesh: NativeEvent["mesh"];
  let bounds: CadVerificationReport["bounds"] | undefined;
  let verified = true;
  try {
    for await (const event of build(recipe, signal)) {
      signal.throwIfAborted();
      limit(events.length < CAD_EXECUTION_LIMITS.events);
      // Account before verification: capacity failures must never invite repair.
      if (event.mesh) {
        limit(++stageCount <= CAD_EXECUTION_LIMITS.meshStages);
        const meshBytes = 8 *
          (event.mesh.positions.length + event.mesh.indices.length + 3);
        limit(bytes + meshBytes <= CAD_EXECUTION_LIMITS.approximateBytes);
        bytes += meshBytes;
      }
      bytes += numericBytes(event.details) + numericBytes(event.construction) +
        (event.artifact?.bytes.byteLength ?? 0);
      limit(bytes <= CAD_EXECUTION_LIMITS.approximateBytes);
      protocol(!completed);
      protocol(event.kind !== "failed" && event.kind !== "cancelled");
      if (event.mesh) {
        protocol(finalMesh === undefined && event.kind !== "completed");
        const meshBounds = checkMesh(event.mesh);
        verified &&= meshBounds !== undefined;
        if (event.kind === "editable_proposal") {
          finalMesh = event.mesh;
          bounds = meshBounds;
        }
      }
      if (event.kind === "editable_proposal") {
        protocol(event.mesh !== undefined);
      }
      // Artifact publication is a distinct event after the final mesh.
      if (event.artifact || event.kind === "artifact") {
        protocol(finalMesh !== undefined && !event.mesh && !!event.artifact);
      }
      if (event.kind === "completed") {
        protocol(finalMesh !== undefined);
        completed = true;
      }
      events.push(event);
    }
    signal.throwIfAborted();
    protocol(completed && finalMesh !== undefined);
    if (!verified || !bounds) return failure("verification");
    return {
      ok: true,
      events,
      report: {
        units: "mm",
        bounds,
        volume: finalMesh.volume,
        area: finalMesh.area,
        openEdges: 0,
        stageCount,
        designIntent: "unknown",
        standards: "unknown",
        exactProof: false,
      },
    };
  } catch (error) {
    // Cancellation wins even if cleanup fails with a recognizable geometry error.
    signal.throwIfAborted();
    const code = geometryFailure(error);
    if (code) return failure(code);
    throw error;
  }
}
