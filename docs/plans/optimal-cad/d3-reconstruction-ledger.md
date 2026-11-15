# D3 reconstruction process-pilot audit ledger

Date: 2026-11-15. Scope: the legacy Python reconstruction CLI and nearby orchestration helpers. This is an audit, not a compatibility deprecation. No active source, test or CLI entry point was removed or modified for this batch.

## Baseline and decision

Physical lines include blanks, comments and docstrings, matching the PLAN inventory method.

- [drawing_flow.py](../../../products/krly3d/reconstruction/cli/drawing_flow.py): 381 lines; SHA-256 `95a0f2bf142cd71c8a708e2b4836342449a1412364e337326a527ff9974294b9`.
- [sheet_flow.py](../../../products/krly3d/reconstruction/cli/sheet_flow.py): 230 lines; SHA-256 `8affdd21b70a1160a9b78f2ffc6db2f430e8e1bb11beff3e2d30ae1ab479c27d`.

Both remain active, totaling 611 physical lines. No independent dead-code slice was proven. Gross removed = 0; added handwritten production = 0; added generated = 0; test delta = 0; net active source reduction = 0. The sole addition is this audit ledger, measured separately below. No new dependency was introduced.

## Consumer evidence

- Python import: `sheet_flow.py` imports `ROOT`, `interrupted` and `positive` directly from `drawing_flow` (line 21). These helpers remain necessary even before a sheet starts a child.
- CLI launch: `Sheet.part` builds `sys.executable …/drawing_flow.py` at line 105 and forwards the image, seed, height, pixel unit, output, shared remaining deadline and binary directory. Removing the single flow breaks the sheet compatibility CLI.
- Direct process tests: [drawing_flow_smoke.py](../../../tests/drawing_flow_smoke.py) names the script at line 17 and launches it through `command`/`run_flow`; its four tests cover real geometry, replayed inputs, holes/deadlines and cancellation.
- Direct sheet tests: [drawing_sheet_smoke.py](../../../tests/drawing_sheet_smoke.py) names the sheet script at line 23. Its process inspection at lines 126/135 also explicitly requires a real `drawing_flow.py` child. Ten tests cover sheet event/geometry identity, manifest validation, existing outputs, retained completed parts, duplicate components, common deadlines and process-group cancellation.
- Published CLI documentation: [README.md](../../../products/krly3d/reconstruction/cli/README.md) lines 3–7 explicitly retain the process compatibility slice; line 11 invokes the Python CLI. [SHEET.md](../../../products/krly3d/reconstruction/cli/SHEET.md) line 24 invokes the sheet CLI and documents its replay/event/cleanup contracts.
- Shared-host documentation: [SHARED-RUNTIME.md](../../SHARED-RUNTIME.md) around line 153 explicitly retains the Python sheet for the separate-process compatibility path and its verification. The native implementation's existence therefore does not establish replacement authorization or lack of consumers.
- Additional documentation consumers exist in HANDOFF, DEVELOPMENT-STATUS, BREP-PLANAR and PRODUCT-ARCHITECTURE-HANDOFF; historical architecture evidence was retained.

The search used `rg -n 'drawing_flow|sheet_flow|drawing-flow|sheet-flow'`, followed by a hidden-file scan for script paths/imports, excluding `.git`, `.tmp`, `.ctx`, build output and dependencies. Source inspection followed the actual Python import and subprocess command construction; references embedded in source-baseline/PLAN were treated as inventory evidence, not runtime consumers.

## Nearby helper and duplication review

AST inventory and manual call-site inspection found no unused definitions in either script. `Peer` initialization/RPC/call/notification/close all participate in the active flow. `Flow.remaining`, `emit`, `save`, `operation`, `mcp`, `planar`, `drawing`, `likeness_bridge` and `run` are reachable from `main`; the shared signal/scalar helpers are also imported by the sheet. Sheet manifest parsing uses `unique_object` through `json.loads(object_pairs_hook=…)`, `number` through `parts_file`, and every `Sheet` method is reached by normal execution or failure cleanup.

The nearby native [host-cli](../../../products/krly3d/reconstruction/host-cli/) and [runtime](../../../products/krly3d/reconstruction/runtime/) are active replacement routes, not proven dead copies. The Python flow still creates replay journals, invokes real MCP/domain executables and checks legacy likeness evidence. The sheet forwards and validates child protocol/identity/sequence, bounds child event lines at 1 MiB and coordinates independent process groups. The single flow bounds domain replies at 24 MiB and manages different RPC/artifact semantics. Similar deadline/event/cleanup code is not byte-identical or semantically interchangeable; extracting a shared framework without a concrete saving and parity proof is outside this audit.

The native/process parity suites remain valuable; `shared_runtime_smoke.py` directly invokes separate domain executables rather than importing either Python pilot. That distinction does not negate the dedicated pilot consumers above.

## Verification and deletion gate

The retained real-process suites were executed against the built debug binaries:

- `python3 tests/drawing_flow_smoke.py`: 4 tests passed (1.234 s).
- `python3 tests/drawing_sheet_smoke.py`: 10 tests passed (19.748 s).

The files' SHA-256 values were checked again afterward and match the baseline above; no source bytes changed.

Safe removal would first require explicit retirement of the documented process CLI or migration of its import/subprocess consumers, followed by demonstrated event/replay/artifact/error/deadline/cancellation parity for the selected replacement. Tests must be retained or migrated as independent behavior evidence. No such deprecation or migration was performed here.

No source was archived under `.tmp/refactor/d3-reconstruction/` because none was proven unused. No restore operation is necessary. To undo this audit alone, remove this newly added ledger; do not restore unrelated dirty workspace files.

Documentation delta: this ledger adds 47 physical lines.
