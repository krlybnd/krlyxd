import type { CadFeature, CadSketch, Construction } from '../../gen/krly/studio/v1/studio_pb.ts';
import type { ModelSnapshot } from './model';

export type CadPlaybackStage = Pick<ModelSnapshot, 'key' | 'name' | 'phase' | 'meshes' | 'construction'> & { featureId?: string; sketchFeatureId?: string };
const assert = (value: unknown, message: string): void => { if (!value) throw new Error(`Érvénytelen CAD modellfa: ${message}`); };
const finite = (value: number) => Number.isFinite(value) && Math.abs(value) <= 10_000;
const pair = (value: number[]) => value.length === 2 && value.every(finite);
export const featureLabels: Record<string, string> = { sketch: 'Vázlat', extrude: 'Kihúzás', pocket: 'Zseb', fillet: 'Lekerekítés', box: 'Doboz', cylinder: 'Henger', sphere: 'Gömb', torus: 'Tórusz', union: 'Egyesítés', subtract: 'Kivágás', through_hole: 'Átmenő furat', circular_hole_pattern: 'Körkiosztás', extrude_profile: 'Vázlat kihúzása' };

export function validateCadConstruction(data: Construction): void {
  assert(data.version === 2 && data.units === 'mm' && data.coordinateSystem === 'right_handed_z_up', 'verzió / koordinátarendszer');
  assert(data.features.length > 0 && data.features.length <= 16, 'legfeljebb 16 művelet');
  const seen = new Set<string>(); let solids = 0;
  const solidIds = new Set<string>();
  for (const feature of data.features) {
    assert(/^[a-z][a-z0-9_-]{0,31}$/.test(feature.id) && !seen.has(feature.id), 'azonosító');
    assert(feature.title.length > 0 && feature.title.length <= 80 && Object.hasOwn(featureLabels, feature.kind), 'művelet');
    assert(feature.inputs.length <= 2 && feature.inputs.every(input => seen.has(input)), 'előzmény');
    seen.add(feature.id);
    if (feature.kind !== 'sketch') assert(++solids <= 8, 'legfeljebb 8 test');
    assert(feature.parameters.length <= 12 && feature.selections.length <= 32 && feature.selections.every(value => value.length <= 120), 'paraméterkorlát');
    for (const parameter of feature.parameters) assert(parameter.name.length <= 40 && parameter.unit.length <= 16 && parameter.text.length <= 80 && parameter.values.length <= 3 && parameter.values.every(feature.kind === 'circular_hole_pattern' && parameter.name === 'startAngle' ? Number.isFinite : finite), 'méretadat');
    if (feature.kind === 'circular_hole_pattern') {
      assert(feature.inputs.length === 1 && solidIds.has(feature.inputs[0]) && !feature.sketch, 'a körkiosztás előzménye natív test');
      assert(feature.parameters.length === 5, 'körkiosztás paraméterei');
      for (const [name, length, unit] of [['center', 2, 'mm'], ['radius', 1, 'mm'], ['patternRadius', 1, 'mm'], ['count', 1, ''], ['startAngle', 1, 'deg']] as const) {
        const matches = feature.parameters.filter(parameter => parameter.name === name);
        assert(matches.length === 1, 'hiányzó vagy ismételt körkiosztás-paraméter');
        const parameter = matches[0];
        assert(parameter.values.length === length && parameter.unit === unit && parameter.text === '', 'körkiosztás méretadata / mértékegysége');
        if (name === 'radius' || name === 'patternRadius') assert(parameter.values[0] > 0, 'körkiosztás sugara');
        if (name === 'count') assert(Number.isInteger(parameter.values[0]) && parameter.values[0] >= 1 && parameter.values[0] <= 16, 'körkiosztás darabszáma');
      }
    }
    if (feature.kind !== 'sketch') solidIds.add(feature.id);
    if (feature.kind === 'sketch') assert(Boolean(feature.sketch), 'hiányzó vázlat');
    if (feature.sketch) {
      const sketch = feature.sketch;
      assert(sketch.plane === 'XY' && finite(sketch.offset) && pair(sketch.start) && sketch.curves.length > 0 && sketch.curves.length <= 64, 'vázlatsík / görbekorlát');
      let from = sketch.start;
      for (const curve of sketch.curves) {
        assert(pair(curve.to) && (curve.kind === 'line' || curve.kind === 'arc'), 'görbe');
        if (curve.kind === 'arc') {
          assert(pair(curve.center), 'ívközéppont');
          const a = Math.hypot(from[0] - curve.center[0], from[1] - curve.center[1]);
          const b = Math.hypot(curve.to[0] - curve.center[0], curve.to[1] - curve.center[1]);
          assert(a > 0 && Math.abs(a - b) <= Math.max(1e-7, a * 1e-7) && Math.hypot(from[0] - curve.to[0], from[1] - curve.to[1]) > 0, 'ív sugara / végpontja');
        }
        from = curve.to;
      }
    }
  }
  assert(data.features.at(-1)?.id === data.activeFeatureId && data.features.at(-1)?.kind !== 'sketch', 'aktív natív test');
}

export function arcSweep(from: number[], to: number[], center: number[], clockwise: boolean): number {
  const start = Math.atan2(from[1] - center[1], from[0] - center[0]);
  const end = Math.atan2(to[1] - center[1], to[0] - center[0]);
  let sweep = end - start;
  if (clockwise && sweep >= 0) sweep -= Math.PI * 2;
  if (!clockwise && sweep <= 0) sweep += Math.PI * 2;
  return sweep;
}

/** Bounded display approximation only: no triangulation, fill, extrusion or solid changes. */
export function sketchDisplayPoints(sketch: CadSketch): [number, number, number][] {
  const points: [number, number, number][] = [[sketch.start[0], sketch.start[1], sketch.offset]];
  let from = sketch.start;
  for (const curve of sketch.curves) {
    if (curve.kind === 'arc') {
      const radius = Math.hypot(from[0] - curve.center[0], from[1] - curve.center[1]);
      const start = Math.atan2(from[1] - curve.center[1], from[0] - curve.center[0]);
      const sweep = arcSweep(from, curve.to, curve.center, curve.clockwise);
      const segments = Math.min(128, Math.max(2, Math.ceil(Math.abs(sweep) / (Math.PI / 64))));
      for (let i = 1; i < segments; i++) {
        const angle = start + sweep * i / segments;
        points.push([curve.center[0] + radius * Math.cos(angle), curve.center[1] + radius * Math.sin(angle), sketch.offset]);
      }
    }
    points.push([curve.to[0], curve.to[1], sketch.offset]);
    from = curve.to;
  }
  if (from[0] !== sketch.start[0] || from[1] !== sketch.start[1]) points.push([...points[0]]);
  return points;
}

export function expandCadStages(stages: CadPlaybackStage[]): CadPlaybackStage[] {
  const expanded: CadPlaybackStage[] = [];
  const seen = new Set<string>();
  let previousMeshes: CadPlaybackStage['meshes'] = [];
  let previousFeatures: CadFeature[] = [];
  for (const stage of stages) {
    const data = stage.construction;
    if (data?.version !== 2) { expanded.push(stage); previousMeshes = stage.meshes; continue; }
    validateCadConstruction(data);
    assert(previousFeatures.every((feature, index) => JSON.stringify(feature) === JSON.stringify(data.features[index])), 'a korábbi műveletek megváltoztak');
    for (const feature of data.features) {
      if (feature.kind === 'sketch' && !seen.has(feature.id)) expanded.push({ ...stage, key: `${stage.key}:sketch:${feature.id}`, name: `Vázlat · ${feature.title}`, phase: 'sketch_profile', featureId: feature.id, sketchFeatureId: feature.id, meshes: previousMeshes });
      seen.add(feature.id);
    }
    expanded.push({ ...stage, featureId: data.activeFeatureId });
    previousFeatures = data.features;
    previousMeshes = stage.meshes;
  }
  return expanded;
}
