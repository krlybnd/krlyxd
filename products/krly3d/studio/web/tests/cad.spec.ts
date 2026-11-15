import { create, toBinary } from '@bufbuild/protobuf';
import { test, expect, type Route } from '@playwright/test';
import { HealthResponseSchema, RunResponseSchema } from '../../gen/krly/studio/v1/studio_pb.ts';
import { construction, mesh } from './cadFixtures.ts';
import { domIds, featureId } from '../../ssot/domIds.ts';
import { dom, expectFeatureDom } from './dom.ts';
function frame(bytes: Uint8Array, flag = 0) { const header = Buffer.alloc(5); header[0] = flag; header.writeUInt32BE(bytes.length, 1); return Buffer.concat([header, bytes]); }
async function respond(route: Route, messages: Uint8Array[]) { await route.fulfill({ contentType: 'application/grpc-web+proto', body: Buffer.concat([...messages.map(message => frame(message)), frame(Buffer.from('grpc-status: 0\r\n'), 0x80)]) }); }

test('named CAD tree shows separate arc profiles at their planes and selects only real solid snapshots', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message));
  let runs = 0;
  await page.route('**/krly.studio.v1.StudioService/*', async route => {
    if (route.request().url().endsWith('/Health')) return respond(route, [toBinary(HealthResponseSchema, create(HealthResponseSchema, { chat: true, native: true, model: 'mock' }))]);
    expect(route.request().url()).toContain('/Run'); runs++;
    if (runs > 1) return respond(route, [toBinary(RunResponseSchema, create(RunResponseSchema, { runId: 'chat-only', seq: 0, kind: 'completed', phase: 'completed' }))]);
    const events = [
      { kind: 'status', phase: 'cad_review', text: 'Ellenőrzött CAD terv.' },
      ...[1, 3, 4].map((index, i) => ({ kind: 'mesh', phase: index === 4 ? 'editable_proposal' : 'construction_step', text: ['Alaptest', 'Zseb', 'Lekerekítés'][i], mesh: mesh(1 + i), construction: construction(index) })),
      { kind: 'completed', phase: 'completed' },
    ];
    await respond(route, events.map((event, seq) => toBinary(RunResponseSchema, create(RunResponseSchema, { ...event, seq, runId: 'cad-tree' }))));
  });
  await page.goto('/');
  await expect(page.getByText('Szolgáltatás elérhető', { exact: true })).toBeVisible();
  const chatTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Beszélgetés' });
  if (await chatTab.isVisible()) await chatTab.click();
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill('Íves vázlat, kihúzás, zseb és lekerekítés');
  await page.getByRole('button', { name: 'Üzenet küldése', exact: true }).click();
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
  const modelTab = page.getByRole('navigation', { name: 'Munkaterületek' }).getByRole('button', { name: 'Modell' });
  if (await modelTab.isVisible()) await modelTab.click();
  const tree = dom(page, domIds.studio.modelTree);
  await expect(tree).toHaveRole('tree');
  await expect(tree).toHaveAccessibleName('CAD modellfa');
  const feature = (id: string) => domIds.studio.modelTree.feature.key(featureId(id));
  const select = (id: string) => dom(page, feature(id).select);
  const roots = tree.locator(':scope > details');
  await expect(roots).toHaveCount(2);
  const origin = tree.getByRole('treeitem', { name: 'Origó', exact: true });
  const steps = tree.getByRole('treeitem', { name: 'Lépések', exact: true });
  const references = tree.getByRole('group', { name: 'Origó referenciái' });
  const features = tree.getByRole('group', { name: 'CAD lépések' });
  await expect(references.getByRole('treeitem')).toHaveCount(7);
  await expect(features.getByRole('treeitem')).toHaveCount(5);
  await origin.click();
  await expect(origin).toHaveAttribute('aria-expanded', 'false');
  await expect(references).toBeHidden();
  await expect(features).toBeVisible();
  await steps.click();
  await expect(steps).toHaveAttribute('aria-expanded', 'false');
  await expect(features).toBeHidden();
  await origin.click();
  await expect(references).toBeVisible();
  await expect(features).toBeHidden();
  await steps.click();
  await expect(steps).toHaveAttribute('aria-expanded', 'true');
  await select('rounding').click();
  await expect(tree).toContainText('Sugár: 0,5 mm');
  await expectFeatureDom(page, feature('rounding'), true);
  await expectFeatureDom(page, feature('outline'), false);
  await expect(select('rounding')).toHaveRole('treeitem');
  await expect(select('rounding')).toHaveAttribute('aria-selected', 'true');
  // Exercise the real rendered contract: duplicate instances must remain ambiguous.
  await dom(page, feature('rounding')).evaluate(element => element.after(element.cloneNode(true)));
  await expect(dom(page, feature('rounding'))).toHaveCount(2);
  await expect(select('rounding')).toHaveCount(2);
  await dom(page, feature('rounding')).last().evaluate(element => element.remove());
  await select('rounding').evaluate(element => element.after(element.cloneNode(true)));
  await expect(select('rounding')).toHaveCount(2);
  await select('rounding').last().evaluate(element => element.remove());

  // A matching identifier outside its declared parent must not be found.
  await dom(page, feature('rounding')).evaluate(element => {
    (window as unknown as { restoreFeature: () => void }).restoreFeature = () => parent.insertBefore(element, next);
    const parent = element.parentNode!; const next = element.nextSibling;
    document.body.append(element);
  });
  await expect(dom(page, feature('rounding'))).toHaveCount(0);
  await expect(select('rounding')).toHaveCount(0);
  await page.evaluate(() => (window as unknown as { restoreFeature: () => void }).restoreFeature());
  // Order and visible text do not participate in feature addressing.
  const originalText = await select('rounding').textContent();
  await select('rounding').evaluate(element => {
    element.textContent = 'Fillet · Edge rounding';
    element.parentElement!.parentElement!.prepend(element.parentElement!);
  });
  await expect(select('rounding')).toHaveText('Fillet · Edge rounding');
  await expectFeatureDom(page, feature('rounding'), true);
  await select('rounding').evaluate((element, text) => { element.textContent = text; }, originalText);

  await expect(page.locator('.geometry-stats')).toContainText('4 háromszög');
  const canvas = page.locator('.viewport-canvas canvas');
  // Compare the WebGL canvas itself: selecting a reference must actually draw
  // geometry, not merely change the tree's selection styling.
  const renderedCanvas = () => canvas.screenshot({ style: '.viewport-canvas ~ * { visibility: hidden !important; }' });
  const baseline = await renderedCanvas();
  for (const name of ['X tengely', 'Y tengely', 'Z tengely', 'Origópont · (0, 0, 0)', 'XY sík · z = 0', 'XZ sík', 'YZ sík']) {
    const reference = references.getByRole('treeitem', { name, exact: true });
    await reference.click();
    await expect(reference).toHaveAttribute('aria-selected', 'true');
    await expect.poll(async () => (await renderedCanvas()).equals(baseline)).toBe(false);
    await expect(page.locator('.geometry-stats')).toContainText('4 háromszög');
    await reference.click();
    await expect(reference).toHaveAttribute('aria-selected', 'false');
    await expect.poll(async () => (await renderedCanvas()).equals(baseline)).toBe(true);
  }
  await select('outline').click();
  await expect(page.getByRole('status', { name: 'Látható vázlatprofil' })).toContainText('XY, Z = 7,5 mm');
  await expect(tree).toContainText('Ív · R 5 mm · 180°');
  await expect(tree).toContainText('Szakasz · 10 mm');
  await expect(page.locator('.geometry-stats')).toContainText('0 háromszög');
  await expect(page.locator('.viewport-canvas canvas')).toBeVisible();
  await select('cut_profile').click();
  await expect(page.getByRole('status', { name: 'Látható vázlatprofil' })).toContainText('Z = 19,5 mm');
  await expect(page.locator('.geometry-stats')).toContainText('4 háromszög');
  await select('pocket').click();
  await expect(tree).toContainText('Mélység: 4 mm');
  await expect(page.getByRole('status', { name: 'Látható vázlatprofil' })).toHaveCount(0);
  await select('base').click();
  await expect(tree).toContainText('Magasság: 12 mm');
  await tree.getByRole('button', { name: 'Modellépítés újrajátszása' }).click();
  await expect(select('outline')).toHaveAttribute('aria-current', 'step');
  await expect(select('base')).toHaveAttribute('aria-current', 'step');
  await expect(select('cut_profile')).toHaveAttribute('aria-current', 'step');
  await expect(select('rounding')).toHaveAttribute('aria-current', 'step');
  // A subsequent text-only turn must not make the completed CAD tree unselectable.
  if (await chatTab.isVisible()) await chatTab.click();
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill('Köszönöm');
  await page.getByRole('button', { name: 'Üzenet küldése', exact: true }).click();
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toHaveCount(2);
  if (await modelTab.isVisible()) await modelTab.click();
  await select('cut_profile').click();
  await expect(page.getByRole('status', { name: 'Látható vázlatprofil' })).toContainText('Z = 19,5 mm');
  expect(runs).toBe(2); expect(errors).toEqual([]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});

test('legacy image-contour construction remains version one with its original tree', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message));
  await page.route('**/krly.studio.v1.StudioService/*', async route => {
    if (route.request().url().endsWith('/Health')) return respond(route, [toBinary(HealthResponseSchema, create(HealthResponseSchema, { chat: true, native: true }))]);
    await respond(route, [
      create(RunResponseSchema, { runId: 'legacy', seq: 0, kind: 'mesh', phase: 'editable_proposal', mesh: { ...mesh(), units: '' }, construction: { version: 1, coordinateSystem: 'right_handed_z_up', units: 'model', sourceCenter: [0, 0], sourceBounds: [-5, -5, 5, 5], sketchPlane: 'XY', sketchOffset: 0, sketchLoops: [{ pointsXy: [-5, -5, 5, -5, 5, 5, -5, 5] }], extrusionDirection: [0, 0, 1], extrusionDistance: 12 } }),
      create(RunResponseSchema, { runId: 'legacy', seq: 1, kind: 'completed', phase: 'completed' }),
    ].map(event => toBinary(RunResponseSchema, event)));
  });
  await page.goto('/');
  await expect(page.getByText('Szolgáltatás elérhető', { exact: true })).toBeVisible();
  const tabs = page.getByRole('navigation', { name: 'Munkaterületek' });
  if (await tabs.isVisible()) await tabs.getByRole('button', { name: 'Beszélgetés' }).click();
  await page.getByLabel('Üzenet a Studiónak', { exact: true }).fill('Korábbi kontúr');
  await page.getByRole('button', { name: 'Üzenet küldése', exact: true }).click();
  await expect(page.getByRole('button', { name: /Végrehajtás befejezve/ })).toBeVisible();
  if (await tabs.isVisible()) await tabs.getByRole('button', { name: 'Modell' }).click();
  const tree = page.getByRole('tree', { name: 'B-rep műveleti fa' });
  await expect(tree).toBeVisible(); await expect(tree).toContainText('Vázlat1');
  await expect(tree).toContainText('Kihúzás1');
  await expect(page.getByRole('tree', { name: 'CAD modellfa', exact: true })).toHaveCount(0);
  await expect(tree.getByRole('progressbar', { name: 'Kihúzás előrehaladása' })).toHaveAttribute('aria-valuenow', '100');
  expect(errors).toEqual([]);
});
