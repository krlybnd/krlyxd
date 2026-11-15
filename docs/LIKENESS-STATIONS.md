# Likeness station evidence

The active legacy `likeness` query now has an opt-in station report. It compares
sampled outer silhouette extents; it never changes the model or drawing. Existing
`report=legacy` output and IoU claims retain their behavior.

The [public station contract](../crates/kernel/src/ports/likeness/report.rs) lives
beside the existing likeness port. The [view implementation](../crates/view/src/likeness/report.rs)
reuses the current mesh projection, raster filling and IoU calculation. Moving the
complete likeness API to standalone view adapters remains #26; these active providers
have not become obsolete and should not be moved to `.tmp/`.

## Request

With a model node `part` and joined peer drawing `vis:top`, send this through the
existing CLI JSON-RPC/MCP facade:

```json
{"jsonrpc":"2.0","id":"check","method":"likeness","params":{"nodes":["part"],"against":"vis:top","report":"stations","tolerance":1,"precision":"raster","stairs":"pixel-diagonal","frame":[0,0,0.8660254037844386,0.5,-0.5,0.8660254037844386]}}
```

First omit `tolerance` to inspect evidence without a verdict. `frame` is optional:
`ox,oy,ux,uy,vx,vy` gives an origin and orthonormal local u/v basis inside the drawing's
existing world-axis plane. The default is the identity basis. Rotated and reflected
bases preserve world length units. Arbitrary perspective/photo pose fitting is #6.

- `report=stations` selects the additive report; `legacy` remains the default.
- `precision=raster` reads pixel extents. `contour` linearly intersects projected mesh
  triangles and supplied drawing polylines at the same sampled stations.
- `tolerance` is a finite nonnegative endpoint threshold. Each station reports its
  effective threshold and pass/fail. A missing side always fails.
- `stairs=none` is the default. `pixel-diagonal` adds `sqrt(2) * effective_cell` to
  raster station thresholds. This is a chosen allowance, not a certified bound on
  shallow-edge scanline error; it is rejected in contour mode.

## Precision and evidence

Raster evidence cannot support a tolerance below the effective cell, even when its
polyline is interpolated in contour mode. The cell may be coarser than the source
pixel to bound work; the report states its actual value. A declared vector source
with finite nonnegative uncertainty can support fractional thresholds, but no
threshold may be below that declared uncertainty. Contour mode requires a source.

Peer metadata uses `evidence: {kind: "raster"|"vector", source, uncertainty}`;
unknown uncertainty is omitted or `"unknown"`. Missing metadata defaults to raster
with unknown uncertainty. The real krlyvis `polygons` reply labels its placed outlines
`krlyvis.placed-pixel-outline/1`, retains pixel-derived status/unknown uncertainty,
and pins the resolved reference node. Simplified pixel contours are never relabelled
vector evidence just because their coordinates contain decimals.

The model remains a tessellated approximation. Requested mesh tolerance does not
prove achieved error; that error remains unknown. Contour intersections provide
fractional coordinates, not a certification of image-measurement or model accuracy.
IoU remains raster-sampled in either mode. Matches concern sampled outer extents,
not holes, topology, full continuous silhouette agreement or reconstructed geometry.

## Report and provenance

`report_version` is `krly3d.likeness.stations/1`. It contains:

- All occupied u/v `stations`, ordered by axis and line index; each has a report-local
  `id`, `at`, model/drawing extent, signed endpoint `delta`, maximum absolute `off`,
  effective `tolerance` and `matches`.
- Up to four separated `worst` stations from each local axis, including matching ones;
  `compared`/`failed` counts are in u/v order, and aggregate `matches` summarizes the
  requested sampled comparisons.
- `frame`, `grid`, policy, pinned model node versions, complete peer coordinate snapshot,
  resolved reference version where supplied, source/evidence/uncertainty and requested
  mesh tolerance. Station IDs are scoped to this retained reference and grid.

Missing extents/deltas are empty arrays. Without a threshold, tolerance is `"unknown"`
and verdict is `"not-requested"`. Invalid axes, frames, nonfinite numbers, impossible
resolution or missing evidence are errors. Tessellation failures retain their face,
tolerance and reason rather than being converted into empty silhouettes.

## Reproduce

```sh
cargo test -p krly3d-view --test likeness
cargo build -p krly3d-cli -p krlyvis-cli
python3 crates/view/tests/likeness_peer.py
```

The [geometry tests](../crates/view/tests/likeness.rs) cover rotated local axes,
reflection/swapped world axes, diagonal allowance, vector fractional differences,
missing-side verdicts, invalid evidence, nonmutation and default compatibility.
The [real peer replay](../crates/view/tests/likeness_peer.py) creates a temporary PGM,
starts krly3d joined to the actual krlyvis binary, and checks 13 calls: matching and
shifted geometry, both rotated axes, raster precision floor, pinned versions,
legacy output and unchanged model state after reports. No named aircraft/moai fixture
is required. Final integration: [development status](DEVELOPMENT-STATUS.md).
