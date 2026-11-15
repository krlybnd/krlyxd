# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp==2.3.0", "jsonschema==4.26.0"]
# ///
"""Real SDK acceptance: uv run tests/mesh_adapter_smoke.py after building both binaries."""

import argparse
import asyncio
import copy
import json
import math
from pathlib import Path
import tempfile

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from jsonschema import Draft202012Validator


def request(name, node="part", **options):
    return {
        "request_id": name,
        "protocol": {"major": 2, "minor": 0},
        "api": {"major": 2, "minor": 0},
        "input": {"node": node, **options},
    }


def payload(result):
    return json.loads("".join(item.text for item in result.content if item.type == "text"))


async def call(session, name, arguments):
    result = await session.call_tool(name, arguments)
    return result, payload(result)


async def collect(session, operation_id):
    events = []
    async with asyncio.timeout(30):
        while len(events) < 2:
            result, data = await call(session, "operation_events", {"operation_id": operation_id})
            events.extend(data["events"])
            if data["events"]:
                terminal = data["events"][-1]["body"]["kind"]
                assert result.is_error == (terminal in ("failed", "cancelled")), result
            await asyncio.sleep(0.005)
    assert [event["seq"] for event in events] == [0, 1], events
    assert events[0]["body"]["kind"] == "started", events
    assert all(event["operation_id"] == operation_id for event in events)
    return events


def normalized(result):
    result = copy.deepcopy(result)
    # Session/artifact identities intentionally differ between independent processes.
    assert result["provenance"]["source"]
    result["provenance"]["source"] = "<session-source>"
    if result.get("mesh"):
        assert result["mesh"]["id"]
        result["mesh"]["id"] = "<artifact-id>"
    return result


async def cli_events(binary, model, document):
    process = await asyncio.create_subprocess_exec(
        str(binary), "--model", str(model),
        stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
    )
    try:
        stdout, stderr = await asyncio.wait_for(process.communicate(json.dumps(document).encode()), 30)
    except BaseException:
        if process.returncode is None:
            process.kill()
        await process.wait()
        raise
    assert process.returncode == 0, (process.returncode, stderr)
    assert not stderr, stderr
    events = [json.loads(line) for line in stdout.splitlines()]
    assert [event["body"]["kind"] for event in events] == ["started", "completed"]
    return events


async def run(cli, mcp):
    describe = await asyncio.create_subprocess_exec(
        str(cli), "--describe", stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
    )
    out, err = await describe.communicate()
    assert describe.returncode == 0 and not err, err
    descriptor = json.loads(out)
    event_validator = Draft202012Validator(descriptor["output_schema"])
    with tempfile.TemporaryDirectory(prefix="krlyxd-mcp-smoke-") as folder:
        model = Path(folder) / "fixture.k3d"
        model.write_text(
            "k3d 1\nlayer geo\npart = box w=6 d=4 h=2\n"
            "part = box w=3 d=4 h=2\nball = sphere r=5\n",
            encoding="utf-8",
        )
        parameters = StdioServerParameters(command=str(mcp), args=["--model", str(model)])
        async with stdio_client(parameters) as (reader, writer):
            async with ClientSession(reader, writer, read_timeout_seconds=10) as session:
                initialized = await session.initialize()
                listed = await session.list_tools()
                tools = {tool.name: tool for tool in listed.tools}
                assert {"mesh", "operation_events", "operation_cancel", "operation_release"} <= tools.keys()
                schema = tools["mesh"].input_schema
                assert schema == descriptor["call_schema"]
                Draft202012Validator.check_schema(schema)
                cases = [
                    (request("defaults"), 24.0),
                    (request("versioned", "part@1", triangles=True), 48.0),
                    (request("precision", tolerance={"value": 0.10000000000000002, "unit": "model"}), 24.0),
                    (request("sphere", "ball", tolerance={"value": 0.1, "unit": "model"}), None),
                ]
                for document, volume in cases:
                    Draft202012Validator(schema).validate(document)
                    accepted, data = await call(session, "mesh", document)
                    assert not accepted.is_error, data
                    handle = data["operation_id"]
                    duplicate, _ = await call(session, "mesh", document)
                    assert duplicate.is_error
                    events = await collect(session, handle)
                    for event in events:
                        event_validator.validate(event)
                    assert all(event["request_id"] == document["request_id"] for event in events)
                    result = events[-1]["body"]["result"]
                    assert result["open"] == 0
                    if volume is None:
                        assert abs(result["volume"] - 4 * math.pi * 125 / 3) <= 4 * math.pi * 25 * 0.1
                    else:
                        assert result["volume"] == volume
                    cli_stream = await cli_events(cli, model, document)
                    for event in cli_stream:
                        event_validator.validate(event)
                    expected = cli_stream[-1]["body"]["result"]
                    assert normalized(result) == normalized(expected), (result, expected)

                for document in [
                    request("missing", "absent"),
                    {**request("bad-version"), "api": {"major": 999, "minor": 0}},
                    {**request("unknown"), "extra": True},
                ]:
                    rejected, _ = await call(session, "mesh", document)
                    assert rejected.is_error

                deadline = {**request("deadline"), "timeout_ms": 0}
                accepted, data = await call(session, "mesh", deadline)
                assert not accepted.is_error
                events = await collect(session, data["operation_id"])
                assert events[-1]["body"]["diagnostic"]["code"] == "deadline_exceeded"

                long_call = request("cancel", "ball", tolerance={"value": 0.0005, "unit": "model"})
                accepted, data = await call(session, "mesh", long_call)
                assert not accepted.is_error
                handle = data["operation_id"]
                for _ in range(2):
                    cancelled, _ = await call(session, "operation_cancel", {"operation_id": handle})
                    assert not cancelled.is_error
                events = await collect(session, handle)
                assert events[-1]["body"]["kind"] == "cancelled"
                async with asyncio.timeout(10):
                    while True:
                        released, _ = await call(session, "operation_release", {"operation_id": handle})
                        if not released.is_error:
                            break
                        await asyncio.sleep(0.01)
                missing, _ = await call(session, "operation_events", {"operation_id": handle})
                assert missing.is_error
                await session.send_ping()
                print(f"PASS: MCP {initialized.protocol_version}; 4 CLI/MCP geometry-parity cases; "
                      "schema, rejection, duplicate IDs, deadline, repeated cancel, release, session reuse")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, default=Path("target/debug/krly3d-mesh"))
    parser.add_argument("--mcp", type=Path, default=Path("target/debug/krly3d-mesh-mcp"))
    args = parser.parse_args()
    asyncio.run(run(args.cli.resolve(), args.mcp.resolve()))
