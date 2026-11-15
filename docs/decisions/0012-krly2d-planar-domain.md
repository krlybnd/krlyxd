# ADR 0012 — krly2d közös síkbeli tartomány

Dátum: 2026-11-14. Állapot: elfogadott termékirány; az első API- és natív híd-szelet integrálva. Ellenőrzések: [fejlesztési napló](../DEVELOPMENT-STATUS.md); követés: [#27](https://github.com/krlybnd/krlyxd/issues/27).

## Helyzet

A rastervision és a B-rep egyaránt síkbeli rajzgeometriával dolgozik. A közös alakzatleírás ne a nyers `krlyxd`-be és ne a B-rep belső modelljébe kerüljön. A B-rep továbbra is a saját topológiai és natív geometriai szolgáltatóinak gazdája. A korábbi planar B-rep szerződést a [BREP-PLANAR.md](../BREP-PLANAR.md) és az [ADR 0010](0010-brep-planar-contract.md) írja le.

## Döntés

Legyen külön termék a `krly2d`, közös, immutable síkbeli tartománnyal a rastervision és a B-rep számára. A nyers `krlyxd` doménfüggetlen marad. A krly2d típusok a termék API-jában élnek; a topológia elfogadása és a B-rep építése a meglévő B-rep szolgáltatókon keresztül történik.

A célzott termékstruktúra:

```text
products/krly2d/
├── api/
├── engine/
├── cli/
└── mcp/
```

Ez célstruktúra, nem kész csomaglista. A jelenlegi szelet a `products/krly2d/api` alatti `krly2d-api` crate-re korlátozódik, amely közvetlenül csak a raw `krlyxd`-től függ. A többi könyvtár még nem jelent megvalósított engine-t, CLI-t vagy MCP-t.

Az API validált, immutable `Path` és `Region` értékeket ad. A `Path` szegmensei egyenesek, kvadratikus és köbös Bézier-görbék, valamint elliptikus ívek; a görbék analitikus paramétereit a típusok megőrzik. A `Region` külső határt, opcionális lyukakat és hosszúságegységet tartalmaz. A validáció mezőket, értékeket és szerkezetet ellenőriz; önmagában nem bizonyít egyszerűséget, önmetszés-mentességet, lyukbefoglalást vagy érvényes topológiai orientációt.

A rastervision `SharedDrawing` a régiót `Arc<Region>` formában teszi megoszthatóvá. A helyi shared runtime ezt adja át a meglévő B-rep natív hídjának. A régi planar wire-hívás ugyanarra a hídra delegál, így nem keletkezik második geometriai megvalósítás. A régiót az engine-ek immutable artifactként osztják meg; a revíziók és publikálási események tulajdonosa a runtime marad, a képből származó bizonyíték pedig külön vision-metaadat.

A B-rep híd első változata csak egyenes szegmensekből álló régiókat fogad el. A görbe szegmenst tartalmazó régiót egyértelműen elutasítja, amíg a B-rep szolgáltató adaptációja el nem készül. A görbék API-beli jelenléte tehát nem állít görbe B-rep konstrukciós támogatást.

## Hatókör és következő lépések

Ebben a szeletben nincs SVG/DWG import, teljes 2D geometriai kernel, boolean, offset, vázlatmegoldó, renderelő, CLI vagy MCP. A következő bounded importkísérlet az SVG-t célozza: az átalakításokat, mértékegységeket és kitöltési szemantikát meg kell őrizni; a nem támogatott konstrukciókat el kell utasítani. A DXF/DWG külön, későbbi csereformátum-adapter feladata. A szemantikus rekonstrukció (mit ábrázol a rajz) külön marad a geometriai rajzolástól.

A natív sheet első szelete párhuzamosan készült el; ez a krly2d egyik fogyasztója, a teljes 2D termék készenlétét nem bizonyítja. A shared-runtime kompozícióra és korlátaira az [ADR 0011](0011-shared-runtime.md) vonatkozik.
