import { create, fromBinary, toBinary, type MessageInitShape } from '@bufbuild/protobuf';
import { test, expect, type Page, type Route } from '@playwright/test';
import { HealthResponseSchema, RunRequestSchema, RunResponseSchema, type RunRequest, type RunResponse } from '../gen/krly/studio/v1/studio_pb.ts';

function frame(bytes: Uint8Array, flag = 0) {
  const header = Buffer.alloc(5); header[0] = flag; header.writeUInt32BE(bytes.length, 1);
  return Buffer.concat([header, bytes]);
}
async function respond(route: Route, messages: Uint8Array[]) {
  await route.fulfill({ contentType: 'application/grpc-web+proto', body: Buffer.concat([
    ...messages.map(message => frame(message)), frame(Buffer.from('grpc-status: 0\r\n'), 0x80),
  ]) });
}
const call = (id: string, extra = {}) => ({ id, stage: 'expert', model: 'mock-expert', inputTokens: 100, outputTokens: 20, cachedTokens: 30, totalTokens: 120, costUsd: 0.00000123, costKind: 'reported', tokensKnown: true, searchCalls: 0, ...extra });
const question = (id: string) => ({ id, title: `Pontosítás ${id}`, reason: 'A tervhez szükséges pontosítás.', options: [
  { id: 'a', label: 'Kör alakú', description: 'Lekerekített alaprajz' },
  { id: 'b', label: 'Szögletes', description: 'Egyenes oldalak' },
  { id: 'c', label: 'Üreges', description: 'Belső kivágás' },
  { id: 'd', label: 'Tömör', description: 'Kitöltött belső' },
] });
type Event = Omit<MessageInitShape<typeof RunResponseSchema>, '$typeName'>;
function event(kind: string, fields: Event = {}) {
  return create(RunResponseSchema, { runId: 'mock-run', kind, phase: kind, ...fields });
}
function ask(id: string, calls = [call('one')]) {
  return create(RunResponseSchema, { runId: 'mock-run', kind: 'question', phase: 'expert', question: question(id), usage: { calls } });
}
function usageEvent(kind: string, calls: ReturnType<typeof call>[], fields: Event = {}) {
  return create(RunResponseSchema, { runId: 'mock-run', kind, phase: kind, ...fields, usage: { calls } });
}
async function open(page: Page, handler: (request: RunRequest, index: number, route: Route) => Promise<RunResponse[] | null> | RunResponse[]) {
  const requests: RunRequest[] = [];
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  // Block all nonlocal traffic as well as every unhandled RPC.
  await page.route('**/*', async route => {
    const url = new URL(route.request().url());
    if (url.origin !== 'http://127.0.0.1:4328') return route.abort();
    if (!url.pathname.startsWith('/krly.studio.v1.StudioService/')) return route.continue();
    if (url.pathname.endsWith('/Health')) return respond(route, [toBinary(HealthResponseSchema, create(HealthResponseSchema, { chat: true, native: true, model: 'mock', speech: false }))]);
    expect(url.pathname).toBe('/krly.studio.v1.StudioService/Run');
    const body = route.request().postDataBuffer()!;
    const request = fromBinary(RunRequestSchema, body.subarray(5, 5 + body.readUInt32BE(1)));
    requests.push(request);
    const events = await handler(request, requests.length, route);
    if (events) await respond(route, events.map((value, seq) => toBinary(RunResponseSchema, { ...value, seq })));
  });
  await page.goto('/');
  await expect(page.getByText('Szolgáltatás elérhető', { exact: true })).toBeVisible();
  const chatTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Beszélgetés' });
  if (await chatTab.isVisible()) await chatTab.click();
  return { requests, errors };
}
async function send(page: Page, text = 'Tervezz egy modellt') {
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill(text);
  await page.getByRole('button', { name: 'Üzenet küldése', exact: true }).click();
}
const form = (page: Page, id: string) => page.getByRole('form', { name: `Pontosítás ${id}` });

test('checkbox answers suspend, sequential questions freeze original parameters and preserve composer drafts', async ({ page }) => {
  const { requests, errors } = await open(page, (_request, index) => index === 1 ? [
    event('status', { phase: 'geometry' }), ask('q1'),
  ] : index === 2 ? [ask('q2', [call('one'), call('two', { stage: 'research', searchCalls: 2 })])] : [event('message', { text: 'Kész a terv.' }), usageEvent('completed', [call('one'), call('two')])]);
  await send(page);
  await expect(page.getByRole('button', { name: /Válaszra vár/ })).toBeVisible();
  await expect(page.getByLabel('Üzenet a Studiónak', { exact: true })).toBeEnabled();
  await expect(page.getByLabel('Üzenet a Studiónak', { exact: true })).toHaveValue('');
  await expect(form(page, 'q1').getByRole('checkbox')).toHaveCount(4);
  await expect(page.getByRole('radio')).toHaveCount(0);
  await expect(form(page, 'q1').getByRole('button')).toBeDisabled();
  await form(page, 'q1').getByRole('checkbox', { name: /Kör alakú/ }).check();
  await form(page, 'q1').getByRole('checkbox', { name: /Üreges/ }).check();
  expect(requests).toHaveLength(1);
  await page.getByRole('button', { name: 'Építési beállítások', exact: true }).click();
  await page.getByLabel(/^Magasság/).fill('0');
  await page.getByLabel(/^Képpontlépték/).fill('99');
  await page.getByRole('button', { name: 'Építési beállítások', exact: true }).click();
  await page.getByRole('button', { name: 'Építés', exact: true }).click();
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill('Későbbi saját üzenet');
  await form(page, 'q1').getByRole('button').click();
  await expect(form(page, 'q2')).toBeVisible();
  await expect(form(page, 'q1').getByRole('checkbox').first()).toBeDisabled();
  expect(requests[1]).toMatchObject({ questionId: 'q1', answerIds: ['a', 'c'], mode: 'chat', text: '', attachmentIds: [], height: 3, pixelUnit: 1, tolerance: 0.05 });
  await expect(page.locator('.user-message').last()).toContainText('Válasz: Kör alakú; Üreges');
  await form(page, 'q2').getByRole('checkbox', { name: /Tömör/ }).check();
  await form(page, 'q2').getByRole('button').click();
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
  expect(requests[2]).toMatchObject({ questionId: 'q2', answerIds: ['d'], mode: 'chat' });
  await expect(form(page, 'q2').getByRole('checkbox').last()).toBeDisabled();
  await expect(page.getByLabel('Üzenet a Studiónak', { exact: true })).toHaveValue('Későbbi saját üzenet');
  await expect(page.locator('.usage-details').last()).toContainText('2 AI-hívás');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  expect(errors).toEqual([]);
});

test('resume error, transport closure and abort retain the same question; duplicate submits are ignored', async ({ page }) => {
  let release: (() => void) | undefined;
  const { requests, errors } = await open(page, async (_request, index, route) => {
    if (index === 1) return [ask('retry')];
    if (index === 2) return [usageEvent('error', [call('one'), call('failed', { costKind: 'unknown', tokensKnown: false })], { text: 'Átmeneti hiba' })];
    if (index === 3) return [event('status')]; // Missing terminal is retryable too.
    if (index === 4) {
      await new Promise<void>(resolve => { release = resolve; });
      await route.abort();
      return null;
    }
    return [usageEvent('completed', [call('one'), call('failed', { costKind: 'unknown', tokensKnown: false }), call('success')])];
  });
  await send(page);
  const retry = form(page, 'retry');
  await retry.getByRole('checkbox', { name: /Tömör/ }).check();
  await retry.getByRole('button').click();
  await expect(page.getByText('Átmeneti hiba', { exact: true })).toBeVisible();
  await expect(retry.getByRole('button')).toBeEnabled();
  await expect(page.locator('.usage-details').last()).toContainText('2 AI-hívás');
  await retry.getByRole('button').click();
  await expect(page.getByText('Az adatfolyam befejező esemény nélkül lezárult.', { exact: true })).toBeVisible();
  await retry.evaluate(node => { (node as HTMLFormElement).requestSubmit(); (node as HTMLFormElement).requestSubmit(); });
  await expect.poll(() => requests.length).toBe(4);
  await expect(retry.getByRole('checkbox').first()).toBeDisabled();
  await page.getByRole('button', { name: 'Végrehajtás leállítása', exact: true }).click();
  await expect(retry.getByRole('button')).toBeEnabled();
  release!();
  await retry.getByRole('button').click();
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
  expect(requests).toHaveLength(5);
  expect(requests.slice(1).every(request => request.questionId === 'retry' && request.answerIds.join() === 'd')).toBe(true);
  await expect(page.locator('.usage-details').last()).toContainText('3 AI-hívás');
  expect(errors).toEqual([]);
});

test('cumulative snapshots dedupe and update calls; costs, unknown tokens and safe sources stay explicit', async ({ page }) => {
  const initial = call('one', { costUsd: 0.00000001 });
  const estimated = call('two', { stage: 'research', costKind: 'estimated', costUsd: 0.00000002, searchCalls: 3 });
  const unknown = call('three', { costKind: 'unknown', costUsd: 0, tokensKnown: false, totalTokens: 999 });
  await open(page, (_request, index) => index === 1 ? [
    usageEvent('status', [call('one', { costKind: 'unknown', tokensKnown: false })], { phase: 'expert' }),
    usageEvent('status', [initial, estimated], { phase: 'research' }),
    ask('cost', [initial, estimated, initial]),
  ] : [usageEvent('error', [initial, estimated, unknown], { text: 'Próbáld újra', sources: [
    { title: 'Biztonságos forrás', url: 'https://example.com/reference' },
    { title: 'HTTP forrás', url: 'http://example.com/reference' },
    { title: 'Tiltott forrás', url: 'javascript:alert(1)' },
    { title: 'Hibás URL', url: 'broken' },
  ] })]);
  await send(page);
  const waitingUsage = page.locator('.usage-details').first();
  await waitingUsage.locator('summary').click();
  await expect(waitingUsage.locator('.usage-calls li')).toHaveCount(2);
  await expect(waitingUsage.locator('.usage-total')).toContainText('0,00000003 USD · jelentett és becsült');
  await expect(waitingUsage).toContainText('240 token összesen');
  await expect(waitingUsage).toContainText('gyorsítótár: 30');
  await expect(waitingUsage).toContainText('3 keresés');
  await expect(page.getByText('SZAKÉRTŐ', { exact: true }).first()).toBeVisible();
  await expect(page.getByText('KUTATÁS', { exact: true })).toBeVisible();
  await form(page, 'cost').getByRole('checkbox').first().check();
  await form(page, 'cost').getByRole('button').click();
  const usage = page.locator('.usage-details').last();
  await usage.locator('summary').click();
  await expect(usage.locator('.usage-calls li')).toHaveCount(3);
  await expect(usage.locator('.usage-total')).toContainText('Részleges költség: 0,00000003 USD');
  await expect(usage.locator('.usage-total')).toContainText('1 hívás költsége ismeretlen');
  await expect(usage).toContainText('240 token (részleges');
  await expect(usage.locator('.usage-calls li').last()).toContainText('Tokenszám ismeretlen');
  await expect(usage.locator('.usage-calls li').last()).not.toContainText('0,00 USD');
  const sources = page.locator('.response-sources').last();
  await expect(sources.getByRole('link')).toHaveCount(2);
  await expect(sources.getByRole('link', { name: 'Biztonságos forrás' })).toHaveAttribute('rel', 'noopener noreferrer');
  await expect(sources.getByRole('link', { name: 'HTTP forrás' })).toHaveAttribute('href', 'http://example.com/reference');
  await expect(sources.getByText('Tiltott forrás')).toBeVisible();
});

test('an ordinary request supersedes pending forms, and absent usage is not free', async ({ page }) => {
  const { requests } = await open(page, (_request, index) => index === 1 ? [ask('old', [call('unknown', { costKind: 'unrecognized', tokensKnown: false })])] : [event('error', { text: 'Új kérés hibája' })]);
  await send(page);
  await page.locator('.usage-details summary').click();
  await expect(page.locator('.usage-total')).toContainText('Ismeretlen költség');
  await expect(page.locator('.usage-details')).not.toContainText('0,00 USD');
  await form(page, 'old').getByRole('checkbox').first().check();
  await send(page, 'Másik feladat');
  await expect(page.getByText('Új kérés hibája', { exact: true })).toBeVisible();
  await expect(form(page, 'old').getByRole('button')).toBeDisabled();
  await expect(form(page, 'old').getByRole('checkbox').first()).toBeDisabled();
  expect(requests[1].questionId).toBe('');
  expect(requests[1].answerIds).toEqual([]);
  const usage = page.locator('.usage-details').last();
  await usage.locator('summary').click();
  await expect(usage).toContainText('A használat és a költség még nem ismert.');
  await expect(usage).not.toContainText('0 AI-hívás');
});

test('native-only usage is scoped to the operation and geometry still requires a final mesh', async ({ page }) => {
  const { requests } = await open(page, () => [usageEvent('usage', []), event('completed')]);
  const modelTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Modell', exact: true });
  if (await modelTab.isVisible()) await modelTab.click();
  await page.getByRole('button', { name: 'Demó betöltése', exact: true }).click();
  const chatTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Beszélgetés' });
  if (await chatTab.isVisible()) await chatTab.click();
  await expect(page.getByText('A szolgáltatás nem küldött végleges geometriát. A korai háló nem végleges modell.', { exact: true })).toBeVisible();
  const usage = page.locator('.usage-details');
  await usage.locator('summary').click();
  await expect(usage).toContainText('Ebben a műveletben 0 AI-hívás. Ez nem a teljes alkalmazás költsége.');
  expect(requests[0].mode).toBe('demo');
});

test('HITL geometry errors roll back the model and a successful resume keeps local replay', async ({ page }) => {
  const mesh = (volume: number) => ({ positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1], indices: [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3], volume, area: 4, openEdges: 0, source: `mock-${volume}` });
  const { requests, errors } = await open(page, (_request, index) => {
    if (index === 1) return [event('mesh', { phase: 'editable_proposal', mesh: mesh(1) }), usageEvent('completed', [])];
    if (index === 2) return [ask('geometry')];
    if (index === 3) return [event('mesh', { phase: 'coarse_preview', mesh: mesh(2) }), usageEvent('error', [call('one')], { text: 'Geometriai próbahiba' })];
    return [event('mesh', { phase: 'construction_step', text: 'Első lépés', mesh: mesh(2) }), event('mesh', { phase: 'editable_proposal', text: 'Végleges lépés', mesh: mesh(3) }), usageEvent('completed', [call('one'), call('two')])];
  });
  await send(page, 'Első modell');
  await expect(page.locator('.model-summary')).toContainText('1 egység³');
  await send(page, 'Pontosítsuk a modellt');
  await expect(form(page, 'geometry')).toBeVisible();
  await expect(page.locator('.model-summary')).toContainText('1 egység³');
  await form(page, 'geometry').getByRole('checkbox').first().check();
  await form(page, 'geometry').getByRole('button').click();
  await expect(page.getByText('Geometriai próbahiba', { exact: true })).toBeVisible();
  await expect(page.locator('.model-summary')).toContainText('1 egység³');
  await expect(page.locator('.model-summary')).not.toContainText('Előzetes geometria');
  await expect(form(page, 'geometry').getByRole('button')).toBeEnabled();
  await form(page, 'geometry').getByRole('button').click();
  await expect(page.locator('.model-summary')).toContainText('3 egység³');
  const modelTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Modell', exact: true });
  if (await modelTab.isVisible()) await modelTab.click();
  const progress = page.getByRole('progressbar', { name: 'Kirajzolt háromszögek', exact: true });
  await expect(progress).toHaveAttribute('aria-valuenow', '4');
  await page.getByRole('button', { name: 'Kirajzolás újra', exact: true }).click();
  await expect(progress).toHaveAttribute('aria-valuenow', '4');
  await expect(page.locator('.draw-stage')).toContainText('2/2');
  expect(requests).toHaveLength(4);
  expect(errors).toEqual([]);
});
