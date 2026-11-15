export interface StudioMesh {
  id: string;
  name: string;
  positions: number[];
  indices?: number[];
  normals?: number[];
  color?: string;
}

export function validateMesh(mesh: StudioMesh): void {
  const positions = mesh.positions;
  if (!positions.length || positions.length % 3 || !positions.every(value => Number.isFinite(value) && Number.isFinite(Math.fround(value)))) {
    throw new Error(`Érvénytelen csúcspontok: ${mesh.name}`);
  }
  if (mesh.indices && (!mesh.indices.length || mesh.indices.length % 3 || !mesh.indices.every(index => Number.isSafeInteger(index) && index >= 0 && index < positions.length / 3))) {
    throw new Error(`Érvénytelen háromszögindexek: ${mesh.name}`);
  }
  if (!mesh.indices && positions.length % 9) throw new Error(`Hiányos háromszögek: ${mesh.name}`);
}

export type CameraView = 'isometric' | 'top' | 'front';
export type DisplayMode = 'solid' | 'wire';

export interface SceneStats {
  objects: number;
  triangles: number;
  vertices: number;
  dimensions: [number, number, number] | null;
}
