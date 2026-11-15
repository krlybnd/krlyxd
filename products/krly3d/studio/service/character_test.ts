import { createServer } from "node:http";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { StudioService } from "../gen/krly/studio/v1/studio_pb.ts";
import { routes } from "./rpc.ts";
import { assert, equal } from "./ai/test_support.ts";

// Requires the two real native binaries and the fixed repository recipe.
Deno.test("real character gRPC-Web: native field, print, mm mesh and byte-exact downloads", async () => {
  const server = createServer(
    connectNodeAdapter({ routes, writeMaxBytes: 72 * 1024 * 1024 }),
  );
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert(address && typeof address !== "string");
  const client = createClient(
    StudioService,
    createGrpcWebTransport({ baseUrl: `http://127.0.0.1:${address.port}` }),
  );
  const started = performance.now();
  try {
    assert(
      (await client.health({})).nativeCharacter,
      "build field and print native binaries first",
    );
    const events = [];
    for await (
      const event of client.run({
        sessionId: crypto.randomUUID(),
        mode: "character",
        height: 8,
        pixelUnit: .025,
        tolerance: .05,
      }, { timeoutMs: 120_000 })
    ) {
      events.push(event);
      if (event.kind !== "artifact") {
        console.log(
          `${Math.round(performance.now() - started)} ms ${event.phase}: ${
            event.text.slice(0, 120)
          }`,
        );
      }
    }
    assert(events.at(-1)?.kind === "completed", JSON.stringify(events.at(-1)));
    const final = events.find((event) => event.phase === "final_mesh")?.mesh;
    assert(
      final && final.positions.length > 1000 && final.indices.length > 1000,
    );
    equal(final.units, "mm");
    assert(final.source.startsWith("field:"));
    assert(final.volume > 0 && final.area > 0 && final.openEdges === 0);
    assert(!events.some((event) => event.phase === "editable_proposal"));
    assert(
      events.some((event) => event.phase === "sampling") &&
        events.some((event) => event.phase === "field_mesh"),
    );
    assert(events.every((event, index) => event.seq === index));
    equal(events.find((event) => event.kind === "usage")?.usage?.calls, []);
    const artifacts = new Map<string, Uint8Array>();
    for (const event of events) {
      if (event.artifact) {
        const result = await client.download({ id: event.artifact.id });
        equal(result.name, event.artifact.name);
        equal(result.data.length, event.artifact.size);
        artifacts.set(result.name, result.data);
      }
    }
    equal([...artifacts.keys()].sort(), [
      "mesh.json",
      "pikachu.field.json",
      "pikachu.stl",
      "print-report.json",
    ]);
    const recipe = await Deno.readFile(
      new URL("../../../../models/pikachu.field.json", import.meta.url),
    );
    assert(
      artifacts.get("pikachu.field.json")!.every((byte, index) =>
        byte === recipe[index]
      ),
    );
    equal(artifacts.get("pikachu.field.json")!.length, recipe.length);
    const stl = artifacts.get("pikachu.stl")!;
    equal(
      new DataView(stl.buffer, stl.byteOffset).getUint32(80, true),
      final.indices.length / 3,
    );
    equal(stl.length, 84 + 50 * final.indices.length / 3);
    const report = JSON.parse(
      new TextDecoder().decode(artifacts.get("print-report.json")),
    );
    assert(report.exported && report.float32_roundtrip.status === "passed");
    equal(report.print_ready, false);
    equal(report.self_intersections.status, "passed");
    equal(report.float32_roundtrip.self_intersections.status, "passed");
    equal(report.self_intersections.exact, false);
    equal(report.triangles, final.indices.length / 3);
    const source = JSON.parse(
      new TextDecoder().decode(artifacts.get("mesh.json")),
    );
    equal(source.points.flat(), final.positions);
    equal(source.triangles.flat(), final.indices);
    console.log(
      `Character verified: ${final.positions.length / 3} vertices, ${
        final.indices.length / 3
      } triangles, ${stl.length} STL bytes, ${
        Math.round(performance.now() - started)
      } ms`,
    );
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((error) => error ? reject(error) : resolve())
    );
  }
});
