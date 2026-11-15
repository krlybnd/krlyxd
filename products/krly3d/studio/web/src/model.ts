import type { MeshData, StudioEvent } from './client';
import { validateMesh, type StudioMesh } from './scene.ts';
import { validateCadConstruction } from './cadConstruction';

export interface ModelSnapshot {
  key: string;
  name: string;
  phase: string;
  mesh: MeshData;
  meshes: StudioMesh[];
  construction?: NonNullable<StudioEvent['construction']>;
}

export interface ModelState {
  confirmed: ModelSnapshot | null;
  preview: ModelSnapshot | null;
  runId: string | null;
  notice: string | null;
  stages: ModelSnapshot[];
  generation: string | null;
}

export type ModelAction =
  | { type: 'begin'; runId: string }
  | { type: 'preview'; runId: string; snapshot: ModelSnapshot }
  | { type: 'complete'; runId: string }
  | { type: 'rollback'; runId: string; reason: 'error' | 'cancelled'; affectsGeometry: boolean };

export const initialModelState: ModelState = { confirmed: null, preview: null, runId: null, notice: null, stages: [], generation: null };

export function prepareModel(mesh: MeshData, runId: string, name = mesh.source || 'Aktuális modell', stage?: { key: string; phase: string }, construction?: NonNullable<StudioEvent['construction']>): ModelSnapshot {
  const data: StudioMesh = { id: runId, name, positions: mesh.positions, indices: mesh.indices };
  validateMesh(data);
  if (construction?.version === 2) validateCadConstruction(construction);
  if (![mesh.volume, mesh.area].every(value => Number.isFinite(value) && value >= 0) || !Number.isSafeInteger(mesh.openEdges) || mesh.openEdges < 0) {
    throw new Error('A geometria mérési adatai érvénytelenek.');
  }
  return { key: stage?.key ?? runId, name, phase: stage?.phase ?? 'editable_proposal', mesh, meshes: [data], construction };
}

// A mesh event is a provisional result. Only the run's terminal completion
// commits it; transport errors, native errors and cancellation roll it back.
export function modelReducer(state: ModelState, action: ModelAction): ModelState {
  if (action.type === 'begin') return { ...state, preview: null, runId: action.runId, generation: action.runId, notice: null, stages: [] };
  if (action.runId !== state.runId) return state;
  if (action.type === 'preview') return { ...state, preview: action.snapshot, stages: [...state.stages.slice(0, 7), action.snapshot] };
  if (action.type === 'complete') return { ...state, confirmed: state.preview ?? state.confirmed, preview: null, runId: null, notice: null };
  const changed = action.affectsGeometry || Boolean(state.preview);
  const result = state.confirmed ? 'Az utolsó elkészült modell látható.' : 'Nem készült végleges modell.';
  return {
    ...state, preview: null, runId: null, stages: [],
    notice: changed ? `${action.reason === 'cancelled' ? 'Az építés leállítva.' : 'Az építés sikertelen.'} ${result}` : null,
  };
}
