import { readFile, writeFile } from 'node:fs/promises';
import { deflateSync } from 'node:zlib';
import { fromBinary } from '@bufbuild/protobuf';
import { expect, test as base, type Download, type Page, type Response } from '@playwright/test';
import { RunResponseSchema, type Mesh, type RunResponse } from '../gen/krly/studio/v1/studio_pb.ts';

export const SERVICE = '/krly.studio.v1.StudioService/';

type BrowserError = { message: string; url: string; line?: number };

export const test = base.extend<{ expectedBadUpload: boolean }>({
  expectedBadUpload: [false, { option: true }],
  page: async ({ page, request, expectedBadUpload }, use, testInfo) => {
    // Check 4311 directly as well as the UI proxy: missing native engines are a
    // failing prerequisite, never a reason to silently skip these integrations.
    const health = await request.post(`${process.env.STUDIO_SERVICE_URL ?? 'http://127.0.0.1:4311'}${SERVICE}Health`, {
      headers: { 'content-type': 'application/json', 'connect-protocol-version': '1' },
      data: {},
    });
    expect(health.ok(), 'The existing service must be reachable').toBeTruthy();
    expect((await health.json()).native, 'Both real native binaries must be available').toBe(true);
    const pageErrors: string[] = [];
    const consoleErrors: BrowserError[] = [];
    const expectedErrors: BrowserError[] = [];
    let pageErrorCount = 0, consoleErrorCount = 0;
    page.on('pageerror', error => {
      pageErrorCount++;
      if (pageErrors.length < 100) pageErrors.push(error.stack ?? error.message);
    });
    page.on('console', message => {
      if (message.type() !== 'error') return;
      const location = message.location();
      const entry = { message: message.text(), url: location.url, line: location.lineNumber };
      // Only the explicit rejection test may ignore this exact failed Upload.
      // All THREE errors and unrelated HTTP failures always fail the test.
      const expected400 = expectedBadUpload &&
        /^Failed to load resource: the server responded with a status of 400 \(Bad Request\)$/.test(entry.message) &&
        entry.url.split('?')[0].endsWith(`${SERVICE}Upload`);
      if (expected400) {
        if (expectedErrors.length < 100) expectedErrors.push(entry);
      } else {
        consoleErrorCount++;
        if (consoleErrors.length < 100) consoleErrors.push(entry);
      }
    });
    try {
      await page.goto('/');
      await expect(page.getByText('Szolgáltatás elérhető', { exact: true })).toBeVisible();
      await use(page);
    } finally {
      const errorsPath = testInfo.outputPath('browser-errors.json');
      await writeFile(errorsPath, JSON.stringify({ pageErrorCount, consoleErrorCount, pageErrors, consoleErrors, expectedErrors }, null, 2));
      await testInfo.attach('browser-errors', { contentType: 'application/json', path: errorsPath });
      expect.soft(pageErrors, `Uncaught page errors (${pageErrorCount})`).toEqual([]);
      expect.soft(consoleErrors, `Unexpected console errors (${consoleErrorCount})`).toEqual([]);
    }
  },
});

/** Observe actual browser responses; no request interception or native mocks. */
export function rpcResponse(page: Page, method: string) {
  return page.waitForResponse(response => new URL(response.url()).pathname === `${SERVICE}${method}`);
}

export async function grpcWeb(response: Response) {
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toMatch(/^application\/grpc-web\+proto/);
  const bytes = await response.body();
  const messages: Uint8Array[] = [];
  const trailers: Record<string, string> = {};
  let offset = 0;
  let ended = false;
  while (offset < bytes.length) {
    expect(ended, 'No frames may follow gRPC trailers').toBe(false);
    expect(bytes.length - offset).toBeGreaterThanOrEqual(5);
    const flag = bytes[offset];
    const size = bytes.readUInt32BE(offset + 1);
    offset += 5;
    expect(size).toBeLessThanOrEqual(bytes.length - offset);
    const payload = bytes.subarray(offset, offset + size);
    offset += size;
    if (flag === 0x80) {
      ended = true;
      for (const line of payload.toString('utf8').trim().split('\r\n')) {
        const colon = line.indexOf(':');
        expect(colon).toBeGreaterThan(0);
        trailers[line.slice(0, colon).toLowerCase()] = line.slice(colon + 1).trim();
      }
    } else {
      expect(flag, 'Expected an uncompressed Protobuf data frame').toBe(0);
      messages.push(payload);
    }
  }
  // A trailers-only error may be represented in HTTP headers instead.
  const headerStatus = response.headers()['grpc-status'];
  expect(ended || headerStatus !== undefined, 'RPC must deliver a terminal gRPC status').toBe(true);
  return { messages, trailers: { ...(headerStatus === undefined ? {} : { 'grpc-status': headerStatus }), ...trailers } };
}

export async function completedRun(response: Response): Promise<RunResponse[]> {
  const wire = await grpcWeb(response);
  expect(wire.trailers['grpc-status']).toBe('0');
  const events = wire.messages.map(bytes => fromBinary(RunResponseSchema, bytes));
  expect(events.length).toBeGreaterThan(3);
  expect(events.map(event => event.seq)).toEqual(events.map((_, index) => index));
  expect(new Set(events.map(event => event.runId)).size).toBe(1);
  expect(events[0].runId).not.toBe('');
  expect(events[0].phase).toBe('accepted');
  expect(events.filter(event => event.kind === 'error')).toEqual([]);
  expect(events.filter(event => event.kind === 'completed')).toHaveLength(1);
  expect(events.at(-1)?.kind).toBe('completed');
  for (const event of events) expect(Number.isFinite(event.elapsedMs) && event.elapsedMs >= 0).toBe(true);
  return events;
}

export function nativeMesh(events: RunResponse[], phase = 'editable_proposal'): Mesh {
  const event = events.find(event => event.kind === 'mesh' && event.phase === phase);
  expect(event?.mesh, `Real mesh at ${phase}`).toBeDefined();
  const mesh = event!.mesh!;
  expect(mesh.source).toMatch(/^brep-session:/);
  expect(mesh.positions.length).toBeGreaterThan(0);
  expect(mesh.positions.length % 3).toBe(0);
  expect(mesh.positions.every(Number.isFinite)).toBe(true);
  expect(mesh.indices.length).toBeGreaterThan(0);
  expect(mesh.indices.length % 3).toBe(0);
  expect(mesh.indices.every(index => Number.isInteger(index) && index >= 0 && index < mesh.positions.length / 3)).toBe(true);
  expect(mesh.volume).toBeGreaterThan(0);
  expect(mesh.area).toBeGreaterThan(0);
  expect(mesh.openEdges).toBe(0);
  return mesh;
}

export async function expectRendered(page: Page, mesh: Mesh) {
  const viewport = page.getByRole('region', { name: 'Modell és 3D munkatér' });
  await expect(viewport.locator('canvas')).toBeVisible();
  await expect(viewport.getByText('1 test', { exact: true })).toBeVisible();
  const count = new Intl.NumberFormat('hu-HU', { maximumFractionDigits: 2 }).format(mesh.indices.length / 3);
  await expect(viewport.getByText(`${count} háromszög`, { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'A rajztól a térig.' })).toHaveCount(0);
  expect(await viewport.locator('canvas').evaluate(canvas => {
    const context = (canvas as HTMLCanvasElement).getContext('webgl2');
    return context !== null && !context.isContextLost();
  }), 'The native mesh must reach a live WebGL viewport').toBe(true);
}

export async function downloadArtifact(page: Page, name: string): Promise<Buffer> {
  const pending = page.waitForEvent('download');
  await page.getByRole('button', { name: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`) }).click();
  const download: Download = await pending;
  expect(download.suggestedFilename()).toBe(name);
  expect(await download.failure()).toBeNull();
  const stream = await download.createReadStream();
  expect(stream).not.toBeNull();
  const chunks: Buffer[] = [];
  for await (const chunk of stream!) chunks.push(Buffer.from(chunk));
  const bytes = Buffer.concat(chunks);
  expect(bytes.length).toBeGreaterThan(0);
  return bytes;
}

export async function expectedDemoDocument() {
  return await readFile(new URL('../../../../models/plate-with-hole.k3d', import.meta.url));
}

/** 32x24 grayscale ring, 364 material pixels, including a 13x8 through hole. */
export function drawingPng(): Buffer {
  const width = 32, height = 24;
  const rows = Buffer.alloc((width + 1) * height);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const material = x >= 3 && x < 29 && y >= 3 && y < 21 && !(x >= 9 && x < 22 && y >= 8 && y < 16);
    rows[y * (width + 1) + x + 1] = material ? 0 : 255;
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header[8] = 8;
  function chunk(name: string, payload: Buffer): Buffer {
    const typeAndData = Buffer.concat([Buffer.from(name), payload]);
    let crc = 0xffffffff;
    for (const byte of typeAndData) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
    }
    const size = Buffer.alloc(4); size.writeUInt32BE(payload.length);
    const checksum = Buffer.alloc(4); checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
    return Buffer.concat([size, typeAndData, checksum]);
  }
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', header), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0)),
  ]);
}

export async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow, 'Mobile content must fit the viewport width').toBeLessThanOrEqual(1);
}
