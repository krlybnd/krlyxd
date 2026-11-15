import { BufferGeometry, EdgesGeometry, Float32BufferAttribute, Vector3 } from 'three';
import { toCreasedNormals } from 'three/examples/jsm/utils/BufferGeometryUtils.js';
import type { StudioMesh } from './scene';

export function createRenderGeometry(mesh: StudioMesh): { surface: BufferGeometry; edges: EdgesGeometry } {
  const source = new BufferGeometry();
  source.setAttribute('position', new Float32BufferAttribute(mesh.positions, 3));
  if (mesh.indices) source.setIndex(mesh.indices);
  const edges = new EdgesGeometry(source, 24);
  let surface = source;
  let reference: BufferGeometry | null = null;
  try {
    if (mesh.normals?.length === mesh.positions.length && mesh.normals.every(Number.isFinite)) {
      source.setAttribute('normal', new Float32BufferAttribute(mesh.normals, 3));
    } else {
      surface = source.index ? source.toNonIndexed() : source;
      reference = surface.clone();
      reference.computeBoundingBox();
      const box = reference.boundingBox!;
      const size = box.getSize(new Vector3());
      const center = box.getCenter(new Vector3());
      const extent = Math.max(size.x, size.y, size.z);
      // The utility hashes positions at a fixed precision. Normalize only the
      // temporary normal calculation so tiny model units retain distinct corners.
      if (extent > 0) reference.translate(-center.x, -center.y, -center.z).scale(1000 / extent, 1000 / extent, 1000 / extent);
      toCreasedNormals(reference, Math.PI / 6);
      surface.setAttribute('normal', reference.getAttribute('normal'));
    }
    surface.computeBoundingBox();
    surface.computeBoundingSphere();
    return { surface, edges };
  } catch (cause) {
    surface.dispose(); edges.dispose();
    throw cause;
  } finally {
    reference?.dispose();
    if (surface !== source) source.dispose();
  }
}
