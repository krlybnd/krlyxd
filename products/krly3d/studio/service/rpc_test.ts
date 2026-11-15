import { createServer } from "node:http";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { StudioService } from "../gen/krly/studio/v1/studio_pb.ts";
import { routes } from "./rpc.ts";

function assert(v: unknown, message = "assertion failed"): asserts v {
  if (!v) throw new Error(message);
}
Deno.test("real gRPC-Web: native demo, image pipeline, downloads and invalid input", async () => {
  const server = createServer(connectNodeAdapter({ routes }));
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert(address && typeof address !== "string");
  const client = createClient(
    StudioService,
    createGrpcWebTransport({ baseUrl: `http://127.0.0.1:${address.port}` }),
  );
  const common = {
    sessionId: crypto.randomUUID(),
    text: "",
    mode: "demo",
    height: 3,
    pixelUnit: 1,
    tolerance: 0.1,
  };
  try {
    assert((await client.health({})).native, "build native binaries first");
    const demo = [];
    for await (const event of client.run(common)) demo.push(event);
    assert(demo.at(-1)?.kind === "completed", JSON.stringify(demo.at(-1)));
    assert(
      demo.some((e) =>
        e.mesh && e.mesh.positions.length > 0 && e.mesh.openEdges === 0
      ),
    );
    const model = demo.find((e) => e.artifact?.name.endsWith(".k3d"))?.artifact;
    assert(model);
    const downloaded = await client.download({ id: model.id });
    assert(new TextDecoder().decode(downloaded.data).includes("k3d"));
    const width = 24, height = 20;
    const head = new TextEncoder().encode(`P5 ${width} ${height} 255\n`);
    const data = new Uint8Array(head.length + width * height);
    data.set(head);
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        data[head.length + y * width + x] =
          (x >= 3 && x < 21 && y >= 3 && y < 17 &&
              !(x >= 8 && x < 15 && y >= 7 && y < 13))
            ? 0
            : 255;
      }
    }
    const uploaded = await client.upload({
      name: "ring.pgm",
      mime: "image/x-portable-graymap",
      data,
    });
    const events = [];
    for await (
      const event of client.run({
        ...common,
        mode: "build",
        attachmentIds: [uploaded.id],
      })
    ) events.push(event);
    assert(events.at(-1)?.kind === "completed", JSON.stringify(events.at(-1)));
    const meshes = events.filter((e) => e.mesh).map((e) => e.mesh!);
    assert(meshes.length >= 2, "coarse and final mesh required");
    assert(meshes.at(-1)!.openEdges === 0);
    assert(Math.abs(meshes.at(-1)!.volume - (18 * 14 - 7 * 6) * 3) < 1e-6);
    assert(events.some((e) => e.artifact?.name.endsWith(".png")));
    assert(events.some((e) => e.artifact?.name === "coarse-mesh.json"));
    for (const phase of ["evidence", "preview", "validation"]) {
      assert(
        events.some((e) => e.kind === "status" && e.phase === phase),
        `${phase} must be visible in the execution log`,
      );
    }
    assert(events.every((e, i) => e.seq === i));
    let rejected = false;
    try {
      await client.upload({
        name: "bad.svg",
        mime: "image/png",
        data: new TextEncoder().encode("<svg>not a png</svg>"),
      });
    } catch {
      rejected = true;
    }
    assert(rejected, "MIME spoof must be rejected");
    const failed = [];
    for await (
      const event of client.run({
        ...common,
        sessionId: crypto.randomUUID(),
        mode: "build",
      })
    ) failed.push(event);
    assert(failed.at(-1)?.kind === "error");
    assert(!failed.some((e) => e.kind === "completed"));
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      server.close((err) => err ? reject(err) : resolve())
    );
  }
});
