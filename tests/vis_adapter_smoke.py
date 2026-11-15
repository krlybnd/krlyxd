# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp==2.3.0", "jsonschema==4.26.0"]
# ///
"""Real krlyvis CLI/MCP/legacy evidence parity through the official MCP SDK."""
import asyncio
import base64
import copy
import hashlib
import json
from pathlib import Path
import struct
import tempfile
import zlib

from jsonschema import Draft202012Validator
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug"


async def execute(command, data=b""):
    process = await asyncio.create_subprocess_exec(
        *map(str, command), stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    try:
        async with asyncio.timeout(30):
            out, err = await process.communicate(data)
    except BaseException:
        if process.returncode is None:
            process.kill()
        await process.wait()
        raise
    assert process.returncode == 0, err.decode()
    return out.decode()


def raster(format):
    width, height = 48, 32
    pixels = bytes(0 if ((3 <= x < 23 and 4 <= y < 28 and not (8 <= x < 16 and 10 <= y < 20))
                        or (32 <= x < 44 and 8 <= y < 24)) else 255
                   for y in range(height) for x in range(width))
    if format == "pnm":
        return b"P5 48 32 255\n" + pixels
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\0" + pixels[y*width:(y+1)*width] for y in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


async def legacy(image, seed, scale):
    calls = [
        ("picture", {"id": "image", "path": str(image)}),
        ("outline", {"id": "outline", "inputs": ["image"], "simplify": 0, "method": "trace-holes",
                     **({"seed": seed} if seed is not None else {})}),
        ("frame", {"id": "top", "inputs": ["outline"], "view": "top",
                   "points": [0, 0, 0, 0, 1, 1, scale, scale]}),
        ("polygons", {"node": "top@1"}),
    ]
    requests = b"".join((json.dumps({"jsonrpc": "2.0", "id": n, "method": name, "params": params}) + "\n").encode()
                        for n, (name, params) in enumerate(calls))
    replies = [json.loads(line) for line in (await execute([BIN / "krlyvis"], requests)).splitlines()]
    assert len(replies) == 4 and all(reply.get("result", {}).get("ok") for reply in replies), replies
    return replies[-1]["result"]["data"]


async def main():
    cli = BIN / "krlyvis-drawing"
    descriptor = json.loads(await execute([cli, "--describe"]))
    for key in ("call_schema", "output_schema"):
        Draft202012Validator.check_schema(descriptor[key])
    inputs = Draft202012Validator(descriptor["call_schema"])
    outputs = Draft202012Validator(descriptor["output_schema"])
    async with stdio_client(StdioServerParameters(command=str(BIN / "krlyvis-drawing-mcp"))) as streams:
        async with ClientSession(*streams) as session:
            await session.initialize()
            listed = await session.list_tools()
            assert [t.name for t in listed.tools] == ["vis_drawing"]
            assert listed.tools[0].input_schema == descriptor["call_schema"]
            with tempfile.TemporaryDirectory() as temporary:
                for format in ("pnm", "png"):
                    data = raster(format)
                    path = Path(temporary) / ("source.pgm" if format == "pnm" else "source.png")
                    path.write_bytes(data)
                    for seed, area, holes in ((None, 400, 1), ([5.5, 6.5], 400, 1), ([35, 10], 192, 0)):
                        call = {"protocol": "krlyvis.drawing/1", "request_id": f"{format}-{seed}",
                                "input": {"image": {"format": format, "data": base64.b64encode(data).decode()},
                                          "pixel_unit": 0.25, "seed": seed, "source": "caller:fixture"}}
                        inputs.validate(call)
                        direct = json.loads(await execute([cli], json.dumps(call).encode()))
                        response = await session.call_tool("vis_drawing", call)
                        assert not response.is_error, response
                        assert len(response.content) == 1 and response.content[0].type == "text"
                        remote = json.loads(response.content[0].text)
                        outputs.validate(direct)
                        outputs.validate(remote)
                        assert remote["evidence"] == direct["evidence"] == await legacy(path, seed, 0.25)
                        assert remote["quality"] == direct["quality"]
                        assert remote["evidence"]["material_area"] == area * 0.25**2
                        assert len(remote["evidence"]["polygons"]) == holes + 1
                        assert remote["provenance"]["image_sha256"] == hashlib.sha256(data).hexdigest()
                        assert remote["provenance"]["source"] == "caller:fixture"
                        invalid = copy.deepcopy(call)
                        invalid["input"]["seed"] = [10, 12]  # hole: must not silently pick another component
                        rejected = await session.call_tool("vis_drawing", invalid)
                        assert rejected.is_error
                        retried = await session.call_tool("vis_drawing", call)
                        assert not retried.is_error
                        assert json.loads(retried.content[0].text)["evidence"] == remote["evidence"]
                        await session.send_ping()
    print("krlyvis SDK: 6 CLI/MCP/legacy parity cases, PNM/PNG, holes/scales/seeds/image hashes, errors/reuse/ping passed")


if __name__ == "__main__":
    asyncio.run(main())
