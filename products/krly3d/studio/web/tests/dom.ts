import { expect, type Locator, type Page } from '@playwright/test';
import type { DomNode } from '../../ssot/domIds.ts';

// Hex-escape every code point, including quotes, slashes and Unicode. No raw key interpolation.
const cssValue = (value: string) => `"${Array.from(value, char => `\\${char.codePointAt(0)!.toString(16)} `).join('')}"`;
/** Follow actual DOM ancestors, including the repeated feature instance. */
export function dom(page: Page, node: DomNode): Locator {
  let scope: Page | Locator = page;
  for (const ancestor of node.scopes) {
    scope = scope.locator(`[data-domid=${cssValue(ancestor.id)}]${ancestor.key === undefined ? '' : `[data-domkey=${cssValue(ancestor.key)}]`}`);
  }
  return scope as Locator;
}
/** Cardinality is part of the contract, also for currently collapsed details. */
export async function expectFeatureDom(page: Page, feature: DomNode & { select: DomNode; details: DomNode }, expanded: boolean) {
  await expect(dom(page, feature)).toHaveCount(1);
  await expect(dom(page, feature.select)).toHaveCount(1);
  await expect(dom(page, feature.details)).toHaveCount(expanded ? 1 : 0);
}
