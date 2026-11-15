#!/usr/bin/env python3
"""Real drawing-to-proposal process checks. Uses built debug domain binaries."""
import base64
import struct
import zlib
import hashlib
import json
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FLOW = ROOT / "products/krly3d/reconstruction/cli/drawing_flow.py"


class DrawingFlow(unittest.TestCase):
    def command(self, output, *extra):
        return [sys.executable, str(FLOW), "--pixel-unit", "1", "--height", "3", "--output", str(output), *extra]

    def run_flow(self, output, *extra):
        result = subprocess.run(self.command(output, *extra), capture_output=True, text=True, timeout=30)
        return result, [json.loads(line) for line in result.stdout.splitlines()]

    def test_concave_drawing_yields_early_mesh_editable_geometry_and_matching_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "flow"
            result, events = self.run_flow(output, "--demo")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual([e["kind"] for e in events], ["started", "evidence", "coarse_preview", "editable_proposal", "preview", "validation", "completed"])
            self.assertEqual([e["seq"] for e in events], list(range(7)))
            self.assertTrue(events[-1]["matches"])
            self.assertEqual(events[2]["artifact_id"], events[3]["artifact_id"])
            self.assertEqual(events[3]["replaces_revision"], 1)
            self.assertIsNone(events[0]["assumptions"]["confidence"])
            proposal = (output / "proposal.k3d").read_text()
            self.assertNotIn("coarse =", proposal)
            self.assertIn("profile path=", proposal)
            self.assertIn("part = extrude outline_profile h=3", proposal)
            self.assertTrue((output / "preview.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n"))
            journal = [json.loads(line) for line in (output / "rpc.jsonl").read_text().splitlines()]
            built = next(r for r in journal if r["peer"] == "brep-mcp" and r["request"]["method"] == "tools/call")
            self.assertGreater(built["elapsed_ms"], events[2]["elapsed_ms"])
            self.assertEqual(built["request"]["params"]["name"], "brep_planar")
            self.assertFalse(any(r["request"]["method"] in ("profile", "extrude") for r in journal))
            domain = json.loads((output / "brep-result.json").read_text())
            self.assertEqual(domain["model"], proposal)
            self.assertEqual(domain["source"], "sha256:" + hashlib.sha256((output / "evidence.json").read_bytes()).hexdigest())
            self.assertEqual(events[3]["brep_provenance"], domain["provenance"])
            vision = json.loads((output / "vis-result.json").read_text())
            self.assertEqual(vision["evidence"], json.loads((output / "evidence.json").read_text()))
            self.assertEqual(vision["provenance"]["image_sha256"], hashlib.sha256((output / "drawing.pgm").read_bytes()).hexdigest())
            self.assertEqual(events[1]["provenance"], vision["provenance"])
            vis_call = next(r for r in journal if r["peer"] == "vis-mcp" and r["request"]["method"] == "tools/call")
            self.assertEqual(vis_call["request"]["params"]["name"], "vis_drawing")
            bridge = next(r for r in journal if r["request"]["method"] == "vis.picture")
            self.assertGreater(bridge["elapsed_ms"], events[3]["elapsed_ms"])
            self.assertIn("exactly matched", events[5]["evidence_bridge"])
            report = json.loads((output / "likeness.json").read_text())
            self.assertEqual(report["provenance"]["nodes"], ["part@1"])
            self.assertEqual(report["provenance"]["reference"]["node"], "top@1")
            coarse = json.loads((output / "coarse-mesh.json").read_text())
            self.assertEqual(coarse["volume"], 32*24*3)
            # Reload the actual editable proposal and verify its concavity changes volume.
            call = {"protocol":{"major":2,"minor":0},"api":{"major":2,"minor":0},"request_id":"reload", "input":{"node":"part"}}
            mesh = subprocess.run([str(ROOT / "target/debug/krly3d-mesh"), "--model", str(output / "proposal.k3d")],
                                  input=json.dumps(call), text=True, capture_output=True, timeout=20)
            self.assertEqual(mesh.returncode, 0, mesh.stderr)
            final = json.loads(mesh.stdout.splitlines()[-1])["body"]["result"]
            self.assertAlmostEqual(final["volume"], (32*10+12*14)*3)
            self.assertEqual(final["open"], 0)
            # Running again must not replace any output.
            again, _ = self.run_flow(output, "--demo")
            self.assertNotEqual(again.returncode, 0)
            self.assertEqual((output / "proposal.k3d").read_text(), proposal)

    def test_user_image_scale_and_height_are_explicit_and_replayed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            picture = root / "source.pgm"
            picture.write_bytes(b"P5 24 20 255\n" + bytes(0 if 3 <= x < 21 and 3 <= y < 17 else 255 for y in range(20) for x in range(24)))
            output = root / "result"
            result, events = self.run_flow(output, "--image", str(picture), "--pixel-unit", "0.25", "--height", "2")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(events[-1]["matches"])
            self.assertEqual(events[0]["assumptions"]["pixel_unit"], 0.25)
            self.assertEqual(events[0]["assumptions"]["height"], 2)
            self.assertEqual((output / "drawing.pgm").read_bytes(), picture.read_bytes())

    def test_holes_survive_editable_reload_and_deadlines_never_claim_completion(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            hole = root / "hole.pgm"
            hole.write_bytes(b"P5 24 24 255\n" + bytes(0 if 2 <= x < 22 and 2 <= y < 22 and not (8 <= x < 16 and 8 <= y < 16) else 255 for y in range(24) for x in range(24)))
            result, events = self.run_flow(root / "hole", "--image", str(hole))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("preserves", events[0]["assumptions"]["interior_features"])
            solid = json.loads((root / "hole/solid-check.json").read_text())
            self.assertEqual(solid["hole_count"], 1)
            self.assertAlmostEqual(solid["mesh_volume"], (20*20-8*8)*3)
            self.assertEqual(solid["open_edges"], 0)
            self.assertTrue(solid["matches"])
            # An independent exact-ray top view must see background through the centre.
            request = {"protocol":{"major":1,"minor":0},"api":{"major":1,"minor":0},
                       "request_id":"hole-ray","input":{"node":"part","width":64,"height":64,
                       "camera":{"kind":"named","name":"top"},"backend":"raycast"}}
            render = subprocess.run([str(ROOT / "target/debug/krly3d-view"),"--model",str(root / "hole/proposal.k3d")],
                                    input=json.dumps(request),text=True,capture_output=True,timeout=20)
            self.assertEqual(render.returncode,0,render.stderr)
            png = base64.b64decode(json.loads(render.stdout)["image"]["data"])
            position, compressed = 8, bytearray()
            while position < len(png):
                size=struct.unpack_from(">I",png,position)[0]
                if png[position+4:position+8] == b"IDAT": compressed.extend(png[position+8:position+8+size])
                position += size+12
            pixels=zlib.decompress(compressed)
            stride=64*3+1
            self.assertTrue(all(pixels[y*stride]==0 for y in range(64)))
            self.assertEqual(pixels[32*stride+1+32*3:32*stride+1+33*3],b"\xff\xff\xff")
            self.assertNotEqual(pixels[32*stride+1+12*3:32*stride+1+13*3],b"\xff\xff\xff")
            validation = next(e for e in events if e["kind"] == "validation")
            self.assertFalse(validation["interior_features_validated"])
            self.assertIn("holes", validation["scope"])
            self.assertTrue(any("holes" in q for q in events[-1]["next_questions"]))
            result, events = self.run_flow(root / "deadline", "--demo", "--timeout", "0.000001")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(events[-1]["kind"], "failed")
            self.assertNotIn("completed", [e["kind"] for e in events])

    def test_interrupt_returns_cancelled_and_does_not_continue_to_proposal(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "cancel"
            process = subprocess.Popen(self.command(output, "--demo"), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                self.assertTrue(select.select([process.stdout], [], [], 10)[0])
                first = json.loads(process.stdout.readline())
                self.assertEqual(first["kind"], "started")
                process.send_signal(signal.SIGINT)
                out, err = process.communicate(timeout=10)
                self.assertEqual(process.returncode, 130, err)
                events = [json.loads(line) for line in out.splitlines()]
                self.assertEqual(events[-1]["kind"], "cancelled")
                self.assertFalse((output / "proposal.k3d").exists())
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    unittest.main()
