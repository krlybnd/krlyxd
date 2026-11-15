import { create, fromBinary, toBinary, type MessageInitShape } from '@bufbuild/protobuf';
import { test, expect, type Route } from '@playwright/test';
import { DownloadResponseSchema, HealthResponseSchema, RunRequestSchema, RunResponseSchema } from '../gen/krly/studio/v1/studio_pb.ts';

function frame(bytes: Uint8Array, flag = 0) {
  const header = Buffer.alloc(5); header[0] = flag; header.writeUInt32BE(bytes.length, 1);
  return Buffer.concat([header, bytes]);
}
async function respond(route: Route, messages: Uint8Array[]) {
  await route.fulfill({ contentType: 'application/grpc-web+proto', body: Buffer.concat([
    ...messages.map(message => frame(message)), frame(Buffer.from('grpc-status: 0\r\n'), 0x80),
  ]) });
}
const mesh = { positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1], indices: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3], volume: 1 / 6, area: 2.366, openEdges: 0, units: 'mm', source: 'field:krly3d.field.native/1:pikachu.field.json' };
const names = ['pikachu.stl', 'print-report.json', 'pikachu.field.json', 'mesh.json'];
type Event = MessageInitShape<typeof RunResponseSchema>;

for (const succeeds of [true, false]) test(`character ${succeeds ? 'completes with mm mesh and downloads' : 'rejects a stream without final mesh'}`, async ({ page }) => {
  const errors: string[] = [];
  let runs = 0;
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', async route => {
    const url = new URL(route.request().url());
    if (url.origin !== 'http://127.0.0.1:4328') return route.abort();
    if (!url.pathname.startsWith('/krly.studio.v1.StudioService/')) return route.continue();
    if (url.pathname.endsWith('/Health')) return respond(route, [toBinary(HealthResponseSchema, create(HealthResponseSchema, { nativeCharacter: true, native: false, chat: false }))]);
    if (url.pathname.endsWith('/Download')) return respond(route, [toBinary(DownloadResponseSchema, create(DownloadResponseSchema, { name: 'pikachu.stl', mime: 'model/stl', data: new Uint8Array(284) }))]);
    expect(url.pathname).toBe('/krly.studio.v1.StudioService/Run');
    const body = route.request().postDataBuffer()!;
    const request = fromBinary(RunRequestSchema, body.subarray(5, 5 + body.readUInt32BE(1)));
    expect(request).toMatchObject({ mode: 'character', text: '', attachmentIds: [] });
    runs++;
    const events: Event[] = [
      { kind: 'status', phase: 'sampling', text: 'A natív térfogati mező mintavételezése elindult.' },
      { kind: 'status', phase: 'field_mesh', text: 'Natív háromszögháló készül.' },
      { kind: 'status', phase: 'print_validation', text: 'Ellenőrzött: 0 nyitott él. Önmetszés · eredeti mesh: megfelelt; 2195272 pár. Önmetszés · float32 STL: megfelelt; 2195322 pár. Közelítő vizsgálat, nem egzakt bizonyítás. Falvastagság: ismeretlen. Nyomtathatóság nem igazolt.' },
      ...(succeeds ? [{ kind: 'mesh', phase: 'final_mesh', text: 'Stilizált Pikachu · térbeli mesh', mesh }] : []),
      ...names.map(name => ({ kind: 'artifact', phase: 'artifact', artifact: { id: name, name, mime: name.endsWith('.stl') ? 'model/stl' : 'application/json', size: 284 } })),
      { kind: 'usage', phase: 'usage', usage: { calls: [] } },
      { kind: 'completed', phase: 'completed' },
    ];
    await respond(route, events.map((event, seq) => toBinary(RunResponseSchema, create(RunResponseSchema, { ...event, seq, runId: 'character-ui' }))));
  });
  await page.goto('/');
  await expect(page.getByText('Szolgáltatás elérhető', { exact: true })).toBeVisible();
  // Use the welcome action on mobile too: character works with no AI/B-rep health.
  const tab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Beszélgetés' });
  if (await tab.isVisible()) await tab.click();
  await page.getByRole('button', { name: /Térbeli figura Stilizált Pikachu/ }).click();
  if (succeeds) {
    await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
    await expect(page.locator('.model-summary')).toContainText('mm³');
    await expect(page.locator('.model-summary')).toContainText('mm²');
    await expect(page.locator('.usage-details')).toContainText('0 AI-hívás');
    await expect(page.locator('.execution-events')).toContainText('Önmetszés · eredeti mesh: megfelelt; 2195272 pár');
    await expect(page.locator('.execution-events')).toContainText('Önmetszés · float32 STL: megfelelt; 2195322 pár');
    await expect(page.locator('.execution-events')).toContainText('Falvastagság: ismeretlen');
    await expect(page.locator('.execution-events')).not.toContainText('B-REP');
    for (const name of names) await expect(page.getByRole('button', { name: new RegExp(`^${name}`) })).toBeEnabled();
    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: /^pikachu.stl/ }).click();
    expect((await download).suggestedFilename()).toBe('pikachu.stl');
    const modelTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Modell' });
    if (await modelTab.isVisible()) await modelTab.click();
    await expect(page.locator('.viewport-status')).toContainText('mm');
    await expect(page.locator('.viewport-canvas canvas')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Térbeli figura újraépítése' })).toBeEnabled();
  } else {
    await expect(page.getByRole('button', { name: /A végrehajtás megszakadt/ })).toBeVisible();
    await expect(page.getByRole('button', { name: /^pikachu.stl/ })).toBeDisabled();
    await expect(page.locator('.model-summary')).toHaveCount(0);
  }
  expect(runs).toBe(1);
  expect(errors).toEqual([]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
