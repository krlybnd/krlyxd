import { readdir, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';
const studio = fileURLToPath(new URL('..', import.meta.url));
async function scan(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await scan(path);
    else if (/\.[cm]?[jt]sx?$/.test(entry.name) && relative(studio, path) !== 'web/tests/dom.ts') {
      const source = await readFile(path, 'utf8');
      if (/data-domid|data-domkey/.test(source)) throw new Error(`Raw DOM contract attribute in ${relative(studio, path)}; use domIds attrs / dom().`);
    }
  }
}
await scan(join(studio, 'web/src'));
await scan(join(studio, 'web/tests'));
console.log('DOM contract consumers use the canonical typed source.');
