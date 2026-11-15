#!/usr/bin/env python3
"""Real-process smoke checks for native ``krly3d-reconstruct --parts``.

Run after building ``target/debug/krly3d-reconstruct``. This module uses only
the Python standard library and generates its PNM/PNG fixtures directly. It
never builds the CLI. Checks native geometry, retained failures and replay from
the saved input snapshot without requiring the original files.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib


ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / "target/debug/krly3d-reconstruct"
PROTOCOL = "krly3d.shared-sheet/1"
PIXEL_UNIT = 0.25
STAGES = ["evidence", "coarse_preview", "editable_proposal", "preview", "validation"]
PART_EXPECTATIONS = {
    "Ring": {
        "pixel_area": 24 * 24 - 10 * 10,
        "holes": 1,
        "xy_bounds": ((1.0, 7.0), (1.25, 7.25)),
        "height": 2.5,
    },
    "Bar": {
        "pixel_area": 12 * 9,
        "holes": 0,
        "xy_bounds": ((9.75, 12.75), (2.0, 4.25)),
        "height": 6.0,
    },
}

# The first component is a rectangular ring; the second is a disconnected bar.
# Their nonzero and distinct pixel coordinates make lost drawing placement
# visible in the native mesh and evidence artifacts.
WIDTH = 64
HEIGHT = 40
RING = {"id": "Ring", "seed": [6, 7], "height": 2.5}
BAR = {"id": "Bar", "seed": [42, 11], "height": 6.0}
PENDING = {"id": "Pending", "seed": [42, 11], "height": 3.0}


def material(x, y):
    ring = (4 <= x < 28 and 5 <= y < 29
            and not (10 <= x < 20 and 11 <= y < 21))
    bar = 39 <= x < 51 and 8 <= y < 17
    return ring or bar


def raster(image_format):
    pixels = bytes(0 if material(x, y) else 255
                   for y in range(HEIGHT) for x in range(WIDTH))
    if image_format == "pnm":
        return f"P5 {WIDTH} {HEIGHT} 255\n".encode("ascii") + pixels
    if image_format != "png":
        raise ValueError(f"unsupported fixture format: {image_format}")

    def chunk(name, data):
        return (struct.pack(">I", len(data)) + name + data
                + struct.pack(">I", zlib.crc32(name + data)))

    rows = b"".join(b"\0" + pixels[y * WIDTH:(y + 1) * WIDTH]
                    for y in range(HEIGHT))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows))
            + chunk(b"IEND", b""))


class SharedSheetSmoke(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not CLI.is_file() or not CLI.stat().st_mode & 0o111:
            raise RuntimeError(f"built native CLI unavailable: {CLI}")

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.image = self.make_image("drawing", "pnm")
        self.parts_file = self.root / "parts.json"
        self.output = self.root / "sheet"

    def make_image(self, name, image_format):
        suffix = ".png" if image_format == "png" else ".pgm"
        path = self.root / (name + suffix)
        path.write_bytes(raster(image_format))
        return path

    def write_parts(self, parts):
        self.parts_file.write_text(json.dumps(parts, allow_nan=True) + "\n")

    def command(self, image=None, output=None, parts_file=None, extra=()):
        command = [CLI, "--image", image or self.image,
                   "--parts", parts_file or self.parts_file,
                   "--pixel-unit", str(PIXEL_UNIT)]
        command.extend(("--output", self.output if output is None else output))
        command.extend(extra)
        return list(map(str, command))

    def invoke(self, command, *, success=True, timeout=150):
        result = subprocess.run(command, cwd=self.root, capture_output=True,
                                text=True, timeout=timeout)
        if success:
            self.assertEqual(result.returncode, 0,
                             f"command: {command}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}")
        else:
            self.assertNotEqual(result.returncode, 0,
                                f"command unexpectedly succeeded: {command}\n{result.stdout}")
        events = []
        for line in result.stdout.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError as error:
                self.fail(f"stdout is not NDJSON ({error}): {line!r}")
            self.assertIsInstance(event, dict, line)
            self.assertEqual(event.get("protocol"), PROTOCOL, line)
            events.append(event)
        return result, events

    def assert_journal(self, output, result, events):
        text = (Path(output) / "events.jsonl").read_text()
        self.assertEqual(text, result.stdout)
        self.assertEqual([json.loads(line) for line in text.splitlines()], events)
        self.assertEqual([event["seq"] for event in events], list(range(len(events))))
        self.assertEqual([event["elapsed_ms"] for event in events],
                         sorted(event["elapsed_ms"] for event in events))
        self.assertEqual(len({event["sheet_id"] for event in events}), 1)

    @staticmethod
    def wrappers(events, part_id):
        return [event["event"] for event in events
                if event.get("kind") == "part_event" and event.get("part_id") == part_id]

    @staticmethod
    def indices(events, kind, part_id=None):
        return [index for index, event in enumerate(events)
                if event.get("kind") == kind
                and (part_id is None or event.get("part_id") == part_id)]

    def read_manifest(self, output):
        return json.loads((Path(output) / "manifest.json").read_text())

    def assert_completed_part(self, output, image, part, entry, *, image_hash):
        output = Path(output)
        part_id = part["id"]
        expected = PART_EXPECTATIONS[part_id]
        relative_output = f"parts/{part_id}"
        self.assertEqual(entry["id"], part_id)
        self.assertEqual(entry["seed"], part["seed"])
        self.assertEqual(entry["height"], part["height"])
        self.assertEqual(entry["status"], "completed")
        self.assertEqual(entry["output"], relative_output)
        self.assertIsNone(entry["error"])

        artifact_names = {
            "evidence": "evidence.json",
            "coarse_mesh": "coarse-mesh.json",
            "model": "proposal.k3d",
            "mesh": "mesh.json",
            "preview": "preview.png",
            "likeness": "likeness.json",
        }
        expected_artifacts = {
            key: f"{relative_output}/{name}" for key, name in artifact_names.items()
        }
        self.assertEqual(entry["artifacts"], expected_artifacts)
        paths = {key: output / value for key, value in entry["artifacts"].items()}
        for key, path in paths.items():
            self.assertTrue(path.is_file(), f"{part_id}: missing {key}: {path}")

        result = entry["result"]
        expected_area = expected["pixel_area"] * PIXEL_UNIT ** 2
        expected_volume = expected_area * part["height"]
        self.assertEqual(result["height"], part["height"])
        self.assertEqual(result["pixel_unit"], PIXEL_UNIT)
        self.assertEqual(result["seed"], part["seed"])
        self.assertEqual(result["source"], str(image))
        self.assertEqual(result["image_sha256"], image_hash)
        self.assertEqual(result["artifacts"], entry["artifacts"])
        self.assertEqual(result["provenance"], entry["provenance"])
        self.assertEqual(result["matches"], entry["matches"])
        self.assertEqual(result["open"], 0)
        self.assertAlmostEqual(result["volume"], expected_volume, places=8)
        self.assertAlmostEqual(result["expected_volume"], expected_volume, places=8)
        self.assertAlmostEqual(entry["volume"], expected_volume, places=8)
        self.assertAlmostEqual(entry["expected_volume"], expected_volume, places=8)
        self.assertEqual(entry["provenance"]["source"], str(image))
        self.assertEqual(entry["provenance"]["seed"], part["seed"])
        self.assertEqual(entry["provenance"]["pixel_unit"], PIXEL_UNIT)
        self.assertEqual(entry["provenance"]["image_sha256"], image_hash)

        evidence = json.loads(paths["evidence"].read_text())
        self.assertEqual(evidence["provenance"]["image_sha256"], image_hash)
        self.assertEqual(evidence["provenance"]["source"], str(image))
        self.assertEqual(evidence["provenance"]["seed"], part["seed"])
        self.assertEqual(evidence["provenance"]["pixel_unit"], PIXEL_UNIT)
        self.assertAlmostEqual(evidence["evidence"]["unit"], PIXEL_UNIT)
        self.assertAlmostEqual(evidence["evidence"]["material_area"], expected_area, places=8)
        polygons = evidence["evidence"]["polygons"]
        self.assertEqual(len(polygons), expected["holes"] + 1)
        world_points = [point for polygon in polygons for point in polygon]
        self.assertTrue(world_points)
        for axis, (low, high) in enumerate(expected["xy_bounds"]):
            self.assertAlmostEqual(min(point[axis] for point in world_points), low, places=8)
            self.assertAlmostEqual(max(point[axis] for point in world_points), high, places=8)

        mesh = json.loads(paths["mesh"].read_text())
        self.assertEqual(mesh["open"], 0)
        self.assertAlmostEqual(mesh["volume"], expected_volume, places=8)
        self.assertEqual(mesh["provenance"]["source"], result["brep_source"])
        points = mesh["mesh"]["data"]["points"]
        triangles = mesh["mesh"]["data"]["triangles"]
        self.assertTrue(points)
        self.assertTrue(triangles)
        self.assertEqual(mesh["points"], len(points))
        self.assertEqual(mesh["triangles"], len(triangles))
        self.assertTrue(all(len(point) == 3 and all(math.isfinite(v) for v in point)
                            for point in points))
        self.assertTrue(all(len(triangle) == 3 and all(
            type(index) is int and 0 <= index < len(points) for index in triangle)
                            for triangle in triangles))
        # Recompute signed volume from the actual triangles, independently of
        # the runtime's reported volume and the evidence-area multiplication.
        volumes = []
        top_area = 0.0
        for indices in triangles:
            a, b, c = (points[index] for index in indices)
            volumes.append((a[0] * (b[1] * c[2] - b[2] * c[1])
                            + a[1] * (b[2] * c[0] - b[0] * c[2])
                            + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6)
            if all(abs(p[2] - part["height"]) < 1e-8 for p in (a, b, c)):
                top_area += abs((b[0] - a[0]) * (c[1] - a[1])
                                - (b[1] - a[1]) * (c[0] - a[0])) / 2
        self.assertAlmostEqual(abs(math.fsum(volumes)), expected_volume, places=8)
        self.assertAlmostEqual(top_area, expected_area, places=8,
                               msg="top triangles must preserve the ring hole")
        for axis, (low, high) in enumerate(expected["xy_bounds"]):
            self.assertAlmostEqual(min(point[axis] for point in points), low, places=8)
            self.assertAlmostEqual(max(point[axis] for point in points), high, places=8)
        self.assertAlmostEqual(min(point[2] for point in points), 0.0, places=8)
        self.assertAlmostEqual(max(point[2] for point in points), part["height"], places=8)

        self.assertTrue(paths["preview"].read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))
        likeness = json.loads(paths["likeness"].read_text())
        self.assertIs(type(entry["matches"]), bool)
        self.assertIs(entry["matches"], True)
        self.assertIs(likeness["matches"], entry["matches"])
        preview_source = entry["preview_provenance"]["source"]
        likeness_source = likeness["provenance"]["source"]
        self.assertTrue(preview_source["source"])
        self.assertEqual(preview_source["source"], result["brep_source"])
        self.assertEqual(likeness_source, preview_source)
        self.assertTrue(preview_source["session"])
        self.assertTrue(preview_source["build_key"])
        self.assertEqual(likeness["provenance"]["node"], preview_source["node"])
        self.assertEqual(likeness["provenance"]["reference"]["polygons"], polygons)
        self.assertEqual(likeness["provenance"]["reference"]["source"], "sha256:" + image_hash)

        return paths

    def assert_sheet_artifacts(self, output, image, parts, result, events):
        output = Path(output)
        manifest = self.read_manifest(output)
        image_hash = hashlib.sha256(Path(image).read_bytes()).hexdigest()
        image_format = "png" if Path(image).suffix.lower() == ".png" else "pnm"
        self.assertEqual(manifest["protocol"], PROTOCOL)
        self.assertEqual(manifest["status"], "completed")
        self.assertEqual(manifest["source"], str(image))
        self.assertEqual(manifest["pixel_unit"], PIXEL_UNIT)
        self.assertEqual(manifest["image_sha256"], image_hash)
        self.assertEqual(manifest["provenance"]["source"], str(image))
        self.assertEqual(manifest["provenance"]["pixel_unit"], PIXEL_UNIT)
        self.assertEqual(manifest["provenance"]["image_sha256"], image_hash)
        self.assertEqual(manifest["artifacts"], {
            "events": "events.jsonl", "input": f"input.{image_format}", "parts": "parts.json"
        })
        self.assertEqual(manifest["replay"]["image"], manifest["artifacts"]["input"])
        self.assertEqual(manifest["replay"]["parts"], manifest["artifacts"]["parts"])
        self.assertEqual(manifest["replay"]["pixel_unit"], PIXEL_UNIT)
        self.assertEqual(manifest["replay"]["tolerance"], 0.05)
        self.assertEqual((output / manifest["replay"]["image"]).read_bytes(),
                         Path(image).read_bytes())
        self.assertEqual(json.loads((output / manifest["replay"]["parts"]).read_text()), parts)
        self.assertEqual([part["id"] for part in manifest["parts"]],
                         [part["id"] for part in parts])
        self.assert_journal(output, result, events)
        self.assertEqual(events[-1]["manifest"], manifest)

        brep_sources = []
        for part in parts:
            part_id = part["id"]
            entry = next(record for record in manifest["parts"] if record["id"] == part_id)
            self.assert_completed_part(output, image, part, entry, image_hash=image_hash)
            brep_sources.append(entry["result"]["brep_source"])
            wrapped = self.wrappers(events, part_id)
            stages = {event["kind"]: event for event in wrapped}
            self.assertEqual(stages["evidence"]["image_sha256"], image_hash)
            self.assertEqual(stages["evidence"]["artifact"], entry["artifacts"]["evidence"])
            self.assertEqual(stages["coarse_preview"]["artifact"], entry["artifacts"]["coarse_mesh"])
            self.assertEqual(stages["editable_proposal"]["artifact"], entry["artifacts"]["model"])
            self.assertEqual(stages["editable_proposal"]["mesh"], entry["artifacts"]["mesh"])
            coarse = stages["coarse_preview"]
            editable = stages["editable_proposal"]
            self.assertEqual(coarse["artifact_id"], editable["artifact_id"])
            self.assertEqual(coarse["artifact_revision"], 1)
            self.assertEqual(editable["artifact_revision"], 2)
            self.assertEqual(editable["replaces_revision"], 1)
            self.assertEqual(stages["preview"]["artifact"], entry["artifacts"]["preview"])
            self.assertEqual(stages["preview"]["provenance"], entry["preview_provenance"])
            self.assertEqual(stages["validation"]["artifact"], entry["artifacts"]["likeness"])
            self.assertIs(stages["validation"]["matches"], entry["matches"])

        completed_events = [event for event in events if event["kind"] == "part_completed"]
        self.assertEqual([event["part_id"] for event in completed_events],
                         [part["id"] for part in parts])
        for event, part in zip(completed_events, parts):
            entry = next(record for record in manifest["parts"] if record["id"] == part["id"])
            self.assertEqual(event["result"], entry["result"])
        self.assertEqual(len(set(brep_sources)), len(parts),
                         "each selected part must retain its own editable source identity")
        self.assertEqual(result.stdout, (output / "events.jsonl").read_text())
        return manifest

    def assert_successful_sheet_events(self, events, part_ids):
        self.assertTrue(events, "successful invocation emitted no sheet events")
        self.assertEqual(events[0].get("kind"), "started")
        self.assertEqual(events[-1].get("kind"), "completed")
        self.assertEqual(sum(event.get("kind") == "started" for event in events), 1)
        self.assertEqual(sum(event.get("kind") == "completed" for event in events), 1)
        self.assertEqual([event["seq"] for event in events], list(range(len(events))))
        self.assertTrue(all(event["revision"] == 1 for event in events))
        sheet_ids = {event["sheet_id"] for event in events}
        self.assertEqual(len(sheet_ids), 1)
        self.assertTrue(next(iter(sheet_ids)))
        for event in events:
            self.assertTrue(math.isfinite(event["elapsed_ms"]))
            self.assertTrue(math.isfinite(event["stage_ms"]))
            self.assertGreaterEqual(event["elapsed_ms"], 0)
            self.assertGreaterEqual(event["stage_ms"], 0)
        kinds = [event.get("kind") for event in events]
        self.assertEqual(kinds, ["started"] +
                         [kind for _ in part_ids for kind in
                          ["part_event"] * len(STAGES) + ["part_completed"]] + ["completed"])
        self.assertEqual(events[0].get("part_count"), len(part_ids))
        self.assertTrue(set(kinds).issubset(
            {"started", "part_event", "part_completed", "completed"}))
        self.assertEqual(kinds.count("part_completed"), len(part_ids))
        self.assertEqual([event.get("part_id") for event in events
                          if event.get("kind") == "part_event"],
                         [part_id for part_id in part_ids
                          for _ in self.wrappers(events, part_id)])
        for part_id in part_ids:
            wrapped = self.wrappers(events, part_id)
            inner_kinds = [event.get("kind") for event in wrapped]
            positions = []
            for stage in STAGES:
                self.assertEqual(inner_kinds.count(stage), 1,
                                 f"{part_id}: {inner_kinds}")
                positions.append(inner_kinds.index(stage))
            self.assertEqual(positions, sorted(positions),
                             f"{part_id}: coarse preview must precede editable proposal")
            completed_index = next(i for i, event in enumerate(events)
                                   if event.get("kind") == "part_completed"
                                   and event.get("part_id") == part_id)
            self.assertGreater(completed_index, max(self.indices(events, "part_event", part_id)))
            self.assertGreater(completed_index, 0)

    def assert_failed_sheet_events(self, events):
        kinds = [event.get("kind") for event in events]
        self.assertNotIn("completed", kinds)
        self.assertTrue(any(kind in ("failed", "cancelled") for kind in kinds),
                        f"missing failed/cancelled terminal event: {kinds}")

    def test_pnm_and_png_emit_ordered_per_part_stages_and_artifacts(self):
        for image_format in ("pnm", "png"):
            with self.subTest(image_format=image_format):
                image = self.make_image("two-parts-" + image_format, image_format)
                output = self.root / ("sheet-" + image_format)
                parts = [RING, BAR]
                self.write_parts(parts)
                result, events = self.invoke(self.command(image, output))
                self.assert_successful_sheet_events(events, ["Ring", "Bar"])
                manifest = self.assert_sheet_artifacts(output, image, parts, result, events)
                self.assertEqual([part["status"] for part in manifest["parts"]],
                                 ["completed", "completed"])

    def test_output_argument_is_required(self):
        self.write_parts([RING])
        command = self.command()
        index = command.index("--output")
        del command[index:index + 2]
        result, events = self.invoke(command, success=False)
        self.assertIn("missing --output", result.stderr)
        self.assertEqual(events, [])
        self.assertFalse(self.output.exists())

    def test_saved_input_replays_after_original_inputs_are_deleted(self):
        parts = [BAR, RING]  # selection order must not fall back to the largest component
        self.write_parts(parts)
        result, events = self.invoke(self.command())
        original = self.assert_sheet_artifacts(self.output, self.image, parts, result, events)
        self.image.unlink()
        self.parts_file.unlink()
        replay = original["replay"]
        replay_output = self.root / "replayed"
        saved_image = self.output / replay["image"]
        saved_parts = self.output / replay["parts"]
        command = [str(CLI), "--image", str(saved_image), "--parts", str(saved_parts),
                   "--output", str(replay_output), "--pixel-unit", str(replay["pixel_unit"]),
                   "--tolerance", str(replay["tolerance"])]
        result, events = self.invoke(command)
        self.assert_successful_sheet_events(events, ["Bar", "Ring"])
        repeated = self.assert_sheet_artifacts(replay_output, saved_image, parts, result, events)
        self.assertEqual(repeated["image_sha256"], original["image_sha256"])
        for old, new in zip(original["parts"], repeated["parts"]):
            for key in ("volume", "expected_volume", "matches", "height", "seed"):
                self.assertEqual(old[key], new[key])
            old_mesh = json.loads((self.output / old["artifacts"]["mesh"]).read_text())
            new_mesh = json.loads((replay_output / new["artifacts"]["mesh"]).read_text())
            self.assertEqual(old_mesh["mesh"]["data"], new_mesh["mesh"]["data"])

    def test_sheet_rejects_single_part_options_and_bad_scalars_before_output(self):
        self.write_parts([RING])
        for index, extra in enumerate((("--height", "2"), ("--seed", "6,7"),
                                       ("--tolerance", "nan"), ("--tolerance", "0"))):
            with self.subTest(extra=extra):
                output = self.root / f"bad-options-{index}"
                _, events = self.invoke(self.command(output=output, extra=extra), success=False)
                self.assertEqual(events, [])
                self.assertFalse(output.exists())

    def test_malformed_image_rejects_before_output(self):
        self.write_parts([RING])
        self.image.write_bytes(b"invalid PNM")
        _, events = self.invoke(self.command(), success=False)
        self.assertEqual(events, [])
        self.assertFalse(self.output.exists())

    @unittest.skipUnless(sys.platform.startswith("linux"), "broken pipe requires POSIX")
    def test_broken_stdout_retains_failed_manifest_without_conflicting_terminal(self):
        self.write_parts([RING, BAR])
        read_fd, write_fd = os.pipe()
        os.close(read_fd)
        try:
            process = subprocess.Popen(self.command(), cwd=self.root, stdout=write_fd,
                                       stderr=subprocess.PIPE, text=True)
        finally:
            os.close(write_fd)
        try:
            _, stderr = process.communicate(timeout=20)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
            self.fail("broken stdout did not stop sheet work")
        self.assertNotEqual(process.returncode, 0, stderr)
        manifest = self.read_manifest(self.output)
        self.assertEqual(manifest["status"], "failed")
        self.assertEqual(manifest["error"]["code"], "io")
        self.assertEqual([part["status"] for part in manifest["parts"]], ["pending", "pending"])
        events = [json.loads(line) for line in (self.output / "events.jsonl").read_text().splitlines()]
        self.assertEqual([event["kind"] for event in events], ["started"])
        self.assertFalse(any((self.output / "parts").iterdir()))
        self.assertEqual((self.output / manifest["replay"]["image"]).read_bytes(), self.image.read_bytes())

    def test_duplicate_component_keeps_first_and_stops_before_third(self):
        duplicate = {"id": "RingAgain", "seed": [24, 27], "height": 4.0}
        parts = [RING, duplicate, PENDING]
        self.write_parts(parts)
        result, events = self.invoke(self.command(output=self.output), success=False)
        self.assert_journal(self.output, result, events)
        self.assert_failed_sheet_events(events)
        self.assertEqual(sum(event.get("kind") == "part_completed" for event in events), 1)
        self.assertEqual([event.get("kind") for event in self.wrappers(events, "Ring")], STAGES)
        # Component identity is checked before the duplicate part publishes stages.
        self.assertEqual(self.wrappers(events, "RingAgain"), [])
        self.assertEqual(self.wrappers(events, "Pending"), [])
        completed = self.indices(events, "part_completed")[0]
        self.assertLess(max(self.indices(events, "part_event", "Ring")), completed)
        terminal = events[-1]
        self.assertEqual(terminal["kind"], "failed")
        self.assertEqual(terminal["error"]["code"], "duplicate_component")
        manifest = self.read_manifest(self.output)
        self.assertEqual(manifest["status"], "failed")
        self.assertEqual([entry["status"] for entry in manifest["parts"]],
                         ["completed", "failed", "pending"])
        self.assertEqual(manifest["parts"][1]["error"]["code"], "duplicate_component")
        self.assertEqual(manifest["parts"][2]["result"], None)
        self.assertEqual(terminal["manifest"], manifest)
        self.assertFalse((self.output / "parts/RingAgain").exists())
        self.assertFalse((self.output / "parts/Pending").exists())
        self.assert_completed_part(self.output, self.image, RING, manifest["parts"][0],
                                   image_hash=hashlib.sha256(self.image.read_bytes()).hexdigest())

    def test_later_bad_seed_preserves_first_and_leaves_third_pending(self):
        bad = {"id": "BadSeed", "seed": [31, 5], "height": 4.0}  # background gap
        parts = [RING, bad, PENDING]
        self.write_parts(parts)
        result, events = self.invoke(self.command(output=self.output), success=False)
        self.assert_journal(self.output, result, events)
        self.assert_failed_sheet_events(events)
        self.assertEqual(sum(event.get("kind") == "part_completed" for event in events), 1)
        self.assertEqual([event.get("kind") for event in self.wrappers(events, "Ring")], STAGES)
        self.assertEqual(self.wrappers(events, "Pending"), [])
        self.assertEqual(self.wrappers(events, "BadSeed"), [])
        self.assertLess(max(self.indices(events, "part_event", "Ring")),
                        self.indices(events, "part_completed")[0])
        manifest = self.read_manifest(self.output)
        self.assertEqual(manifest["status"], "failed")
        self.assertEqual([entry["status"] for entry in manifest["parts"]],
                         ["completed", "failed", "pending"])
        self.assertTrue(manifest["parts"][1]["error"]["message"])
        self.assertEqual(manifest["parts"][2]["result"], None)
        self.assertEqual(events[-1]["manifest"], manifest)
        self.assert_completed_part(self.output, self.image, RING, manifest["parts"][0],
                                   image_hash=hashlib.sha256(self.image.read_bytes()).hexdigest())

    def test_invalid_parts_are_rejected_before_output(self):
        valid = dict(RING)
        cases = [
            ("malformed JSON", '[{"id":'),
            ("object instead of array", json.dumps(valid)),
            ("empty array", "[]"),
            ("too many parts", json.dumps([dict(valid, id=f"P{i}") for i in range(17)])),
            ("unknown field", json.dumps([dict(valid, tolerance=0.1)])),
            ("duplicate JSON keys", '[{"id":"First","id":"Second","seed":[6,7],"height":2}]'),
            ("escaped duplicate key", '[{"id":"First","\\u0069d":"Second","seed":[6,7],"height":2}]'),
            ("positional part array", '[["Ring",[6,7],2]]'),
            ("over byte limit", " " * (1024 * 1024 + 1)),
            ("duplicate IDs", json.dumps([valid, dict(valid)])),
            ("path traversal ID", json.dumps([dict(valid, id="../escape")])),
            ("slash in ID", json.dumps([dict(valid, id="a/b")])),
            ("boolean height", json.dumps([dict(valid, height=True)])),
            ("boolean seed", json.dumps([dict(valid, seed=[True, 7])])),
            ("nonfinite height", '[{"id":"Ring","seed":[6,7],"height":NaN}]'),
            ("nonfinite seed", '[{"id":"Ring","seed":[Infinity,7],"height":2}]'),
        ]
        for index, (label, source) in enumerate(cases):
            with self.subTest(case=label):
                self.parts_file.write_text(source + "\n")
                output = self.root / f"invalid-{index}"
                _, events = self.invoke(self.command(output=output), success=False)
                self.assertFalse(output.exists(), label)
                self.assertFalse(any(event.get("kind") in
                                     ("part_event", "part_completed", "completed")
                                     for event in events), label)

    def test_existing_output_is_never_overwritten(self):
        self.write_parts([RING])
        directory = self.root / "existing-directory"
        directory.mkdir()
        sentinel = directory / "keep.txt"
        sentinel.write_text("preserve\n")
        regular_file = self.root / "existing-file"
        regular_file.write_text("also preserve\n")
        for output in (directory, regular_file):
            with self.subTest(output=output.name):
                _, events = self.invoke(self.command(output=output), success=False)
                self.assertFalse(any(event.get("kind") == "completed" for event in events))
                self.assertEqual(sentinel.read_text(), "preserve\n")
                self.assertEqual(regular_file.read_text(), "also preserve\n")
                self.assertEqual(list(directory.iterdir()), [sentinel])

    def test_invalid_and_tiny_timeout_fail_without_sheet_completion(self):
        self.write_parts([RING])
        for index, value in enumerate(("0", "-1", "nan", "inf", "-inf")):
            with self.subTest(invalid_timeout=value):
                output = self.root / f"invalid-timeout-{index}"
                _, events = self.invoke(self.command(output=output,
                                                     extra=("--timeout", value)),
                                        success=False)
                self.assertFalse(output.exists())
                self.assertFalse(any(event.get("kind") == "completed" for event in events))

        tiny_output = self.root / "tiny-timeout"
        _, events = self.invoke(self.command(output=tiny_output,
                                             extra=("--timeout", "0.000000001")),
                                success=False, timeout=10)
        self.assertFalse(any(event.get("kind") == "completed" for event in events))
        if events:
            self.assert_failed_sheet_events(events)
        # Deadline admission may or may not create the output directory before it
        # fires. If the manifest exists, require only that it remains valid JSON;
        # do not make timing-sensitive assertions about which files were created.
        manifest = tiny_output / "manifest.json"
        if manifest.is_file():
            retained = json.loads(manifest.read_text())
            self.assertIsInstance(retained, dict)
            self.assertIn(retained.get("status"), ("failed", "cancelled"))


if __name__ == "__main__":
    unittest.main()
