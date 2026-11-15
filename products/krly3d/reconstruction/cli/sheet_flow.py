#!/usr/bin/env python3
"""Build individually editable proposals for explicitly selected subjects on a drawing.

A sheet is not an inferred assembly. Each part has a seed and an explicit thickness;
all share one supplied pixel scale. Bounded serial execution preserves early events.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import select
import signal
import subprocess
import sys
import time
import uuid

from drawing_flow import ROOT, interrupted, positive

MAX_PARTS = 16
MAX_EVENT = 1024 * 1024


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def number(value):
    return not isinstance(value, bool) and isinstance(value, (int, float)) and math.isfinite(value)


def parts_file(path):
    with path.open("rb") as source:
        data = source.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError("parts manifest exceeds 1 MiB")
    parts = json.loads(data, object_pairs_hook=unique_object)
    if not isinstance(parts, list) or not 1 <= len(parts) <= MAX_PARTS:
        raise ValueError("parts must be an array of 1..16 entries")
    ids = set()
    for part in parts:
        if not isinstance(part, dict) or set(part) != {"id", "seed", "height"}:
            raise ValueError("each part requires exactly id, seed, height")
        name = part["id"]
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_-]{0,63}", name) or name in ids:
            raise ValueError("part IDs must be unique safe names (letter then up to 63 letters/digits/_/-)")
        ids.add(name)
        seed = part["seed"]
        if not isinstance(seed, list) or len(seed) != 2 or not all(number(v) and v >= 0 for v in seed):
            raise ValueError("seed requires two finite nonnegative pixel coordinates")
        if not number(part["height"]) or part["height"] <= 0:
            raise ValueError("height must be finite and positive")
    return parts


class Sheet:
    def __init__(self, args, parts):
        self.args = args
        self.parts = parts
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.start = time.monotonic()
        self.deadline = self.start + args.timeout
        self.id = str(uuid.uuid4())
        self.seq = 0
        self.events = (self.output / "events.jsonl").open("w")
        self.manifest = {"protocol": "krly3d.drawing-sheet/1", "sheet_id": self.id,
                         "status": "running", "pixel_unit": args.pixel_unit,
                         "assumptions": ["orthographic top drawing", "uniform per-part extrusion",
                                         "visible pixel holes assumed through-holes", "no inferred assembly constraints"],
                         "parts": [{**part, "status": "pending"} for part in parts]}
        self.write_manifest()

    def remaining(self):
        value = self.deadline - time.monotonic()
        if value <= 0:
            raise TimeoutError("sheet deadline exceeded")
        return value

    def write_manifest(self):
        self.manifest["elapsed_ms"] = (time.monotonic() - self.start) * 1000
        temporary = self.output / "manifest.pending"
        temporary.write_text(json.dumps(self.manifest, indent=2, allow_nan=False) + "\n")
        temporary.replace(self.output / "manifest.json")

    def emit(self, kind, **data):
        event = {"protocol": "krly3d.drawing-sheet/1", "sheet_id": self.id, "seq": self.seq,
                 "kind": kind, "elapsed_ms": (time.monotonic() - self.start) * 1000, **data}
        self.seq += 1
        line = json.dumps(event, allow_nan=False)
        self.events.write(line + "\n")
        self.events.flush()
        print(line, flush=True)

    def part(self, part, image):
        output = self.output / "parts" / part["id"]
        command = [sys.executable, str(Path(__file__).with_name("drawing_flow.py")),
                   "--image", str(image), "--seed", *map(str, part["seed"]),
                   "--height", str(part["height"]), "--pixel-unit", str(self.args.pixel_unit),
                   "--output", str(output), "--timeout", str(self.remaining()),
                   "--bin-dir", str(self.args.bin_dir.resolve())]
        part.update(status="running", output=str(output))
        self.write_manifest()
        mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        child = None
        terminal = None
        child_seq = 0
        child_id = None
        buffered = b""
        with (self.output / f"{part['id']}.stderr").open("wb") as err:
            try:
                child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=err, start_new_session=True,
                                         preexec_fn=lambda: signal.pthread_sigmask(signal.SIG_SETMASK, mask))
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)
                while True:
                    if not select.select([child.stdout], [], [], self.remaining())[0]:
                        raise TimeoutError("sheet deadline exceeded during part")
                    chunk = os.read(child.stdout.fileno(), 65536)
                    if not chunk:
                        break
                    buffered += chunk
                    while b"\n" in buffered:
                        line, buffered = buffered.split(b"\n", 1)
                        if len(line) > MAX_EVENT:
                            raise ValueError("part event exceeds 1 MiB")
                        event = json.loads(line)
                        if terminal or event.get("protocol") != "krly3d.drawing-flow/1" or event.get("seq") != child_seq:
                            raise ValueError("invalid child event sequence")
                        if child_seq == 0:
                            child_id = event.get("flow_id")
                        if event.get("flow_id") != child_id or not child_id:
                            raise ValueError("invalid child flow identity")
                        child_seq += 1
                        self.emit("part_event", part_id=part["id"], event=event)
                        if event["kind"] in ("completed", "failed", "cancelled"):
                            terminal = event
                    if len(buffered) > MAX_EVENT:
                        raise ValueError("part event exceeds 1 MiB")
                child.wait(timeout=self.remaining())
                if buffered or child.returncode or not terminal or terminal["kind"] != "completed":
                    raise RuntimeError(f"part {part['id']} did not complete; inspect its journal/stderr")
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)
                if child is not None and child.poll() is None:
                    # Let the child orchestrator clean up its independently grouped domain peers.
                    os.killpg(child.pid, signal.SIGTERM)
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid, signal.SIGKILL)
                        child.wait()
                if child is not None:
                    child.stdout.close()
        evidence = json.loads((output / "evidence.json").read_text())
        signature = hashlib.sha256(json.dumps(evidence["polygons"], sort_keys=True).encode()).hexdigest()
        if any(p.get("component_digest") == signature for p in self.manifest["parts"] if p is not part):
            raise ValueError(f"part {part['id']} selects a component already assigned to another part")
        part.update(status="completed", proposal=terminal["proposal"], preview=str(output / "preview.png"),
                    matches=terminal["matches"], component_digest=signature,
                    artifact_id=terminal["artifact_id"], revision=terminal["revision"],
                    solid_check=json.loads((output / "solid-check.json").read_text()))
        self.write_manifest()
        self.emit("part_completed", part=part)

    def run(self):
        with self.args.image.open("rb") as source:
            image = source.read(16 * 1024 * 1024 + 1)
        if len(image) > 16 * 1024 * 1024:
            raise ValueError("image exceeds 16 MiB")
        picture = self.output / ("drawing" + self.args.image.suffix)
        picture.write_bytes(image)
        self.manifest.update(image=str(picture), image_sha256=hashlib.sha256(image).hexdigest())
        self.write_manifest()
        self.emit("started", parts=self.parts, image_sha256=self.manifest["image_sha256"])
        for part in self.manifest["parts"]:
            self.remaining()
            self.part(part, picture)
        self.manifest.update(status="completed", matches=all(p["matches"] for p in self.manifest["parts"]))
        self.write_manifest()
        self.emit("completed", manifest=str(self.output / "manifest.json"), matches=self.manifest["matches"])

    def fail(self, status, error):
        self.manifest.update(status=status, diagnostic=str(error) or status)
        for part in self.manifest["parts"]:
            if part["status"] == "running":
                part.update(status=status, diagnostic=str(error) or status)
        self.write_manifest()
        self.emit(status, diagnostic=str(error) or status, manifest=str(self.output / "manifest.json"))


def main():
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--parts", type=Path, required=True)
    parser.add_argument("--pixel-unit", type=positive, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=positive, default=120)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "target/debug")
    args = parser.parse_args()
    sheet = None
    try:
        parts = parts_file(args.parts)  # reject malformed manifests before creating any outputs
        sheet = Sheet(args, parts)
        sheet.run()
        return 0
    except (Exception, KeyboardInterrupt) as error:
        status = "cancelled" if isinstance(error, KeyboardInterrupt) else "failed"
        if sheet:
            try:
                sheet.fail(status, error)
            except OSError:
                pass
        print(f"drawing-sheet: {error or status}", file=sys.stderr)
        return 130 if status == "cancelled" else 1
    finally:
        if sheet:
            sheet.events.close()


if __name__ == "__main__":
    sys.exit(main())
