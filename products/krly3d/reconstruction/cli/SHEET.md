# Multiple selected parts from one drawing

The sheet runner composes the [single-part drawing flow](README.md), retaining its
hole-aware geometry, uncertainty disclosures, early preview, editable model and checks.
It produces independently editable parts in the drawing's shared coordinates, **not**
an inferred assembly, constraint solution or automatic image segmentation UI.

## Run

Create a UTF-8 parts manifest:

```json
[
  {"id": "frame", "seed": [5, 5], "height": 3},
  {"id": "bracket", "seed": [37, 7], "height": 5}
]
```

Each seed is a pixel inside the desired connected material component. Pixel coordinates
use the input image frame (x right, y down). All parts use one explicit model-units-per-pixel
scale; each has its own user-supplied uniform thickness.

```sh
python3 products/krly3d/reconstruction/cli/sheet_flow.py \
  --image /absolute/path/sheet.png --parts /absolute/path/parts.json \
  --pixel-unit 1 --output /tmp/new-sheet-result
```

Use the domain build command in the single-part README first. `--timeout` (default 120s)
is a whole-sheet computation budget; each child gets the remaining budget. `--bin-dir`
selects the executable directory. Linux/POSIX is required.

The manifest must contain 1..16 entries, each with exactly `id`, `seed`, `height`.
IDs start with an ASCII letter and contain at most 64 ASCII letters/digits/underscores/
hyphens. Duplicate IDs, duplicate JSON keys, path traversal, nonfinite numbers, booleans
as dimensions, nonpositive height and unknown fields fail before output creation.
The manifest is capped at 1 MiB and the image at 16 MiB. Output must be a new directory;
existing results are never overwritten. The input image is copied once before any part
runs, then each child snapshots that same image for its standalone replay.

## Result and event contract

`manifest.json` is atomically replaced at admission and each stage transition. It contains
sheet identity, image hash, shared scale, assumptions and per-part input/status. Completed
parts include output directory, model path/hash, PNG, logical artifact identity/revision,
likeness verdict and actual material-volume/mesh-closure checks. It can be inspected even
if later work fails. Each `parts/<id>/` directory contains the full single-part artifacts.

The `krly3d.drawing-sheet/1` NDJSON events are:

- `started`: validated selection list and source image hash.
- `part_event`: `part_id` plus the original single-part event, including coarse preview
  before editable geometry. Child flow IDs/sequences are preserved and validated.
- `part_completed`: the complete per-part manifest record after child exit and checks.
- `completed`: final manifest path and aggregate likeness matches (completion itself is
  not a claim that every sampled view matches).
- `failed` / `cancelled`: diagnostic and manifest path; no subsequent completed event.

Outer events have their own sheet ID, sequence and elapsed time. A bounded serial runner
retains no unbounded event queue: stdout backpressure stops further processing. Individual
child events are limited to 1 MiB. File artifacts have the single-part domain limits;
there is no global disk quota. Blocked stdout can delay deadline observation until output
resumes. SIGINT/SIGTERM propagates to the active child orchestrator for process-group
cleanup; completed parts remain, current work is marked failed/cancelled, untouched parts
remain pending. There is no resume/automatic retry mode yet.

Different names must select different components. After a child finishes, its canonical
world polygons are compared with completed component digests. Duplicate selection fails
the sheet rather than silently delivering the same component twice. The second child's
local artifacts may exist for diagnosis, but it is not marked completed by the sheet.

## Scope and verification

Selection is explicit; text, noise and other disconnected pixels are not automatically
classified as parts. Hole contours become through-holes of the specified thickness.
Likeness still checks outer extents; volume/closure checks do not prove arbitrary hidden
geometry. Parts retain drawing placement; no assembly relation is inferred. Shared-host
orchestration, automatic recognition, review/accept editing and resumable refinement remain
in #24/#26 and their domain issues.

```sh
python3 tests/drawing_flow_smoke.py
python3 tests/drawing_sheet_smoke.py
```

Tests use real executables and distinct components, including a through-hole and different
thicknesses. They check manifests, geometry evidence, partial failure, invalid selections,
input validation and preservation of existing output. No geometry algorithm is duplicated
in this orchestration layer.
