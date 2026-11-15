#!/usr/bin/env python3
"""Independent, read-only audit of PyMeshLab self-intersection selections.

No application dependency or native-check bypass. SAT and --self-test use only
Python's standard library. Full audits optionally require NumPy and PyMeshLab:

  uv run --with numpy==2.5.4 --with pymeshlab==2025.7.post1 \
    tests/field_print_geometry.py PATH/mesh.json --expect-flags 322 434
  python3 tests/field_print_geometry.py --self-test

The optional expected counts reproduce the original character fixture; they do
NOT whitelist any faces. Every selected face must be covered by reproduced
pairwise selections, and EVERY reproduced pair must independently have a strict
separating gap. Both original f64 and STL-equivalent f32 coordinates are tested.
Failure, an unexplained pair, an uncovered face, or a budget overflow exits
nonzero. Missing optional packages exit 2 with installation instructions.

Rationale: PyMeshLab can flag tilted, coplanar but disjoint triangles. Projecting
onto a dominant coordinate plane and finding a separating axis proves disjointness
without relying on PyMeshLab's intersection predicate. Near-contact/overlap is
never excused. This audit explains the checker selections; it does not certify
unselected pairs, topology, wall thickness, or printability. Keep native full-mesh
preflight enabled, including its independent f32 round-trip check.
"""

import argparse
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import sys

MAX_BYTES = 64 * 1024 * 1024
MAX_FLAGS = 5000
MAX_PAIR_CHECKS = 100_000


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def coplanar_gap(a, b):
    """Return strict planar separation evidence, or None (never a clearance)."""
    scale = max(1.0, *(abs(float(x)) for tri in (a, b) for p in tri for x in p))
    tolerance = max(1e-12, 128 * math.ulp(scale))
    normals = [cross(sub(t[1], t[0]), sub(t[2], t[0])) for t in (a, b)]
    lengths = [math.sqrt(dot(n, n)) for n in normals]
    if min(lengths) == 0 or not all(math.isfinite(n) for n in lengths):
        return None
    normals = [tuple(x / length for x in n) for n, length in zip(normals, lengths)]
    distances = [abs(dot(sub(p, base[0]), n)) for tri, base, n in ((a, b, normals[1]), (b, a, normals[0])) for p in tri]
    if max(distances) > tolerance or abs(dot(*normals)) < 1 - 1e-10:
        return None
    dropped = max(range(3), key=lambda i: abs(normals[0][i]))
    projected = [[tuple(float(p[i]) for i in range(3) if i != dropped) for p in tri] for tri in (a, b)]
    # Translate before dot products to reduce cancellation far from the origin.
    origin = projected[0][0]
    pa, pb = [[sub(p, origin) for p in tri] for tri in projected]
    gap = 0.0
    for tri in (pa, pb):
        for i in range(3):
            edge = sub(tri[(i + 1) % 3], tri[i])
            length = math.hypot(*edge)
            if length == 0:
                return None
            axis = (-edge[1] / length, edge[0] / length)
            aa, bb = ([dot(p, axis) for p in t] for t in (pa, pb))
            gap = max(gap, min(aa) - max(bb), min(bb) - max(aa))
    if gap <= tolerance:
        return None
    return {"projected_gap_mm": gap, "plane_distance_mm": max(distances), "tolerance_mm": tolerance}


def self_test():
    a = [(0., 0., 0.), (2., 0., 0.), (0., 2., 0.)]
    separated = [(1.2, 1.2, 0.), (2., 1.2, 0.), (1.2, 2., 0.)]
    overlap = [(0.2, 0.2, 0.), (0.8, 0.2, 0.), (0.2, 0.8, 0.)]
    # Overlapping AABBs are insufficient; the diagonal SAT axis separates these.
    assert coplanar_gap(a, separated) is not None
    assert coplanar_gap(a, overlap) is None
    assert coplanar_gap(a, a) is None
    assert coplanar_gap(a, [(0., 0., 0.)] * 3) is None
    assert coplanar_gap(a, [(0.2, 0.2, -1.), (0.2, 0.2, 1.), (1., 1., 0.)]) is None
    assert coplanar_gap(a, [(2., 0., 0.), (3., 0., 0.), (2., 1., 0.)]) is None
    def tilt(tri):
        return [(x + 17., y + 8., 0.73 * x + 16.) for x, y, _ in tri]
    assert coplanar_gap(tilt(a), tilt(separated)) is not None
    assert coplanar_gap(tilt(a), tilt(overlap)) is None


def load_mesh(path, np):
    with path.open("rb") as source:
        raw = source.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        raise ValueError("mesh JSON exceeds the 64 MiB audit budget")
    data = json.loads(raw)
    if not isinstance(data, dict) or set(data) != {"points", "triangles"}:
        raise ValueError("expected exactly points and triangles tuple arrays")
    if not 0 < len(data["points"]) <= 1_500_000 or not 0 < len(data["triangles"]) <= 500_000:
        raise ValueError("mesh exceeds audit size bounds or is empty")
    for triangle in data["triangles"]:
        if not isinstance(triangle, list) or len(triangle) != 3 or any(type(i) is not int or not 0 <= i < len(data["points"]) for i in triangle):
            raise ValueError("triangle indices must be in-range integers")
    vertices = np.asarray(data["points"], dtype=np.float64)
    faces = np.asarray(data["triangles"], dtype=np.int32)
    if vertices.shape != (len(data["points"]), 3) or not np.isfinite(vertices).all():
        raise ValueError("points must be finite 3D coordinates")
    return vertices, faces, hashlib.sha256(raw).hexdigest()


def selection(vertices, faces, ml):
    meshset = ml.MeshSet()
    meshset.add_mesh(ml.Mesh(vertex_matrix=vertices, face_matrix=faces))
    meshset.compute_selection_by_self_intersections_per_face()
    return meshset.current_mesh().face_selection_array().copy()


def audit(vertices, faces, np, ml):
    selected = np.flatnonzero(selection(vertices, faces, ml))
    if len(selected) > MAX_FLAGS:
        raise ValueError("selected-face budget exceeded; diagnosis is incomplete")
    triangles = vertices[faces[selected]]
    lo, hi = triangles.min(axis=1), triangles.max(axis=1)
    covered, unexplained = set(), []
    checked = reproduced = separated = 0
    min_gap = math.inf
    max_plane_distance = 0.0
    for ia, a in enumerate(selected):
        candidates = np.flatnonzero(np.all(hi[ia] >= lo, axis=1) & np.all(hi >= lo[ia], axis=1))
        for ib in candidates:
            if ib <= ia:
                continue
            checked += 1
            if checked > MAX_PAIR_CHECKS:
                raise ValueError("pair budget exceeded; diagnosis is incomplete")
            b = selected[ib]
            # Preserve shared vertex identities in isolated calls: no topology changes.
            ids, inverse = np.unique(faces[[a, b]], return_inverse=True)
            if not selection(vertices[ids], inverse.reshape(2, 3), ml).any():
                continue
            reproduced += 1
            evidence = coplanar_gap(triangles[ia].tolist(), triangles[ib].tolist())
            if evidence is None:
                unexplained.append([int(a), int(b)])
                continue
            separated += 1
            covered.update((int(a), int(b)))
            min_gap = min(min_gap, evidence["projected_gap_mm"])
            max_plane_distance = max(max_plane_distance, evidence["plane_distance_mm"])
    uncovered = sorted(set(map(int, selected)) - covered)
    return {
        "flagged_faces": len(selected), "candidate_pairs_tested": checked,
        "reproduced_flagged_pairs": reproduced, "strictly_separated_coplanar_pairs": separated,
        "covered_faces": len(covered), "minimum_projected_gap_mm": min_gap if separated else None,
        "maximum_plane_distance_mm": max_plane_distance if separated else None,
        "unexplained_pair_count": len(unexplained), "unexplained_pair_examples": unexplained[:8],
        "uncovered_face_count": len(uncovered), "uncovered_face_examples": uncovered[:8],
        "passed": not unexplained and not uncovered,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mesh", type=Path, nargs="?")
    parser.add_argument("--self-test", action="store_true", help="run dependency-free SAT regressions")
    parser.add_argument("--expect-flags", nargs=2, type=int, metavar=("F64", "F32"), help="optional exact fixture count assertions, never exemptions")
    args = parser.parse_args()
    self_test()
    if args.mesh is None:
        if not args.self_test:
            parser.error("provide a mesh JSON path or --self-test")
        print(json.dumps({"sat_self_test": "passed"}))
        return 0
    try:
        import numpy as np
        import pymeshlab as ml
    except ImportError as error:
        print(f"Optional audit dependencies unavailable: {error}. Install numpy and pymeshlab in an isolated environment; see this script's docstring.", file=sys.stderr)
        return 2
    vertices, faces, digest = load_mesh(args.mesh, np)
    with np.errstate(over="ignore"):
        rounded = vertices.astype(np.float32).astype(np.float64)
    if not np.isfinite(rounded).all():
        raise ValueError("STL-equivalent f32 conversion produced nonfinite coordinates")
    results = {"f64": audit(vertices, faces, np, ml), "f32": audit(rounded, faces, np, ml)}
    matches = args.expect_flags is None or [results[k]["flagged_faces"] for k in ("f64", "f32")] == args.expect_flags
    passed = matches and all(r["passed"] for r in results.values())
    print(json.dumps({
        "mesh": str(args.mesh), "sha256": digest,
        "versions": {p: importlib.metadata.version(p) for p in ("numpy", "pymeshlab")},
        "results": results, "expected_counts_match": matches, "passed": passed,
        "scope": "Diagnosis of PyMeshLab selections only; retain full native print preflight.",
    }, indent=2))
    return 0 if passed else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, RuntimeError) as error:
        print(f"Audit incomplete: {error}", file=sys.stderr)
        sys.exit(1)
