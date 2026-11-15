import { create } from '@bufbuild/protobuf';
import { ConstructionSchema } from '../../gen/krly/studio/v1/studio_pb.ts';
import { buildCadConstruction } from '../../service/native_construction.ts';
export const recipe = { units: 'mm', steps: [
  { type: 'sketch', id: 'outline', title: 'Íves alapvázlat', start: [-5, 0], segments: [{ type: 'line', to: [5, 0] }, { type: 'arc', center: [0, 0], to: [-5, 0], clockwise: false }], z: 7.5 },
  { type: 'extrude', id: 'base', title: 'Alaptest', inputs: ['outline'], height: 12 },
  { type: 'sketch', id: 'cut_profile', title: 'Zseb vázlata', start: [1, 0], segments: [{ type: 'arc', center: [0, 0], to: [-1, 0], clockwise: false }, { type: 'arc', center: [0, 0], to: [1, 0], clockwise: false }], z: 19.5 },
  { type: 'pocket', id: 'pocket', title: 'Süllyesztett zseb', inputs: ['base', 'cut_profile'], depth: 4 },
  { type: 'fillet', id: 'rounding', title: 'Éllekerekítés', inputs: ['pocket'], edges: ['e1'], radius: .5 },
] };
export const construction = (index: number) => create(ConstructionSchema, buildCadConstruction(recipe, index));
export const mesh = (scale = 1) => ({ positions: [0, 0, 7.5, 5 * scale, 0, 7.5, 0, 5 * scale, 7.5, 0, 0, 19.5], indices: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3], volume: 50 * scale * scale, area: 50, openEdges: 0, units: 'mm', source: 'brep-session:test:cad' });
