# Studio DOM contract pilot

[domIds.ts](./domIds.ts) is the canonical, strongly typed source for the first
`CadFeatureTree` slice. Object keys derive every dot-separated identifier;
consumers spread `.attrs` and tests pass the same nodes to `dom(page, node)`.
The runtime derivation keeps the literal TypeScript shape, so there is no generated
file to drift. YAML parsing, a generator executable, and their dependencies are
intentionally deferred until this narrow contract has proved useful.

`studio` is the application ancestor, `modelTree` is the actual tree, and
`feature.key(featureId(apiId))` identifies a feature wrapper with `data-domkey`.
Only API-valid CAD identifiers are accepted (lowercase ASCII, maximum 32
characters); indices and translated titles never become keys. The `select` and
conditionally mounted `details` children inherit the full instance scope.
This pilot supports one Studio root; multiple simultaneous models/windows need
an explicit root instance key before extending it to that use case.

[dom.ts](../web/tests/dom.ts) resolves each ancestor against the actual DOM,
hex-escapes selector values, and supplies feature cardinality assertions. Locator
actions retain Playwright strictness, so duplicate matches fail. Roles, labels,
and keyboard behavior remain the component's accessibility API.

`npm run build` checks raw attribute usage, TypeScript (including the negative
compile fixtures), and the production bundle. Run the focused E2E after building:

```sh
npx playwright test --config web/tests/playwright.config.ts --grep 'named CAD tree'
```

That existing test runs desktop and mobile, checks selection and collapsed detail
presence, injects duplicate instances and a feature under the wrong ancestor,
and verifies stable addressing after order and visible-text changes. This is a
mocked RPC UI contract test, not evidence of native CAD execution or a complete
HU/EN localization suite. The feature dependency DAG is not the DOM hierarchy.

The typed CAD panel uses the same nested contract for its session selector,
accepted state, change list and `.typ` source editor. Its real-service E2E follows
those nodes without literal `data-domid` strings.
