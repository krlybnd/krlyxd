#!/usr/bin/env python3
"""Issue #7 real legacy-facade/krlyvis peer replay, using only Python's stdlib.

Build krly3d-cli and krlyvis-cli, then run this file. Optional --output writes the
complete request/reply evidence for documentation. No image measurement is labelled
subpixel: krlyvis returns a pixel-derived contour with unknown uncertainty.
"""
import argparse
import json
import math
from pathlib import Path
import select
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--krly3d", type=Path, default=root / "target/debug/krly3d")
    parser.add_argument("--krlyvis", type=Path, default=root / "target/debug/krlyvis")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    records = []
    with tempfile.TemporaryDirectory(prefix="krlyxd-likeness-") as temporary:
        picture = Path(temporary) / "rectangle.pgm"
        pixels = bytes(0 if 4 <= x < 44 and 4 <= y < 24 else 255 for y in range(28) for x in range(48))
        picture.write_bytes(b"P5 48 28 255\n" + pixels)
        process = subprocess.Popen(
            [str(args.krly3d), "--peer", f"vis={args.krlyvis}"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1,
        )
        try:
            def rpc(method, **params):
                request = {"jsonrpc": "2.0", "id": len(records) + 1, "method": method, "params": params}
                process.stdin.write(json.dumps(request) + "\n")
                process.stdin.flush()
                assert select.select([process.stdout], [], [], 20)[0], f"timeout: {method}"
                line = process.stdout.readline()
                assert line, f"closed output: {method}"
                reply = json.loads(line)
                assert reply["id"] == request["id"]
                records.append({"request": request, "reply": reply})
                return reply

            def ok(method, **params):
                reply = rpc(method, **params)
                assert "error" not in reply, reply
                assert reply["result"]["ok"], reply
                return reply["result"]

            ok("vis.picture", id="image", path=str(picture))
            ok("vis.outline", id="outline", inputs=["image"], simplify=0)
            ok("vis.frame", id="top", inputs=["outline"], view="top", points=[4,4,0,0,44,24,40,20])
            ok("box", id="part", w=40, d=20, h=3, at=[20,10,0])
            before = ok("save")
            legacy = ok("likeness", against="vis:top")
            assert legacy["text"] == "like vis:top: IoU 1; area model 800, drawing 800\n", legacy
            assert legacy["data"] == {"iou": 1, "model_area": 800, "drawing_area": 800, "worst": []}, legacy
            angle = math.radians(33)
            report = ok("likeness", against="vis:top", report="stations", precision="contour", tolerance=1,
                        frame=[0,0,math.cos(angle),math.sin(angle),-math.sin(angle),math.cos(angle)])
            data = report["data"]
            assert data["matches"] is True, report
            assert data["report_version"] == "krly3d.likeness.stations/1"
            assert {s["along"] for s in data["worst"]} == {"u", "v"}
            assert all(s["matches"] is True and s["tolerance"] == 1 for s in data["stations"])
            provenance = data["provenance"]
            assert provenance["reference"]["node"] == "top@1"
            assert provenance["nodes"] == ["part@1"]
            assert provenance["drawing_source"] == "krlyvis.placed-pixel-outline/1"
            assert provenance["drawing_evidence"] == "raster"
            assert provenance["drawing_uncertainty"] == "unknown"
            assert provenance["mesh_achieved_error"] == "unknown"
            rejected = rpc("likeness", against="vis:top", report="stations", precision="contour", tolerance=0.25)
            assert "subpixel" in json.dumps(rejected), rejected
            assert "error" in rejected or rejected["result"]["ok"] is False, rejected
            assert ok("save") == before
            assert ok("likeness", against="vis:top") == legacy
            ok("transform", id="part", inputs=["part"], move=[0,3,0])
            mismatch = ok("likeness", against="vis:top", nodes=["part"], report="stations", precision="contour", tolerance=1)
            assert mismatch["data"]["matches"] is False
            assert any(s["matches"] is False for s in mismatch["data"]["stations"])
            pinned = ok("likeness", against="vis:top@1", nodes=["part@1"], report="stations", precision="contour", tolerance=1)
            assert pinned["data"]["matches"] is True
            process.stdin.close()
            assert process.wait(timeout=10) == 0
            assert not process.stderr.read()
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
    if args.output:
        args.output.write_text(json.dumps({"case": "issue7-real-krlyvis-peer", "records": records}, indent=2) + "\n")
    print(f"PASS: {len(records)} real peer calls; contour matches, both rotated local axes, raster precision floor, pinned provenance, legacy output and nonmutation")


if __name__ == "__main__":
    main()
