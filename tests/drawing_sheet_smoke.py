#!/usr/bin/env python3
"""Real-process drawing-sheet contract checks; use existing debug binaries.

Run with ``python3 tests/drawing_sheet_smoke.py``. No geometry peers are mocked
and no build is started. Lifecycle checks use Linux /proc to observe children.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
SHEET = ROOT / "products/krly3d/reconstruction/cli/sheet_flow.py"
BIN = ROOT / "target/debug"
PROTOCOL = "krly3d.drawing-sheet/1"
PIXEL_UNIT = 0.25
PARTS = [
    {"id": "Plate_1", "seed": [48.5, 14.5], "height": 2.5},
    {"id": "Ring-2", "seed": [6, 8], "height": 7},
]


def descendants(pid):
    """Return live descendants with start times, protecting against PID reuse."""
    found = {}
    pending = [pid]
    while pending:
        parent = pending.pop()
        try:
            children = Path(f"/proc/{parent}/task/{parent}/children").read_text().split()
        except (FileNotFoundError, ProcessLookupError):
            continue
        for child in map(int, children):
            state = process_identity(child)
            if state is not None and child not in found:
                found[child] = state[1]
                pending.append(child)
    return found


def process_identity(pid):
    try:
        # The command name in parentheses may itself contain spaces.
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return fields[0], fields[19]  # state and starttime (field 22)
    except (FileNotFoundError, ProcessLookupError):
        return None


def alive(pid, started):
    current = process_identity(pid)
    return current is not None and current[1] == started and current[0] != "Z"


class LiveSheet:
    """Bounded NDJSON reads and cleanup, including when a test assertion fails."""

    def __init__(self, command):
        self.stderr = tempfile.TemporaryFile()
        self.process = subprocess.Popen(
            command, stdout=subprocess.PIPE, stderr=self.stderr, start_new_session=True
        )
        self.buffer = b""
        self.events = []
        self.observed = {}
        self.timers = []

    def __enter__(self):
        return self

    def read(self, timeout):
        deadline = time.monotonic() + max(0, timeout)
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.process.stdout], [], [], remaining)[0]:
                raise AssertionError("timed out waiting for drawing-sheet NDJSON")
            chunk = os.read(self.process.stdout.fileno(), 65536)
            if not chunk:
                if self.buffer:
                    raise AssertionError(f"unterminated NDJSON: {self.buffer!r}")
                return None
            self.buffer += chunk
        line, self.buffer = self.buffer.split(b"\n", 1)
        event = json.loads(line)
        self.events.append(event)
        self.observed.update(descendants(self.process.pid))
        return event

    def until(self, predicate, timeout=20):
        deadline = time.monotonic() + timeout
        while True:
            event = self.read(deadline - time.monotonic())
            if event is None:
                raise AssertionError(f"sheet exited before expected event: {self.events!r}; {self.error()}")
            if predicate(event):
                return event

    def finish(self, timeout=10):
        deadline = time.monotonic() + timeout
        while self.read(deadline - time.monotonic()) is not None:
            pass
        self.process.wait(timeout=max(0.01, deadline - time.monotonic()))

    def error(self):
        self.stderr.seek(0)
        return self.stderr.read().decode(errors="replace")

    def stop_flow(self):
        """Pause the real direct child while its parent's deadline keeps ticking."""
        children = Path(
            f"/proc/{self.process.pid}/task/{self.process.pid}/children"
        ).read_text().split()
        for child in map(int, children):
            try:
                command = Path(f"/proc/{child}/cmdline").read_bytes().split(b"\0")
                if any(Path(os.fsdecode(arg)).name == "drawing_flow.py" for arg in command if arg):
                    os.kill(child, signal.SIGSTOP)
                    identity = process_identity(child)
                    if identity is not None:
                        self.observed[child] = identity[1]
                        self.observed.update(descendants(child))
                        return child
            except (FileNotFoundError, ProcessLookupError):
                continue
        raise AssertionError("no active real drawing_flow.py child at forwarded started event")

    def resume_at(self, pid, deadline):
        # Let the real child handle pending termination after the deadline. A
        # permanently SIGSTOPped orchestrator cannot clean up separate peers.
        def resume():
            try:
                os.kill(pid, signal.SIGCONT)
            except ProcessLookupError:
                pass
        timer = threading.Timer(max(0, deadline - time.monotonic()), resume)
        self.timers.append(timer)
        timer.start()

    def __exit__(self, *_):
        for timer in self.timers:
            timer.cancel()
        self.observed.update(descendants(self.process.pid))
        # Each domain peer may own a separate session; kill observed descendants
        # as well as the sheet group if the test fails while one is SIGSTOPped.
        for pid, started in self.observed.items():
            if alive(pid, started):
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        try:
            os.killpg(self.process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.process.wait(timeout=5)
        self.process.stdout.close()
        self.stderr.close()


class DrawingSheet(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # A missing implementation must be visible as an error, not a green skip.
        if not SHEET.is_file():
            raise RuntimeError(f"sheet CLI is not ready: {SHEET}")
        missing = [name for name in ("krly3d", "krlyvis", "krly3d-mesh", "krly3d-view")
                   if not os.access(BIN / name, os.X_OK)]
        if missing:
            raise RuntimeError(f"required built executables unavailable in {BIN}: {missing}")

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.image = self.root / "disconnected.pgm"
        # The ring is larger. Requesting the small plate first catches fallback
        # to largest-component selection, and the gap catches merged outlines.
        pixels = bytes(
            0 if ((4 <= x < 28 and 6 <= y < 38
                   and not (12 <= x < 20 and 16 <= y < 28))
                  or (44 <= x < 64 and 10 <= y < 26)) else 255
            for y in range(48) for x in range(72)
        )
        self.image.write_bytes(b"P5 72 48 255\n" + pixels)
        self.parts_file = self.root / "parts.json"
        self.write_parts(PARTS)
        self.output = self.root / "sheet"

    def write_parts(self, parts):
        self.parts_file.write_text(json.dumps(parts) + "\n")

    def command(self, output=None, *extra):
        return [sys.executable, str(SHEET), "--image", str(self.image),
                "--parts", str(self.parts_file), "--pixel-unit", str(PIXEL_UNIT),
                "--output", str(output if output is not None else self.output),
                "--bin-dir", str(BIN), *extra]

    def run_sheet(self, output=None, *extra):
        result = subprocess.run(self.command(output, *extra), capture_output=True,
                                text=True, timeout=45, cwd=self.root)
        events = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertTrue(all(isinstance(event, dict) for event in events), result.stdout)
        self.assertTrue(all(event.get("protocol") == PROTOCOL for event in events), result.stdout)
        return result, events

    def manifest(self):
        return json.loads((self.output / "manifest.json").read_text())

    def assert_reference(self, reference, expected, *bases):
        value = reference["path"] if isinstance(reference, dict) else reference
        self.assertIsInstance(value, str)
        path = Path(value)
        candidates = [path.resolve()] if path.is_absolute() else [(base / path).resolve() for base in bases]
        self.assertIn(expected.resolve(), candidates)
        self.assertTrue(expected.is_file() if expected.suffix else expected.is_dir())

    def assert_part(self, entry, part, *, area, holes, low, high):
        part_output = self.output / "parts" / part["id"]
        self.assertEqual(entry["id"], part["id"])
        self.assertEqual(entry["status"], "completed")
        self.assertIs(entry["matches"], True)
        self.assert_reference(entry["output"], part_output, self.output)
        self.assert_reference(entry["proposal"], part_output / "proposal.k3d", self.output, part_output)
        self.assert_reference(entry["preview"], part_output / "preview.png", self.output, part_output)
        for name in ("drawing.pgm", "assumptions.json", "evidence.json", "coarse.k3d",
                     "coarse-mesh.json", "proposal.k3d", "proposal-mesh.json", "solid-check.json",
                     "preview.png", "likeness.json", "events.jsonl", "rpc.jsonl"):
            self.assertTrue((part_output / name).is_file(), name)
        self.assertEqual((part_output / "drawing.pgm").read_bytes(), self.image.read_bytes())
        self.assertTrue((part_output / "preview.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))
        assumptions = json.loads((part_output / "assumptions.json").read_text())
        self.assertEqual(assumptions["pixel_unit"], PIXEL_UNIT)
        self.assertEqual(assumptions["height"], part["height"])
        solid = json.loads((part_output / "solid-check.json").read_text())
        self.assertEqual(entry["solid_check"], solid)
        volume = area * PIXEL_UNIT ** 2 * part["height"]
        self.assertEqual(solid["hole_count"], holes)
        self.assertAlmostEqual(solid["expected_volume"], volume)
        self.assertAlmostEqual(solid["mesh_volume"], volume)
        self.assertEqual(solid["open_edges"], 0)
        self.assertIs(solid["matches"], True)
        evidence = json.loads((part_output / "evidence.json").read_text())
        self.assertAlmostEqual(evidence["material_area"], area * PIXEL_UNIT ** 2)
        self.assertEqual(len(evidence["polygons"]), holes + 1)
        # Reload the editable file in an independent real domain process. Its
        # vertices prove positions and heights survived serialization as well.
        request = {"protocol": {"major": 2, "minor": 0}, "api": {"major": 2, "minor": 0},
                   "request_id": "sheet-reload-" + part["id"],
                   "input": {"node": "part", "triangles": True}}
        reload = subprocess.run([str(BIN / "krly3d-mesh"), "--model", str(part_output / "proposal.k3d")],
                                input=json.dumps(request), text=True, capture_output=True, timeout=20)
        self.assertEqual(reload.returncode, 0, reload.stderr)
        mesh = json.loads(reload.stdout.splitlines()[-1])["body"]["result"]
        self.assertAlmostEqual(mesh["volume"], volume)
        self.assertEqual(mesh["open"], 0)
        points = mesh["mesh"]["data"]["points"]
        for axis in range(3):
            self.assertAlmostEqual(min(p[axis] for p in points), low[axis])
            self.assertAlmostEqual(max(p[axis] for p in points), high[axis])

    def assert_no_sheet_completion(self, events):
        # Nested child completions remain legitimate after a partial failure.
        self.assertNotIn("completed", [event["kind"] for event in events])

    def test_two_explicit_components_preserve_holes_height_position_and_event_order(self):
        result, events = self.run_sheet()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(events[0]["kind"], "started")
        self.assertEqual(events[-1]["kind"], "completed")
        self.assertEqual(sum(e["kind"] == "started" for e in events), 1)
        self.assertEqual(sum(e["kind"] == "completed" for e in events), 1)
        self.assertEqual([e["seq"] for e in events], list(range(len(events))))
        manifest = self.manifest()
        self.assertEqual(manifest["status"], "completed")
        self.assertEqual([p["id"] for p in manifest["parts"]], [p["id"] for p in PARTS])
        expected = ["started"]
        for part in PARTS:
            original = [json.loads(line) for line in
                        (self.output / "parts" / part["id"] / "events.jsonl").read_text().splitlines()]
            forwarded = [e["event"] for e in events
                         if e["kind"] == "part_event" and e["part_id"] == part["id"]]
            self.assertEqual(forwarded, original)
            self.assertEqual(original[0]["kind"], "started")
            self.assertEqual(original[-1]["kind"], "completed")
            self.assertTrue(all(e["protocol"] == "krly3d.drawing-flow/1" for e in original))
            entry = next(p for p in manifest["parts"] if p["id"] == part["id"])
            self.assertEqual(entry["artifact_id"], original[-1]["artifact_id"])
            self.assertEqual(entry["revision"], original[-1]["revision"])
            expected.extend(["part_event"] * len(original) + ["part_completed"])
        expected.append("completed")
        self.assertEqual([e["kind"] for e in events], expected)
        self.assertEqual([e["part"]["id"] for e in events if e["kind"] == "part_completed"],
                         [p["id"] for p in PARTS])
        self.assertEqual([e["part_id"] for e in events if e["kind"] == "part_event"],
                         [p["id"] for p in PARTS for _ in
                          (self.output / "parts" / p["id"] / "events.jsonl").read_text().splitlines()])
        self.assert_part(manifest["parts"][0], PARTS[0], area=20 * 16, holes=0,
                         low=[11, 2.5, 0], high=[16, 6.5, 2.5])
        self.assert_part(manifest["parts"][1], PARTS[1], area=24 * 32 - 8 * 12, holes=1,
                         low=[1, 1.5, 0], high=[7, 9.5, 7])
        # Replaying into the same completed sheet must preserve every artifact.
        before = self.snapshot(self.output)
        again, rejected = self.run_sheet()
        self.assertNotEqual(again.returncode, 0)
        self.assert_no_sheet_completion(rejected)
        self.assertEqual(self.snapshot(self.output), before)

    @staticmethod
    def snapshot(directory):
        return {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in directory.rglob("*") if p.is_file()}

    def test_parts_schema_rejects_invalid_values_before_starting_children(self):
        valid = PARTS[0]
        cases = [("object", valid), ("null", None), ("empty", []),
                 ("too_many", [dict(valid, id=f"P{i}") for i in range(17)]),
                 ("non_object", ["part"]), ("duplicate", [valid, dict(valid)]),
                 ("extra_key", [dict(valid, assembly=True)])]
        for key in ("id", "seed", "height"):
            cases.append(("missing_" + key, [{k: v for k, v in valid.items() if k != key}]))
        for value in ("", "1plate", "../escape", "a/b", "a\\b", "/absolute", ".", "..",
                      "a.b", "a b", "é", "A" * 65, None, 12, True):
            cases.append((f"id_{value!r}", [dict(valid, id=value)]))
        for value in (None, [], [1], [1, 2, 3], "1,2", {"x": 1, "y": 2},
                      [-1, 2], [1, -0.5], [math.nan, 2], [1, math.inf],
                      [-math.inf, 2], [True, 2], [1, False], ["1", 2]):
            cases.append((f"seed_{value!r}", [dict(valid, seed=value)]))
        for value in (0, -1, math.nan, math.inf, -math.inf, None, True, "2", []):
            cases.append((f"height_{value!r}", [dict(valid, height=value)]))
        for index, (label, parts) in enumerate(cases):
            with self.subTest(case=label):
                self.write_parts(parts)
                output = self.root / f"invalid-{index}"
                result, events = self.run_sheet(output)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assert_no_sheet_completion(events)
                self.assertFalse(any(e["kind"] in ("part_event", "part_completed") for e in events))
                self.assertFalse(any(output.rglob("proposal.k3d")))
                self.assertFalse((self.root / "escape").exists())

    def test_malformed_parts_json_is_rejected(self):
        for index, text in enumerate(("[{", "[] trailing", "")):
            with self.subTest(text=text):
                self.parts_file.write_text(text)
                result, events = self.run_sheet(self.root / f"malformed-{index}")
                self.assertNotEqual(result.returncode, 0)
                self.assert_no_sheet_completion(events)
                self.assertFalse(any(e["kind"] == "part_event" for e in events))

    def test_pixel_scale_and_timeout_require_finite_positive_values(self):
        for option in ("--pixel-unit", "--timeout"):
            for index, value in enumerate(("0", "-1", "nan", "inf", "-inf")):
                with self.subTest(option=option, value=value):
                    result, events = self.run_sheet(self.root / f"{option[2:]}-{index}",
                                                    option + "=" + value)
                    self.assertNotEqual(result.returncode, 0)
                    self.assert_no_sheet_completion(events)
                    self.assertFalse(any(e["kind"] == "part_event" for e in events))

    def test_existing_directory_file_and_symlink_are_never_overwritten(self):
        directory = self.root / "existing"
        directory.mkdir()
        sentinel = directory / "manifest.json"
        sentinel.write_bytes(b"preserve existing manifest\n")
        regular = self.root / "existing-file"
        regular.write_bytes(b"preserve existing file\n")
        link = self.root / "existing-link"
        link.symlink_to(directory, target_is_directory=True)
        empty = self.root / "existing-empty"
        empty.mkdir()
        for output in (directory, regular, link, empty):
            with self.subTest(output=output.name):
                result, events = self.run_sheet(output)
                self.assertNotEqual(result.returncode, 0)
                self.assert_no_sheet_completion(events)
                self.assertEqual(sentinel.read_bytes(), b"preserve existing manifest\n")
                self.assertEqual(regular.read_bytes(), b"preserve existing file\n")
                self.assertTrue(link.is_symlink())
                self.assertEqual(list(empty.iterdir()), [])
                self.assertEqual(list(directory.iterdir()), [sentinel])

    def test_invalid_second_seed_preserves_completed_first_part_without_false_completion(self):
        # These are syntactically valid seeds: background, hole interior, and
        # outside the raster. None may silently select a different component.
        for index, seed in enumerate(([36, 42], [16, 20], [80, 5])):
            with self.subTest(seed=seed):
                self.output = self.root / f"partial-{index}"
                self.write_parts([PARTS[0], dict(PARTS[1], seed=seed)])
                result, events = self.run_sheet()
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assert_no_sheet_completion(events)
                manifest = self.manifest()
                self.assertEqual(manifest["status"], "failed")
                finished = [p for p in manifest["parts"] if p["status"] == "completed"]
                self.assertEqual([p["id"] for p in finished], [PARTS[0]["id"]])
                self.assert_part(finished[0], PARTS[0], area=20 * 16, holes=0,
                                 low=[11, 2.5, 0], high=[16, 6.5, 2.5])
                self.assertEqual([e["part"]["id"] for e in events if e["kind"] == "part_completed"],
                                 [PARTS[0]["id"]])
                second = [e["event"] for e in events
                          if e["kind"] == "part_event" and e["part_id"] == PARTS[1]["id"]]
                self.assertNotIn("completed", [e["kind"] for e in second])
                self.assertFalse((self.output / "parts" / PARTS[1]["id"] / "proposal.k3d").exists())

    def test_two_seeds_in_same_component_fail_after_second_child_without_double_assignment(self):
        self.write_parts([PARTS[0], dict(PARTS[1], seed=[54, 18])])
        result, events = self.run_sheet()
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assert_no_sheet_completion(events)
        manifest = self.manifest()
        self.assertEqual(manifest["status"], "failed")
        self.assertEqual([p["id"] for p in manifest["parts"] if p["status"] == "completed"],
                         [PARTS[0]["id"]])
        self.assertEqual([e["part"]["id"] for e in events if e["kind"] == "part_completed"],
                         [PARTS[0]["id"]])
        second = [e["event"] for e in events
                  if e["kind"] == "part_event" and e["part_id"] == PARTS[1]["id"]]
        self.assertEqual(second[-1]["kind"], "completed")
        self.assertTrue((self.output / "parts" / PARTS[1]["id"] / "solid-check.json").is_file())
        self.assert_part(manifest["parts"][0], PARTS[0], area=20 * 16, holes=0,
                         low=[11, 2.5, 0], high=[16, 6.5, 2.5])

    def test_immediate_deadline_fails_without_sheet_completion(self):
        result, events = self.run_sheet(None, "--timeout", "0.000001")
        self.assertNotEqual(result.returncode, 0)
        self.assert_no_sheet_completion(events)
        self.assertEqual(self.manifest()["status"], "failed")

    @unittest.skipUnless(sys.platform.startswith("linux"), "observes real children through /proc")
    def test_deadline_is_shared_across_parts_and_reaps_stalled_child(self):
        timeout = 12
        start = time.monotonic()
        with LiveSheet(self.command(None, "--timeout", str(timeout))) as live:
            live.until(lambda e: e["kind"] == "part_event" and e["part_id"] == PARTS[0]["id"]
                       and e["event"]["kind"] == "started")
            first = live.stop_flow()
            time.sleep(4)  # spend known batch budget without depending on renderer speed
            os.kill(first, signal.SIGCONT)
            live.until(lambda e: e["kind"] == "part_event" and e["part_id"] == PARTS[1]["id"]
                       and e["event"]["kind"] == "started", timeout=timeout)
            second = live.stop_flow()
            live.resume_at(second, start + timeout + 0.5)
            live.finish(timeout=timeout + 5)
            elapsed = time.monotonic() - start
            self.assertNotEqual(live.process.returncode, 0, live.error())
            self.assertLess(elapsed, timeout + 3.5, "deadline was reset for the second part")
            self.assert_no_sheet_completion(live.events)
            manifest = self.manifest()
            self.assertEqual(manifest["status"], "failed")
            self.assertEqual([p["id"] for p in manifest["parts"] if p["status"] == "completed"],
                             [PARTS[0]["id"]])
            self.assert_children_gone(live)

    def assert_children_gone(self, live):
        self.assertTrue(live.observed, "test did not observe a real child")
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            remaining = [pid for pid, started in live.observed.items() if alive(pid, started)]
            if not remaining:
                return
            time.sleep(0.02)
        self.fail(f"sheet left live descendants behind: {remaining}")

    @unittest.skipUnless(sys.platform.startswith("linux"), "observes real children through /proc")
    def test_cancellation_terminates_active_process_group_and_preserves_finished_part(self):
        for signum in (signal.SIGINT, signal.SIGTERM):
            with self.subTest(signal=signum):
                self.output = self.root / f"cancel-{signum}"
                with LiveSheet(self.command()) as live:
                    live.until(lambda e: e["kind"] == "part_event" and e["part_id"] == PARTS[1]["id"]
                               and e["event"]["kind"] == "coarse_preview")
                    second = live.stop_flow()
                    first_output = self.output / "parts" / PARTS[0]["id"]
                    before = self.snapshot(first_output)
                    live.process.send_signal(signum)
                    os.kill(second, signal.SIGCONT)
                    live.finish(timeout=10)
                    self.assertNotEqual(live.process.returncode, 0, live.error())
                    self.assert_no_sheet_completion(live.events)
                    manifest = self.manifest()
                    self.assertEqual(manifest["status"], "cancelled")
                    self.assertEqual([p["id"] for p in manifest["parts"] if p["status"] == "completed"],
                                     [PARTS[0]["id"]])
                    self.assertEqual([e["part"]["id"] for e in live.events if e["kind"] == "part_completed"],
                                     [PARTS[0]["id"]])
                    self.assertEqual(self.snapshot(first_output), before)
                    self.assert_children_gone(live)


if __name__ == "__main__":
    unittest.main()
