import { createServer } from "node:http";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import { routes } from "./rpc.ts";

const port = Number(Deno.env.get("STUDIO_PORT") ?? "4311");
if (!Number.isInteger(port) || port < 1024 || port > 65535) {
  throw new Error("Invalid STUDIO_PORT");
}
const handler = connectNodeAdapter({
  routes,
  readMaxBytes: 24 * 1024 * 1024,
  writeMaxBytes: 72 * 1024 * 1024,
  maxTimeoutMs: 120_000,
});
const origins = new Set([
  "http://127.0.0.1:4310",
  "http://localhost:4310",
  `http://127.0.0.1:${port}`,
  `http://localhost:${port}`,
]);
const mime: Record<string, string> = {
  html: "text/html; charset=utf-8",
  js: "text/javascript; charset=utf-8",
  css: "text/css; charset=utf-8",
  svg: "image/svg+xml",
  ico: "image/x-icon",
  woff2: "font/woff2",
};
const server = createServer(async (req, res) => {
  const origin = req.headers.origin;
  const host = req.headers.host?.split(":")[0];
  if (
    !host || !["127.0.0.1", "localhost"].includes(host) ||
    (origin && !origins.has(origin))
  ) {
    res.writeHead(403);
    res.end("Local studio origin required");
    return;
  }
  res.setHeader("X-Content-Type-Options", "nosniff");
  if (origin) {
    res.setHeader("Access-Control-Allow-Origin", origin);
    res.setHeader("Vary", "Origin");
  }
  res.setHeader(
    "Access-Control-Allow-Headers",
    "content-type,x-grpc-web,x-user-agent,grpc-timeout,connect-protocol-version,connect-timeout-ms",
  );
  res.setHeader("Access-Control-Allow-Methods", "POST,GET,OPTIONS");
  res.setHeader("Access-Control-Expose-Headers", "grpc-status,grpc-message");
  if (req.method === "OPTIONS") {
    res.writeHead(204);
    res.end();
    return;
  }
  const path = (req.url ?? "/").split("?")[0];
  if (path.startsWith("/krly.studio.v1.StudioService/")) {
    handler(req, res);
    return;
  }
  if (req.method === "GET" && path === "/healthz") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end('{"service":"krly3d-studio","protocol":"grpc-web","ready":true}');
    return;
  }
  if (
    req.method !== "GET" || !/^\/[a-zA-Z0-9_./-]*$/.test(path) ||
    path.includes("..")
  ) {
    res.writeHead(404);
    res.end();
    return;
  }
  try {
    const file = path === "/" ? "index.html" : path.slice(1);
    const bytes = await Deno.readFile(
      new URL(`../dist/${file}`, import.meta.url),
    );
    res.writeHead(200, {
      "content-type": mime[file.split(".").pop() ?? ""] ??
        "application/octet-stream",
      "Cache-Control": file === "index.html"
        ? "no-cache"
        : "public,max-age=3600",
    });
    res.end(bytes);
  } catch {
    res.writeHead(404);
    res.end(
      "Build the studio UI or open the Vite development server on port 4310.",
    );
  }
});
server.listen(
  port,
  "127.0.0.1",
  () => console.log(`KRLY Studio service http://127.0.0.1:${port}`),
);
// Active streams retain their cancellation signal; close sockets on shutdown so
// native children get a stop request rather than becoming detached workers.
for (const sig of ["SIGINT", "SIGTERM"] as const) {
  Deno.addSignalListener(sig, () => {
    server.close();
    server.closeAllConnections();
  });
}
