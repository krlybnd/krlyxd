# krly2d

A `krly2d` külön termék a rastervision és a B-rep által megosztott síkbeli rajzgeometriához. A termékhatárt az [ADR 0012](decisions/0012-krly2d-planar-domain.md) rögzíti. Ez az első, integrált natív szelet állapotleírása; nem teljes 2D kernel vagy kész termék.

## Jelenlegi szelet

A `products/krly2d/api` alatti `krly2d-api` a raw `krlyxd`-re támaszkodik. Validált, immutable `Path` és `Region` típusokat ad: a szegmens egyenes, kvadratikus/köbös Bézier vagy elliptikus ív lehet; a régió külső határt, lyukakat és hosszúságegységet tartalmaz. A validáció strukturális, nem helyettesíti a topológiai ellenőrzést.

A rastervision `SharedDrawing` `Arc<Region>`-t oszt meg a helyi runtime-mal, amely a meglévő B-rep natív hídjához továbbítja. A régi planar wire-hívás is ezt a hidat használja. A híd jelenleg csak egyenes szegmenseket fogad; görbe régió esetén hibát ad. A görbék analitikus adatai megmaradnak az API-ban, de B-rep konstrukciós támogatásuk későbbi szolgáltató-adaptációt igényel.

## Határ és terv

A felhasználó 2026-11-14-i pontosítása szerint az SVG/DXF/DWG-import és a további
önálló krly2d-bővítés későbbi backlog. A mai fókusz a meglévő rajz → 3D út;
az alábbi importterv nem a mai végrehajtási sorrend.

A termék tervezett rétegei az `api`, `engine`, `cli` és `mcp`; jelenleg az API és a meglévő motorokba való bekötés készült el. A `krlyxd` nyers, doménfüggetlen marad, a B-rep topológia és natív építés a meglévő providerek tulajdona. Az immutable régió megosztható a shared hostban, míg a revíziók és események kezelése a runtime feladata. A képi bizonyíték és bizonytalanság vision-metaadat marad, nem válik a geometria részévé.

A következő bounded formátumkísérlet az SVG-import. Meg kell őriznie az átalakításokat, egységeket és kitöltési szemantikát, egyébként az adott konstrukciót el kell utasítania. A DXF/DWG külön, későbbi csereadapter lehet. A rajz szemantikus értelmezése külön feladat a geometriától.

Ebben az állapotban nincs SVG/DWG import, teljes 2D solver, boolean/offset, vázlatmegoldó, renderelő, önálló CLI/MCP vagy késztermék-állítás. A B-rep planar háttérhez lásd a [BREP-PLANAR.md](BREP-PLANAR.md), a host működéséhez pedig a [SHARED-RUNTIME.md](SHARED-RUNTIME.md) dokumentumot. A natív sheet elkészült első szelete ezt a régiót használja; a teljes CLI lifecycle még külön feladat. Követés: [#27](https://github.com/krlybnd/krlyxd/issues/27).


## Konkrét határok

A natív API konstruktorait, erőforráskorlátait és hibakódjait az
[API README](../products/krly2d/api/README.md) írja le. A végpontok és kontrollpontok
minden véges f64 értéket megőriznek, beleértve a nagy, subnormal és előjeles nulla
értékeket. A numerikus művelet megengedett tartományát a fogyasztó szabja meg.
A jelenlegi B-rep fogyasztó legfeljebb 256 csúcsot, model egységet és egyenes
határokat fogad; a 2D szerződés ennél több alakzatot tud tárolni. Egyik limit sem
jelent általános fizikai pontossági garanciát.

A vision a raszteres datumot és a világkoordinátás régiót is megtartja a korábbi
adapterek miatt. A régió Arc-ja közös, de a B-rep providerhez ideiglenes poligonok
és profilszöveg készülnek; teljes másolatmentességet nem állítunk. A képhash és a
bizonytalanság a vision tulajdona, a forráselem-azonosítók majd az importáló adapter
felelőssége lesznek. Az ellenőrzések a [fejlesztési naplóban](DEVELOPMENT-STATUS.md)
szerepelnek.
