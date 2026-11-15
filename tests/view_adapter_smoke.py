# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp==2.3.0", "jsonschema==4.26.0"]
# ///
"""Real SDK/schema/PNG parity: build view CLI and MCP, then uv run this file."""
import asyncio
import base64
import copy
import json
from pathlib import Path
import struct
import zlib

from jsonschema import Draft202012Validator
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / "target/debug/krly3d-view"
MCP = ROOT / "target/debug/krly3d-view-mcp"
MODEL = ROOT / "models/plate-with-hole.k3d"


async def cli(*args, document=None):
    process = await asyncio.create_subprocess_exec(
        str(CLI), *map(str, args), stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
    )
    try:
        async with asyncio.timeout(30):
            out, err = await process.communicate(None if document is None else json.dumps(document).encode())
    except BaseException:
        if process.returncode is None:
            process.kill()
        await process.wait()
        raise
    assert process.returncode == 0, err.decode()
    assert not err, err
    return json.loads(out)


def normalized(value):
    value = copy.deepcopy(value)
    source = value["provenance"]["source"]
    assert source["source"] and source["session"]
    source["source"] = "<source>"
    source["session"] = "<session>"
    value.pop("timing")
    return value


def check_png(encoded, width, height):
    data = base64.b64decode(encoded, validate=True)
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    position, compressed, ended = 8, bytearray(), False
    while position < len(data):
        length = struct.unpack_from(">I", data, position)[0]
        kind = data[position + 4:position + 8]
        body = data[position + 8:position + 8 + length]
        crc = struct.unpack_from(">I", data, position + 8 + length)[0]
        assert zlib.crc32(kind + body) == crc
        if kind == b"IHDR":
            w, h, depth, colour, *_ = struct.unpack(">IIBBBBB", body)
            assert (w, h, depth, colour) == (width, height, 8, 2)
        elif kind == b"IDAT":
            compressed.extend(body)
        elif kind == b"IEND":
            ended = True
        position += length + 12
    assert ended and position == len(data)
    assert len(zlib.decompress(compressed)) == height * (1 + width * 3)


async def operations(session):
    descriptor = await cli("--describe-operations")
    for key in ("call_schema", "output_schema"):
        Draft202012Validator.check_schema(descriptor[key])
    schema = Draft202012Validator(descriptor["output_schema"])

    async def tool(name, args):
        response = await session.call_tool(name, args)
        assert not response.is_error, response
        return json.loads(response.content[0].text)

    async def collect(identity):
        events = []
        async with asyncio.timeout(30):
            while len(events) < 2:
                event = await tool("view_next", identity)
                if event is None:
                    await asyncio.sleep(0.01)
                    continue
                schema.validate(event)
                events.append(event)
        assert [event["seq"] for event in events] == [0, 1]
        assert events[0]["body"]["kind"] == "started"
        assert await tool("view_next", identity) is None
        return events

    async def release(identity):
        async with asyncio.timeout(10):
            while (await session.call_tool("view_release", identity)).is_error:
                await asyncio.sleep(0.01)
        assert (await session.call_tool("view_next", identity)).is_error

    for backend in ("raycast", "mesh"):
        call = {"protocol": {"major": 2, "minor": 0}, "api": {"major": 2, "minor": 0},
                "request_id": "operation-" + backend,
                "input": {"node": "part", "width": 48, "height": 32, "backend": backend}}
        Draft202012Validator(descriptor["call_schema"]).validate(call)
        identity = await tool("view_start", call)
        assert (await session.call_tool("view_start", call)).is_error
        await session.send_ping()
        events = await collect(identity)
        rendered = events[1]["body"]["result"]
        sync = copy.deepcopy(call)
        sync["protocol"]["major"] = sync["api"]["major"] = 1
        assert normalized(rendered) == normalized(await cli("--model", MODEL, document=sync))
        process = await asyncio.create_subprocess_exec(str(CLI), "--operations", "--model", str(MODEL),
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(process.communicate(json.dumps(call).encode()), 30)
        assert process.returncode == 0 and not err, err
        cli_events = [json.loads(line) for line in out.splitlines()]
        assert len(cli_events) == 2
        for event in cli_events: schema.validate(event)
        assert normalized(cli_events[1]["body"]["result"]) == normalized(rendered)
        await release(identity)
        # Same request ID can be explicitly reused after release.
        call["timeout_ms"] = 0
        identity = await tool("view_start", call)
        terminal = (await collect(identity))[1]["body"]
        assert terminal["kind"] == "failed" and terminal["diagnostic"]["code"] == "deadline_exceeded"
        await release(identity)

    call.pop("timeout_ms")
    call["input"].update(width=2048, height=2048, backend="raycast")
    identity = await tool("view_start", call)
    await session.send_ping()
    await tool("view_cancel", identity)
    assert (await collect(identity))[1]["body"]["kind"] == "cancelled"
    await release(identity)
    # Connection EOF while a large operation is running must terminate promptly.
    process = await asyncio.create_subprocess_exec(str(MCP), "--model", str(MODEL),
        stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    frames = [
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"eof","version":"1"}}},
        {"jsonrpc":"2.0","method":"notifications/initialized"},
        {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"view_start","arguments":call}},
    ]
    try:
        out, err = await asyncio.wait_for(process.communicate(("\n".join(map(json.dumps, frames))+"\n").encode()), 5)
        assert process.returncode == 0 and not err, err
        replies = [json.loads(line) for line in out.splitlines()]
        assert len(replies) == 2 and not replies[1]["result"]["isError"]
    finally:
        if process.returncode is None: process.kill(); await process.wait()


async def main():
    descriptor = await cli("--describe")
    request_schema = Draft202012Validator(descriptor["call_schema"])
    result_schema = Draft202012Validator(descriptor["output_schema"])
    Draft202012Validator.check_schema(descriptor["call_schema"])
    Draft202012Validator.check_schema(descriptor["output_schema"])
    parameters = StdioServerParameters(command=str(MCP), args=["--model", str(MODEL)])
    async with stdio_client(parameters) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            listed = await session.list_tools()
            assert [tool.name for tool in listed.tools] == ["view_render", "view_start", "view_next", "view_cancel", "view_release"]
            assert listed.tools[0].input_schema == descriptor["call_schema"]
            cameras = [
                {"kind": "named", "name": "iso"},
                {"kind": "orthographic", "toward": [1, 2, -3], "up": [0, 0, 1]},
                {"kind": "pinhole", "eye": [100, -100, 80], "toward": [-1, 1, -0.8], "up": [0, 0, 1], "focal": 80},
            ]
            for backend in ("raycast", "mesh"):
                for camera in cameras:
                    document = {"protocol": {"major": 1, "minor": 0}, "api": {"major": 1, "minor": 0},
                                "request_id": f"{backend}-{camera['kind']}",
                                "input": {"node": "part", "width": 64, "height": 48, "backend": backend, "camera": camera}}
                    request_schema.validate(document)
                    result = await session.call_tool("view_render", document)
                    assert not result.is_error, result
                    rendered = json.loads(result.content[0].text)
                    result_schema.validate(rendered)
                    check_png(rendered["image"]["data"], 64, 48)
                    assert result.content[1].data == rendered["image"]["data"]
                    assert result.content[1].mime_type == "image/png"
                    assert rendered["quality"]["positional_uncertainty"] is None
                    assert normalized(rendered) == normalized(await cli("--model", MODEL, document=document))
            invalid = copy.deepcopy(document)
            invalid["input"]["width"] = 0
            rejected = await session.call_tool("view_render", invalid)
            assert rejected.is_error
            invalid = copy.deepcopy(document)
            invalid["input"]["node"] = "missing"
            assert (await session.call_tool("view_render", invalid)).is_error
            assert not (await session.call_tool("view_render", document)).is_error
            await session.send_ping()
            await operations(session)
    print("view SDK smoke: 6 CLI/MCP PNG parity cases; schemas, errors, v2 operation CLI/MCP parity, deadlines, cancellation, release and EOF passed")


if __name__ == "__main__":
    asyncio.run(main())
