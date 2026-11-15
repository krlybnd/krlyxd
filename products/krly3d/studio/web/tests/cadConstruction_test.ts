import { create, equals, fromBinary, fromJson, toBinary, toJson } from '@bufbuild/protobuf';
import { ConstructionSchema } from '../../gen/krly/studio/v1/studio_pb.ts';
import { buildCadConstruction } from '../../service/native_construction.ts';
import { validateCadRecipe } from '../../service/ai/cad-recipe.ts';
import { arcSweep, expandCadStages, featureLabels, sketchDisplayPoints, validateCadConstruction } from '../src/cadConstruction.ts';
import { prepareModel } from '../src/model.ts';
import { RevealTimeline } from '../src/reveal.ts';
import { construction, mesh, recipe } from './cadFixtures.ts';
const assert = (value: unknown) => { if (!value) throw new Error('Assertion failed'); };
const equal = (a: unknown, b: unknown) => assert(JSON.stringify(a) === JSON.stringify(b));
const rejects = (fn: () => unknown) => { let failed = false; try { fn(); } catch { failed = true; } assert(failed); };
const circleRecipe = () => ({ units: 'mm', steps: [
  { type: 'sketch_circle', id: 'circle', title: 'Karima körvázlata', center: [3, -2], radius: 25, z: 7.5 },
  { type: 'extrude', id: 'base', title: 'Karimatest', inputs: ['circle'], height: 12 },
  { type: 'sketch_circle', id: 'bore', title: 'Központi furat vázlata', center: [3, -2], radius: 4, z: 19.5 },
  { type: 'pocket', id: 'pocket', title: 'Központi furat', inputs: ['base', 'bore'], depth: null },
  { type: 'circular_hole_pattern', id: 'pattern', title: 'Csavarfuratok', inputs: ['pocket'], center: [3, -2], patternRadius: 16, radius: 2, count: 6, startAngle: 30 },
] });

Deno.test('circle and hole-pattern v2 metadata round-trip through protobuf binary and JSON without losing analytic parameters', () => {
  for (const [count, startAngle] of [[1, 0], [6, 30], [16, -450], [6, 720000], [6, Number.MAX_VALUE]]) {
    const input = circleRecipe(); input.steps[4].count = count; input.steps[4].startAngle = startAngle;
    const original = JSON.stringify(input);
    const data = create(ConstructionSchema, buildCadConstruction(validateCadRecipe(input), 4));
    const binary = fromBinary(ConstructionSchema, toBinary(ConstructionSchema, data));
    const json = fromJson(ConstructionSchema, toJson(ConstructionSchema, data));
    for (const restored of [binary, json]) {
      validateCadConstruction(restored);
      assert(equals(ConstructionSchema, restored, data));
      equal(restored.features.map(feature => feature.kind), ['sketch', 'extrude', 'sketch', 'pocket', 'circular_hole_pattern']);
      equal(restored.activeFeatureId, 'pattern');
      equal(restored.features[0].parameters.map(({ name, values, unit, text }) => ({ name, values, unit, text })), [
        { name: 'center', values: [3, -2], unit: 'mm', text: '' },
        { name: 'radius', values: [25], unit: 'mm', text: '' },
        { name: 'z', values: [7.5], unit: 'mm', text: '' },
      ]);
      const sketch = restored.features[0].sketch!;
      equal(sketch.start, [28, -2]); equal(sketch.offset, 7.5);
      equal(sketch.curves.map(curve => [curve.kind, curve.to, curve.center, curve.clockwise]), [
        ['arc', [-22, -2], [3, -2], false], ['arc', [28, -2], [3, -2], false],
      ]);
      equal(arcSweep(sketch.start, sketch.curves[0].to, sketch.curves[0].center, false), Math.PI);
      equal(arcSweep(sketch.curves[0].to, sketch.start, sketch.curves[1].center, false), Math.PI);
      const points = sketchDisplayPoints(sketch);
      equal(points[0], points.at(-1));
      assert(points.every(([x, y, z]) => Math.abs(Math.hypot(x - 3, y + 2) - 25) < 1e-10 && z === 7.5));
      const pattern = restored.features[4];
      equal(pattern.inputs, ['pocket']); assert(!pattern.sketch);
      equal(pattern.parameters.map(({ name, values, unit, text }) => ({ name, values, unit, text })), [
        { name: 'center', values: [3, -2], unit: 'mm', text: '' },
        { name: 'radius', values: [2], unit: 'mm', text: '' },
        { name: 'patternRadius', values: [16], unit: 'mm', text: '' },
        { name: 'count', values: [count], unit: '', text: '' },
        { name: 'startAngle', values: [startAngle], unit: 'deg', text: '' },
      ]);
    }
    equal(JSON.stringify(input), original);
  }
  equal(featureLabels.circular_hole_pattern, 'Körkiosztás');
});

Deno.test('circle timeline stays a distinct sketch and retains the supplied solid snapshots through the pattern stage', () => {
  const input = circleRecipe();
  const stages = [1, 3, 4].map((index, i) => prepareModel(mesh(i + 1), 'test', String(index), { key: String(index), phase: index === 4 ? 'editable_proposal' : 'construction_step' }, create(ConstructionSchema, buildCadConstruction(input, index))));
  const original = JSON.stringify(stages);
  const expanded = expandCadStages(stages);
  equal(expanded.map(stage => stage.featureId), ['circle', 'base', 'bore', 'pocket', 'pattern']);
  equal(expanded.filter(stage => stage.sketchFeatureId).map(stage => [stage.sketchFeatureId, stage.phase]), [['circle', 'sketch_profile'], ['bore', 'sketch_profile']]);
  equal(expanded[0].meshes, []);
  assert(expanded[2].meshes === stages[0].meshes);
  for (const [expandedIndex, sourceIndex] of [[1, 0], [3, 1], [4, 2]]) {
    assert(expanded[expandedIndex].meshes === stages[sourceIndex].meshes);
    assert(expanded[expandedIndex].meshes[0].positions === stages[sourceIndex].meshes[0].positions);
    assert(expanded[expandedIndex].meshes[0].indices === stages[sourceIndex].meshes[0].indices);
  }
  equal(JSON.stringify(stages), original);
  rejects(() => buildCadConstruction(input, 0));
  rejects(() => buildCadConstruction(input, 2));
});

Deno.test('circle and pattern metadata reject invalid dimensions, counts, units and non-solid pattern inputs', () => {
  for (const changes of [{ radius: 0 }, { radius: -1 }, { center: [0, 0, 0] }, { center: [9999, 0] }, { z: NaN }, { inputs: ['base'] }]) {
    const input = circleRecipe(); Object.assign(input.steps[0], changes);
    rejects(() => buildCadConstruction(input, 1));
  }
  for (const changes of [{ count: 0 }, { count: 17 }, { count: 1.5 }, { count: NaN }, { radius: 0 }, { patternRadius: -1 }, { center: [0, 0, 0] }, { startAngle: Infinity }, { inputs: ['circle'] }, { inputs: ['pattern'] }, { inputs: [] }]) {
    const input = circleRecipe(); Object.assign(input.steps[4], changes);
    rejects(() => buildCadConstruction(input, 4));
  }
  const valid = create(ConstructionSchema, buildCadConstruction(circleRecipe(), 4));
  for (const count of [0, 17, 1.5]) {
    const data = structuredClone(valid); data.features[4].parameters.find(parameter => parameter.name === 'count')!.values = [count];
    rejects(() => validateCadConstruction(data));
  }
  for (const changes of [{ name: 'count', unit: 'mm' }, { name: 'center', values: [0, 0, 0] }, { name: 'radius', values: [0] }, { name: 'patternRadius', values: [-1] }, { name: 'startAngle', unit: 'mm' }]) {
    const data = structuredClone(valid); Object.assign(data.features[4].parameters.find(parameter => parameter.name === changes.name)!, changes);
    rejects(() => validateCadConstruction(data));
  }
  const missing = structuredClone(valid); missing.features[4].parameters.pop();
  rejects(() => validateCadConstruction(missing));
  const duplicate = structuredClone(valid); duplicate.features[4].parameters[4] = duplicate.features[4].parameters[0];
  rejects(() => validateCadConstruction(duplicate));
  const sketchInput = structuredClone(valid); sketchInput.features[4].inputs = ['circle'];
  rejects(() => validateCadConstruction(sketchInput));
  const fakeSketch = structuredClone(valid); fakeSketch.features[4].sketch = fakeSketch.features[0].sketch;
  rejects(() => validateCadConstruction(fakeSketch));
});

Deno.test('circle sketches do not consume any of the eight native solid slots', () => {
  const input = { units: 'mm', steps: Array.from({ length: 8 }, (_, index) => [
    { type: 'sketch_circle', id: `circle${index}`, title: 'Kör', center: [0, 0], radius: 2, z: 0 },
    { type: 'extrude', id: `solid${index}`, title: 'Test', inputs: [`circle${index}`], height: 1 },
  ]).flat() };
  const data = create(ConstructionSchema, buildCadConstruction(input, 15));
  validateCadConstruction(data);
  equal(data.features.length, 16);
  equal(data.features.filter(feature => feature.kind === 'sketch').length, 8);
  equal(data.activeFeatureId, 'solid7');
});

Deno.test('CAD feature IDs match the recipe contract, including hyphens and the 32-character limit', () => {
  for (const featureId of ['outline-1', 'a' + 'b'.repeat(31)]) {
    const input = structuredClone(recipe);
    input.steps[0].id = featureId;
    input.steps[1].inputs = [featureId];
    const data = create(ConstructionSchema, buildCadConstruction(input, 1));
    validateCadConstruction(data);
    equal(data.features[0].id, featureId);
    equal(data.features[1].inputs, [featureId]);
    const stage = prepareModel(mesh(), 'test', 'Base', { key: 'base', phase: 'construction_step' }, data);
    equal(expandCadStages([stage]).map(item => item.featureId), [featureId, 'base']);
  }
  for (const featureId of ['Uppercase', '1outline', 'with space', 'a' + 'b'.repeat(32)]) {
    const input = structuredClone(recipe);
    input.steps[0].id = featureId;
    input.steps[1].inputs = [featureId];
    rejects(() => buildCadConstruction(input, 1));
    const data = construction(1);
    data.features[0].id = featureId;
    data.features[1].inputs = [featureId];
    rejects(() => validateCadConstruction(data));
  }
});

Deno.test('CAD metadata preserves analytic curves and offset; display approximation never mutates native solids', () => {
  const data = construction(4); validateCadConstruction(data);
  equal(data.features.map(f => f.kind), ['sketch', 'extrude', 'sketch', 'pocket', 'fillet']);
  equal(data.activeFeatureId, 'rounding');
  const analytic = JSON.stringify(data);
  const points = sketchDisplayPoints(data.features[0].sketch!);
  assert(points.length > 3 && points.length <= 64 * 128 + 2);
  assert(points.every(point => point[2] === 7.5));
  assert(points.every(point => point[1] >= -1e-10));
  assert(points.some(point => Math.abs(point[1] - 5) < 1e-10));
  equal(points[0], points.at(-1));
  equal(JSON.stringify(data), analytic);
  const clockwise = structuredClone(data.features[0].sketch!); clockwise.curves[1].clockwise = true;
  assert(sketchDisplayPoints(clockwise).some(point => Math.abs(point[1] + 5) < 1e-10));
  const native = mesh(); const original = JSON.stringify(native);
  const snapshot = prepareModel(native, 'test', 'Final', { key: 'solid', phase: 'editable_proposal' }, data);
  equal(JSON.stringify(snapshot.mesh), original);
  equal(snapshot.meshes[0].positions, native.positions);
  equal(snapshot.meshes[0].indices, native.indices);
});

Deno.test('CAD timeline inserts profiles in recipe order using preceding real solids, never fake sketch meshes', () => {
  const stages = [1, 3, 4].map((index, i) => prepareModel(mesh(1 + i), 'test', String(index), { key: String(index), phase: index === 4 ? 'editable_proposal' : 'construction_step' }, construction(index)));
  const expanded = expandCadStages(stages);
  equal(expanded.map(stage => stage.featureId), ['outline', 'base', 'cut_profile', 'pocket', 'rounding']);
  equal(expanded[0].meshes, []);
  assert(expanded[1].meshes === stages[0].meshes && expanded[2].meshes === stages[0].meshes);
  assert(expanded[3].meshes === stages[1].meshes && expanded[4].meshes === stages[2].meshes);
  const invalid = structuredClone(stages); invalid[1].construction!.features[0].sketch!.offset = 9;
  rejects(() => expandCadStages(invalid));
  const timeline = new RevealTimeline(); timeline.start(0, 0, 850, true, 0, true);
  equal(timeline.read(100).status, 'playing'); equal(timeline.read(900).status, 'complete');
});

Deno.test('CAD construction bounds reject malformed profiles, future dependencies and standalone sketch mesh stages', () => {
  rejects(() => buildCadConstruction(recipe, 0));
  const overflow = structuredClone(recipe); overflow.steps = Array(17).fill(recipe.steps[0]);
  rejects(() => buildCadConstruction(overflow, 1));
  const data = construction(4);
  const bad = structuredClone(data); bad.features[0].sketch!.curves[1].center = [NaN, 0];
  rejects(() => validateCadConstruction(bad));
  const tooMany = structuredClone(data); tooMany.features[0].sketch!.curves = Array(65).fill(data.features[0].sketch!.curves[0]);
  rejects(() => validateCadConstruction(tooMany));
  const future = structuredClone(data); future.features[1].inputs = ['rounding'];
  rejects(() => validateCadConstruction(future));
  const snapshot = buildCadConstruction(recipe, 1); snapshot.features[0].sketch!.start[0] = -100;
  equal(recipe.steps[0].start, [-5, 0]);
  const legacy = create(ConstructionSchema, { version: 1, units: 'model', sketchLoops: [{ pointsXy: [0, 0, 1, 0, 0, 1] }] });
  const stage = prepareModel(mesh(), 'legacy', 'Image', undefined, legacy);
  equal(expandCadStages([stage])[0], stage);
});
