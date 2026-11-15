#!/usr/bin/env python3
"""Replayable single-outline planar drawing proposal. Uses existing domain processes.

No recognition model or geometry solver lives here. stdout is flow/1 NDJSON;
artifacts and a replay journal are written only into a new output directory.
"""
import argparse
import base64
import hashlib
import json
import math
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[4]
MAX_REPLY = 24 * 1024 * 1024


def positive(value):
    value = float(value)
    if not math.isfinite(value) or value <= 0:
        raise argparse.ArgumentTypeError("must be finite and positive")
    return value


class Peer:
    def __init__(self, flow, signal_mask, *, command=None, name="peer"):
        self.flow = flow
        self.name = name
        self.buffer = b""
        self.serial = 0
        self.stderr = (flow.output / f"{name}.stderr").open("wb")
        self.process = subprocess.Popen(
            command or [str(flow.bin / "krly3d"), "--peer", f"vis={flow.bin / 'krlyvis'}"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            start_new_session=True, preexec_fn=lambda: signal.pthread_sigmask(signal.SIG_SETMASK, signal_mask),
        )

    def rpc(self, method, /, **params):
        self.serial += 1
        request = {"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params}
        self.process.stdin.write((json.dumps(request, allow_nan=False) + "\n").encode())
        self.process.stdin.flush()
        while b"\n" not in self.buffer:
            remaining = self.flow.remaining()
            if not select.select([self.process.stdout], [], [], remaining)[0]:
                raise TimeoutError(f"deadline during {method}")
            chunk = os.read(self.process.stdout.fileno(), 65536)
            if not chunk:
                raise RuntimeError(f"peer closed during {method}; see {self.name}.stderr")
            self.buffer += chunk
            if len(self.buffer) > MAX_REPLY:
                raise RuntimeError("peer response exceeds 24 MiB")
        line, self.buffer = self.buffer.split(b"\n", 1)
        reply = json.loads(line)
        self.flow.journal.write(json.dumps({"elapsed_ms": (time.monotonic()-self.flow.start)*1000, "peer": self.name, "request": request, "reply": reply}) + "\n")
        self.flow.journal.flush()
        if reply.get("id") != self.serial or "error" in reply or "result" not in reply:
            raise RuntimeError(f"{method}: {reply}")
        return reply["result"]

    def call(self, method, /, **params):
        result = self.rpc(method, **params)
        if not result.get("ok"):
            raise RuntimeError(f"{method}: {result}")
        return result

    def notify(self, method):
        self.process.stdin.write((json.dumps({"jsonrpc": "2.0", "method": method}) + "\n").encode())
        self.process.stdin.flush()

    def close(self):
        # The process group includes the krlyvis peer; interrupted RPCs must not orphan it.
        try:
            if self.process.poll() is None:
                self.process.stdin.close()
                try:
                    self.process.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    os.killpg(self.process.pid, signal.SIGTERM)
                    self.process.wait(timeout=2)
        finally:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.stderr.close()


class Flow:
    def __init__(self, args):
        self.args = args
        self.bin = args.bin_dir.resolve()
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.id = str(uuid.uuid4())
        self.seq = 0
        self.start = self.stage_start = time.monotonic()
        self.deadline = self.start + args.timeout
        self.journal = (self.output / "rpc.jsonl").open("w")
        self.events = (self.output / "events.jsonl").open("w")

    def remaining(self):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("flow deadline exceeded")
        return remaining

    def emit(self, kind, **data):
        now = time.monotonic()
        event = {"protocol": "krly3d.drawing-flow/1", "flow_id": self.id, "seq": self.seq,
                 "kind": kind, "elapsed_ms": (now-self.start)*1000,
                 "stage_ms": (now-self.stage_start)*1000, **data}
        line = json.dumps(event, allow_nan=False)
        self.events.write(line + "\n")
        self.events.flush()
        self.seq += 1
        print(line, flush=True)  # backpressure: never queue the next stage ahead of the consumer
        self.stage_start = time.monotonic()

    def save(self, name, value):
        path = self.output / name
        text = value if isinstance(value, str) else json.dumps(value, indent=2, allow_nan=False) + "\n"
        path.write_text(text)
        return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def operation(self, binary, model, name, input):
        call = {"protocol": {"major": 2, "minor": 0}, "api": {"major": 2, "minor": 0},
                "request_id": f"{self.id}:{name}", "timeout_ms": max(0, int(self.remaining()*1000)), "input": input}
        command = [str(self.bin / binary), "--model", str(model)]
        if binary == "krly3d-view":
            command += ["--operations"]
        # File-backed domain streams avoid unbounded in-memory subprocess output.
        path = self.output / f"{name}.events.jsonl"
        with path.open("wb") as out, (self.output / f"{name}.stderr").open("wb") as err:
            mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
            child = None
            try:
                child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=out, stderr=err, start_new_session=True,
                                         preexec_fn=lambda: signal.pthread_sigmask(signal.SIG_SETMASK, mask))
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)
                child.communicate(json.dumps(call).encode(), timeout=self.remaining())
                if child.returncode:
                    raise RuntimeError(f"{name} failed; inspect {name}.events.jsonl and {name}.stderr")
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)
                if child is not None and child.poll() is None:
                    os.killpg(child.pid, signal.SIGTERM)
                    try:
                        child.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid, signal.SIGKILL)
                        child.wait()
        if path.stat().st_size > MAX_REPLY:
            raise RuntimeError(f"{name} output exceeds 24 MiB")
        events = [json.loads(line) for line in path.read_text().splitlines()]
        if len(events) != 2 or [e["body"]["kind"] for e in events] != ["started", "completed"]:
            raise RuntimeError(f"invalid {name} event sequence")
        if [e["seq"] for e in events] != [0, 1] or any(e["request_id"] != call["request_id"] for e in events):
            raise RuntimeError(f"invalid {name} event identity")
        return events[-1]["body"]["result"]

    def mcp(self, binary, tool, call, name):
        """Invoke a stateless domain tool and retain its canonical request/result."""
        self.save(f"{name}-request.json", call)
        mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        client = None
        try:
            client = Peer(self, mask, command=[str(self.bin / binary)], name=f"{name}-mcp")
            signal.pthread_sigmask(signal.SIG_SETMASK, mask)
            initialized = client.rpc("initialize", protocolVersion="2025-11-25", capabilities={},
                                     clientInfo={"name": "krly3d-drawing-flow", "version": "1"})
            if initialized.get("protocolVersion") != "2025-11-25":
                raise RuntimeError(f"unsupported {name} MCP protocol")
            client.notify("notifications/initialized")
            response = client.rpc("tools/call", name=tool, arguments=call)
            content = response.get("content", [])
            if response.get("isError") or len(content) != 1 or content[0].get("type") != "text":
                raise RuntimeError(f"{name} tool failed: {response}")
            result = json.loads(content[0]["text"])
            if result.get("protocol") != call["protocol"] or result.get("request_id") != call["request_id"]:
                raise RuntimeError(f"invalid {name} result identity")
            self.save(f"{name}-result.json", result)
            return result
        finally:
            try:
                if client:
                    client.close()
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)

    def planar(self, polygons, source):
        call = {"protocol": "krly3d.brep.planar/1", "request_id": f"{self.id}:planar",
                "input": {"polygons": polygons, "height": self.args.height,
                          "length_unit": "model", "source": source}}
        result = self.mcp("krly3d-brep-mcp", "brep_planar", call, "brep")
        if (result.get("source") != source or result.get("node") != "part@1"
                or not isinstance(result.get("model"), str)):
            raise RuntimeError("invalid B-rep planar document identity")
        return result

    def drawing(self, picture):
        formats = {".png": "png", ".jpg": "jpeg", ".jpeg": "jpeg",
                   ".pgm": "pnm", ".ppm": "pnm", ".pnm": "pnm"}
        format = formats.get(picture.suffix.lower())
        if format is None:
            raise ValueError("drawing requires PNG, JPEG or PNM input")
        data = picture.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        source = "sha256:" + digest
        call = {"protocol": "krlyvis.drawing/1", "request_id": f"{self.id}:drawing",
                "input": {"image": {"format": format, "data": base64.b64encode(data).decode()},
                          "pixel_unit": self.args.pixel_unit, "seed": self.args.seed, "source": source}}
        result = self.mcp("krlyvis-drawing-mcp", "vis_drawing", call, "vis")
        if (result.get("provenance", {}).get("image_sha256") != digest
                or result["provenance"].get("source") != source):
            raise RuntimeError("krlyvis result does not identify the supplied image bytes")
        return result

    def likeness_bridge(self, peer, picture, evidence):
        """Temporary legacy evidence replay, checked against the domain result."""
        peer.call("vis.picture", id="image", path=str(picture))
        selection = {"seed": self.args.seed} if self.args.seed else {}
        peer.call("vis.outline", id="outline", inputs=["image"], simplify=0, method="trace-holes", **selection)
        peer.call("vis.frame", id="top", inputs=["outline"], view="top",
                  points=[0, 0, 0, 0, 1, 1, self.args.pixel_unit, self.args.pixel_unit])
        legacy = peer.call("vis.polygons", node="top@1")["data"]
        if legacy != evidence:
            raise RuntimeError("legacy likeness evidence differs from krlyvis domain evidence")

    def run(self):
        args = self.args
        if args.demo:
            picture = self.output / "drawing.pgm"
            pixels = bytes(0 if 4 <= x < 36 and 4 <= y < 28 and (x < 16 or y < 14) else 255
                           for y in range(32) for x in range(40))
            picture.write_bytes(b"P5 40 32 255\n" + pixels)
        else:
            picture = args.image.resolve()
        if picture.stat().st_size > 16 * 1024 * 1024:
            raise ValueError("image exceeds 16 MiB")
        # Preserve exact input bytes so later source changes cannot invalidate the replay.
        if not args.demo:
            snapshot = self.output / ("drawing" + picture.suffix)
            snapshot.write_bytes(picture.read_bytes())
            picture = snapshot
        assumptions = {"view": "orthographic top", "shape": "one planar outline extruded uniformly",
                       "pixel_unit": args.pixel_unit, "height": args.height, "length_unit": "model",
                       "scale_source": "user supplied", "height_source": "user supplied",
                       "hidden_surfaces": "assumed constant cross-section; unobserved",
                       "interior_features": "trace-holes preserves resolved pixel holes as through-holes; hidden/blind cavities unobserved",
                       "subject_selection": "explicit pixel seed" if args.seed else "largest connected component",
                       "seed": args.seed,
                       "confidence": None, "measurement_uncertainty": None,
                       "next_questions": ["Is this a top orthographic view?", "Are thickness, scale and hidden features correct?", "Are there holes or other subjects missing from the outer contour?"]}
        self.save("assumptions.json", assumptions)
        self.emit("started", assumptions=assumptions, image_sha256=hashlib.sha256(picture.read_bytes()).hexdigest())
        # Defer stop signals until the spawned peer has an owner/finally cleanup.
        mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        peer = None
        try:
            peer = Peer(self, mask)
            signal.pthread_sigmask(signal.SIG_SETMASK, mask)
            drawing = self.drawing(picture)
            evidence = drawing["evidence"]
            polygons = evidence["polygons"]
            if not polygons or any(len(p) < 3 for p in polygons) or sum(map(len, polygons)) > 256:
                raise ValueError("pilot requires one outer loop with holes and at most 256 total vertices")
            points = polygons[0]
            if any(len(p) != 2 or any(not math.isfinite(v) for v in p) for polygon in polygons for p in polygon):
                raise ValueError("invalid contour coordinates")
            evidence_artifact = self.save("evidence.json", evidence)
            self.emit("evidence", artifact=evidence_artifact, evidence=evidence["evidence"],
                      reference="vis:top@1", provenance=drawing["provenance"], vertex_count=sum(map(len, polygons)), holes=len(polygons)-1)
            low = [min(p[i] for p in points) for i in range(2)]
            high = [max(p[i] for p in points) for i in range(2)]
            checkpoint = peer.call("mark")["data"]["mark"]
            peer.call("box", id="coarse", w=high[0]-low[0], d=high[1]-low[1], h=args.height,
                      at=[(low[0]+high[0])/2, (low[1]+high[1])/2, 0])
            coarse = self.save("coarse.k3d", peer.call("save")["text"])
            mesh = self.operation("krly3d-mesh", Path(coarse["path"]), "coarse-mesh",
                                  {"node": "coarse", "triangles": True, "tolerance": {"value": args.pixel_unit, "unit": "model"}})
            if not mesh.get("mesh"):
                raise RuntimeError("coarse preview has no mesh artifact")
            logical_id = f"{self.id}:proposal"
            self.emit("coarse_preview", artifact_id=logical_id, revision=1,
                      artifact=self.save("coarse-mesh.json", mesh), provider_artifact_id=mesh["mesh"]["id"],
                      approximation="bounding prism, not contour reconstruction", quality=mesh["quality"])
            # This work starts only after the coarse event has flushed to the consumer.
            peer.call("rollback", mark=checkpoint)
            built = self.planar(polygons, "sha256:" + evidence_artifact["sha256"])
            proposal = self.save("proposal.k3d", built["model"])
            # Likeness still lives in the legacy facade; load the exact domain result.
            peer.call("open", format="k3d", text=built["model"])
            self.emit("editable_proposal", artifact_id=logical_id, revision=2, replaces_revision=1,
                      artifact=proposal, node="part@1", assumptions=assumptions,
                      brep_provenance=built["provenance"])
            final_mesh = self.operation("krly3d-mesh", Path(proposal["path"]), "proposal-mesh",
                                        {"node": "part@1", "triangles": True,
                                         "tolerance": {"value": args.pixel_unit, "unit": "model"}})
            expected_volume = evidence["material_area"] * args.height
            volume_matches = math.isclose(final_mesh["volume"], expected_volume, rel_tol=1e-8, abs_tol=1e-10)
            solid_check = {"hole_count":len(polygons)-1, "expected_volume":expected_volume,
                           "mesh_volume":final_mesh["volume"], "open_edges":final_mesh["open"],
                           "matches":volume_matches and final_mesh["open"] == 0,
                           "scope":"material area times uniform height and mesh closure; not full topology proof"}
            self.save("proposal-mesh.json", final_mesh)
            self.save("solid-check.json", solid_check)
            if not solid_check["matches"]:
                raise RuntimeError("proposal does not preserve measured material volume or mesh closure")
            render = self.operation("krly3d-view", Path(proposal["path"]), "preview",
                                    {"node": "part@1", "width": 320, "height": 240, "backend": "mesh",
                                     "tolerance": {"value": args.pixel_unit, "unit": "model"}})
            png = self.output / "preview.png"
            png.write_bytes(base64.b64decode(render["image"]["data"], validate=True))
            self.emit("preview", artifact_id=logical_id, revision=2, path=str(png),
                      provenance=render["provenance"], quality=render["quality"])
            self.likeness_bridge(peer, picture, evidence)
            report = peer.call("likeness", against="vis:top@1", nodes=["part@1"], report="stations",
                               precision="contour", tolerance=args.pixel_unit)["data"]
            self.emit("validation", artifact=self.save("likeness.json", report), matches=report["matches"],
                      scope="sampled top-view outer extents only; does not validate holes or hidden geometry",
                      interior_features_validated=False, solid_check=solid_check,
                      evidence_bridge="legacy replay exactly matched domain evidence")
        finally:
            try:
                if peer:
                    peer.close()
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)
        self.emit("completed", artifact_id=logical_id, revision=2, matches=report["matches"],
                  proposal=proposal, next_questions=assumptions["next_questions"])


def interrupted(_signal, _frame):
    raise KeyboardInterrupt


def main():
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--image", type=Path)
    source.add_argument("--demo", action="store_true", help="generate a small concave L-shaped drawing")
    parser.add_argument("--seed", type=float, nargs=2, help="explicit pixel x y inside the intended connected component")
    parser.add_argument("--pixel-unit", type=positive, required=True, help="model units per pixel, supplied by user")
    parser.add_argument("--height", type=positive, required=True, help="uniform extrusion height, supplied by user")
    parser.add_argument("--output", type=Path, required=True, help="new directory; existing paths are never overwritten")
    parser.add_argument("--timeout", type=positive, default=120, help="whole computation deadline in seconds")
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "target/debug")
    args = parser.parse_args()
    if args.seed and any(not math.isfinite(v) or v < 0 for v in args.seed):
        parser.error("seed must contain finite nonnegative pixel coordinates")
    flow = None
    try:
        flow = Flow(args)
        flow.run()
        return 0
    except (Exception, KeyboardInterrupt) as error:
        kind = "cancelled" if isinstance(error, KeyboardInterrupt) else "failed"
        if flow:
            try:
                flow.emit(kind, diagnostic=str(error) or "interrupted")
            except (BrokenPipeError, OSError):
                pass
        print(f"drawing-flow: {error or kind}", file=sys.stderr)
        return 130 if kind == "cancelled" else 1
    finally:
        if flow:
            flow.journal.close()
            flow.events.close()


if __name__ == "__main__":
    sys.exit(main())
