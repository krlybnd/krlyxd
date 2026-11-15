# /// script
# requires-python = ">=3.12"
# dependencies = ["mcp==2.3.0", "jsonschema==4.26.0"]
# ///
"""Official MCP client + CLI parity and independent mesh reload of planar B-rep."""
import asyncio
import copy
import json
from pathlib import Path
import tempfile

from jsonschema import Draft202012Validator
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / "target/debug/krly3d-brep"
MCP = ROOT / "target/debug/krly3d-brep-mcp"
MESH = ROOT / "target/debug/krly3d-mesh"


async def execute(command, document=None):
    process = await asyncio.create_subprocess_exec(
        *map(str, command), stdin=asyncio.subprocess.PIPE,
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
    return out.decode()


async def main():
    descriptor = json.loads(await execute([CLI, "--describe"]))
    for key in ("call_schema", "output_schema"):
        Draft202012Validator.check_schema(descriptor[key])
    request_schema = Draft202012Validator(descriptor["call_schema"])
    result_schema = Draft202012Validator(descriptor["output_schema"])
    fixtures = [
        ("offset-plate", [[[3, 5], [13, 5], [13, 11], [3, 11]]], 2, 120),
        ("through-hole", [[[0, 0], [10, 0], [10, 8], [0, 8]],
                          [[2, 2], [2, 6], [6, 6], [6, 2]]], 3, 192),
        ("concave", [[[0, 0], [8, 0], [8, 2], [3, 2], [3, 7], [0, 7]]], 0.5, 15.5),
    ]
    async with stdio_client(StdioServerParameters(command=str(MCP))) as streams:
        async with ClientSession(*streams) as session:
            await session.initialize()
            listed = await session.list_tools()
            assert [tool.name for tool in listed.tools] == ["brep_planar"]
            assert listed.tools[0].input_schema == descriptor["call_schema"]
            with tempfile.TemporaryDirectory() as temporary:
                for name, polygons, height, volume in fixtures:
                    call = {"protocol": "krly3d.brep.planar/1", "request_id": name,
                            "input": {"polygons": polygons, "height": height,
                                      "length_unit": "model", "source": "fixture:" + name}}
                    request_schema.validate(call)
                    cli = json.loads(await execute([CLI], call))
                    response = await session.call_tool("brep_planar", call)
                    assert not response.is_error, response
                    assert len(response.content) == 1 and response.content[0].type == "text"
                    mcp = json.loads(response.content[0].text)
                    result_schema.validate(cli)
                    result_schema.validate(mcp)
                    for key in ("protocol", "request_id", "model", "node", "source", "quality"):
                        assert cli[key] == mcp[key], (name, key)
                    assert cli["node"] == "part@1"
                    assert cli["source"] == call["input"]["source"]
                    assert cli["provenance"] != mcp["provenance"], "sessions must have independent provenance"
                    model = Path(temporary) / f"{name}.k3d"
                    model.write_text(mcp["model"])
                    request = {"protocol": {"major": 2, "minor": 0}, "api": {"major": 2, "minor": 0},
                               "request_id": name, "input": {"node": "part@1"}}
                    mesh_events = [json.loads(line) for line in
                                   (await execute([MESH, "--model", model], request)).splitlines()]
                    assert [e["body"]["kind"] for e in mesh_events] == ["started", "completed"]
                    mesh = mesh_events[-1]["body"]["result"]
                    assert abs(mesh["volume"] - volume) < 1e-8, (name, mesh["volume"], volume)
                    assert mesh["open"] == 0
                    invalid = copy.deepcopy(call)
                    invalid["input"]["height"] = 0
                    rejected = await session.call_tool("brep_planar", invalid)
                    assert rejected.is_error, rejected
                    # Failed input cannot poison the stateless session or alter later output.
                    retried = await session.call_tool("brep_planar", call)
                    assert not retried.is_error
                    assert json.loads(retried.content[0].text)["model"] == cli["model"]
                    await session.send_ping()
    print("B-rep SDK: 3 CLI/MCP schema+model parity cases, independent volume/closure reload, errors/reuse/ping passed")


if __name__ == "__main__":
    asyncio.run(main())
