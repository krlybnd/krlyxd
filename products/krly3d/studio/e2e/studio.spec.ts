import { create, fromBinary, toBinary } from '@bufbuild/protobuf';
import { readFile } from 'node:fs/promises';
import { expect } from '@playwright/test';
import { RunRequestSchema, RunResponseSchema, type RunRequest } from '../gen/krly/studio/v1/studio_pb.ts';
import { domIds } from '../ssot/domIds.ts';
import { dom } from '../web/tests/dom.ts';
import {
  completedRun, downloadArtifact, drawingPng, expectedDemoDocument, expectRendered,
  grpcWeb, nativeMesh, rpcResponse, SERVICE, test,
} from './helpers.ts';

function grpcFrame(bytes: Uint8Array, flag = 0) {
  const header = Buffer.alloc(5); header[0] = flag; header.writeUInt32BE(bytes.length, 1);
  return Buffer.concat([header, bytes]);
}

test('demo renders the native plate mesh and downloads the unchanged k3d model', async ({ page }) => {
  const response = rpcResponse(page, 'Run');
  await page.getByRole('button', { name: 'Demó betöltése', exact: true }).click();
  const events = await completedRun(await response);
  const mesh = nativeMesh(events);
  expect(mesh.source).toContain('part@3');
  expect(mesh.volume).toBeGreaterThan(23_000);
  expect(mesh.volume).toBeLessThan(24_000);
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
  await expect(page.locator('.event-completed')).toHaveCount(1);
  await expectRendered(page, mesh);
  await expect(page.getByText('Zárt háló', { exact: true })).toBeVisible();
  expect(await downloadArtifact(page, 'plate-with-hole.k3d')).toEqual(await expectedDemoDocument());
  await page.getByRole('button', { name: 'Drótváz', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Drótváz', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'Felületek', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Felületek', exact: true })).toHaveAttribute('aria-pressed', 'true');
});

test.describe('real mesh playback', () => {
  test.use({ reducedMotion: 'no-preference' });

  test('native triangles appear progressively and replay without another Run', async ({ page }) => {
    let requests = 0;
    page.on('request', request => { if (new URL(request.url()).pathname === `${SERVICE}Run`) requests++; });
    const response = rpcResponse(page, 'Run');
    await page.getByRole('button', { name: 'Demó betöltése', exact: true }).click();
    const mesh = nativeMesh(await completedRun(await response));
    const total = mesh.indices.length / 3;
    const progress = page.getByRole('progressbar', { name: 'Kirajzolt háromszögek', exact: true });
    const partial = async () => progress.evaluate(element => {
      const shown = Number(element.getAttribute('aria-valuenow'));
      const count = Number(element.getAttribute('aria-valuemax'));
      return { shown, count, partial: shown > 0 && shown < count };
    });
    await expect(progress).toBeVisible();
    await expect.poll(async () => {
      const sample = await partial();
      return sample.count === total && sample.partial;
    }, { message: 'Some real native triangles must appear before the full mesh' }).toBe(true);
    await expect(progress).toHaveAttribute('aria-valuenow', String(total));
    await expect(progress).toHaveAttribute('aria-valuemax', String(total));
    await expectRendered(page, mesh);

    await page.getByRole('button', { name: 'Kirajzolás újra', exact: true }).click();
    await expect.poll(async () => {
      const sample = await partial();
      return sample.count === total && sample.partial;
    }, { message: 'Replay must reveal the existing mesh progressively again' }).toBe(true);
    await expect(progress).toHaveAttribute('aria-valuenow', String(total));
    await expectRendered(page, mesh);
    expect(requests, 'Playback must not rerun the native computation').toBe(1);
    await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toHaveCount(1);
  });

  test('complex demo preserves and replays the real construction stages in order', async ({ page }) => {
    // Observe the UI before the request: early native stages can arrive and
    // finish drawing while the final gRPC response body is still streaming.
    await page.evaluate(() => {
      const observed = window as typeof window & { studioPlaybackStages: string[] };
      observed.studioPlaybackStages = [];
      new MutationObserver(() => {
        const label = document.querySelector('.draw-stage')?.textContent?.replace(/^\d+\/\d+ · /, '').trim();
        if (label && label !== observed.studioPlaybackStages.at(-1) && observed.studioPlaybackStages.length < 64) observed.studioPlaybackStages.push(label);
      }).observe(document.body, { childList: true, subtree: true, characterData: true });
    });
    let requests = 0;
    page.on('request', request => { if (new URL(request.url()).pathname === `${SERVICE}Run`) requests++; });
    const response = rpcResponse(page, 'Run');
    await page.getByRole('button', { name: 'Összetett demó', exact: true }).click();
    const events = await completedRun(await response);
    const steps = events.filter(event => event.kind === 'mesh');
    expect(steps.map(event => event.phase)).toEqual(['construction_step', 'construction_step', 'construction_step', 'editable_proposal']);
    expect(new Set(steps.map(event => event.mesh?.source)).size).toBe(steps.length);
    const labels = steps.map(event => event.text);
    expect(labels.every(Boolean)).toBe(true);
    const mesh = nativeMesh(events);
    expect(mesh.source).toContain('part@14');
    const total = String(mesh.indices.length / 3);
    const progress = page.getByRole('progressbar', { name: 'Kirajzolt háromszögek', exact: true });
    const observedStages = () => page.evaluate(() => (window as typeof window & { studioPlaybackStages: string[] }).studioPlaybackStages);
    await expect(progress).toHaveAttribute('aria-valuemax', total);
    await expect(progress).toHaveAttribute('aria-valuenow', total);
    await expect.poll(observedStages, { message: 'Every actual mesh stage must be shown in native order' }).toEqual(labels);
    await expectRendered(page, mesh);

    await page.evaluate(() => { (window as typeof window & { studioPlaybackStages: string[] }).studioPlaybackStages = []; });
    await page.getByRole('button', { name: 'Kirajzolás újra', exact: true }).click();
    await expect.poll(async () => (await observedStages())[0]).toBe(labels[0]);
    await expect(progress).toHaveAttribute('aria-valuemax', total);
    await expect(progress).toHaveAttribute('aria-valuenow', total);
    await expect.poll(observedStages, { message: 'Replay must retain the actual intermediate snapshots' }).toEqual(labels);
    await expectRendered(page, mesh);
    expect(requests, 'Replaying all four steps must not invoke Run again').toBe(1);
  });
});

test('synthetic PNG upload builds through the real native stages and completes with matching artifacts', async ({ page }) => {
  await page.getByLabel('Kép kiválasztása', { exact: true }).setInputFiles({ name: 'ring.png', mimeType: 'image/png', buffer: drawingPng() });
  await expect(page.getByRole('button', { name: 'Építés', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('img', { name: 'ring.png', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Építési beállítások', exact: true }).click();
  await page.getByLabel(/^Magasság/).fill('3');
  await page.getByLabel(/^Képpontlépték/).fill('0.25');
  await page.getByLabel(/^Tolerancia/).fill('0.05');
  await page.getByRole('button', { name: 'Építési beállítások', exact: true }).click();
  const upload = rpcResponse(page, 'Upload');
  const run = rpcResponse(page, 'Run');
  await page.getByRole('button', { name: 'Modell építése', exact: true }).click();
  expect((await grpcWeb(await upload)).trailers['grpc-status']).toBe('0');
  const events = await completedRun(await run);
  expect(events.filter(event => event.kind === 'artifact').every(event => event.phase === 'artifact')).toBe(true);
  const phases = events.filter(event => ['started', 'evidence', 'coarse_preview', 'editable_proposal', 'preview', 'validation'].includes(event.phase));
  expect(phases.map(event => event.phase)).toEqual(['started', 'evidence', 'coarse_preview', 'editable_proposal', 'preview', 'validation']);
  const coarse = nativeMesh(events, 'coarse_preview');
  const mesh = nativeMesh(events);
  const built = events.find(event => event.phase === 'editable_proposal');
  expect(built?.construction).toMatchObject({
    version: 1,
    coordinateSystem: 'right_handed_z_up',
    units: 'model',
    sketchPlane: 'XY',
    sketchOffset: 0,
    extrusionDirection: [0, 0, 1],
    extrusionDistance: 3,
  });
  expect(built?.construction?.sketchLoops.map(loop => loop.hole)).toEqual([false, true]);
  expect(mesh.volume).toBeCloseTo(364 * 0.25 ** 2 * 3, 8);
  expect(coarse.volume).toBeGreaterThan(mesh.volume);
  expect(coarse.source).not.toBe(mesh.source);
  await expect(page.locator('.event-completed')).toHaveCount(1);
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
  await expectRendered(page, mesh);
  const tree = page.getByRole('tree', { name: 'B-rep műveleti fa', exact: true });
  await expect(tree).toBeVisible();
  for (const reference of ['X tengely', 'Y tengely', 'Z tengely', 'Origópont · (0, 0, 0)', 'XY sík · z = 0', 'XZ sík', 'YZ sík']) {
    await expect(tree.getByRole('treeitem').filter({ hasText: reference })).toHaveCount(1);
  }
  const originNode = tree.getByRole('treeitem', { name: 'Origó', exact: true });
  const stepsNode = tree.getByRole('treeitem', { name: 'Lépések', exact: true });
  const [originBounds, stepsBounds] = await Promise.all([originNode.boundingBox(), stepsNode.boundingBox()]);
  expect(originBounds?.x).toBe(stepsBounds?.x);
  await originNode.click();
  await expect(originNode).toHaveAttribute('aria-expanded', 'false');
  await expect(tree.getByRole('treeitem').filter({ hasText: 'X tengely' })).toHaveCount(0);
  await originNode.click();
  await expect(originNode).toHaveAttribute('aria-expanded', 'true');
  await stepsNode.click();
  await expect(stepsNode).toHaveAttribute('aria-expanded', 'false');
  await expect(tree.getByRole('treeitem').filter({ hasText: 'Kihúzás1' })).toHaveCount(0);
  await stepsNode.click();
  await expect(stepsNode).toHaveAttribute('aria-expanded', 'true');
  await expect(tree.getByRole('treeitem').filter({ hasText: 'Vázlat1' })).toContainText('Vázlat1 · XY, z = 0');
  const extrusionNode = tree.getByRole('treeitem').filter({ hasText: 'Kihúzás1' });
  await expect(extrusionNode).toContainText('Kihúzás1 · +Z 3');
  await extrusionNode.click();
  await expect(extrusionNode).toHaveAttribute('aria-expanded', 'false');
  await expect(tree.getByRole('treeitem').filter({ hasText: 'Vázlat1' })).toHaveCount(0);
  await extrusionNode.click();
  await expect(extrusionNode).toHaveAttribute('aria-expanded', 'true');
  await expect(tree).toContainText('Forrásközép → XY-origó: (4, 3)');
  await expect(tree.getByText('Furat ·', { exact: false })).toBeVisible();
  const constructionProgress = tree.getByRole('progressbar', { name: 'Kihúzás előrehaladása' });
  await expect(constructionProgress).toHaveAttribute('aria-valuenow', '100');
  await page.evaluate(() => {
    const samples: number[] = [];
    (window as Window & { __extrusionProgressSamples?: number[] }).__extrusionProgressSamples = samples;
    (window as Window & { __extrusionProgressTimer?: number }).__extrusionProgressTimer = window.setInterval(() => {
      const progress = document.querySelector('[aria-label="Kihúzás előrehaladása"]');
      if (progress) samples.push(Number(progress.getAttribute('aria-valuenow')));
    }, 15);
  });
  await tree.getByRole('button', { name: 'Modellépítés újrajátszása' }).click();
  // Completion can still be 100 from the previous replay when the click returns.
  await expect.poll(() => page.evaluate(() =>
    ((window as Window & { __extrusionProgressSamples?: number[] }).__extrusionProgressSamples ?? [])
      .some(value => value > 0 && value < 100)), { timeout: 5000 }).toBe(true);
  await expect(constructionProgress).toHaveAttribute('aria-valuenow', '100', { timeout: 3000 });
  const progressSamples = await page.evaluate(() => {
    const state = window as Window & { __extrusionProgressSamples?: number[]; __extrusionProgressTimer?: number };
    if (state.__extrusionProgressTimer) window.clearInterval(state.__extrusionProgressTimer);
    return state.__extrusionProgressSamples ?? [];
  });
  expect(progressSamples.some(value => value > 0 && value < 100), 'Replay should expose an intermediate extrusion fill state').toBe(true);
  await tree.getByRole('treeitem').filter({ hasText: 'Vázlat1' }).click();
  await expect(tree.getByRole('treeitem').filter({ hasText: 'Vázlat1' })).toHaveAttribute('aria-selected', 'true');
  await tree.getByRole('treeitem').filter({ hasText: 'XY sík' }).click();
  await expect(tree.getByRole('treeitem').filter({ hasText: 'XY sík' })).toHaveAttribute('aria-selected', 'true');
  for (const label of ['KONTÚR', 'ELŐNÉZET', 'ELLENŐRZÉS']) {
    await expect(page.locator('.execution-events').getByText(label, { exact: true })).toBeVisible();
  }
  const coarseArtifact = JSON.parse((await downloadArtifact(page, 'coarse-mesh.json')).toString('utf8'));
  expect(coarseArtifact.volume).toBeCloseTo(coarse.volume, 8);
  expect(coarseArtifact.provenance.source).toBe(coarse.source);
  const evidence = JSON.parse((await downloadArtifact(page, 'evidence.json')).toString('utf8'));
  expect(evidence.evidence.polygons).toHaveLength(2);
  expect(evidence.evidence.material_area).toBeCloseTo(364 * 0.25 ** 2, 8);
  expect((await downloadArtifact(page, 'proposal.k3d')).toString('utf8')).toMatch(/^k3d 1\n/);
  expect([...(await downloadArtifact(page, 'preview.png')).subarray(0, 8)]).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
  const likeness = JSON.parse((await downloadArtifact(page, 'likeness.json')).toString('utf8'));
  expect(likeness.matches).toBe(true);
  expect(likeness.provenance.source.source).toBe(mesh.source);
  const manifest = JSON.parse((await downloadArtifact(page, 'manifest.json')).toString('utf8'));
  expect(manifest.execution).toBe('in_process');
  expect(manifest.brep_source).toBe(mesh.source);
  expect(manifest.volume).toBeCloseTo(mesh.volume, 8);
  expect(manifest.expected_volume).toBeCloseTo(mesh.volume, 8);
});

test('text-only build can be submitted without forcing an image upload', async ({ page }) => {
  let requestValue: RunRequest | undefined;
  await page.route('**/krly.studio.v1.StudioService/Run', async route => {
    const body = route.request().postDataBuffer()!;
    requestValue = fromBinary(RunRequestSchema, body.subarray(5, 5 + body.readUInt32BE(1)));
    const event = toBinary(RunResponseSchema, create(RunResponseSchema, {
      runId: 'text-build-mock', seq: 0, kind: 'error', phase: 'expert',
      text: 'Mock: a szakértő fogadta a képleírásos kérést.',
    }));
    const trailers = new TextEncoder().encode('grpc-status: 0\r\n');
    await route.fulfill({
      status: 200,
      contentType: 'application/grpc-web+proto',
      body: Buffer.concat([grpcFrame(event), grpcFrame(trailers, 0x80)]),
    });
  });
  await page.getByRole('button', { name: /Alkatrész leírása vagy rajz/ }).click();
  const prompt = 'Készíts egy egyszerű, zárt sziluettet.';
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill(prompt);
  await expect(page.getByRole('button', { name: 'Modell építése', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Modell építése', exact: true }).click();
  await expect(page.getByText('Mock: a szakértő fogadta a képleírásos kérést.', { exact: true })).toBeVisible();
  expect(requestValue).toMatchObject({ mode: 'build', text: prompt, attachmentIds: [] });
});

test('text CAD presents the verified typed result and editable requirements', async ({ page }) => {
  await page.route('**/krly.studio.v1.StudioService/Run', async route => {
    const mesh = { positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1], indices: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3], volume: 1 / 6, area: 2.36, openEdges: 0, source: 'brep-session:typed-cad', units: 'm' };
    const values = [
      { runId: 'typed-cad', seq: 0, kind: 'status', phase: 'accepted', text: 'Kérés fogadva.' },
      { runId: 'typed-cad', seq: 1, kind: 'mesh', phase: 'validated', text: 'Típusos cél ellenőrizve.', mesh },
      { runId: 'typed-cad', seq: 2, kind: 'artifact', phase: 'artifact', text: 'design.typ', artifact: { id: 'design', name: 'design.typ', mime: 'text/plain', size: 100 } },
      { runId: 'typed-cad', seq: 3, kind: 'completed', phase: 'completed', text: 'Kész.' },
    ].map(value => grpcFrame(toBinary(RunResponseSchema, create(RunResponseSchema, value))));
    const trailers = grpcFrame(new TextEncoder().encode('grpc-status: 0\r\n'), 0x80);
    await route.fulfill({ status: 200, contentType: 'application/grpc-web+proto', body: Buffer.concat([...values, trailers]) });
  });
  await page.getByRole('button', { name: /Alkatrész leírása vagy rajz/ }).click();
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill('15 mm külső és 6 mm belső sugarú gyűrű, 8 mm magas.');
  const response = rpcResponse(page, 'Run');
  await page.getByRole('button', { name: 'Modell építése', exact: true }).click();
  await completedRun(await response);
  await expect(page.getByText('Zárt háló', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /design\.typ/ })).toBeVisible();
  await expect(page.getByRole('tree', { name: 'CAD modellfa' })).toHaveCount(0);
});

test.describe('upload rejection', () => {
  test.use({ expectedBadUpload: true });
  test('unsupported file is rejected locally and a PNG-disguised SVG is rejected by the real service', async ({ page }) => {
    const requests: string[] = [];
    page.on('request', request => {
      if (request.url().includes(`${SERVICE}Upload`) || request.url().includes(`${SERVICE}Run`)) requests.push(request.url());
    });
    const svg = Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>');
    const picker = page.getByLabel('Kép kiválasztása', { exact: true });
    await picker.setInputFiles({ name: 'drawing.svg', mimeType: 'image/svg+xml', buffer: svg });
    await expect(page.getByRole('alert')).toContainText('PNG vagy JPEG képet csatolj.');
    await expect(page.locator('.attachment-preview')).toHaveCount(0);
    expect(requests).toEqual([]);
    await picker.setInputFiles({ name: 'pretend.png', mimeType: 'image/png', buffer: svg });
    const upload = rpcResponse(page, 'Upload');
    await page.getByRole('button', { name: 'Modell építése', exact: true }).click();
    expect((await grpcWeb(await upload)).trailers['grpc-status']).toBe('3');
    await expect(page.getByRole('alert')).toContainText(/PNG, JPEG vagy PNM/);
    await expect(page.getByRole('button', { name: /A végrehajtás megszakadt/ })).toBeVisible();
    expect(requests.filter(url => url.endsWith(`${SERVICE}Upload`))).toHaveLength(1);
    expect(requests.filter(url => url.endsWith(`${SERVICE}Run`))).toHaveLength(0);
    await expect(page.locator('.event-completed')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Modell építése', exact: true })).toBeEnabled();
  });
});

test('typed CAD review keeps a native model visible through refresh, undo and redo', async ({ page }) => {
  test.skip(!process.env.KRLYSCI_BIN, 'The adjacent publication-as-code CLI is required.');
  const base = await readFile(new URL('../../../../cad-knowledge/cases/solid-ring.typ', import.meta.url), 'utf8');
  const review = await readFile(new URL('../../../../cad-knowledge/cases/solid-ring-fill-hole.typ', import.meta.url), 'utf8');
  await page.getByRole('button', { name: 'Típusos CAD' }).click();
  await dom(page, domIds.studio.typedCad.source).fill(base);
  await page.getByRole('button', { name: 'Modell ellenőrzése és létrehozása' }).click();
  await expect(dom(page, domIds.studio.typedCad.state)).toContainText('r0 · 1 elfogadott állapot');
  await dom(page, domIds.studio.typedCad.source).fill(review);
  await page.getByRole('button', { name: 'Javítás ellenőrzése és elfogadása' }).click();
  await expect(dom(page, domIds.studio.typedCad.state)).toContainText('r1 · 2 elfogadott állapot');
  await expect(dom(page, domIds.studio.typedCad.state.changes)).toContainText('hole');
  await expect(page.getByText('1 test', { exact: true })).toBeVisible();
  await page.reload();
  await page.getByRole('button', { name: 'Típusos CAD' }).click();
  await expect(dom(page, domIds.studio.typedCad.state)).toContainText('r1 · 2 elfogadott állapot');
  await page.getByRole('button', { name: 'Visszavonás' }).click();
  await expect(dom(page, domIds.studio.typedCad.state)).toContainText('r0 · 2 elfogadott állapot');
  await page.getByRole('button', { name: 'Újra alkalmazás' }).click();
  await expect(dom(page, domIds.studio.typedCad.state)).toContainText('r1 · 2 elfogadott állapot');
});
