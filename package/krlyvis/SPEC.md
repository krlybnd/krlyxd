# krlyvis – technical specification

krlyvis is a separate vision product. It reads pictures and keeps versioned picture,
outline and placed-view nodes that an agent can query or use to derive cross-sections. Its
`analyse` query finds straight edges, vanishing points, a camera and lengths in a chosen
plane. The standard pipeline uses no learned model; its steps are deterministic searches
and geometric calculations. krlyvis shares foundational types, the plugin mould, document
and claim grammar from `krly3d-core`, while keeping its vision ports, data and `Context` in
`package/krlyvis`. The product boundary and eventual target API packaging are described in
[`docs/PRODUCT-ARCHITECTURE-HANDOFF.md`](../../docs/PRODUCT-ARCHITECTURE-HANDOFF.md).

## 1. What one picture can give, and what it cannot

| From | Follows | Needs |
|---|---|---|
| straight edges | segments with sub-pixel ends | nothing |
| edges parallel in the world | their vanishing point | at least three such edges |
| two vanishing points of square directions | focal length (lens centre assumed in the middle) | a box-shaped world (Manhattan assumption) |
| three finite vanishing points | focal length and lens centre (orthocentre) | strong three-point perspective |
| camera + a plane named by two axes | every length in that plane, up to one scale | the segments to lie in that plane |
| one known length in the plane | the scale | the user or the agent says which segment it is |
| camera + flat ground | for every vertical segment standing on it: height and distance of its foot | the segment's lower end touches the ground; the camera's height or one standing height sets the unit |

Where the camera stands is not known from one picture; lengths measured without a known
one are in multiples of the camera's distance to the plane. A segment is measured in the
plane asked for whether or not it lies there: choosing the plane is the reader's call.

## 2. Architecture

This section describes the current implementation unless marked as target architecture.
The current krlyvis CLI reuses the krly3d transport library; the accepted product direction
calls for krlyvis to have its own versioned public API and CLI/MCP adapters, which is future
packaging work. `VisionSession` is stateful even though each `analyse` call runs its pipeline
on one image.

### 2.1 Layers and patterns

```
┌────────────────────────────────────────────────────────────────────────────┐
│ CLIENTS   AI agent · tests · shell                                         │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ JSON-RPC 2.0 over stdio · MCP · shell
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ cli       VisionSession: krly3d_cli::Endpoint                    [Adapter] │
│           transports reused from krly3d-cli: dialect/{plain,mcp}, shell    │
│           krly3d --peer vis=krlyvis  ->  `vis.analyse` in krly3d's session │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ standard() -> VisionBuilder
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ std       shipped capabilities; one file = one plugin          [Strategy] │
│   read/{pnm,png,jpeg} · make/{picture,outline,frame} · detect/lsd          │
│   vanish/vote · camera · measure · depth · outline · stage   [Chain]      │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ implements ports
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ kernel    contracts and orchestration, zero implementations                │
│   Vision          read(format) · analyse(image, ask)   [Facade]            │
│   VisionSession   versioned vis document + build cache + model             │
│   VisionBuilder   one Registry per family              [Builder]           │
│   Context         clock, registries, stage order       [Dependency Inj.]   │
│   plugin          Plugin<F> · Registry<F>              [Registry]          │
│   stage pipeline  Scene -> Scene, in the given order   [Pipes & Filters]   │
│   ports/<family>  Family + Request + Response + contract [Ports & Adapters]│
│   report          Scene -> text, one fact per line                         │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ uses
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ krly3d-core   Id · Attempt<T> · Error · Clock · Exactness (shared)         │
└────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 How an analysis runs

This pipeline is the `analyse` query; it does not itself add a document node. The session's
separate stateful path is `picture` → `outline` → `frame`. Each successful tool call adds a
versioned statement to the `vis` layer; `open` and `save` read/write that document. Rebuilding
with the same tool, parameters and input keys reuses the successful cached datum, so a stored
picture is decoded once per session. `polygons` returns an outline in pixels or world
coordinates, and `sections` samples two placed views at requested stations and returns
extents plus a rounded-rectangle `profile` path suggestion. These are vision measurements,
not an automatic mesh or B-rep reconstruction.

```
analyse(image, ask)
                ┌────────┐   ┌────────┐   ┌────────┐   ┌─────────┐   ┌───────┐
   Scene ─────▶ │ detect │──▶│ vanish │──▶│ camera │──▶│ measure │──▶│ depth │──▶ Scene ──▶ report
                └───┬────┘   └───┬────┘   └───┬────┘   └────┬────┘   └───┬───┘
                    ▼            ▼            ▼             ▼            ▼
                 detect       vanish       camera        measure      depth      families
                 "lsd"        "vote"       "camera":     "plane"      "ground"
                                           given ▶ centred
                                           "orthocentre"
   a Declined answer becomes a note in the scene; only Failed stops the analysis
   every stage is timed through the injected Clock
```

### 2.3 Design patterns

| Pattern | Where | What it solves |
|---|---|---|
| Ports & Adapters | `kernel/ports` ↔ `std` | the kernel knows no algorithm |
| Strategy | every plugin | LSD can be swapped for another detector by id |
| Registry + Builder + DI | `Registry<F>`, `VisionBuilder`, `Context` | assembly from outside, no global state |
| Chain of Responsibility | `camera` = `given` ▶ `centred` | a known focal length is used before it is estimated |
| Pipes & Filters | the `stage` list | a new step (circles, repetition) is a new stage |
| Facade | `Vision` | the CLI sees one door |
| Adapter | `read` plugins, `cli` | other file formats and transports without touching the kernel |

### 2.4 SOLID

The same as for krly3d: one file is one plugin; a new capability is a new file and one
registration line; every plugin passes the contract test of its family; every port asks
one question; `kernel` knows only ports, `std` implements them, `cli` sees only `Vision`.

## 3. Specification

### 3.1 The mould

| Element | Where |
|---|---|
| Port: `Family` marker, `Request`, `Response` | `kernel/src/ports/<family>/mod.rs` |
| Contract test every plugin runs | `kernel/src/ports/<family>/contract.rs` |
| Plugin: one struct, `impl Plugin<F>`, its tests | `std/src/<family>/<plugin>.rs` |
| Registration: `pub fn register(b: &mut VisionBuilder)` | `std/src/<family>/mod.rs` |

`plugin.rs` re-exports `Plugin`, `Registry` and the other mould types from
`krly3d_core`; the vision families bind them to krlyvis's own `Context`. `Attempt` and the
shared request/value types also come from `krly3d_core`.

### 3.2 Families

| Family | Key | Request → Response | Plugins | Contract |
|---|---|---|---|---|
| `read` | format | bytes → `Gray` | `pnm` (P2 P3 P5 P6, 8/16 bit), `png`, `jpeg` | decodes to the expected image; malformed data is a format error |
| `make` | tool | parameters + prior `Datum`s → `Datum` | `picture`, `outline`, `frame` | makes a picture, extracted outline, or placed outline |
| `detect` | detector | `Gray` → segments | `lsd` | each edge of a drawn quadrilateral found within 0.5 px covering 80%; ≤ 1 segment in noise |
| `vanish` | method | segments → vanishing points | `vote` | three known points (one at infinity) found exactly, with exactly their segments, despite clutter |
| `camera` | method | vanishing points → `Camera` | `given`, `centred` (chain `camera`); `orthocentre` | a known camera's focal length, centre and axes come back to 1e-9 |
| `measure` | method | camera + plane + segments → lengths | `plane` | segments of a known wall measure to 1e-9 |
| `depth` | method | camera + segments (+ eye height or a known standing height) → standing segments | `ground` | posts of known height drawn by a known camera come back to 1e-9; a slanting segment is not a post; eye height and a known post give the same unit |
| `outline` | method | image + region/seed/threshold → polygons | `trace`, `mask` | extracts the selected foreground subject |
| `stage` | name | `Scene` → `Scene` | `detect`, `vanish`, `camera`, `measure`, `depth` | image and question are left unchanged |

### 3.3 Mathematics

| Step | Method |
|---|---|
| Segments | LSD (Grompone von Gioi et al., IPOL 2012), written from the article: Gaussian scaling 0.8, 2 × 2 gradient, region growing by level-line angle (22.5°), rectangle by inertia, density refinement, a-contrario validation (NFA ≤ 1 per image; binomial tail with Lanczos/Windschitl log-gamma). No parameter is tuned per image. |
| Vanishing points | Lines in homogeneous coordinates after conditioning (centre, half diagonal). Every pair of the 100 longest free segments proposes `l₁ × l₂`; the proposal backed by most segment length within 2° wins; refined as the smallest eigenvector of `Σ w l lᵀ`; repeated for up to three points. Deterministic. |
| Focal length, centre given | `(a − c a_w)·(b − c b_w) + f² a_w b_w = 0` for square directions |
| Focal length and centre | centre = orthocentre of the three vanishing points; `f² = −(a − c)·(b − c)` |
| Axes | `d = K⁻¹v`; vertical = most along image y, turned up; x = strongest other, Gram–Schmidt; y = z × x. `off_square` reports the third point's angle from y. |
| Lengths in a plane | `X = r / (±n·r)` on the plane at unit distance; one known length sets the scale |
| Standing heights and distances | ground `up·X = −1` (camera height = 1); foot `F = r / (−up·r)`; height `s = −(F × t)·(up × t) / \|up × t\|²` from `(F + s·up) × t = 0`; distance `\|F + up\|`. Vertical = runs to `K·up` within 2°; the foot is the end whose line of sight points further down |

### 3.4 The command line and JSON-RPC

krlyvis speaks exactly as krly3d does, through the same code (`krly3d-cli`): it is one
more `Endpoint` (`name`, `instructions`, `methods`, `call`).

```
krlyvis                     plain JSON-RPC 2.0 on stdin/stdout, one message per line
krlyvis --mcp               the same as MCP tools for an agent
krlyvis call analyse path=bridge.pgm
krlyvis call analyse path=bridge.pgm plane=xz known=s12=4.5
krly3d --peer vis=krlyvis   krly3d starts krlyvis and answers its methods as `vis.…`
```

Methods include session operations `methods`, `open` and `save`; queries `analyse`,
`polygons` and `sections`; and the stateful tools `picture`, `outline` and `frame`.
`analyse` takes exactly one of `path` or a stored picture `node`, plus `focal` (0 = unknown),
`camera` (`camera` or `orthocentre`), `plane` (`xz`, `yz`, `xy`, empty = none), `known`
(`s12=4.5`; in the plane, or as a standing height), `eye` (the camera's height, 0 =
unknown) and `show`. The reply's text is the report below; its data holds the same
facts with every segment, for a client that computes with them. The `picture` tool supports
`.png`, `.jpg`, `.jpeg`, `.pgm`, `.ppm` and `.pnm`; it reads a file of at most 256 MiB.

A peer (`krly3d_cli::peer`) is a child process spoken to in the plain dialect, one line
out and one back; its method list is read once when it starts. `Joined` serves a session
and its peers as one endpoint, so over MCP the agent sees krly3d's tools and `vis.analyse`
side by side and can measure a photograph while it models. A call to a peer blocks the
session until the answer comes, as every JSON-RPC call on one connection does. The link is
not a callback channel: a request made by the peer while krly3d awaits its reply is refused
with a busy error, and a notification is ignored. Peer calls inside `batch`, asynchronous
forwarding and cancellation propagation are not implemented.

### 3.5 The report

One fact per line, numbers found numerically marked `~` as in krly3d:

```
image 800x600 · 61 segments · 3 vanishing points
v0 at (398.9, -9472.5) · 29 segments · 2213 px
camera ~ by centred: f 692.5 px, centre (400.0, 300.0); x from v1, y from v2, z (up) from v0; y off square by 0.5°; horizon ...
plane zx (square to y) · unit set by s0 = 12 · ends given as (z, x)
s2 (273.4, 122.5)-(269.5, 432.5) 310.1 px v0 ~5.921 (4.364, 5.873)-(-1.557, 5.875)
time detect 37.7 ms, vanish 27.6 ms, camera 0.0 ms, measure 0.0 ms
```

Coordinates in the plane are measured from the foot of the camera, so the ground's `z`
is minus the camera's height.

## 4. Layout

```
package/krlyvis/
├── SPEC.md
└── crates/
    ├── kernel/src/             # krlyvis-kernel: vision contracts and orchestration
    │   ├── plugin.rs (re-export)  context.rs  builder.rs  vision.rs  model.rs
    │   ├── datum.rs  frame.rs  report.rs  scene.rs
    │   ├── image.rs            # Gray, Pixel (pixel centres at i + 0.5, y down)
    │   └── ports/
    │       └── <family>/       # read detect vanish camera measure depth outline make stage
    ├── std/src/                # krlyvis-std: one file per plugin
    │   ├── lib.rs              # standard() -> VisionBuilder
    │   ├── read/{pnm,png,jpeg}.rs
    │   ├── make/{picture,outline,frame}.rs
    │   ├── outline/{trace,mask,simplify}.rs
    │   ├── detect/lsd/{mod, field, region, rect, nfa, sampling}.rs
    │   ├── vanish/vote.rs
    │   ├── camera/{given, centred, orthocentre}.rs
    │   ├── measure/plane.rs
    │   ├── depth/ground.rs
    │   └── stage/{detect, vanish, calibrate, measure}.rs
    ├── std/tests/pipeline.rs   # a building drawn by a known camera, measured back
    └── cli/
        ├── src/args.rs         # serve (plain | mcp) or `call`, as krly3d
        ├── src/session.rs      # VisionSession: the Endpoint over Vision
        └── tests/program.rs    # shell, MCP, and krly3d asking krlyvis as a peer
```

## 5. Verified and not verified

Historical verification snapshot (2026-11-14): every plugin passed its contract; the drawn building gave
f = 692.5 px for a true 700 (1.1%), the third vanishing point 0.5° off square, and every
segment of the front wall measures within 1.5% of what the drawing camera gives for the
same pixels. A 1600 × 900 picture is analysed in about 65 ms (release build; the debug build takes
seconds). Over JSON-RPC: a krly3d session joined with a real `krlyvis` child process
draws a box, analyses a picture through `vis.analyse` and keeps its model; a failed
analysis comes back as a failed reply with its reason; MCP lists `analyse` as read-only.
Depth on the drawn building with `eye=1.6`: its three vertical edges stand 6.04–6.09 high
(6 drawn) at 13.69, 20.90 and 21.70 (true 13.42, 20.88, 21.63; at most 2.1% off), and every
standing segment agrees within 2% with what the drawing camera gives for the same pixels.
Not verified: depth on a real photograph. Window edges and anything else not touching the
ground are listed as standing and come out too far and too high; the reader must pick.

Not verified: no real photograph with known dimensions has been measured yet. Lens
distortion is not modelled; wide-angle photographs bend straight edges and will cost
accuracy. On a picture without perspective (a flat graphic) the vote still returns points
and a camera; `off_square` is then large (9° on such a test image) and the camera means
nothing.

## 6. Next, on the same mould

- `ellipse` family: circles seen as ellipses (arches, columns), their plane and radius.
- `repeat` stage: spacing of repeated elements (windows, pillars) along an axis.
- `region` stage: which segments lie on which face, so that a plane measures only its own
  and `depth` lists only what really touches the ground.
- Occlusion order (which edge is in front) from T-junctions; it needs regions, not segments.
- `undistort` stage: one-parameter radial distortion from the straightness of long edges.
- An overlay image of the segments in the reply, which MCP passes to the agent as an image.
- Calls to a peer inside a krly3d `batch` (today a batch reaches only the local session).


### Placed outline evidence

Placed polygon values additionally carry `evidence` with `kind: "raster"`, source
`krlyvis.placed-pixel-outline/1`, unknown measurement uncertainty, the frame pixel unit
and extraction threshold. Fractional simplified coordinates do not establish subpixel
measurement accuracy. The CLI `polygons` response also includes the resolved `node@version`
so consumers can pin the evidence. See [likeness station contract](../../docs/LIKENESS-STATIONS.md).


### Hole-preserving contour option

`outline method=trace-holes` selects the same connected component as `trace` but retains
its interior voids. It returns one outer loop first and oppositely wound hole loops.
Exact pixel-edge corners are kept (collinear points removed); zero displacement satisfies
the requested nonnegative simplification tolerance. Ambiguous corner contacts fail with
a geometry diagnostic. The original default `trace` continues filling holes.

Outline `area` measurements subtract interior loop areas; placed values now expose
`material_area` in world units squared. This is contour geometry, not a measured uncertainty
bound. The reconstruction pilot assumes those visible voids are through-holes of a uniform
extrusion; hidden/blind cavities and other disconnected components are unresolved.
