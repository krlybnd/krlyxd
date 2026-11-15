# Studio AI and microphone integration

The server uses built-in `fetch` against an OpenAI-compatible LiteLLM endpoint.
Copy [env.example](./env.example) to the studio's private `.env` and configure
`LITELLM_BASE_URL` and `LITELLM_API_KEY`. No dependency installation is required
by the chat adapter. The default chat alias is the reference's faster Haiku
alias, `eu.anthropic.claude-haiku-4-5-20251001-v1:0`. Keep all gateway keys and
upstream credentials in server/proxy processes; never expose them through
`VITE_*` variables or browser requests.

Use an existing gateway if available. The base URL may end in `/v1` and may
contain a gateway path prefix; the adapter appends `/chat/completions`,
`/audio/transcriptions`, `/responses`, or `/embeddings`. `/bedrock` is a
different passthrough protocol and is not a suitable base. Redirects are
rejected. Set an authorized virtual key; the service does not read the proxy
master key as a fallback.

## Optional standalone proxy

[litellm.yaml](./litellm.yaml) preserves the reference aliases and providers:
Bedrock Converse Sonnet/Haiku in `eu-central-1`, Titan embeddings, and xAI
`my-grok-fast` / `my-grok` over its OpenAI-compatible API. AWS credentials and
model access belong to the proxy's credential chain; xAI uses `XAI_API_KEY`
there. Aliases are preserved from the reference, not a claim that an account can
access them. Remove unused entries from a private profile if only one provider
is needed.

The separate [OpenAI profile](./litellm-openai.yaml) runs through the studio's
[Compose service](../compose.yaml), bound to `127.0.0.1:4010`. Its opaque
aliases can be selected through `LITELLM_CHAT_MODEL`, `LITELLM_VISION_MODEL`,
and `LITELLM_SPEECH_MODEL`; the adapter does not infer upstream model
capabilities from alias names.

The additional `speech-to-text` alias uses `openai/whisper-1` and the proxy's
`OPENAI_API_KEY`. This is an optional new STT route, alongside the reference's
Chromium and Azure Speech paths implemented by the server. Azure Speech
credentials do not configure an Azure OpenAI Whisper deployment. To use another
LiteLLM STT provider, configure that provider's `audio_transcription` alias at
the gateway and set `LITELLM_SPEECH_MODEL` to it. See
[LiteLLM's transcription configuration](https://docs.litellm.ai/docs/audio_transcription).
The base profile's Whisper route is for legacy compatibility. For new OpenAI
file-transcription routes, use `gpt-transcribe` as described in the
[OpenAI transcription guide](https://developers.openai.com/api/docs/guides/speech-to-text).

If LiteLLM is already installed, an operator can start a **separate** profile
from the studio directory after supplying its environment:

```sh
litellm --config config/litellm.yaml --host 127.0.0.1 --port 4010
```

The parent integration has started this standalone proxy and verified its
health; existing external proxies remain unchanged. Upstream OpenAI credentials
belong only to the proxy's private `.proxy.env`; the studio service uses a
gateway key from its private `.env`. Both files are ignored and have mode
`0600`. Never copy those values into checked-in configuration or browser
environment variables. The profile needs no database, disables retries, caps
requests, and keeps prompt bodies out of spend logs. Supported logging callbacks
also have message logging disabled; see
[LiteLLM logging](https://docs.litellm.ai/docs/proxy/logging). Do not enable
detailed debug or payload-logging callbacks for user images/audio.

## Server contract

[speech.ts](../service/ai/speech.ts) exports
`transcribe(bytes: Uint8Array, mime: string, signal: AbortSignal): Promise<string>`
for the parent's RPC. Configure `STUDIO_SPEECH_PROVIDER=chromium|azure|litellm`
to pin a route. The default `auto` chooses Azure if `AZURE_SPEECH_KEY` is set,
then Chromium if `CHROMIUM_SPEECH_KEY` is set, otherwise LiteLLM. It does not
retry on a different provider after a failure. `STUDIO_SPEECH_LANGUAGE` defaults
to `hu-HU`.

For LiteLLM, `LITELLM_SPEECH_LANGUAGE_FIELD` selects the multipart hint form:
`language` (default) sends `language=hu` for legacy aliases; `languages` sends
only `languages[]=hu` for `gpt-transcribe` aliases; `none` omits the hint if the
gateway injects it. The locale's base language is used (`en-GB` becomes `en`).
Configure `languages` for the parent's `studio-speech` alias. The adapter never
sends both forms or retries a failed upload with another schema. This follows
the
[OpenAI multipart examples](https://developers.openai.com/api/docs/guides/speech-to-text#add-transcription-context).

- `chromium` preserves the working reference laptop flow: bounded ffmpeg
  conversion to 16 kHz mono PCM, then FLAC, POST to
  `https://www.google.com/speech-api/v2/recognize?client=chromium&lang=hu-HU&key=…`,
  and parse its newline-delimited JSON results. Supply `CHROMIUM_SPEECH_KEY`
  privately; no client key was copied from the reference. This is an explicit
  compatibility route, not a promise of a supported Google Cloud API. The key is
  necessarily in its upstream query string; errors never include that URL.
- `azure` uses `AZURE_SPEECH_KEY` and `AZURE_SPEECH_REGION` (default
  `swedencentral`, matching the reference). The reference SDK exchanges a key
  for a ten-minute browser token. Here, the same Speech resource is used through
  server REST: the subscription key stays on the server, compressed recordings
  are converted to 16 kHz mono PCM/WAV, and `DisplayText` is read only on a
  successful recognition status. No new browser SDK or token RPC is needed. See
  [Azure short-audio REST](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/rest-speech-to-text-short).
- `litellm` calls the optional multipart route described above. Set
  `LITELLM_SPEECH_MODEL=speech-to-text` after supplying its proxy credentials.

Azure and Chromium require installed `ffmpeg` and Deno `--allow-run=ffmpeg` (the
parent's service already grants run permission). Conversion uses piped bytes, an
empty child environment apart from PATH, fixed arguments, no shell, a pipe-only
protocol allowlist, a media demuxer allowlist, and discarded stderr. Inputs are
capped at 8 MiB, decoded PCM at 60 seconds, responses at 128 KiB. No temporary
media files are created. `STUDIO_SPEECH_TIMEOUT_MS` defaults to 30 seconds and
includes conversion, process cleanup, and HTTP response consumption. Abort or
timeout kills and reaps conversion processes. Silence, bad audio, unavailable
ffmpeg, and provider errors are explicit typed failures rather than empty
success.

[provider.ts](../service/ai/provider.ts) exports:

- `getProviderStatus(): ProviderStatus`: `chat`, `vision`, `speech`,
  `embedding`, `model`, the optional capability model names, `speechProvider`,
  `health: "unverified"`, and configuration `reason` / `speechReason` when
  unavailable. Chat requires an explicit endpoint and key; speech reflects the
  selected route independently. This is local configuration status, **not** a
  readiness probe. It never makes a billable call or exposes keys/URLs.
- `plan({text, history?, image?, settings?, signal}): Promise<PlanResult>`.
  History is `{role: "user" | "assistant", content: string}[]`; an image is
  `{bytes: Uint8Array, mime: string}`. Optional `settings` is
  `{height?: number, pixelUnit?: number, seed?: [number, number]}`. Pass UI
  settings here to enforce their values in addition to supplying prompt context.
  The result is
  `{reply, action: "reply" | "reconstruct", height?, pixelUnit?,
  seed?, summary}`.
  `summary` is a short user-visible action description. Both reply and summary
  default to Hungarian unless the user explicitly requests another language.
- Its low-level `transcribe` export is the LiteLLM-only adapter. RPC callers
  should import `transcribe` from [speech.ts](../service/ai/speech.ts), which
  selects the configured route. All routes return trimmed text and share the
  same signature.
- `embed(text: string, signal: AbortSignal): Promise<number[]>` is optional and
  called only explicitly; neither planning nor STT invokes it.
- `ProviderError` has a safe `code`, `message`, and optional `httpStatus`.
  Codes: `not_configured`, `invalid_configuration`, `invalid_input`, `aborted`,
  `timeout`, `network`, `http_error`, `response_too_large`, `invalid_response`,
  `no_speech`, `decoder_unavailable`, `invalid_audio`. Errors do not carry
  upstream response bodies, causes, keys, or media. Map these to protocol errors
  at the server boundary; never report fake success.

The planner accepts only a single named tool call with validated fields. It
rejects unknown fields/actions, parallel calls, refusals, truncated responses,
invalid numbers, and supplied-setting changes. It never evaluates code or
returns the model's content/reasoning fields. Only raster outline extraction
from a top-view drawing with a filled dark silhouette on a light background,
including enclosed light holes, plus uniform extrusion is supported. General
color photos, arbitrary photo-to-3D, and inferred hidden surfaces are outside
the supported action. Both positive `height` and `pixelUnit` must be explicit;
the prompt forbids deriving scale or dimensions from images. Height and pixel
scale are bounded to `1e6`, matching the native scalar limit; seed coordinates
must be nonnegative and at most `1e9`. Without structured settings, numeric
values must also appear in user/host text, never only in image pixels or
assistant history. This is a grounding guard, not semantic proof of user intent.
The caller still owns workspace image existence, user intent, and native
geometry validation.

When structured settings are supplied, the tool schema limits height and pixel
scale to their supplied values or null. Without an explicitly supplied seed, its
schema is exactly `type: null`, and the decoder requires an explicit null. Image
or text coordinates cannot invent a seed in that host path; native component
selection remains automatic. A supplied seed must survive the decoder unchanged.

The named tool remains forced, strict, and nonparallel. Its schema uses the
portable Bedrock subset (types, required fields, enum, nullability, closed
object). String limits, number bounds, and exact seed length are checked
locally:
[Bedrock strict schemas](https://docs.aws.amazon.com/bedrock/latest/userguide/structured-output.html)
do not support the corresponding constraint keywords. The reference's EU Haiku
4.5 profile supports tool calls and structured output; see its
[Bedrock model card](https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-anthropic-claude-haiku-4-5.html).
Extended thinking is not requested with forced tool use. A cold Bedrock strict
schema can take longer than the fast default timeout; an operator using Bedrock
can raise both gateway and `LITELLM_TIMEOUT_MS` limits to 120,000 ms for warmup.

Limits: 16,000 prompt characters; 24 history messages / 48,000 total history
characters; 8 MiB image/audio inputs; 128 KiB streamed response; 1,600
completion tokens. Timeouts include response consumption, accept 100–120,000 ms,
and default to 45 seconds for chat/embedding and 30 seconds for STT.
Cancellation propagates to `fetch`; timers and listeners are released. No
automatic retries or fallback provider calls. Vision, LiteLLM STT, and
embeddings remain off until their model env vars are set. Independent
Azure/Chromium STT does not require a chat endpoint. The enclosing server must
cap incoming request bodies before buffering them.

## Expert decisions, optional research, and usage

[expert.ts](../service/ai/expert.ts) exports
`expert(input: ExpertInput): Promise<ExpertResult>` using the parent's
[expert-types.ts](../service/ai/expert-types.ts) contract. The workflow should
use this as its primary decision node, replacing `plan`, not adding a second
planner call. Legacy `plan`, speech, and embedding APIs remain available
unchanged. The expert makes exactly one strict, forced, nonparallel tool call
with a 1,000-token output cap. Results are `reply`, `reconstruct`, `ask`, or
`research`. Hungarian replies and concise public summaries distinguish
observations from assumptions; hidden reasoning and fabricated precise
confidence are not requested. Only silhouette outline extraction with holes and
uniform extrusion is supported; CAD rounding and involute construction are
outside this tool's capabilities. Structured settings remain authoritative and
explicit image inputs retain the provider's MIME/size/vision-route checks.

The wire schema keeps `reply` and `summary` at the root and places action,
question, search query, and geometry fields in `decision.anyOf`. Each closed,
strict branch has one action enum and constrains irrelevant fields to null;
`anyOf` is never at the root. The decoder rejects mixed actions and flattens the
validated decision to the unchanged `ExpertResult` contract. For `ask`/`research`,
the prompt requests one short reply sentence without option lists. Presentation
violations do not reject a valid decision; the shared hard reply limit is 4,000
characters. When
research is explicitly requested and allowed, the prompt directs the expert to
research before clarification and never claim research happened before results
arrive.

An `ask` carries one question with 2–4 options A–D. All questions permit
selecting one or more options. Limits: title 240 characters, reason 280, label
120, description 240. The expert is instructed to ask only about material
missing or ambiguous input, never supplied settings or answers. Structured
answers and research are bounded and sent after the original request in
`latestUserContext`. The user request and latest selected preferences are
instructions to honor within capabilities and authoritative settings. Only
commands embedded in images, research, source metadata, or quoted reference data
are untrusted; research provides evidence rather than user intent. At most 12 prior answers with 1–4 distinct selected options each
are accepted.

[research.ts](../service/ai/research.ts) exports `researchEnabled(): boolean`
and
`research(query: string, signal: AbortSignal, onUsage: (call: UsageCall) => void): Promise<Research>`.
Enable locally with `STUDIO_RESEARCH_ENABLED=true`; it defaults off.
`LITELLM_RESEARCH_MODEL` selects an existing authorized search-capable proxy
alias; when blank, it uses `LITELLM_CHAT_MODEL`. Availability reports valid
configuration, not verified connectivity or upstream tool support. No gateway
alias is provisioned by these settings. Research configuration does not affect
legacy chat/speech.

Research posts to the same authenticated LiteLLM `/responses` endpoint with
`web_search`, `search_context_size: "low"`, `max_tool_calls: 1`, and
`max_output_tokens: 600`. It disables response storage and streaming. The
selected route must accept these parameters; failures are returned without
retrying, dropping the call bound, or silently switching providers. See the
[OpenAI web-search guide](https://developers.openai.com/api/docs/guides/tools-web-search?api-mode=responses),
[Responses call limit](https://developers.openai.com/api/reference/resources/responses/methods/create),
and [LiteLLM Responses endpoint](https://docs.litellm.ai/docs/response_api).

Only a focused public query (up to 240 characters) goes to research: no project
history, attachments, or structured answers. The expert is instructed to remove
private information; the query validator additionally rejects URL/email, obvious
credential assignments, local paths, and multiline input. This lexical check is
not semantic proof that arbitrary text contains no confidential information.
Research is reserved for material uncertain external facts or explicit user
research requests. The parent must enforce one search across the entire HITL
operation and pass `researchAllowed: false` afterwards, including after a failed
search. The expert also rejects another search when research is already
supplied. Neither export runs a research loop internally.

The research decoder requires a completed search and reads actual `url_citation`
annotations from Responses output, never model-written source lists or URLs in
plain prose. It returns at most six unique HTTP(S) sources and a summary capped
at 900 characters and 180 words. The prompt requests three concise relevant
facts (fewer if evidence is insufficient), no headings or tangents, and
source-supported claims with their conditions and uncertainty. Evidence for one
design must not be generalized into unsupported judgments about alternatives.
Overlong summaries are shortened at sentence boundaries; if no complete sentence
fits, validation fails without another call. Missing usable citations, refusals,
incomplete output, and multiple search calls fail validation. Source URLs are
never fetched by this server. The caller should show returned sources as
clickable citations with the summary.

[usage.ts](../service/ai/usage.ts) records one `UsageCall` through the supplied
callback for each attempted expert/research request, including failures. The
shared HTTP helper saves response headers before body parsing and records body
usage before validating the model action. Thus invalid JSON can still retain a
reported cost; invalid tool arguments still retain valid token counts. A call
cancelled before dispatch emits no usage. The parent can collect synchronously
and flush after each awaited call, including in its failure path. Callback
exceptions cannot mask provider/cancellation errors or cause a retry.

Both chat and Responses token fields, including cached input tokens, are read.
Public call IDs are generated UUIDs, never upstream request/response IDs. Only
the configured alias is exposed, not credentials, deployment IDs, or endpoints.
`x-litellm-response-cost` is trusted **proxy-reported** USD, not an
independently verified invoice. See
[LiteLLM cost headers](https://docs.litellm.ai/docs/proxy/response_headers).
Absent or invalid cost is `costKind: "unknown"`; absent/malformed token totals
have `tokensKnown: false`. Numeric zero placeholders in the contract must not be
displayed as known zero usage or free service. A valid explicit cost header of
zero is reported as such.

Optional `LITELLM_USAGE_RATES_JSON` maps exact aliases to operator-verified
rates with fields `verified: true`, `inputUsdPerMillion`, `outputUsdPerMillion`,
and `cachedInputUsdPerMillion` (required when cached tokens are present). Rates
must be finite and nonnegative. Only valid, complete token counts and verified
tariffs produce `costKind: "estimated"`; cached input is subtracted from
full-price input. No default prices or model-name guesses are used. Operators
must verify the alias's actual routed model and applicable rates before enabling
estimates; invalidate the tariff when routing/pricing changes.
Malformed/missing/unverified tariffs leave cost unknown. Research costs are
never estimated from token-only rates:
[OpenAI pricing](https://developers.openai.com/api/docs/pricing) lists separate
search-tool/content charges, so a missing proxy total remains unknown.

## Browser contract

[voice.ts](../web/src/voice.ts) exports
`startRecording(onError?: (message: string) => void, options?: RecordingOptions):
Promise<{stop: () => Promise<Blob>; cancel: () => void}>`.

Call it directly inside the Record button click, before any awaited network
work. Importing it or mounting a component never asks for microphone access. It
requires HTTPS/localhost and MediaRecorder; it negotiates WebM/Opus, MP4, or
Ogg/Opus. No browser SpeechRecognition service or direct provider credentials
are used.

Use the parent's `transcribe(blob, signal): Promise<string>` with the returned
Blob, preserving `blob.type`. `stop()` is idempotent. By default recording stops
after 60 seconds, releases tracks, and calls `onError` with a **limit notice**:
the UI should show the notice and retain its recording handle. Notices and
recorder errors are Hungarian. `stop()` then retrieves the completed clip. No
upload occurs automatically. Actual errors discard the clip and reject `stop()`.
Callback exceptions cannot interrupt cleanup.

`options` supports `signal`, `maxDurationMs`, and `maxBytes`; limits can only be
lowered from 60 seconds / 8 MiB. Pass an AbortSignal when starting so component
cleanup can cancel **before** the permission promise resolves. Pending
permission times out at 30 seconds; late grants are stopped immediately. A new
recording also cancels the previous pending/active one. Device loss, recorder
errors, empty/large clips, and finalization timeout are explicit errors. Stop
releases hardware immediately even if final data arrives later. Finalization is
bounded to 3 seconds.

Always call `cancel()` on unmount/session change, and abort any pending
transcription separately. Cancellation rejects `AbortError` and invalidates
future `stop()` calls. Gate async UI state updates using that signal or a
session generation so a previous clip/transcript cannot populate a new
conversation.

## Verification

From the studio directory:

```sh
deno test --allow-read=web/src/voice.ts --allow-env --allow-run=ffmpeg service/ai/
deno lint service/ai web/src/voice.ts
```

The tests mock external HTTP calls and use real local ffmpeg conversion for WAV,
WebM, MP4, and Ogg; no credentials or live provider access are needed. Browser
API lifecycle tests use deterministic device/recorder stubs. The web build
checks DOM types. The parent integration reports successful live OpenAI chat
planning (4,106 ms), Hungarian synthetic-clip transcription with
`gpt-transcribe` (988 ms), and a 1,536-dimension embedding. Hardware microphone
capture remains unverified. Preserve `Blob.type` in the parent client's
`transcribe(blob, signal)` call.
