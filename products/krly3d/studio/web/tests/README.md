# CAD construction checks

Run from the Studio directory:

```sh
npm run build
deno test --no-config --node-modules-dir=manual --unstable-sloppy-imports --allow-read web/tests/cadConstruction_test.ts
npx playwright test --config web/tests/playwright.config.ts
```

The isolated browser tests start a preview server on port 4329 and mock only the
RPC boundary. They cover desktop/mobile named feature trees, analytic arc details,
offset planes, real-mesh buffer retention, sketch-only display stages, ordered
replay, persistence after a text-only turn, and the existing image construction v1.
They do not claim to test native fillet support. Main's native checks own that.

Construction v2 carries at most 16 cumulative features and 8 native solid stages.
Sketch curves preserve exact recipe parameters. The viewport uses at most 128 line
segments per arc for display only, with no fill, triangulation or solid extrusion.
