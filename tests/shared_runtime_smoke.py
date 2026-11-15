"""Same-fixture native-host/process parity; timings are smoke data, not a benchmark.

Run after cargo build --workspace. Standard library only. Transport-byte counters
count actual boundary payloads once; they are not allocation or peak RSS measures.
"""
import base64
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import zlib

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug"


def run(command, data=None, success=True):
    encoded = b"" if data is None else json.dumps(data).encode()
    start = time.perf_counter()
    p = subprocess.run(list(map(str, command)), input=encoded, capture_output=True, timeout=30)
    assert (p.returncode == 0) == success, (command, p.stdout.decode(), p.stderr.decode())
    return p.stdout, (time.perf_counter() - start) * 1000, len(encoded) + len(p.stdout)


def raster(kind, image_format):
    width, height = 32, 24
    def material(x, y):
        if kind == "ring":
            return 3 <= x < 29 and 3 <= y < 21 and not (9 <= x < 22 and 8 <= y < 16)
        return (4 <= x < 12 and 3 <= y < 21) or (4 <= x < 27 and 15 <= y < 21)
    data = bytes(0 if material(x, y) else 255 for y in range(height) for x in range(width))
    if image_format == "pnm":
        return b"P5 32 24 255\n" + data
    def chunk(name, value):
        return struct.pack(">I", len(value)) + name + value + struct.pack(">I", zlib.crc32(name + value))
    rows = b"".join(b"\0" + data[y*width:(y+1)*width] for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def check(temporary, kind, image_format, scale, height):
    folder = Path(temporary) / (kind + image_format)
    folder.mkdir()
    data = raster(kind, image_format)
    image = folder / ("input.png" if image_format == "png" else "input.pgm")
    image.write_bytes(data)
    output = folder / "shared"
    command = [BIN / "krly3d-reconstruct", "--image", image, "--output", output,
               "--pixel-unit", scale, "--height", height, "--seed", "5,5", "--tolerance", "0.05"]
    stdout, shared_ms, _ = run(command)
    manifest = json.loads(stdout)
    assert manifest == json.loads((output / "manifest.json").read_text())
    assert manifest["execution"] == "in_process"
    assert manifest["image_sha256"] == hashlib.sha256(data).hexdigest()
    assert manifest["open"] == 0
    assert abs(manifest["volume"] - manifest["expected_volume"]) < 1e-8
    shared_evidence = json.loads((output / "evidence.json").read_text())
    shared_mesh = json.loads((output / "mesh.json").read_text())
    assert shared_mesh["provenance"]["source"] == manifest["brep_source"]
    assert len(shared_evidence["evidence"]["polygons"]) == (2 if kind == "ring" else 1)
    events = [json.loads(line) for line in (output / "events.jsonl").read_text().splitlines()]
    assert [(e["seq"], e["revision"], e["kind"]) for e in events] == [(1, 1, "invalidated"), (2, 1, "committed")]
    # Compare all geometry with a separate decoder, exporter, process and native reload.
    call = {"protocol": "krlyvis.drawing/1", "request_id": "process",
            "input": {"image": {"format": image_format, "data": base64.b64encode(data).decode()},
                      "pixel_unit": scale, "seed": [5, 5], "source": str(image)}}
    evidence_bytes, vis_ms, wire_vis = run([BIN / "krlyvis-drawing"], call)
    evidence = json.loads(evidence_bytes)
    for key in ("evidence", "provenance", "quality"):
        assert evidence[key] == shared_evidence[key], (kind, image_format, key)
    brep_call = {"protocol": "krly3d.brep.planar/1", "request_id": "process",
                 "input": {"polygons": evidence["evidence"]["polygons"], "height": height,
                           "length_unit": "model", "source": "fixture"}}
    native_bytes, brep_ms, wire_brep = run([BIN / "krly3d-brep"], brep_call)
    native = json.loads(native_bytes)
    model = folder / "process.k3d"
    model.write_text(native["model"])
    assert model.read_text() == (output / "proposal.k3d").read_text()
    mesh_call = {"protocol": {"major": 2, "minor": 0}, "api": {"major": 2, "minor": 0},
                 "request_id": "process", "input": {"node": "part@1", "triangles": True,
                   "tolerance": {"value": 0.05, "unit": "model"}}}
    mesh_bytes, mesh_ms, wire_mesh = run([BIN / "krly3d-mesh", "--model", model], mesh_call)
    mesh = json.loads(mesh_bytes.splitlines()[-1])["body"]["result"]
    for key in ("points", "triangles", "volume", "area", "open", "quality"):
        assert mesh[key] == shared_mesh[key], (kind, image_format, key)
    assert mesh["mesh"]["data"] == shared_mesh["mesh"]["data"]
    assert mesh["provenance"]["source"] != shared_mesh["provenance"]["source"], "different processes have independent sessions"
    # No overwrite or partial result on invalid input.
    before = (output / "manifest.json").read_bytes()
    run(command, success=False)
    assert (output / "manifest.json").read_bytes() == before
    bad = command.copy()
    bad[bad.index("--height")+1] = "nan"
    bad[bad.index("--output")+1] = folder / "invalid"
    run(bad, success=False)
    assert not (folder / "invalid").exists()
    return {"fixture": kind + ":" + image_format, "shared_cli_wall_ms": round(shared_ms, 3),
            "three_cli_wall_ms": round(vis_ms + brep_ms + mesh_ms, 3),
            "process_boundary_bytes": wire_vis + wire_brep + wire_mesh + model.stat().st_size,
            "local_internal_transport_bytes": 0,
            "volume": manifest["volume"]}


def main():
    with tempfile.TemporaryDirectory() as temporary:
        rows = [check(temporary, kind, image_format, 0.25, 3)
                for kind in ("ring", "concave") for image_format in ("pnm", "png")]
    print(json.dumps({"cases": rows, "parity": "passed", "limits": "debug smoke; wall time includes different adapter I/O; byte counts are not RSS or allocations"}, indent=2))


if __name__ == "__main__":
    main()
