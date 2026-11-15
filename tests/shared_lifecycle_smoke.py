#!/usr/bin/env python3
"""Process lifecycle regressions for the native 3D reconstruction CLI.

Run after the parent has built target/debug/krly3d-reconstruct; this suite never
builds it. KRLY3D_RECONSTRUCT_CLI can select another already-built binary. Only
the Python standard library is used, with tiny generated PNM fixtures (no SVG).

Linux backpressure tests shrink and completely prefill a pipe BEFORE spawning
the child. Its reader stays open and unread until the child exits. Signals use
a journal/manifest barrier, never a sleep or a deliberately expensive workload.
A journal may contain an attempted terminal that stdout could not deliver;
failed delivery must not append a contradictory terminal.
"""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

try:
    import fcntl
except ImportError:
    fcntl = None


ROOT = Path(__file__).resolve().parents[1]
CLI = Path(os.environ.get("KRLY3D_RECONSTRUCT_CLI",
                          ROOT / "target/debug/krly3d-reconstruct")).resolve()
MODES = ("single", "stream", "sheet")
PROTOCOLS = {
    "single": "krly3d.shared-reconstruction/1",
    "stream": "krly3d.shared-reconstruction/2",
    "sheet": "krly3d.shared-sheet/1",
}
TERMINALS = {"completed", "failed", "cancelled"}
PROMPT_EXIT = 5.0
PIPE_TIMEOUT = 2.0
LINUX_PIPES = (sys.platform.startswith("linux") and fcntl is not None
               and hasattr(fcntl, "F_SETPIPE_SZ"))
PARTS = [
    {"id": "Ring", "seed": [6, 7], "height": 2.5},
    {"id": "Bar", "seed": [42, 11], "height": 6.0},
]
METADATA_KEYS = {
    "revision", "height", "tolerance", "pixel_unit", "seed", "image_sha256",
    "source", "brep_source", "volume", "open", "expected_volume", "confidence",
    "positional_uncertainty",
}
ENVELOPE_KEYS = {"protocol", "execution", "assumptions", "artifacts", "elapsed_ms"}
SHEET_KEYS = {
    "protocol", "sheet_id", "status", "execution", "provenance", "image_sha256",
    "source", "pixel_unit", "tolerance", "assumptions", "parts", "error",
    "artifacts", "replay", "elapsed_ms",
}


def raster():
    """Same ring/bar placement as the shared sheet smoke fixtures."""
    width, height = 64, 40
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            ring = (4 <= x < 28 and 5 <= y < 29
                    and not (10 <= x < 20 and 11 <= y < 21))
            bar = 39 <= x < 51 and 8 <= y < 17
            pixels.append(0 if ring or bar else 255)
    return f"P5 {width} {height} 255\n".encode("ascii") + pixels


def records(data, *, partial_tail=False):
    """Only an unterminated final record may be incomplete after I/O failure."""
    result = []
    lines = data.splitlines(keepends=True)
    for index, line in enumerate(lines):
        try:
            event = json.loads(line)
        except (json.JSONDecodeError, UnicodeDecodeError):
            if partial_tail and index == len(lines) - 1 and not line.endswith(b"\n"):
                break
            raise
        if not isinstance(event, dict):
            raise AssertionError(f"expected a JSON object, got {event!r}")
        result.append(event)
    return result


class FullPipe:
    """A blocking child writer with zero free space and a live, idle reader."""

    def __enter__(self):
        self.reader, self.writer = os.pipe()
        try:
            capacity = fcntl.fcntl(self.writer, fcntl.F_SETPIPE_SZ, 4096)
            self.prefill = b"#" * capacity
            flags = fcntl.fcntl(self.writer, fcntl.F_GETFL)
            fcntl.fcntl(self.writer, fcntl.F_SETFL, flags | os.O_NONBLOCK)
            try:
                written = os.write(self.writer, self.prefill)
                if written != capacity:
                    raise AssertionError(f"prefill wrote {written}/{capacity} bytes")
                try:
                    os.write(self.writer, b"!")
                except BlockingIOError:
                    pass
                else:
                    raise AssertionError("pipe still has space after prefill")
            finally:
                # The CLI inherits a BLOCKING fd: it must make its own writes
                # interruptible, rather than accidentally benefiting from ours.
                fcntl.fcntl(self.writer, fcntl.F_SETFL, flags)
        except BaseException:
            self.__exit__(None, None, None)
            raise
        return self

    def after_exit(self, process):
        if process.poll() is None:
            raise AssertionError("must not drain a pipe while the child is alive")
        os.close(self.writer)
        self.writer = None
        chunks = []
        while True:
            chunk = os.read(self.reader, 65536)
            if not chunk:
                return b"".join(chunks)
            chunks.append(chunk)

    def __exit__(self, *_):
        for name in ("reader", "writer"):
            descriptor = getattr(self, name, None)
            if descriptor is not None:
                os.close(descriptor)
                setattr(self, name, None)


class SharedLifecycleSmoke(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not CLI.is_file() or not os.access(CLI, os.X_OK):
            raise RuntimeError(f"built native CLI unavailable: {CLI}")

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="shared-lifecycle-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.image = self.root / "drawing.pgm"
        self.image.write_bytes(raster())
        self.parts = self.root / "parts.json"
        self.parts.write_text(json.dumps(PARTS) + "\n", encoding="utf-8")

    def command(self, mode, output, *extra, image=None, parts=None):
        command = [CLI, "--image", image or self.image, "--output", output,
                   "--pixel-unit", "0.25"]
        if mode == "sheet":
            command.extend(("--parts", parts or self.parts))
        else:
            command.extend(("--height", "2.5", "--seed", "6,7"))
            if mode == "stream":
                command.append("--stream")
        return list(map(str, [*command, *extra]))

    def invoke(self, command, *, success, timeout=15):
        try:
            result = subprocess.run(command, cwd=self.root, capture_output=True,
                                    timeout=timeout)
        except subprocess.TimeoutExpired:
            self.fail(f"CLI did not exit within {timeout}s: {command}")
        self.assertEqual(result.returncode == 0, success,
                         f"command: {command}\nreturncode: {result.returncode}"
                         f"\nstdout: {result.stdout!r}\nstderr: {result.stderr!r}")
        return result

    @contextmanager
    def child(self, command, *, stdout, stderr=None):
        # A regular file avoids introducing an unrelated unread stderr pipe.
        with tempfile.TemporaryFile() as errors:
            process = subprocess.Popen(command, cwd=self.root, stdin=subprocess.DEVNULL,
                                       stdout=stdout,
                                       stderr=errors if stderr is None else stderr)
            try:
                yield process, errors
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=PROMPT_EXIT)

    @staticmethod
    def stderr(errors):
        errors.seek(0)
        return errors.read().decode("utf-8", errors="replace")

    def wait_failed(self, process, errors, *, since=None):
        since = time.monotonic() if since is None else since
        remaining = PROMPT_EXIT - (time.monotonic() - since)
        try:
            process.wait(timeout=max(0, remaining))
        except subprocess.TimeoutExpired:
            self.fail(f"CLI failed to stop within {PROMPT_EXIT}s; "
                      f"stderr: {self.stderr(errors)!r}")
        self.assertLessEqual(time.monotonic() - since, PROMPT_EXIT,
                             "CLI exit exceeded the lifecycle bound")
        self.assertNotEqual(process.returncode, 0, self.stderr(errors))

    def journal(self, output):
        path = output / "events.jsonl"
        return records(path.read_bytes(), partial_tail=True) if path.exists() else []

    def barrier(self, process, errors, mode, output):
        """Observe admission before signalling; the prefilled pipe cannot drain."""
        until = time.monotonic() + PROMPT_EXIT
        while True:
            if process.poll() is not None:
                self.fail(f"child exited before {mode} output barrier: "
                          f"{self.stderr(errors)!r}")
            try:
                if mode == "single":
                    value = json.loads((output / "manifest.json").read_bytes())
                    ready = value.get("protocol") == PROTOCOLS[mode]
                else:
                    events = self.journal(output)
                    ready = bool(events and events[0].get("kind") == "started")
                    if mode == "sheet":
                        value = json.loads((output / "manifest.json").read_bytes())
                        ready = ready and value.get("status") == "running"
            except (FileNotFoundError, json.JSONDecodeError, UnicodeDecodeError):
                ready = False  # Polling may observe an in-progress file write.
            if ready:
                self.assertIsNone(process.poll(), "child exited at the output barrier")
                return
            if time.monotonic() >= until:
                self.fail(f"no {mode} output barrier within {PROMPT_EXIT}s; "
                          f"stderr: {self.stderr(errors)!r}")
            time.sleep(0.01)  # Poll a predicate; never use a sleep as readiness.

    def assert_terminals(self, events):
        terminals = [event["kind"] for event in events
                     if event.get("kind") in TERMINALS]
        self.assertLessEqual(len(terminals), 1,
                             f"multiple or contradictory terminal attempts: {terminals}")
        if terminals:
            self.assertEqual(events[-1].get("kind"), terminals[0],
                             "events followed a terminal attempt")

    def assert_failure_artifacts(self, mode, output, *, admitted=True):
        self.assert_terminals(self.journal(output))
        manifest = output / "manifest.json"
        if mode != "sheet":
            self.assertFalse(manifest.exists(), "failed single result kept its manifest")
            return None
        if not admitted and not output.exists():
            return None
        self.assertTrue(manifest.is_file(), "admitted sheet lost its retained manifest")
        value = json.loads(manifest.read_bytes())
        self.assertEqual(value["protocol"], PROTOCOLS[mode])
        self.assertIn(value["status"], ("failed", "cancelled"))
        self.assertTrue(value["error"]["code"])
        self.assertTrue(value["error"]["message"])
        for entry in value["parts"]:
            if entry["status"] == "completed":
                self.assertIsInstance(entry["result"], dict)
                self.assertTrue(entry["artifacts"])
                for relative in entry["artifacts"].values():
                    self.assertTrue((output / relative).is_file(), relative)
        return value

    def assert_success_schema(self, mode, output, result):
        manifest = json.loads((output / "manifest.json").read_bytes())
        self.assertEqual(manifest["protocol"], PROTOCOLS[mode])
        if mode == "sheet":
            self.assertEqual(set(manifest), SHEET_KEYS)
            self.assertEqual(manifest["status"], "completed")
            self.assertEqual([part["status"] for part in manifest["parts"]],
                             ["completed", "completed"])
        else:
            extra = {"computation_ms"} if mode == "single" else {"flow_id"}
            self.assertEqual(set(manifest), METADATA_KEYS | ENVELOPE_KEYS | extra)
        events = records(result.stdout)
        if mode == "single":
            self.assertEqual(events, [manifest])
            self.assertNotIn("kind", manifest)
        else:
            self.assertEqual(events[0]["kind"], "started")
            self.assertEqual(events[-1]["kind"], "completed")
            self.assertEqual(events[-1]["manifest"], manifest)
            self.assertEqual([event["seq"] for event in events], list(range(len(events))))
            self.assertTrue(all(event["protocol"] == PROTOCOLS[mode] for event in events))
            self.assertEqual((output / "events.jsonl").read_bytes(), result.stdout)
            self.assert_terminals(events)
        return manifest

    def test_default_and_short_timeouts_preserve_success_schemas(self):
        # Omission exercises the default (120s) without a two-minute test wait.
        for mode in MODES:
            for index, seconds in enumerate((None, "5", "5.5")):
                with self.subTest(mode=mode, timeout=seconds):
                    output = self.root / f"success-{mode}-{index}"
                    extra = () if seconds is None else ("--timeout", seconds)
                    result = self.invoke(self.command(mode, output, *extra), success=True)
                    self.assert_success_schema(mode, output, result)

    def test_timeout_rejection_precedes_output(self):
        invalid = ("0", "-0", "-1", "-0.1", "nan", "NaN", "inf", "-inf",
                   "Infinity", "-Infinity", "1e309", "1e100", "nope", "")
        for mode in MODES:
            for index, seconds in enumerate(invalid):
                with self.subTest(mode=mode, timeout=seconds):
                    output = self.root / f"invalid-{mode}-{index}"
                    result = self.invoke(self.command(mode, output, "--timeout", seconds),
                                         success=False, timeout=PROMPT_EXIT)
                    self.assertEqual(result.stdout, b"")
                    self.assertIn(b"timeout", result.stderr.lower())
                    self.assertFalse(output.exists(), "invalid timeout reserved output")
            for label, extra in (("missing", ("--timeout",)),
                                 ("duplicate", ("--timeout", "5", "--timeout", "6"))):
                with self.subTest(mode=mode, timeout=label):
                    output = self.root / f"invalid-{mode}-{label}"
                    result = self.invoke(self.command(mode, output, *extra),
                                         success=False, timeout=PROMPT_EXIT)
                    self.assertEqual(result.stdout, b"")
                    self.assertFalse(output.exists())

    def test_timeout_never_overwrites_existing_results(self):
        for mode in MODES:
            output = self.root / f"existing-{mode}"
            self.invoke(self.command(mode, output), success=True)
            before = {str(path.relative_to(output)):
                      path.read_bytes() if path.is_file() else None
                      for path in output.rglob("*")}
            for seconds in ("5", "NaN", "0"):
                with self.subTest(mode=mode, timeout=seconds):
                    result = self.invoke(self.command(mode, output, "--timeout", seconds),
                                         success=False, timeout=PROMPT_EXIT)
                    self.assertEqual(result.stdout, b"")
                    after = {str(path.relative_to(output)):
                             path.read_bytes() if path.is_file() else None
                             for path in output.rglob("*")}
                    self.assertEqual(after, before)

    def test_tiny_valid_deadline_fails_promptly_in_every_mode(self):
        for mode in MODES:
            with self.subTest(mode=mode):
                output = self.root / f"tiny-{mode}"
                result = self.invoke(self.command(mode, output, "--timeout", "0.000000001"),
                                     success=False, timeout=PROMPT_EXIT)
                self.assertIn(b"deadline", result.stderr.lower())
                events = records(result.stdout, partial_tail=True)
                self.assert_terminals(events)
                self.assertNotIn("completed", [event.get("kind") for event in events])
                self.assert_failure_artifacts(mode, output, admitted=False)

    @unittest.skipUnless(LINUX_PIPES, "prefilled pipe regressions require Linux fcntl")
    def test_deadline_interrupts_genuinely_full_stdout(self):
        for mode in MODES:
            with self.subTest(mode=mode), FullPipe() as pipe:
                output = self.root / f"deadline-{mode}"
                command = self.command(mode, output, "--timeout", str(PIPE_TIMEOUT))
                with self.child(command, stdout=pipe.writer) as (process, errors):
                    started = time.monotonic()
                    self.barrier(process, errors, mode, output)
                    self.wait_failed(process, errors, since=started)
                    self.assertIn("deadline", self.stderr(errors).lower())
                    self.assertEqual(pipe.after_exit(process), pipe.prefill)
                self.assert_failure_artifacts(mode, output)

    @unittest.skipUnless(LINUX_PIPES, "prefilled pipe signal regressions require Linux")
    def test_sigint_and_sigterm_interrupt_genuinely_full_stdout(self):
        for mode in MODES:
            for signum in (signal.SIGINT, signal.SIGTERM):
                with self.subTest(mode=mode, signal=signum), FullPipe() as pipe:
                    output = self.root / f"signal-{mode}-{signum}"
                    # No explicit timeout: this also rules out a short accidental
                    # default deadline masquerading as cooperative cancellation.
                    with self.child(self.command(mode, output), stdout=pipe.writer) as (process, errors):
                        self.barrier(process, errors, mode, output)
                        sent = time.monotonic()
                        process.send_signal(signum)
                        self.wait_failed(process, errors, since=sent)
                        self.assertIn("cancel", self.stderr(errors).lower(),
                                      "signal exited without a cooperative cancellation diagnostic")
                        self.assertEqual(pipe.after_exit(process), pipe.prefill)
                    self.assert_failure_artifacts(mode, output)

    @unittest.skipUnless(os.name == "posix", "closed pipe readers require POSIX")
    def test_closed_stdout_reader_fails_promptly_in_every_mode(self):
        for mode in MODES:
            with self.subTest(mode=mode):
                output = self.root / f"closed-{mode}"
                reader, writer = os.pipe()
                os.close(reader)
                try:
                    with self.child(self.command(mode, output), stdout=writer) as (process, errors):
                        self.wait_failed(process, errors)
                finally:
                    os.close(writer)
                self.assert_failure_artifacts(mode, output)

    @unittest.skipUnless(LINUX_PIPES, "full stderr regressions require Linux fcntl")
    def test_full_stderr_does_not_hang_errors_or_stdout_deadlines(self):
        for mode in MODES:
            with self.subTest(mode=mode, failure="invalid-timeout"), FullPipe() as errors_pipe:
                output = self.root / f"stderr-invalid-{mode}"
                command = self.command(mode, output, "--timeout", "NaN")
                with self.child(command, stdout=subprocess.DEVNULL,
                                stderr=errors_pipe.writer) as (process, errors):
                    self.wait_failed(process, errors)
                    self.assertEqual(errors_pipe.after_exit(process), errors_pipe.prefill)
                self.assertFalse(output.exists())
            with self.subTest(mode=mode, failure="deadline"), FullPipe() as out_pipe, FullPipe() as errors_pipe:
                output = self.root / f"stderr-deadline-{mode}"
                command = self.command(mode, output, "--timeout", str(PIPE_TIMEOUT))
                with self.child(command, stdout=out_pipe.writer,
                                stderr=errors_pipe.writer) as (process, errors):
                    started = time.monotonic()
                    self.barrier(process, errors, mode, output)
                    self.wait_failed(process, errors, since=started)
                    self.assertEqual(out_pipe.after_exit(process), out_pipe.prefill)
                    self.assertEqual(errors_pipe.after_exit(process), errors_pipe.prefill)
                self.assert_failure_artifacts(mode, output)

    def test_later_sheet_failure_retains_completed_parts(self):
        # Deterministic failure after one real completed part, without racing CPU
        # work against a signal. The existing sheet suite verifies full geometry.
        parts = [PARTS[0], {"id": "BadSeed", "seed": [31, 5], "height": 4}, PARTS[1]]
        self.parts.write_text(json.dumps(parts) + "\n", encoding="utf-8")
        output = self.root / "retained-sheet"
        result = self.invoke(self.command("sheet", output, "--timeout", "5"), success=False)
        manifest = self.assert_failure_artifacts("sheet", output)
        self.assertEqual([entry["status"] for entry in manifest["parts"]],
                         ["completed", "failed", "pending"])
        events = records(result.stdout)
        self.assertEqual((output / "events.jsonl").read_bytes(), result.stdout)
        self.assert_terminals(events)
        completed = [event for event in events if event.get("kind") == "part_completed"]
        self.assertEqual([event["part_id"] for event in completed], ["Ring"])
        self.assertEqual(completed[0]["result"], manifest["parts"][0]["result"])
        self.assertEqual(events[-1]["kind"], "failed")
        self.assertEqual(events[-1]["manifest"], manifest)
        self.assertFalse((output / "parts/Bar").exists())
        self.assertEqual((output / manifest["replay"]["image"]).read_bytes(), self.image.read_bytes())

    @unittest.skipUnless(os.name == "posix", "FIFO admission regression requires POSIX")
    def test_input_fifos_are_rejected_without_waiting_for_a_writer(self):
        image_fifo = self.root / "input-fifo.pgm"
        parts_fifo = self.root / "parts-fifo.json"
        os.mkfifo(image_fifo)
        os.mkfifo(parts_fifo)
        cases = [(mode, image_fifo, None) for mode in MODES]
        cases.append(("sheet", None, parts_fifo))
        for index, (mode, image, parts) in enumerate(cases):
            with self.subTest(mode=mode, fifo="parts" if parts else "image"):
                output = self.root / f"fifo-{index}"
                # No FIFO writer is ever opened. A blocking File::open would
                # hang here even with an otherwise functioning call deadline.
                result = self.invoke(self.command(mode, output, "--timeout", "1",
                                                  image=image, parts=parts),
                                     success=False, timeout=PROMPT_EXIT)
                self.assertEqual(result.stdout, b"")
                self.assertIn(b"regular", result.stderr.lower(),
                              "FIFO must fail admission as nonregular, rather than time out")
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
