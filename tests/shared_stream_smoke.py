#!/usr/bin/env python3
"""End-to-end checks for the opt-in shared-host NDJSON stream.

Run after building target/debug/krly3d-reconstruct. Uses only the Python standard
library and generates its PNG/PNM fixtures directly.
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
PROTOCOL = "krly3d.shared-reconstruction/2"
KINDS = ["started", "evidence", "coarse_preview", "editable_proposal",
         "preview", "validation", "completed"]
OUTPUTS = {"evidence.json", "coarse-mesh.json", "proposal.k3d", "mesh.json",
           "preview.png", "likeness.json", "manifest.json", "events.jsonl"}


def raster(kind, image_format):
    width, height = 32, 24

    def material(x, y):
        if kind == "ring":
            return (3 <= x < 29 and 3 <= y < 21
                    and not (9 <= x < 22 and 8 <= y < 16))
        return ((4 <= x < 12 and 3 <= y < 21)
                or (4 <= x < 27 and 15 <= y < 21))

    pixels = bytes(0 if material(x, y) else 255
                   for y in range(height) for x in range(width))
    if image_format == "pnm":
        return b"P5 32 24 255\n" + pixels

    def chunk(name, data):
        return (struct.pack(">I", len(data)) + name + data
                + struct.pack(">I", zlib.crc32(name + data)))

    rows = b"".join(b"\0" + pixels[y * width:(y + 1) * width]
                    for y in range(height))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows))
            + chunk(b"IEND", b""))


def invoke(command, success=True):
    result = subprocess.run(list(map(str, command)), capture_output=True,
                           text=True, timeout=60)
    if success:
        assert result.returncode == 0, (command, result.stdout, result.stderr)
    else:
        assert result.returncode != 0, (command, result.stdout, result.stderr)
    return result


class SharedStreamSmoke(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def make_image(self, name, kind, image_format):
        suffix = ".png" if image_format == "png" else ".pgm"
        image = self.root / (name + suffix)
        image.write_bytes(raster(kind, image_format))
        return image

    def command(self, image, output, *extra, seed="5,5"):
        command = [CLI, "--stream", "--image", image, "--output", output,
                   "--pixel-unit", "0.25", "--height", "3"]
        if seed is not None:
            command.extend(("--seed", seed))
        command.extend(extra)
        return command

    def test_streamed_png_and_pnm_preserve_geometry_artifacts_and_provenance(self):
        expected_pixels = {"ring": 364, "concave": 234}
        expected_holes = {"ring": 1, "concave": 0}
        for kind in ("ring", "concave"):
            for image_format in ("pnm", "png"):
                with self.subTest(kind=kind, image_format=image_format):
                    image = self.make_image(kind + image_format, kind, image_format)
                    output = self.root / (kind + "-" + image_format + "-stream")
                    seed = None if kind == "concave" and image_format == "png" else "5,5"
                    result = invoke(self.command(image, output, seed=seed))

                    stdout_lines = result.stdout.splitlines()
                    self.assertEqual(len(stdout_lines), 7, result.stdout)
                    events = [json.loads(line) for line in stdout_lines]
                    self.assertEqual([event["kind"] for event in events], KINDS)
                    self.assertEqual([event["seq"] for event in events], list(range(7)))
                    self.assertTrue(all(event["protocol"] == PROTOCOL for event in events))
                    self.assertTrue(all(event["revision"] == 1 for event in events))
                    self.assertEqual(len({event["flow_id"] for event in events}), 1)
                    self.assertTrue(events[0]["flow_id"])
                    for event in events:
                        self.assertIn("elapsed_ms", event)
                        self.assertIn("stage_ms", event)
                        self.assertTrue(math.isfinite(event["elapsed_ms"]))
                        self.assertTrue(math.isfinite(event["stage_ms"]))
                        self.assertGreaterEqual(event["elapsed_ms"], 0)
                        self.assertGreaterEqual(event["stage_ms"], 0)
                    self.assertEqual([event["elapsed_ms"] for event in events],
                                     sorted(event["elapsed_ms"] for event in events))

                    self.assertTrue(OUTPUTS.issubset({path.name for path in output.iterdir()}))
                    journal_lines = (output / "events.jsonl").read_text().splitlines()
                    self.assertEqual(journal_lines, stdout_lines)
                    self.assertEqual([json.loads(line) for line in journal_lines], events)

                    coarse, editable = events[2], events[3]
                    self.assertEqual(coarse["artifact_revision"], 1)
                    self.assertEqual(editable["artifact_revision"], 2)
                    self.assertEqual(coarse["artifact_id"], editable["artifact_id"])
                    self.assertEqual(editable["replaces_revision"], 1)

                    digest = hashlib.sha256(image.read_bytes()).hexdigest()
                    evidence = json.loads((output / "evidence.json").read_text())
                    mesh = json.loads((output / "mesh.json").read_text())
                    likeness = json.loads((output / "likeness.json").read_text())
                    manifest = json.loads((output / "manifest.json").read_text())
                    self.assertEqual(manifest["protocol"], PROTOCOL)
                    self.assertEqual(evidence["provenance"]["image_sha256"], digest)
                    self.assertEqual(manifest["image_sha256"], digest)
                    self.assertEqual(manifest["source"], evidence["provenance"]["source"])
                    self.assertEqual(len(evidence["evidence"]["polygons"]),
                                     expected_holes[kind] + 1)
                    self.assertEqual(len(evidence["evidence"]["polygons"]) - 1,
                                     expected_holes[kind])

                    expected_volume = expected_pixels[kind] * 0.25 ** 2 * 3
                    self.assertAlmostEqual(evidence["evidence"]["material_area"],
                                           expected_pixels[kind] * 0.25 ** 2)
                    self.assertAlmostEqual(mesh["volume"], expected_volume, places=8)
                    self.assertAlmostEqual(manifest["volume"], expected_volume, places=8)
                    self.assertAlmostEqual(manifest["expected_volume"], expected_volume, places=8)
                    self.assertEqual(mesh["open"], 0)
                    self.assertEqual(manifest["open"], 0)

                    # The mesh and rendered preview must identify the same final B-rep
                    # source recorded by the completed manifest.
                    self.assertEqual(mesh["provenance"]["source"], manifest["brep_source"])
                    render_source = events[4]["provenance"]["source"]
                    self.assertEqual(render_source["source"], manifest["brep_source"])
                    self.assertEqual(render_source["node"], "part@1")
                    self.assertEqual(likeness["provenance"]["node"], "part@1")
                    self.assertEqual(likeness["provenance"]["source"], render_source)
                    self.assertEqual(events[5]["matches"], likeness["matches"])
                    self.assertIs(type(events[5]["matches"]), bool)
                    self.assertTrue((output / "preview.png").read_bytes().startswith(
                        b"\x89PNG\r\n\x1a\n"))

                    self.assertEqual(events[6]["manifest"], manifest)

    def test_existing_output_and_invalid_numbers_are_rejected_without_writes(self):
        image = self.make_image("invalid-input", "concave", "pnm")
        existing = self.root / "already-there"
        existing.mkdir()
        sentinel = existing / "keep.txt"
        sentinel.write_text("untouched")
        invoke(self.command(image, existing), success=False)
        self.assertEqual(sentinel.read_text(), "untouched")
        self.assertEqual({path.name for path in existing.iterdir()}, {"keep.txt"})

        bad_numbers = (("--pixel-unit", "nan"), ("--pixel-unit", "0"),
                       ("--height", "inf"), ("--height", "-1"),
                       ("--seed", "nan,5"))
        for index, (option, value) in enumerate(bad_numbers):
            with self.subTest(option=option, value=value):
                output = self.root / f"invalid-{index}"
                command = self.command(image, output)
                command[command.index(option) + 1] = value
                invoke(command, success=False)
                self.assertFalse(output.exists())

    def test_computation_failure_has_terminal_event_and_no_success_manifest(self):
        image = self.root / "broken.png"
        image.write_bytes(b"not a PNG")
        output = self.root / "failed-flow"
        result = invoke(self.command(image, output), success=False)
        events = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual([event["kind"] for event in events], ["started", "failed"])
        self.assertEqual([event["seq"] for event in events], [0, 1])
        self.assertTrue(events[-1]["message"])
        self.assertFalse((output / "manifest.json").exists())
        self.assertEqual((output / "events.jsonl").read_text(), result.stdout)

    @unittest.skipUnless(sys.platform.startswith("linux"),
                         "broken stdout pipe regression requires Linux")
    def test_broken_stream_stdout_fails_without_success_or_conflicting_terminals(self):
        image = self.make_image("broken-stdout", "ring", "pnm")
        output = self.root / "broken-stdout-output"
        read_fd, write_fd = os.pipe()
        os.close(read_fd)
        try:
            process = subprocess.Popen(self.command(image, output), stdout=write_fd,
                                       stderr=subprocess.PIPE, text=True)
        finally:
            os.close(write_fd)

        try:
            _, stderr = process.communicate(timeout=60)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate()
            self.fail("CLI did not exit after stdout became a broken pipe")

        self.assertNotEqual(process.returncode, 0, stderr)
        self.assertFalse((output / "manifest.json").exists())

        journal = output / "events.jsonl"
        if journal.exists():
            events = []
            for line in journal.read_text(errors="replace").splitlines():
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue  # A transport failure may leave a partial journal tail.
                if isinstance(event, dict):
                    events.append(event)
            kinds = {event["kind"] for event in events
                     if isinstance(event.get("kind"), str)}
            self.assertFalse({"completed", "failed"}.issubset(kinds), kinds)

    def test_without_stream_stdout_remains_a_single_manifest(self):
        image = self.make_image("legacy-default", "ring", "pnm")
        output = self.root / "legacy-default-output"
        command = [CLI, "--image", image, "--output", output,
                   "--pixel-unit", "0.25", "--height", "3", "--seed", "5,5"]
        result = invoke(command)
        self.assertEqual(len(result.stdout.splitlines()), 1, result.stdout)
        stdout_manifest = json.loads(result.stdout)
        self.assertNotIn("kind", stdout_manifest)
        self.assertEqual(stdout_manifest["protocol"], "krly3d.shared-reconstruction/1")
        self.assertEqual(stdout_manifest,
                         json.loads((output / "manifest.json").read_text()))
        names = {path.name for path in output.iterdir()}
        self.assertTrue({"evidence.json", "proposal.k3d", "mesh.json", "manifest.json"}
                        .issubset(names))
        self.assertTrue({"coarse-mesh.json", "preview.png", "likeness.json"}.isdisjoint(names))


if __name__ == "__main__":
    unittest.main()
