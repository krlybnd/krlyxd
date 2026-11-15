import { BlobStore, imageFormat, SessionStore } from "./store.ts";
function assert(value: unknown, message = "assertion failed"): asserts value {
  if (!value) throw new Error(message);
}
function rejects(fn: () => unknown) {
  let caught = false;
  try {
    fn();
  } catch {
    caught = true;
  }
  assert(caught);
}
Deno.test("uploads use content signatures and bounded bytes, not caller MIME or paths", () => {
  const png = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10, 0]);
  assert(imageFormat(png) === "png");
  rejects(() =>
    imageFormat(new TextEncoder().encode('<svg onload="alert(1)">'))
  );
  rejects(() => imageFormat(new Uint8Array(17 * 1024 * 1024)));
  const store = new BlobStore();
  const entry = store.put("../../test.png", "image/png", png);
  assert(!entry.name.includes("/"));
  assert(store.get(entry.id).bytes === png);
  rejects(() => store.get("../../private"));
});
Deno.test("session admission cannot overlap; failed work can release and retry", () => {
  const store = new SessionStore();
  const session = store.acquire("session-test");
  rejects(() => store.acquire("session-test"));
  session.history.push({ role: "user", content: "one" });
  store.release(session);
  const next = store.acquire("session-test");
  assert(next.history.length === 1);
  assert(next.active);
  store.release(next);
  const long = store.acquire("session-test");
  long.history = Array.from(
    { length: 12 },
    () => ({ role: "user" as const, content: "x".repeat(8000) }),
  );
  store.release(long);
  assert(long.history.reduce((sum, m) => sum + m.content.length, 0) <= 48_000);
  rejects(() => store.acquire("invalid space"));
});
