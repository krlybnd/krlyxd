import {
  decodeImageSearch,
  fetchImageCandidate,
  publicAddress,
  searchImages,
} from "./image_search.ts";
import { assert, configureLiteLLM, equal, isolated } from "./test_support.ts";
import type { ImageCandidate } from "./image_search.ts";
import type { UsageCall } from "./expert-types.ts";

const candidate: ImageCandidate = {
  imageURL: "https://images.example.org/shape.png",
  caption: "Top view silhouette",
  source: { title: "Reference", url: "https://example.org/reference" },
};
const png = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);

Deno.test("image result decoder accepts bounded cited HTTPS candidates only", () => {
  const result = decodeImageSearch({
    status: "completed",
    output: [{
      type: "web_search_call",
      status: "completed",
      results: [{
        image_url: candidate.imageURL,
        source_website_url: candidate.source.url,
        caption: candidate.caption,
        title: candidate.source.title,
      }, {
        image_url: "http://127.0.0.1/private.png",
        source_website_url: "http://127.0.0.1/",
        title: "Local",
      }],
    }],
  });
  equal(result.candidates, [candidate]);
  equal(result.sources, [candidate.source]);
});

Deno.test("image search uses one bounded image+text web search and records usage", () =>
  isolated(async () => {
    configureLiteLLM();
    Deno.env.set("LITELLM_RESEARCH_MODEL", "search-model");
    Deno.env.set("STUDIO_RESEARCH_ENABLED", "true");
    let body: Record<string, unknown> | undefined;
    globalThis.fetch = (_url, init) => {
      body = JSON.parse(String(init?.body));
      return Promise.resolve(Response.json({
        status: "completed",
        output: [{
          type: "web_search_call",
          status: "completed",
          results: [{
            image_url: candidate.imageURL,
            source_website_url: candidate.source.url,
            caption: candidate.caption,
            title: candidate.source.title,
          }],
        }],
        usage: { input_tokens: 4, output_tokens: 3, total_tokens: 7 },
      }));
    };
    const calls: UsageCall[] = [];
    const result = await searchImages(
      "simple dark silhouette",
      new AbortController().signal,
      (call) => calls.push(call),
    );
    equal(result.candidates.length, 1);
    equal(calls.length, 1);
    equal(calls[0].stage, "image_search");
    equal(calls[0].searchCalls, 1);
    equal(body?.tool_choice, "required");
    equal(body?.max_tool_calls, 1);
    equal(body?.include, ["web_search_call.results"]);
    assert(Array.isArray(body?.tools));
    equal((body!.tools as Record<string, unknown>[])[0].search_content_types, [
      "image",
      "text",
    ]);
    equal((body!.tools as Record<string, unknown>[])[0].image_settings, {
      max_results: 3,
      caption: true,
    });
  }));

Deno.test("web image downloader accepts public bounded PNG and refuses redirects", async () => {
  let seenRedirect: RequestRedirect | undefined;
  const downloaded = await fetchImageCandidate(
    candidate,
    new AbortController().signal,
    (_input, init) => {
      seenRedirect = init?.redirect;
      return Promise.resolve(
        new Response(png, { headers: { "content-type": "image/png" } }),
      );
    },
    () => Promise.resolve(),
  );
  equal(downloaded.mime, "image/png");
  equal([...downloaded.bytes], [...png]);
  equal(seenRedirect, "error");
});

Deno.test("web image downloader rejects local URLs, non-images, and oversized responses", async () => {
  let resolverCalls = 0;
  let localRejected = false;
  try {
    await fetchImageCandidate(
      { ...candidate, imageURL: "https://127.0.0.1/private.png" },
      new AbortController().signal,
      fetch,
      () => {
        resolverCalls++;
        return Promise.resolve();
      },
    );
  } catch {
    localRejected = true;
  }
  assert(localRejected);
  equal(resolverCalls, 0);

  let nonImageRejected = false;
  try {
    await fetchImageCandidate(
      candidate,
      new AbortController().signal,
      () =>
        Promise.resolve(
          new Response("not an image", {
            headers: { "content-type": "text/html" },
          }),
        ),
      () => Promise.resolve(),
    );
  } catch {
    nonImageRejected = true;
  }
  assert(nonImageRejected);

  let oversizedRejected = false;
  try {
    await fetchImageCandidate(
      candidate,
      new AbortController().signal,
      () =>
        Promise.resolve(
          new Response(png, {
            headers: {
              "content-type": "image/png",
              "content-length": String(9 * 1024 * 1024),
            },
          }),
        ),
      () => Promise.resolve(),
    );
  } catch {
    oversizedRejected = true;
  }
  assert(oversizedRejected);
});

Deno.test("public address filter rejects private, reserved, and documentation ranges", () => {
  for (
    const address of [
      "127.0.0.1",
      "10.1.2.3",
      "169.254.10.1",
      "192.0.2.4",
      "203.0.113.1",
      "224.0.0.1",
      "fd00::1",
      "2001:db8::1",
      "::1",
    ]
  ) {
    assert(!publicAddress(address), `${address} must not be treated as public`);
  }
  assert(publicAddress("8.8.8.8"));
  assert(publicAddress("2606:4700:4700::1111"));
});
