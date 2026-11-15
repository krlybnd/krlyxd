# Fejlesztési állapot

## Studio: vázlatos feature-folyamat és MCP-irány — 2026-11-15

A villáskulcs-próba megmutatta: a sikeres natív futás nem igazolja, hogy a kért
forma készült el. Külön íves XY-vázlat, kihúzás, zsebkivágás és támogatott natív
lekerekítés került a receptbe; a geometriai ellenőrzést külön szöveges
tervellenőrzés és legfeljebb egy célzott javítás egészíti ki. A fában v2
műveletleírás viszi a vázlatok analitikus görbéit és a feature-kapcsolatokat.
A valós natív regresszió a 200 mm hosszú, 16 mm pofanyílású íves kulcsot és a
nyél zsebét is ellenőrzi; ez tesztfixture, nem élő AI-tervezési sikermérés.

Az új folyamat élő Studio-próbája kép és keresés nélkül sikerült: saját
80×40×8 mm-es lemez, középen 40×12 mm-es, 2 mm mély vak zseb. A letöltött
recept két külön XY-vázlatot tartalmaz (Z=0 és Z=8 mm); a natív jelentés
80×40×8 mm befoglalót, 24 640 mm³ térfogatot és 0 nyitott élt ad.
A vázlat kiválasztásakor az eltolt sík és a négy méretezett szakasz megjelenik.
Az adott futás 5,1 s, két AI-hívás, 3822 token és 0,002052 USD; ez egy sikeres próba,
nem általános megbízhatósági mérés vagy szabványmegfelelőségi igazolás.
A v2 fa külön, azonos szintű lenyitható Origó és Lépések ágat használ;
az origó tengelyei és referenciasíkjai kiválaszthatók. Friss oldalbetöltés utáni
ismételt natív építéssel a síkválasztás és a köztes vázlatra visszalépés is
működött; az ismételt építés 4,19 s és 0,0011844 USD volt.

Az agent felé a meglévő Rust MCP-sémák legyenek az eszközleírások forrásai.
A fájlos, verziózott művelet-/tutorial-katalógus és opcionális hibrid keresés
irányát az [ADR 0016](decisions/0016-cad-tool-discovery-and-verified-workflow.md)
rögzíti. A futó MCP 45 műveletet kínál; Studio-ba kötött dinamikus keresés és
embeddingindex még nincs. Az alábbi korábbi fogyasztási számok a külön
tervellenőrzés bevezetése előtti futásokhoz tartoznak.

## Studio: szöveges CAD — 2026-11-15

A kép opcionális: a szakértő validált, legfeljebb nyolclépéses milliméteres
CAD-receptet adhat a natív B-rep kernelnek. A saját méretekkel leírt alkatrészhez
egy tervezői hívás elég; a felismert szabványjelölés építési módban közvetlenül
célzott szöveges kutatásra kerül, külön szándékfelismerő hívás nélkül.
Sikeres kutatás 1 óráig, legfeljebb 32 bejegyzéssel újrafelhasználható.
A korábbi feltöltött kép nem váltja át automatikusan az új szöveges építést
kontúrkihúzásra. Natív recept, mm-es JSON-terv és forrásjegyzék letölthető;
ezek nem jelentenek teljes szabványmegfelelőségi igazolást. DWG/STEP fájlimport
továbbra sincs bekötve. Részletek és határok a [Studio leírásában](../products/krly3d/studio/README.md).

Élő böngészős mérés: ISO 7089 M8 alátét első ellenőrzése + építése 5,32 s,
2 AI-hívás, 10 962 token és 0,0300736 USD; ugyanazon források újrafelhasználásával
2,65 s, 1 AI-hívás, 2 096 token és 0,0011816 USD. Ezek az adott futások
szolgáltató által jelentett számai, nem garantált díjak vagy általános benchmarkok.

A további élő próba egy saját 80×50×8 mm szerelőlemez három Ø6 mm átmenő
furattal. A korábbi, AI által méretezett vágóhenger hibás mélységét külön
`through_hole` művelet váltotta fel: a host számítja a teljes átvágást.
A helyes négylépéses modell 4,14 s, 1 AI-hívás, 1 898 token és 0,001124 USD
lett, keresés és kép nélkül. A natív regresszió a furatfalak teljes mélységét,
a térfogatot és a zárt hálót Z=0, +10 és −10 mm eltolásnál is vizsgálja.

## Mai prioritás — felhasználói pontosítás, 2026-11-14

Új felhasználói döntés: **ma nincs folyadékszimuláció**. A mai kipróbálási felület
React/TypeScript → protobuf/gRPC-Web → Deno/TypeScript, kódban definiált LangGraph
vezérléssel, LiteLLM-mel és kép-/hangbevitellel. A Studio alkalmazáskompozíció helye
`products/krly3d/studio`; az elfogadott Rust doménhatárok megmaradnak. Az újabb kérés
szerint a forma kialakulását követhető kirajzolás és összetett natív építési példa
mutatja. Részletek: [Studio](../products/krly3d/studio/README.md),
[ADR 0013](decisions/0013-studio-application-boundary.md).

Ma a meglévő rajz → 3D folyamat fejlesztése a cél. Az SVG/DXF/DWG-import és a
további önálló krly2d-bővítés (#27) későbbi munka; az alábbi korábbi bejegyzésben
említett SVG-következőlépés nem a mai végrehajtási sorrend. A már integrált közös
2D régió a 3D út része marad.

A jelenlegi, támogatott rajz → 3D folyamat befejezéséhez #24 öt nyitott elfogadási
blokkja tartozik: teljes cancellation/deadline/scheduler/backpressure; tényleges
geometriai refinement és javaslatelfogadás; verziózott események és tartós parancs-/
input-replay; támogatott esetek és pontossági küszöbök; összehasonlítható idő-/másolat-/
memóriamérés. Ezekhez #15 és #26 kapcsolódó részei szükségesek, nem a teljes szélesebb
AI/PMI- és doménrollout-backlog. Ez öt nagy munkablokk, még további részfeladatokra
bontható; nem határidőbecslés. A [reprodukálható, látható műveleti nyomvonal
javaslata](decisions/0015-reproducible-construction-timeline.md) ezt a #24-es
esemény/replay és ellenőrzési blokkot konkretizálja. Ennek első B-rep szelete
most megjeleníti a natív sketch/extrude fát és az eredeti kontúrt; a külön időzített
sketch-számítási esemény, a field recipe szemantikus checkpointjei és a pixelrács
megőrzése még nyitott. Ez részeredmény, nem teljes #24-elfogadás. A teljes projekt
18 nyitott issue-ja ennél tágabb kör.

### B-rep műveleti fa és UCS — első szelet

A Studio bal alsó fája a natív kontúrprofilt és kihúzást kapja meg, nem a chat
szövegéből előállított lépésleírás. A vázlat a valódi profilhurkokat és furatokat
őrzi, kiválasztásakor az XY síkon kirajzolja a kontúrt. Az origó a forráskontúr
befoglaló-középpontja; a globális keret jobbkezes, Z-felfelé, a vázlat `XY, z=0`,
a kihúzás `+Z`. Az origó XY/XZ/YZ síkjai kiválaszthatók; eltolási segédsík nem
jelenik meg, mert az aktuális egyetlen kihúzás nem használ ilyet. A forrásbeli
középpontot és befoglaló méreteket a natív esemény rögzíti a reprodukcióhoz.

Az extrudálás még egy natív műveletben állítja elő a teljes B-repet és mesh-t;
ezért az új fa hiteles feature-előzmény, de a vázlat külön számítási köztes
eredménye és a valós időben animált profilalkotás még nincs meg. Az origó/sík/
kiemelés szerződésére írt natív rekonstrukciós próba, a teljes Studio build és
83 service teszt átment. A valódi böngészős képbemenet → native B-rep e2e-ben a
tree megjelent, a vázlat- és referenciasík-választás működött; a mobil e2e is
átment. A #24 marad nyitva.

## 2026-11-14 — Huszonötödik kör: térfogati figura és STL pilot

A felhasználó pontosítása: teljes térbeli Pikachu-figura kell nyomtatáshoz;
a korábbi kihúzott sziluett nem teljesítette ezt a kérést. Három párhuzamos
fejlesztési ág készült: önálló field API/motor/CLI, mesh előellenőrzés + STL,
és Studio-integráció. Részletek: [ADR 0014](decisions/0014-field-mesh-print-pilot.md).

A [figurarecept](../models/pikachu.field.json) 53 csomópontos paraméteres modell,
nem képből felismert geometria. A mintavétel 1 630 512 rácspontból
150 170 csúcsot és 300 340 háromszöget ad. Az STL kb. 15 MB; a méret
72,94 × 36,49 × 101,00 mm. Egyetlen összefüggő, pozitív térfogatú háló,
nyitott/nem-manifold élek, nem-manifold csúcsok, degenerált lapok és
irányítási hibák nélkül. Az eredeti és a float32-re kerekített háló külön
numerikus BVH-önmetszésvizsgálata sikeres (2 195 272 és 2 195 322 vizsgált pár).

A külső PyMeshLab 2025.7.post1 önmetszéskijelölője az eredeti hálón 322,
az STL-en 434 lapot jelölt. A reprodukálható
[független diagnózis](../tests/field_print_geometry.py) ezeket 161/217 egymástól
szigorúan elkülönülő, egy síkban fekvő háromszögpárra vezeti vissza. A
vetített szeparálótengely-próba mindegyiknél pozitív rést mér; a külső jelzés
nem jelentett valódi metszést. Az eredményt nem hallgatjuk el, és a célzott
diagnózis nem helyettesíti az egész háló natív ellenőrzését.

A Studio a milliméteres modellt, az STL-t, a receptet és a vizsgálati jelentést
teszi elérhetővé. A demó nem indít AI-hívást. A teljes nyomtathatóság továbbra
sem automatikus minősítés: a falvastagság-analízis és a nyomtató/szeletelő
beállításai ismeretlenek. #12/#13/#24 teljes elfogadása nyitott; az általános
AI-s karaktertervezés, field MCP, dual contouring és sculpt-eszközök hátravannak.

Mért helyi idő: a natív field építés release-ben 0,85 s, debugban 20,51 s.
A teljes Studio gRPC-próba az új önmetszésvizsgálattal release-ben 5,712 s,
a négy fájl letöltésével 6,044 s; debugban ugyanez 61,196 s. A Studio
először teljes release field+print binárispárt választ, ennek hiányában teljes
debug párt. Nem keveri a profilokat és végrehajtási hibánál nem próbálja újra
a másik profilt. Ezek helyi mérések, nem általános késleltetési garanciák.

Ellenőrzés: 61 érintett Rust teszt és az érintett csomagok strict Clippy-je
sikeres; a Studio integráció 82 szervertesztje, majd az utolsó jelentés-/profil-
módosítások 25 célzott adaptertesztje és a valós gRPC-próba sikeres. Négy új
asztali/mobil UI-próba, TypeScript/Deno ellenőrzés és a felület buildje sikeres.
A külső koplanáris diagnózis és negatív kontrollja is lefutott. Nincs commit/push.

## 2026-11-14 — Huszonnegyedik kör: szaki, A–D pontosítás és hívásköltség

A Studio chat első LangGraph csomópontja most a
[szaki](../products/krly3d/studio/service/ai/expert.ts). Egy döntési hívásban
választ választ, támogatott kontúrkihúzást, egy pontosító kérdést vagy egy rövid
internetes ellenőrzést. A keresés valódi LiteLLM Responses web search, forráslinkekkel;
legfeljebb egyszer történik meg az egész kérésben, a folytatásokon át is.
A rövid, látható megfigyelések nem privát modellgondolatmenetek.

A [pontosítás](../products/krly3d/studio/service/questions.ts) munkamenethez
kötött, 30 percig érvényes kérdésazonosítót használ. Az egyetlen A–D checkbox
űrlapon egy vagy több válasz adható; több kérdés egymás után következik. A
folytatás az eredeti képet és paramétereket használja, a korábbi döntéseket
megőrzi. Az új kérés felülírja a régi kérdést, az idegen/lejárt válasz elutasított;
a várakozás nem foglal számítási helyet. Ez memóriabeli alkalmazásfolyamat,
nem tartós checkpoint vagy geometriai javaslatelfogadás.

A [költségpanel](../products/krly3d/studio/web/src/UsageDetails.tsx) hívásonként
és a folytatásokkal együtt mutatja a tokeneket, cache-t, kereséseket és USD díjat.
A hívásazonosító alapú összesítés nem duplázza meg a pillanatképeket. A jelentett,
becsült, részleges és ismeretlen költség külön jelölt; a native-only út nulla
AI-hívást mutat. A diktálás és az egész alkalmazás számlája nem része a panelnek.
A natív-only vagy túl nagy korábban feltöltött kép már nem töri el a szöveges chatet.

Az élő próba két fontos finomítást hozott: a döntési séma egymást kizáró
ágakat kapott, a választott cél pedig tényleges assistant/user párbeszédfordulóként
is visszakerül a modellhez, hogy ne kérdezzen rá újra. A tervezett kihúzásnál a
szolgáltatás saját státusza jelzi az építés indítását; az AI nem jelentheti késznek
a még el nem futott natív műveletet.

**Élő böngészős próba:** az eredeti 512×512 Wikimedia-fogaskerék feltöltése után
keresés → két forrás → A–B kérdés → A (díszítő modell) válasz → valódi natív
építés sikeres. A kérdésig 6,35 s, a válasz utáni kész modell 2,72 s;
584 háromszög, zárt háló, 7,85 × 7,85 × 8 befoglaló. A lenyitott költségpanel
4 AI-hívást, egy keresést, **14 827 tokent és 0,031748 USD** proxy által jelentett
összeget mutatott. Ebből a keresés 0,0287788 USD. Ezek helyi megfigyelések,
nem késleltetési vagy jövőbeli árazási garanciák.

**Ellenőrzés:** 77 Deno szerverteszt sikeres; az utolsó párbeszéd-/megjelenítési
finomítások érintett expert/research/workflow tesztjei külön is sikeresek.
12 kontrollált asztali/mobil HITL-költség UI teszt és 6 valódi natív Playwright
regresszió, production build, Deno type-check, lint és diff-check sikeres.
A Rust kernel ebben a körben nem változott. A #15/#24 tágabb tartós proposal/replay/
refinement elfogadásai továbbra is nyitottak; nincs commit vagy push.

## 2026-11-14 — Huszonharmadik kör: Studio és teljes felbontású képből építés

A [Studio](../products/krly3d/studio/README.md) React/Three.js munkateret,
protobuf/gRPC-Web → Deno kapcsolatot és kódban definiált LangGraph folyamatot kapott.
Saját, helyi LiteLLM-proxy szolgálja ki a chatet, képelemzést és hangátírást;
a provider-kulcsok csak Gitből kizárt szerveroldali fájlokban vannak. A böngésző
valódi natív státuszokat, korai/végleges hálókat és letölthető artifactokat kap.
Hiba/megszakítás visszaállítja az utolsó kész modellt. A magyar hangátírás
szintetikus beszédmintával ellenőrzött; a tényleges mikrofonrögzítés még nem.

A forma kialakulása háromszögenként követhető, szüneteltethető és újrajátszható.
Az összetett demó négy tényleges natív építési állapotot mutat: alaplemez,
rögzítőfuratok, támasztóbakok, végül kivágott hátfal és merevítőbordák.
Ez a kapott geometriák megjelenítési lejátszása; a tartós szerkesztési parancs-replay
és a geometriai refinement továbbra is külön, nyitott feladat.

A valódi [Wikimedia-fogaskerék](https://commons.wikimedia.org/wiki/File:Gear-icon-white-background.png)
512×512-es képe először beleütközött a 256 kontúrpontos határba. A 64 pixeles
kerülőút látványosan lépcsős lett, ezért a javítás az eredeti képet tartja meg:
a [kontúregyszerűsítő](../package/krlyvis/crates/std/src/outline/simplify_holes.rs)
forráspixelben megadott eltéréssel csökkenti a pontokat, ellenőrzi az önmetszést,
a hurkok érintkezését, irányát és a furatok tartalmazását. Nem biztonságos
egyszerűsítésnél megmarad az eredeti kontúr; a 256 pontos korlát nem nőtt.
A natív API/CLI alapértéke továbbra is nulla, a Studio kifejezetten egy
forráspixelt kér. A képmérés bizonytalansága ismeretlen; analitikus kör-/ívillesztést
ez a javítás nem végez.

Az eredeti képet újra **a böngésző fájlválasztóján keresztül**, szöveges kéréssel
küldtük be. A javított futás **3,24 s**, 584 háromszög, zárt háló, megmaradó
középső furat; befoglaló 7,85 × 7,85 × 8 modellegység, 0,025 egység/pixel.
Ez egy helyi megfigyelés, nem sebességvállalás.

**Ellenőrzés:** 49 Deno szerverteszt, 13 célzott Rust kontúrteszt és 91 vision
API/engine + rekonstrukciós runtime/CLI teszt, 6 valódi
Playwright böngészőteszt (4 regressziós + 2 kirajzolás/újrajátszás), production UI
build, Deno type-check és szigorú workspace all-target Clippy sikeres.
Az utóellenőrzésben javítottuk a megszakított, olvasás nélkül parkoló RPC-k
számítási helyének felszabadítását és a demók függetlenségét a lejárt képektől;
három új lifecycle regressziós teszt és az ismételt natív gRPC-integráció sikeres.
A korábbi 1237-es teljes Rust futás történeti eredmény, nem e kör újramérése.
Az implementáció továbbra is helyi, nincs commit/push. #24 és #15 frissítve;
a tágabb elfogadási feltételeik miatt nyitva maradnak. Folyadék és SVG/DWG nem része
ennek a körnek.

## 2026-11-14 — Huszonkettedik kör: közös CLI és natív számítási lifecycle

A mai rajz → 3D scope-on belül elkészült a következő életciklus-szelet, három
párhuzamos Sol agenttel. Az SVG/DXF/DWG és a további önálló krly2d munka halasztva.

- Mindhárom natív CLI-mód közös `--timeout` határidőt kap (alapérték 120 s), már
  a bemeneti fájlok olvasása előtt. SIGINT/SIGTERM atomikus jelzését egy figyelőszál
  vezeti át a közös tokenre; a signal handler nem zárol és nem végez I/O-t.
- A [megszakítható Unix I/O](../products/krly3d/reconstruction/host-cli/src/io_control.rs)
  megtelt stdout pipe mellett is ellenőrzi a tokent. Nincs korlátlan eseménysor vagy
  blokkolva hátrahagyott írószál. A bemenet korlátos, darabolt és reguláris fájl;
  FIFO-t nem vár meg. A stderr hibaüzenet is rövid best-effort határidőt kap.
- A [vision controlled út](../products/krlyvis/engine/src/source.rs) hash-, raszter-,
  komponens- és lyukkontúr-ciklusokban ellenőriz. Ugyanazt a képet és providerregisztert
  osztja meg; a kész képforrás/evidence nem tartja meg a futás tokenjét.
- A [planar controlled építő](../products/krly3d/brep/engine/src/planar.rs) path-,
  élpár-, tartalmazási és véges-test ciklusokban, illetve providerhatárokon ellenőriz.
  A végső elfogadás rövid publikálási guardot kap. A kész owner/snapshot független
  a konstrukció tokenjétől; konstrukció és callback-képes destruktor nem fut e lock alatt.
- A runtime a coarse/final B-rep és a single/sheet vision útba is ugyanazt a tokent
  adja. Megmaradt a kész részek és a sheet hibamanifest megtartása, a single
  sikermanifest best-effort visszavonása és az ellentmondó terminális írás tilalma.

**Korlátok:** a külső dekóder, egyes szinkron providerműveletek, allokáció/destruktor
és reguláris fájlrendszeri műveletek még nem keményen megszakíthatók. A B-rep
checkpoint-átadás új document-build cache-t indít, ezért későbbi edit újraépíthet
node-okat. Leálláskor terminális stdout esemény nem garantált; a két kimenet és az
artifactok nem tranzakciósak. Bounded párhuzamos scheduler, teljes memória-/
költségkeret, tartós parancs-replay, refinement és fidelity továbbra is nyitott.
Részletes szerződés: [shared runtime](SHARED-RUNTIME.md#közös-cli-életciklus).

**Ellenőrzés:** a teljes `cargo test --workspace` **1236 runtime + 1 compile-fail
doctest = 1237 sikeres teszt, 115 suite, 0 failed/ignored**; ezúttal a doc fázissal
azonos, megszakítás nélküli futás. A célzott hat package futása 181 runtime + 1
doctesttel szintén sikeres; ezek átfedő ellenőrzések. Szigorú workspace all-target
Clippy, workspace build, fmt és diff-check sikeres. A szerkesztő a vizsgált
integrációs fájlokban nem jelez hibát.

A végső binárisokkal **10 lifecycle**, **11 natív sheet**, **5 stream**, **4
shared/process alapút-paritás**, **14 legacy drawing/sheet** folyamatpróba sikeres.
A hivatalos SDK-val **6 vision** és **3 B-rep** paritáseset sikeres, független natív
modellreload ellenőrzéssel. A lifecycle-próbák előre megtöltött pipe mellett
ellenőrzik mindhárom mód timeoutját és SIGINT/SIGTERM-leállását, továbbá a zárt
olvasót, megtelt stderrt, FIFO-elutasítást, sémákat és manifest-megtartást. A natív
próbák a belső ciklusok leállását, újrapróbálást és megtartott artifactok élettartamát
fedik. Ez kooperatív leállási bizonyíték, nem kemény valós idejű garancia.

**Ticketek:** #24/#26 új, kész lifecycle-részfeladattal és a fennmaradó határokkal
frissítve. **18 nyitott / 9 lezárt**; a szélesebb elfogadás miatt teljes issue most
nem zárható. A mai öt nagy munkablokkból a lifecycle jelentős része elkészült, de
az egész blokk még nyitott. Következő terméklépés: tartós command/input replay és
revízióbiztos refinement; utána a scheduler/budget és a fidelity/teljesítménymérés
fennmaradó elfogadása. Az SVG/DXF/DWG továbbra sem mai feladat.

**Munkamegosztás:** Russell a B-rep kontrollt, Popper a vision kontrollt, Bacon a
valódi folyamatokon futó lifecycle próbákat készítette; az integrátor a CLI-jelkezelést,
I/O-t, runtime-bekötést, összevont ellenőrzést és ticketeket. Mindhárom agent lezárva.
A termékstruktúra megmaradt, nincs új geometriai implementáció. Az aktív process/
provider utak maradnak; nincs kiváltott egész fájl, így új `.tmp/` áthelyezés sincs.
A felhasználói submodule-változások és gitlink megmaradtak. Nincs commit/push.

## 2026-11-14 — Huszadik–huszonegyedik kör: natív sheet és a külön krly2d termék

**Elkészült az első közös 2D szelet és a natív többalkatrészes folyamat.** A felhasználó
jóváhagyta a külön `krly2d` terméknevet és a formátumfüggetlen geometriát. A döntést az
[ADR 0012](decisions/0012-krly2d-planar-domain.md), a használható képességeket a
[krly2d leírás](KRLY2D.md) rögzíti.

- A [krly2d-api](../products/krly2d/api/src/lib.rs) immutable Path/Segment/Region
  szerződése egyenest, kvadratikus/köbös Bézier-görbét és elliptikus ívet tárol,
  explicit model/metre egységgel. Véges koordináták, korlátos szegmens-/lyukszám,
  veszteségmentes poligonkonverzió; görbét nem helyettesít automatikusan húrral.
  A validáció strukturális, a topológiát a fogyasztó vizsgálja.
- A [vision evidence](../products/krlyvis/engine/src/shared.rs) egyszer állítja elő
  a világkoordinátás régiót. A [natív B-rep híd](../products/krly3d/brep/engine/src/planar.rs)
  és a [host modell](../products/krly3d/reconstruction/runtime/src/lib.rs) ugyanazt az
  Arc-ot tartja meg; a régi planar wire-hívás is erre az építőre delegál. A korábbi
  providerek adják a topológiai ellenőrzést és testépítést. Nincs új geometriai solver.
  A mostani B-rep híd csak egyenes, model-unit profilokat épít; a görbe/metre
  bemenet explicit hibát ad. SVG/DWG-import még nincs.
- A [DrawingSource](../products/krlyvis/engine/src/source.rs) egyszer dekódol, majd
  az eredeti kép Arc-ját használja minden kijelöléshez. A [Sheet](../products/krly3d/reconstruction/runtime/src/sheet.rs)
  1–16 részt kezel közös deadline-nal, eredeti rajzkoordinátákkal és lyukakkal.
  Azonos komponens második kiválasztását a második evidence előtt elutasítja.
  Hiba/cancel után a kész részek megmaradnak; további részek pendingek, nincs rejtett retry.
- A [host CLI](../products/krly3d/reconstruction/host-cli/src/sheet.rs) `--parts`
  módja alkatrészenként öt szakaszt streamel, megtartott atomikusan cserélt
  manifestet, per-part artifactokat és újrafuttatható kép/parts bemenetet ír.
  `--timeout` sheet módban a teljes futásra közös; output kötelező és új könyvtár.
  A szigorú parts JSON parser a serde-t és a runtime közös szemantikai validációját
  használja. V1/v2 single kompatibilitás megmaradt.

**Ellenőrzés:** a teljes workspace **1212 futótesztje, 88 suite** sikeres, 0 failed/ignored.
A doc fázist közös buildkönyvtárban párhuzamos Cargo-futtatás közben E0463 artifact-hiba
szakította meg. Elkülönített `cargo test --workspace --doc` újrafuttatás: **25 suite,
1 compile-fail doctest, 0 hiba**. Az időközben két szélsőérték-próbával bővült krly2d
szerződés külön végső futása **13/13** sikeres; az új dependency gate külön **10/10**.
Ezek nem összeadandó futásszámok. Szigorú workspace all-target Clippy, workspace build,
fmt és diff-check sikeres; a szerkesztő a vizsgált integrációs fájlokban nem jelez hibát.

A végső binárisokkal **11 natív sheet**, **5 stream**, **4 shared/process alapút-paritás**,
**14 legacy drawing/sheet** folyamatpróba sikeres. A hivatalos SDK-val **6 vision
CLI/MCP/legacy** és **3 B-rep CLI/MCP + független modellreload** paritáseset is sikeres.
A sheet-próbák a térfogatot, lyukakat, elhelyezést, azonos preview/likeness forrást,
részleges hibát, no-overwrite-ot, hibás bemenetet, broken stdoutot és az eredeti
inputok törlése után mentett bemenetből újrafuttatást ellenőrzik. A natív próbák
Arc-identitást, kontrollált deadline/cancelt, callbacksorrendet és írás/flush hibákat
is fednek. Smoke időadatokból általános teljesítményelőnyt nem állítunk.

**Munkamegosztás:** Newton (Sol) az újrahasználható képforrást, Plato (Sol) a natív
sheet CLI-t és valós folyamatpróbákat, Pasteur (Sol) a krly2d szerződést, Averroes
(Luna) a döntésdokumentumokat készítette. Az integrátor a sheet runtime-ot,
vision/B-rep/host régióbekötést, dependency/metadata kapukat és az összevont
ellenőrzést végezte. Agentek lezárva.

**Ticketek:** #24 natív sheet-részfeladata kész; #26 a krly2d határral és a sheet
állapottal frissítve. A [#27](https://github.com/krlybnd/krlyxd/issues/27) a külön
krly2d termék, bounded SVG-import és következő műveletek követője. **18 nyitott / 9 lezárt**;
a tágabb #24/#26 elfogadása még nem teljes, ezért teljes issue-lezárás nincs.

**Következő lépések és korlátok:** bounded SVG-import tényleges közös fogyasztóval;
görbeprofilok adaptere a meglévő providerekre; teljes CLI-jelkezelés és megszakítható
output/vision/planar lifecycle; tartós szerkesztési replay/refinement. A natív sheet
soros, nem párhuzamos alkatrész-scheduler. A kimenetek együtt nem tranzakciósak,
crash-resume nincs. A vision a régi datumot és a régiót is tartja; B-rep konstrukciónál
ideiglenes poligon/profilszöveg készül, ezért nincs általános zero-copy állítás.
Az aktív Python/provider utak megmaradnak; nincs új archiválandó egész fájl.
A felhasználói submodule-változások és a gitlink megmaradtak. Nincs commit/push.

## 2026-11-14 — Tizenkilencedik kör: célzott cleanup a sheet-bővítés előtt

A felhasználó kérésére az új host/adapter útvonalat átnéztük. A tíz meglévő
architektúra/dependency/one-copy ellenőrzés sikeres: a doménhatárok és a matematikai
implementációk egyszerisége megmaradt. A konkrét rendezés az adapter és a host control
határán történt; nincs új generikus keretrendszer vagy geometriai implementáció.

- A [CLI belépő](../products/krly3d/reconstruction/host-cli/src/main.rs) már csak
  vezényel. Az argumentumok és bounded képfájl-betöltés az
  [options](../products/krly3d/reconstruction/host-cli/src/options.rs), a közös
  artifact/provenance/feltételezés/manifest adatok az
  [output](../products/krly3d/reconstruction/host-cli/src/output.rs) modulhoz kerültek.
  A két protokollváltozat ugyanabból a közös metaadatból épül.
- Javítottuk a hibás végállapotot: a completed írásának/flushának hibája után nem
  maradhat szándékosan sikermanifest és nem készülhet ellentmondó failed kísérlet.
  A sikermanifest visszavonása best-effort; törlési hiba külön jelzett. Ismert
  journal/stdout hiba után nincs további írás. Hat injektált íróteszt ellenőrzi
  a számítási, korai output és mindkét terminális write/flush hibahatárt.
- `revise_controlled` és `revise_if_current_controlled`: a későbbi módosítások és
  javaslatok is caller-owned megszakítás/deadline tokent kapnak. Az admissionkor
  lejárt kérés nem invalidálhatja a kész állapotot, és felszabadítja a slotot.
  A változatlan evidence/B-rep újrahasználata megmarad.

**Ellenőrzés:** 30 runtime + 6 CLI + 10 architektúrateszt = **46 célzott Rust-teszt**,
0 hiba. Öt stream smoke, négy shared/process alapút-paritáseset és a refaktor előtti/
utáni v1/v2 teljes stdout/artifact-összehasonlítás sikeres. Utóbbiból csak az időadatokat
és a folyamatlokális azonosítókat normalizáltuk; a PNG/k3d és a többi adat megegyezett.
Workspace all-target Clippy, fmt, diff-check és a host CLI build sikeres. A teljes
workspace-tesztcsomagot most nem futtattuk újra; az előző 1168-as adat történeti baseline.

Carver (Sol high) a CLI rendezését, Linnaeus (Luna high) a valódi broken-pipe próbát,
az integrátor a controlled edit API-t és az összevont ellenőrzést végezte. Agentek lezárva.
#24/#26 követése frissítve; **17 nyitott / 9 lezárt**, új teljes lezárás nincs.
A Python sheet/process út még használatban van, nincs kiváltott egész fájl, ezért nincs
új `.tmp/` áthelyezés. A következő funkcionális blokk továbbra is natív sheet/részleges
hibák, majd a teljes CLI és belső vision/planar lifecycle. A jelenlegi kétkimenetes
kézbesítés nem tranzakciós; a blokkoló író és az OS-crash továbbra is külön korlát.
A felhasználói submodule-változások megmaradtak. Nincs commit/push.

## 2026-11-14 — Tizennyolcadik kör: három agent, natív hétlépcsős folyamat

**Elkészült:** a `krly3d-reconstruct --stream` egy folyamatban végzi az evidence →
korai befoglaló mesh → szerkeszthető kontúrmodell → PNG → állomás-likeness lépéseket.
Hét flusholt NDJSON esemény készül, logikai artifact-revízióval, stage/elapsed idővel,
image-hash-sel és megtartott snapshot-provenance-szel. Az alapértelmezett manifest-only
mód kompatibilis maradt. Részletek és használat: [SHARED-RUNTIME](SHARED-RUNTIME.md).

- Hume / Sol high: közvetlen snapshot-render a meglévő render-úttal; külön korai
  coarse lépés. A végleges modell csak az előnézet kiadása után készül.
- Socrates / Sol high: bounded typed raster-evidence/likeness API és natív bridge a
  meglévő station providerhez. Nincs új összehasonlító vagy geometriai algoritmus.
- Turing / Luna high: stream smoke és natív review/control ellenőrzés.
- Integrátor: közös host stop/deadline token, review/publikálási állapot, stream CLI,
  dependency-kapuk, integrációs javítások, dokumentáció és ticketek. Agentek lezárva.

**Ellenőrzés:** `cargo test --workspace`: **1167 runtime + 1 compile-fail doctest =
1168 sikeres teszt**, 107 suite, 0 failed/ignored. Szigorú workspace all-target Clippy,
fmt, diff-check és workspace build sikeres. A végső coarse-getter és teszthelperek
módosítása után az érintett runtime/coarse tesztek külön is sikeresek. Négy új stream
smoke (négy PNG/PNM ring/konkáv geometriai eset, invalid input/no-overwrite, terminal
failure, régi alapmód), négy shared/process alapút-paritáseset, 14 drawing/sheet
folyamatpróba, hat hivatalos SDK view CLI/MCP PNG-paritáseset és v2 lifecycle/séma
ellenőrzések sikeresek. Egy gyűrűmodell új PNG-je vizuálisan is ellenőrizve.

**Korlátok:** likeness csak mintavett külső szakaszhatárok egyezése egy pixelnyi
toleranciával; nem lyuk/topológia vagy fizikai pontosság bizonyítása. A vision-decode és
planar konstrukció belsejében még nincs kooperatív stop. A natív Rust host közös
CallControl-t használ; az új CLI-nek még nincs timeout/cancel kapcsolója vagy
megszakítható írója. A fájl/eseménynapló nem tartós command replay és nem tranzakciós
output. Sheet, teljes lifecycle, általános refinement és fidelity továbbra is nyitott.

**Ticketek:** #24 korai preview/render/likeness részfeladata és #26 natív
snapshot/likeness határa készre jelölve; a szélesebb ticketek nyitva maradnak.
**17 nyitott / 9 lezárt**. A következő blokk a natív sheet/részleges hibák és teljes
CLI-életciklus. A meglévő process/sheet út aktív, nincs új archiválás. A megbeszélt
termékstruktúra és a felhasználó submodule-változásai megmaradtak. Nincs commit/push.

## Ticketaudit a shared-runtime átállás után

A GitHub élő állapota ellenőrizve: **17 nyitott / 9 lezárt**. A #13/#24/#26 ticketekbe bekerült a tizenhetedik kör végső ellenőrzése és a sikeres mesh-cache korlátos tárolása; #24/#26 a teljes helyi rajzfolyamat következő lépéseit is rögzíti. Új tesztfutás nem történt ebben az auditban. Új teljes lezárás nem indokolt: a fennmaradó elfogadási feltételek nyitva maradnak. A közvetlen következő blokk #24 és az azt kiszolgáló #26; a ticketek száma nagyobb munkacsomagokat számol, nem azonos méretű fejlesztési lépéseket.

## 2026-11-14 — Tizenhetedik kör: közös memóriás helyi futtatás

A felhasználó elfogadta a shared-memory irányt. Az [ADR 0011](decisions/0011-shared-runtime.md) és az [átállási terv](SHARED-RUNTIME.md) szerint a helyi belső út típusos Rust API-kon fut egy folyamatban, immutable megosztott artifactokkal. A CLI/MCP külső adapter; az MCP-only belső orchestration követelmény megszűnt. A CTM továbbra is assembly/kényszerek gazdája, a raw kernel doménfüggetlen marad.

**Megvalósult:**

- `krlyvis` saját API/engine/CLI/MCP drawing művelet, computed image SHA-256, placed contour/lyukak, explicit unknown uncertainty. A natív `read_shared` shared datumot tart; JSON csak a wire adapterben készül.
- B-rep `build_planar_retained`, megőrzött owner/snapshot, explicit `to_k3d` export. A korábbi planar wire adapter erre delegál, viselkedése megmarad.
- [Reconstruction runtime](../products/krly3d/reconstruction/runtime/src/lib.rs): rajz → evidence → retained B-rep → mesh közvetlenül. Új kép minden leszármazottat invalidál; magasság csak B-rep/mesh-t; tolerancia megtartja a B-rep-et. Host/revízió alapján visszautasítja az idegen és elavult eredményt. Revízióhoz kötött paraméterjavaslat sem írhatja felül az újabb felhasználói állapotot.
- Legfeljebb négy outstanding job/result, 256 kompakt esemény és explicit gap; a worker immutable bemenettel dolgozik, host-lock nélkül. A caller által tartott régi handle-ek megmaradnak. A sikeres mesh-cache review során talált korlátlan toleranciahalmozást a tulajdonos doménben javítottuk: testenként nyolc FIFO bejegyzés, konkurens deduplikáció, olvasók és cancellation megőrzése. Ez nem teljes RAM-kvóta.
- [Host CLI](../products/krly3d/reconstruction/host-cli/src/main.rs): `krly3d-reconstruct`, evidence/k3d/mesh/manifest/eseményfájlok. A geometria és a mesh között nincs köztes natív mentés–visszatöltés. Az export a CLI végén történik.
- A félbemaradt peer-munka integrálva: lokális/peer vegyes/nested batch, kompatibilis hibák, 32 szintes mélységkorlát, 4 MiB/32768 string keretes interning. A remote blokkoló RPC és in-flight cancellation továbbra is #16.

**Ellenőrzés:** `cargo test --workspace`: **1134 passed (1133 runtime + 1 compile-fail doctest), 103 suite, 0 failed/ignored** a cache-javítás után. Szigorú workspace all-target Clippy, fmt és diff-check sikeres. **14 valós rajz-/sheet folyamatpróba** (25,348 s), **4 azonos inputos shared/process paritáseset** (lyukas és konkáv kép, PNM/PNG), **6 hivatalos SDK vision CLI/MCP/legacy paritáseset**, **3 B-rep CLI/MCP paritáseset** és független modell-visszatöltés sikeres. Az új host tesztjei valódi szálon végzett számítást, Arc-identitást, fordított befejezési sorrendet, stale proposal/foreign host védelmet, hibát/admission felszabadulást és event gap-et fednek. A cache-próba száz tolerancián igazolja a régi natív mesh-allokációk felszabadulását. A szerkesztő nem jelez hibát.

A négy debug összehasonlítás helyi CLI falideje 7,448–11,596 ms, három külön domain CLI összege 25,154–28,846 ms volt. Az adapter-I/O különbözik; ez kis smoke adat, nem általános gyorsulási arány vagy teljesítményígéret. A process határpayload 4797–6010 byte, a helyi belső transport 0; ez nem heap/RSS mérés, a natív projection/mesh/export továbbra is allokál.

**Ticketek:** #5/#15/#16/#22/#24/#26 tartalma átírva, #16/#24/#26 címe is az új szerepet tükrözi; visszaolvasással ellenőrizve. **17 nyitott / 9 lezárt**, új teljes lezárás nincs: a szélesebb elfogadási feltételek megmaradnak. #24-ből a belső MCP-only feltétel kikerült, helyette a teljes helyi host-migráció és lifecycle/refinement szerepel.

**Következő blokk:** a teljes korai coarse preview/render/likeness/sheet folyamat bekötése a hostba, közös deadline/cancellation és tartós command/replay; majd AI proposal/watch/PMI (#15), CTM capability/assembly (#22), multiview/sections/fidelity (#5). A helyi CLI most a számítás után exportál, nincs teljes seven-stage stream, PNG vagy likeness. A synchronous image decode és egyszerű job még nem scheduler/remote lifecycle. A process/sheet pilot aktív marad; nincs új egészfájlos `.tmp` áthelyezés.

**Munkamegosztás:** Hilbert és Helmholtz Sol high: typed vision és retained B-rep; Helmholtz célzott review/cache-javítás; Averroes Luna high: kis peer hibaparitás-javítás. Integrátor: host/CLI, közös Cargo/metadata/dependency kapuk, valós parity, regressziók, dokumentáció és GitHub-issue-k. Minden worker lezárt. A több korábbi körös munkafa és a felhasználói `.ctx/ctxe` változások megőrizve. Nincs commit vagy push.

## 2026-11-14 — Tizenhatodik kör: valódi B-rep API–CLI–MCP fogyasztó

Elkészült a [B-rep planar szerződés](BREP-PLANAR.md) és az [ADR 0010](decisions/0010-brep-planar-contract.md). A meglévő B-rep API/engine mellé önálló CLI/MCP adapter került a megbeszélt termékstruktúrában. Egy stateless kérés külső/belső kontúrokból és vastagságból szerkeszthető natív dokumentumot ad; a motor a meglévő profile/extrude/meet/sweep/trim providereket használja. Hibás, összeérő vagy átfedő kontúrok elutasítása és korlátos, szigorú bemenet; nincs új geometriai algoritmusmásolat.

A rajz- és többalkatrészes folyamat már ténylegesen a `brep_planar` MCP toolon készíti a végső modellt. Megőrzi a domain request/result párt és a kontúrbizonyíték digestjét. A visszaadott dokumentumot tölti újra a mesh/view/legacy likeness ellenőrzés. A coarse box és a krlyvis/likeness még a régi facade-on marad; teljes MCP-migrációt nem állítunk. Az új B-rep művelet szinkron, nincs távoli cancel/operation lifecycle; a folyamat deadline-ja és process-group cleanupja védi a fogyasztót. Ismeretlen mérési bizonytalanság/confidence marad ismeretlen.

**Munkamegosztás:** Euler (Sol high): API/engine és geometriai ellenőrzések; Banach (Sol high): CLI/MCP adapterek és protokolltesztek; integrátor: valódi MCP-fogyasztó, SDK/paritás, függőségi/metadata kapu, dokumentáció és issue. A korábbi munkafa és felhasználói submodule-változások megőrizve; commit/push nincs. Aktív provider továbbra sem elavult, új egész fájlos archiválás nincs.

**Ellenőrzés:** `cargo test --workspace`: **1054 runtime + 1 compile-fail doctest = 1055 passed**, 87 suite, 0 failed/ignored. Szigorú workspace all-target Clippy, fmt és diff-check sikeres. A hivatalos MCP SDK három CLI/MCP schema/model/paritásesete és független térfogat/zártság reload sikeres; a végleges binárisokkal mind a 14 rajz-/sheet folyamatpróba sikeres (25,626 s). A függőségi kapu tiltja a közvetlen geometriai importot a B-rep adapterekben; a központi metadata kapu az új szerződést is vizsgálja. A külön debug demó korai mesh-e 83,0 ms, szerkeszthető javaslata 116,1 ms, teljes ideje 631,2 ms; térfogat 1464 a várt 1464 mellett, nyitott élek 0. PNG vizuálisan ellenőrizve; ezek kis smoke időadatok, nem általános teljesítmény-elfogadás.

**Issue:** #24/#26 kész B-rep planar/MCP részfeladattal frissítve. 17 nyitott / 9 lezárt; a tágabb migráció/refinement nyitva marad. Mindkét worker befejezett, nincs aktív agent.


## 2026-11-14 — Ticketaudit a tizenötödik kör után

A GitHubon #24, #26, #5, #13 és #15 aktuális állapotra rendezve. #24/#26 korábbi körönkénti állapotleírásai helyett kész/hátralévő ellenőrzőlista, bizonyíték és korlátok szerepelnek; a történeti futási eredmények ebben a naplóban megmaradnak. #5 különválasztja a kész síkbeli kontúr-/lyukevidenciát a hiányzó többnézetes/section rekonstrukciótól. #13 rögzíti a már meglévő Rust-only source-face/corner-normal preview szerződést és a hiányzó wire/exchange részt. #15 nem téveszti össze a pilot fájljavaslatait a még hiányzó rendszerszintű proposal/PMI szerződéssel.

**Állapot:** 17 nyitott / 9 lezárt; új ticket és teljes lezárás nincs. A kész részek jelölése nem szűkíti visszamenőleg az eredeti elfogadási feltételeket. A következő összefüggő fejlesztési blokk a #24 által ténylegesen használt krlyvis/B-rep műveletek doméntulajdonú API–CLI–MCP szerződése (#26), meglévő viselkedésparitással; utána indokolt refinement. A tesztfixture-ek név szerinti korábbi elvetése változatlan.

Ez ticket-/dokumentációaudit: új runtime kód és új tesztfutás nincs. A legutóbbi 14 folyamatpróba és a korábbi Rust baseline történeti eredmény marad. A GitHub issue-módosítások publikáltak; a kód/dokumentáció továbbra is helyi, commit/push nélkül.


## 2026-11-14 — Tizenötödik kör: többalkatrészes rajzcsomag

A felhasználó nagyobb fejlesztési lépést kért. Elkészült a [többalkatrészes sheet runner](../products/krly3d/reconstruction/cli/SHEET.md): egy képből explicit seed/név/vastagság alapján 1–16 külön szerkeszthető javaslat, alkatrészenként korai mesh/PNG/lyukmegőrzés/anyagellenőrzés és közös atomikusan frissített eredményjegyzék. Külön kijelölés a single flow-ban is használható.

A folyamat egy teljes időkerettel és soros backpressure-rel fut; hibánál a kész alkatrészek megmaradnak, a továbbiak nem kapnak hamis completed állapotot. A manifest path traversal/duplikáció/hibás számérték ellenőrzött; ugyanaz a komponens más névvel sem kerül csendben kétszer átadásra. Nem automatikus alkatrészfelismerés és nem assembly-kényszermegoldás.

**Munkamegosztás:** integrátor: futtató, seed integráció, dokumentáció/issue; Banach (Sol high): külön valós sheet folyamatpróbák. A nagyobb korábbi munkafa változatlanul megőrizve, nincs commit/push. **Ellenőrzés:** 14 valós folyamatpróba egyben sikeres (58,036 s): 4 single-flow + 10 sheet teszt, köztük különböző koordináták/vastagságok/lyukak, hibás input, felülírás tiltása, dupla komponens, részleges hiba, közös deadline és SIGINT/SIGTERM utáni leszármazott-takarítás. Python fordítás, szerkesztődiagnosztika és diff-check rendben. Rust nem változott ebben a körben, teljes workspace nem futott újra. Külön kétalkatrészes debug demo: 2688,1 ms, mindkét alkatrész egyezik; keret térfogata 1536, L elemé 1560; mindkét PNG vizuálisan ellenőrizve. Ez smoke időadat, nem általános teljesítmény-elfogadás. Banach befejezett, nincs aktív worker. #24 frissítve; 17 nyitott / 9 lezárt issue, teljes issue-lezárás most nincs.


## 2026-11-14 — Tizennegyedik kör: lyukak megőrzése a rajzfolyamatban

A [rajzpilot](../products/krly3d/reconstruction/cli/README.md) már az opt-in `trace-holes` krlyvis plugint használja. A kiválasztott összefüggő anyag külső és belső pixelkontúrjai profilként, majd egyenletes extrúzióként épülnek fel. A régi alapértelmezett `trace` változatlanul kitölti a lyukakat. Sarokban összeérő bizonytalan kontúr hibával leáll; nincs topológiát módosító egyszerűsítés.

A krlyvis területmérése levonja a lyukak területét, a placed evidence `material_area` értéket kap. A pilot a mentett modell újratöltésével külön ellenőrzi a nettó terület × vastagság térfogatot és a mesh zártságát. A likeness továbbra is csak külső állomáskülönbséget mér; nem állítunk teljes topológiai vagy rejtettgeometria-bizonyítást.

**Ellenőrzés:** 80 célzott Rust-teszt sikeres (krlyvis kernel/std/CLI, 9 suite), 4 teljes folyamatpróba sikeres, köztük külön exact-ray felülnézet a lyukon átlátás ellenőrzésére. A 13 hívásos régi peer replay változatlanul sikeres. Célzott all-target Clippy, fmt és diff-check rendben. A külön demo térfogata 1872,0, várt térfogata 1872,0, nyitott mesh-élek száma 0; a PNG vizuálisan ellenőrizve. A teljes workspace nem futott újra; az 1012-es korábbi baseline történeti adat.

**Issue:** #24 frissítve; 17 nyitott / 9 lezárt, teljes issue-t most nem zárunk. A módosítások az aktív krlyvis providerben és az orchestration CLI-ben maradnak; új geometriamásolat vagy elavult egész fájl nincs. #24 teljes MCP/refinement scope-ja nyitva marad. Commit/push nincs.


## 2026-11-14 — Tizenharmadik kör: a termékfolyamat első kipróbálható változata

A felhasználóval egyeztetett irány a [tervben](MULTI-AGENT-DEVELOPMENT-PLAN.md) rögzítve: a következő mérföldkő teljes rajz → geometriai javaslat → előnézet → összehasonlítás; infrastruktúra csak ennek konkrét hiányaihoz. A teszt-/issue-szám nem készültségi százalék. Git-mentési pontok tervezve, automatikus összevont commit/push nincs.

Elkészült a [síkbeli rajzpilot](../products/krly3d/reconstruction/cli/README.md). A CLI valódi domainfolyamatokat vezényel: krlyvis kontúrbizonyíték, korai befoglaló mesh, kontúrból szerkeszthető profil/extrúzió, PNG és állomásriport. A korai esemény a profil létrehozása előtt jelenik meg. Stabil logikai artifact ID/revízió, explicit scale/height/rejtett felület feltételezések, stage/total timing és visszajátszható fájlok készülnek. A végső modellből a durva befoglaló rollbackkel kikerül.

**Fontos feltárt korlát:** a krlyvis kitölti a kiválasztott összefüggő alakzat lyukait. Ezt a pilot explicit közli; a sikeres külsőkontúr-egyezés nem igazol lyukakat, más komponenseket vagy rejtett geometriát. Nem állítunk általános képből 3D rekonstrukciót.

**Ellenőrzés:** négy valós folyamatpróba: konkáv L kontúr és visszatöltött vízzáró térfogat; felhasználói kép/méretarány/vastagság; lyukkitöltési korlát és timeout; SIGINT. A teszt a felülírás tiltását, a korai eseménysorrendet és a pinned provenance-t is ellenőrzi. **Mind a négy próba sikeres (3,913 s).** Külön végső debug demo: korai mesh 76,3 ms; szerkeszthető javaslat 82,8 ms; teljes folyamat 588,1 ms, `matches=true`. A PNG vizuálisan ellenőrizve: konkáv L alak. Ez kis demo smoke, nem általános teljesítmény-elfogadás. Rust kód nem változott; az előző teljes 1012 tesztes baseline érvényes, nem újrafuttatott eredmény.

**Issue-k:** #24 részfeladat és korlátok frissítve; **17 nyitott / 9 lezárt**, új teljes issue-lezárás nincs.

**Scope:** #24 részleges pilot, az MCP-only orchestration, általános refinement és gazdagabb rekonstrukció hátravan. #26 migrációs bridge marad. Aktív régi fájl nem elavult; nincs új `.tmp` áthelyezés. Módosítások helyiek, commit/push nélkül.


## 2026-11-14 — Tizenkettedik kör: önálló view művelet-életciklus

Elkészült a [view v2 műveleti szerződés](VIEW-OPERATIONS.md), az [ADR 0009](decisions/0009-view-operations.md) szerint a meglévő `products/krly3d/view/{api,renderer,cli,mcp}` struktúrában.

- Szigorú v2 codec/descriptor, rögzített snapshotból leválasztott render, korlátos worker/eseménytár; Started és pontosan egy végállapot.
- Határidő, megszakítás, explicit felszabadítás; a megszakított worker tényleges kilépéséig megtartja kapacitását. Runner eldobásakor kooperatív stop, blokkoló join nélkül.
- CLI `--operations` NDJSON és `--describe-operations`; MCP `view_start/next/cancel/release`, számítás közben működő ping, EOF/read/write hibára stop. A szinkron v1 változatlan.
- Hivatalos SDK: hat v1 PNG-paritáseset, két v2 CLI/MCP paritáseset, schema, deadline, cancel, release/reuse és valódi folyamat EOF sikeres. A régi eszközlistateszt egy elem helyett ötöt vár; a geometriai viselkedés nem változott.

**Ellenőrzés:** `cargo test --workspace`: **1011 runtime teszt + 1 compile-fail doctest = 1012 passed**, 0 failed/ignored, 79 suite. Szigorú workspace all-target Clippy, fmt és diff-check sikeres. Nyolc új Rust teszt; a szigorított codec-teszt külön végső futása is sikeres.

**Issue-k:** #26 standalone view-életciklus részfeladata készre jelölve, a további scope külön nyitva. **17 nyitott / 9 lezárt issue**, ebben a körben nincs teljes issue-lezárás.

**Korlátok:** nincs MCP cancellation-notification binding vagy progress/partial image; az output backpressure továbbra is blokkolhatja az olvasást. A legacy `view_render` továbbra is szinkron. #26 szélesebb scene/paint/pick/likeness és doménmigrációja nyitott. Aktív legacy provider nem elavult; nincs új archiválás. A korábbi worker-kvótakorlát után ezt a kört az integrátor végzi. Nincs commit/push.


## 2026-11-14 — Tizenegyedik kör: három önálló task párhuzamosan

A felhasználó kérésére három külön issue készült el, párhuzamos munkával és integrátori befejezéssel.

- **#20 / Integrátor — elkészült, lezárva:** [raw alapréteg-szerződés](RAW-KERNEL.md), [ADR 0008](decisions/0008-raw-foundation-contract.md), közös koordináta/confidence/bizonytalanság és producer-metaadat, központi Cargo/descriptor validáció, explicit doméntulajdon az Accuracy/frame/NURBS kódhoz. **161 célzott teszt + szigorú all-target Clippy sikeres.** A mesh/view wire forma változatlan; raw nem kapott új függőséget.
- **#3 / Archimedes, Sol high + integrátor — elkészült, lezárva:** [tartalomalapú decline memo](MESH-DECLINE-CACHE.md), teljes geometria/config/API/solver kulcs, NURBS effektív állapot, ismeretlen adatok biztonságos kihagyása, bounded eviction/context-isolation és valós triangulációs bizonyíték. 21 célzott mesh teszt és az utólag hozzáadott eviction/context teszt sikeres.
- **#7 / Sartre, Sol high + integrátor — elkészült, lezárva:** [likeness állomásbizonyíték](LIKENESS-STATIONS.md), forgatott lokális u/v tengelyek, explicit lépcsőtűrés, bizonyítékhoz kötött fractional kontúrkoordináták és állomásonkénti `matches`. Kilenc geometriai teszt és 13 hívásos valódi krlyvis peer replay sikeres; a pixelből származó kontúr nem állít subpixel mérési pontosságot.

**Issue állapot: 17 nyitott, 9 lezárt.** Ebben a körben #3, #7 és #20 lezárva; #26 frissítve, a további migráció nyitott.

**Végső ellenőrzés:** `cargo test --workspace`: **1003 runtime teszt + 1 compile-fail doctest = 1004 passed**, 0 failed/ignored, 77 suite. Szigorú workspace all-target Clippy, fmt és diff-check sikeres. Következő nagyobb szelet: #26 kontrollált view művelet-életciklus és a fogyasztók fokozatos átállítása.

**Átvétel:** mindkét worker kvótakorlátnál leállt. A részleges módosításokat az integrátor átvette, átnézte, a cache eviction/context teszttel kiegészítette és a valódi peer/SDK folyamatokat lefuttatta. Az agentek lezárva; nincs háttérben dolgozó worker. A teljes Clippy és a hat view / négy mesh SDK-paritáseset sikeres, 8/8 legacy JSON-RPC válasz változatlan. Minden változás helyi, commit/push nélkül. Aktív legacy provider nem kerül félre; a [.tmp](../.tmp/README.md) archívum megmarad.


## 2026-11-14 — Tizedik kör: view wire és önálló CLI/MCP

Elkészült a [szinkron view wire 1.0](VIEW-ADAPTERS.md), a [CLI](../products/krly3d/view/cli) és az [MCP](../products/krly3d/view/mcp), a megbeszélt termékstruktúrában. A döntést az [ADR 0007](decisions/0007-view-wire-adapters.md) rögzíti.

- **Kierkegaard / Sol high:** önálló CLI, korlátos UTF-8 modell/request bemenet, egy kanonikus JSON-válasz, folyamat- és valódi PNG-paritástesztek.
- **Harvey / Sol high:** MCP inicializálás, egy read-only `view_render` tool, descriptorból származó séma, kanonikus eredmény + MCP PNG, szigorú JSON-RPC feldolgozás és korlátos stdio.
- **Integrátor:** közös codec/descriptor, ismeretlen és duplikált mezők, verziók, számok és kamerák validációja; PNG/base64 eredmény provenance/minőség/idő adatokkal; függőségi kapu, hivatalos SDK-paritás, dokumentáció és issue-k.

**Ellenőrzés:** `cargo test --workspace`: **975 runtime teszt + 1 compile-fail doctest = 976 passed**, 0 failed/ignored, 74 suite. Szigorú workspace Clippy, fmt és diff-check sikeres. Friss binárisokkal 8/8 legacy JSON-RPC válasz változatlan; a mesh hivatalos SDK smoke négy geometriai paritásesete és lifecycle/schema/error ellenőrzései is sikeresek. A hivatalos MCP SDK hat ray/mesh × kamera esetben azonos CLI/MCP PNG-t és metaadatot igazolt (csak session-azonosság/idő normalizált), sémaellenőrzéssel, PNG CRC/dekompresszióval és hibák utáni újrafelhasználással. A smoke kezdeti SDK-mezőnév eltérése javítva (`input_schema`, `is_error`, `mime_type`); a javított futás sikeres.

**Korlátok:** a wire v1 szinkron, nincs műveletazonosító/esemény, wire timeout vagy MCP cancel. Renderelés/kiírás alatt az MCP feldolgozás vár; EOF nem szakítja meg kooperatívan a futó rendert. A Rust kontroll megmarad. A valódi kontrollált view életciklus, multi-node/paint/pick/likeness és a legacy konverziós bridge további kiváltása #26-ban nyitva marad. A két adapter közvetlenül csak a view API/renderer helyi csomagokat fogyaszthatja; nincs új geometriai algoritmusmásolat.

**Issue és munkafa:** #26 kész szinkron adapter-szelete és hátralévő scope-ja külön jelölve. **20 nyitott, 6 lezárt issue**, új teljes issue-lezárás nem indokolt. Mindkét worker befejezett és leállt. Nincs commit/push. A `.ctx/ctxe` user változásai és gitlink-pin érintetlenek. Egész fájl most sem vált elavulttá; az aktív legacy provider marad, a [.tmp archívum](../.tmp/README.md) megőrizve.


## 2026-11-14 — Kilencedik kör: typed view API és renderer

Elkészült az [önálló Rust view API és renderer](VIEW-API.md), az [ADR 0006](decisions/0006-typed-view-renderer.md) szerint a `products/krly3d/view/{api,renderer}` csomagokban. A régi `krly3d-view` facade aktív marad; az új API neve `krly3d-view-api`, az orchestration csomagé `krly3d-view-renderer`.

- **Integrátor:** engine-független API 1.0 típusok, request/verzió/méret/mértékegység/kamera validáció, PNG/face coverage/minőség/provenance/idő eredmény. Öt validációs teszt, közös named-camera és kooperatív PNG helper, függőségi kapu, futtatható példa és dokumentáció.
- **Singer / Sol high:** `ViewEngine`/`PreparedRender` egy rögzített B-rep forrásra. Név szerinti, fitted ortho és pinhole kamera, exact ray és mesh backend az eredeti algoritmusokkal. Másik rendererhez vagy lecserélt B-rep ownerhez tartozó prepared hívás elutasítva; szerkesztés/rollback nem cseréli ki a rögzített geometriát.
- Nyolc integrációs teszt: PNG- és láthatófelület-paritás a legacy render/view lekérdezésekkel mindkét backend és kameramód mellett, valódi PNG-dekódolás, verzió/ownership, hibás bemenet, render/tally/PNG/final-publication közbeni deadline/cancel és tiszta újrapróbálás. Az injektált órával a render- és encoding-idő külön ellenőrzött; visszafelé mozduló óra negatív idő helyett nullát ad.

**Ellenőrzés:** a végső workspace futás **938 teszt + 1 compile-fail doctest**, összesen **939 passed, 0 failed, 0 ignored**, 67 suite; 13 új teszt a nyolcadik körhöz képest. Szigorú workspace Clippy, fmt és diff-check sikeres. Friss binárissal 8/8 legacy JSON-RPC válasz változatlan; hivatalos MCP SDK négy geometriai paritásesete és meglévő schema/error/deadline/cancel/release/session-reuse ellenőrzései sikeresek. Ez utóbbi a meglévő mesh adaptereket ellenőrzi, nem új view wire adaptert.

A [render_snapshot példa](../products/krly3d/view/renderer/examples/render_snapshot.rs) a tényleges `models/plate-with-hole.k3d` modell `part@3` verziójáról mesh módban 256×192 PNG-t készített, 147726 bájttal és 7 látható face-szel. Ez debug buildes használati smoke, nem teljesítmény-elfogadás. A kimenet ideiglenes ellenőrző fájl; a futtatási parancs a [view API leírásában](VIEW-API.md) található.

**Korlátok és issue:** ez lokális Rust szerződés, egy node/render, nem új JSON protokoll vagy CLI/MCP adapter. Multi-node scene, paint, pick/likeness, wire descriptor/codec, standalone adapterek és progress/partial artifact még hátravannak. Mindkét kép sampled; a mesh requested tolerance nem elért hibakorlát, confidence/uncertainty ismeretlen. A #26 kész részfeladata és megmaradt scope-ja frissítve; **20 issue nyitott, 6 lezárt**, új lezárás nincs.

A worker befejezett és leállt. A raw réteg nem kapott view/domain függőséget, az API csak raw és publikus B-rep identity típusoktól függ; a renderer adapterimportját dependency gate tiltja. A közös kamera- és PNG-implementáció a legacy csomagban marad, új algoritmusmásolat nélkül. Egész fájl nem vált elavulttá; a [.tmp archívum](../.tmp/README.md) megmarad. Nincs commit/push; a `.ctx/ctxe` felhasználói változásai és gitlink-pin érintetlenek. Következő szelet: a view wire szerződés és önálló adapterek, a snapshot/konverziós bridge további kiváltásával.


## 2026-11-14 — Nyolcadik kör: B-rep snapshot ownership

A [B-rep snapshot-határ](BREP-SNAPSHOTS.md) a [0005-ös döntés](decisions/0005-brep-snapshot-boundary.md) szerint elkészült és a mesh fogyasztó átállt rá. Az új csomagok: [B-rep API](../products/krly3d/brep/api) és [B-rep engine](../products/krly3d/brep/engine), a megbeszélt termékstruktúrában.

- **Darwin / Sol high:** engine-független Rust identity rekord, natív betöltés, tulajdonosi azonosság, egyszer feloldott verzió és immutable geometriai snapshot. Kilenc valódi geometriai/ownership teszt és egy privát mezőket ellenőrző compile-fail doctest.
- **Integrátor:** a mesh engine már B-rep ownert komponál; saját betöltés/verziófeloldás megszűnt. `from_brep` megőrzi a közös provenance-t. A snapshot saját szolgáltatásokat is megtart, így a későbbi szerkesztés/rollback és az owner megszűnése után is használható. A Context-klón megosztja a registryt, cache-t és meglévő kontrollt; külön call-control csak az adott hívásra kerül.
- A mesh bridge-revízió `krly3d.mesh.brep/2`; v1/v2 wire sémák változatlanok. A source ID továbbra is opaque, most a B-rep ownerből ered. Külön regresszió bizonyítja a B-rep → mesh összekötés azonosságát és az eredeti geometriát szerkesztés után.

**Ellenőrzés:** 53 célzott teszt doctesttel együtt; a végső `cargo test --workspace` **925 teszt + 1 compile-fail doctest**, összesen **926 passed, 0 failed, 0 ignored** (60 suite). Szigorú workspace Clippy, fmt és diff-check sikeres. Friss binárissal 8/8 legacy JSON-RPC válasz változatlan; hivatalos MCP SDK: négy geometriai CLI/MCP paritáseset és schema/error/deadline/cancel/release/session-reuse ellenőrzések sikeresek. Az integráció során javítva az új tesztek negatív dobozméretre és lebegőpontos térfogatra vonatkozó feltételezése; a végső kapuk ezek után futottak.

**Struktúra:** a B-rep API csak raw primitívektől függ. A függőségi kapu kizárja az engine/adapter beszivárgását az API-ba, és a B-rep engine közvetlen mesh/view/transport importját. A legacy std közvetetten még mesh adattípusokat használ; ez dokumentált átmeneti bridge. A snapshot `legacy_shape`/`legacy_context` hozzáférése, a geometriai műveletek és a preview Context/Body facade továbbra is átmeneti. Nem hirdetünk új B-rep wire képességet vagy teljes doménmigrációt.

**Issue-k:** #26 frissítve a kész részfeladattal és a megmaradt konverziós/view szerződésekkel; nincs új lezárás. **20 nyitott, 6 lezárt issue.** Következő szelet: típusos geometriai konverzió és standalone view API/adapters, majd a tényleges korai kép → mesh → B-rep folyamat (#24). A név szerinti repülőgép/moai tesztek továbbra is elvetve.

A worker leállt; nincs commit/push. A felhasználói `.ctx/ctxe` változások és a gitlink-pin érintetlenek. Egész fájl most nem vált elavulttá: a mesh engine megtartja a számítási feladatait, a legacy kernel/std még aktív. Nem keletkezett új archiválandó fájl; a korábbi [.tmp archívum](../.tmp/README.md) megmarad.


## 2026-11-14 — Név szerinti modelltesztek elengedve

A felhasználó kifejezett döntése alapján az F-35/Boeing 737/moai regressziós követelmények megszűntek; később más módon validáljuk a funkciókat. [#18](https://github.com/krlybnd/krlyxd/issues/18) **elvetett feladatként lezárva**, nem teljesített tesztként. A név szerinti benchmark már nem blokkoló feltétel.

A meglévő implementáció és a hetedik kör 915 sikeres tesztje alapján [#1 mesh-render](https://github.com/krlybnd/krlyxd/issues/1) **a módosított elfogadással lezárva**. A standalone view/B-rep API-migráció továbbra is #26. #5 és #6 funkcionális feladatai nyitva maradnak, konkrét F-35/moai függőség nélkül; #5 új referencia- és küszöbválasztást igényel az elfogadáshoz.

**26 issue: 20 nyitott, 6 lezárt (#1, #2, #17, #18, #19, #21).** GitHub és az aktív handoff/terv frissítve. Kód vagy teszt nem törlődött, új tesztfuttatás nem szükséges ehhez a scope-/dokumentációváltozáshoz. A korábbi beszámolókban szereplő fixture-blokkolás történeti állapot. Nincs commit/push.



## 2026-11-14 — Hetedik kör: opt-in mesh preview és #2 lezárás

Elkészült a [mesh előnézet](MESH-PREVIEW.md) első működő szelete. Az új backend már a [products/krly3d/view/mesh](../products/krly3d/view/mesh) csomagban él, az [ADR 0004](decisions/0004-mesh-preview-boundary.md) szerinti határral. A legacy view facade, kamera és pontos pick aktív marad; nincs most kiváltott implementáció, amit archiválni kellene.

- **Linnaeus / Sol high:** publikus Rust preview adat, eredeti face-provenance és saroknormálok; a meglévő tesszellátor/cache újrahasználata, kontrollált konverzió. A v1/v2 mesh wire szerződés változatlan.
- **Ramanujan / Sol high:** ortho/pinhole raster, near-plane és viewport clipping, z-buffer, perspektívahelyes interpoláció, face-paint, közös árnyalás/élszabályok; tényleges pixelmunka közbeni cancel/deadline és biztonságos publikálás tesztelve.
- **Integrátor:** CLI/MCP opt-in `renderer=mesh`, minőség/idő metaadat és szöveg, függőségi határkapu, összehasonlító példa, review és teljes ellenőrzések. A raycast az alapértelmezés; exact pick változatlan.

**Ellenőrzés:** `cargo test --workspace`: **915 passed, 0 failed, 0 ignored**, 55 suite és sikeres doctest-szakasz; 32 új teszt. Szigorú workspace Clippy, fmt és diff-check sikeres. Az új kapcsoló miatt egy régi sémateszt elvárt paraméterlistája frissült, majd a teljes workspace újrafutott. Friss binárissal 8/8 legacy JSON-RPC válasz változatlan; a hivatalos Python MCP SDK négy geometriai paritásesete és lifecycle/schema/error ellenőrzései sikeresek. Az új előnézetnek külön CLI/MCP PNG-paritás tesztje van.

**Mérés:** AMD Ryzen 7 250 w/ Radeon 780M, release build, `models/plate-with-hole.k3d`, `part`, iso 256×192, tolerancia 0.05 modell-egység: exact **19.358 ms**, hideg mesh **82.447 ms**, meleg mesh **2.688 ms**, silhouette IoU **1.000000**. A hideg/meleg kép azonos; PNG-kódolás nincs benne. Ez egyetlen helyi mérés, nem késleltetési garancia. A box/sphere többkamerás regressziók 0.004 tolerancián legalább 0.99 IoU-t kérnek; ez nem bizonyítja minden modellre az alapértelmezett tolerancia minőségét. Reprodukció a [preview leírásban](MESH-PREVIEW.md).

**Issue-k:** [#2](https://github.com/krlybnd/krlyxd/issues/2) lezárva a bizonyított disconnect/state-preservation elfogadás alapján. A legacy operation/event binding külön, még nyitott tétel a [#26-ban](https://github.com/krlybnd/krlyxd/issues/26); peer forwarding #16. [#1](https://github.com/krlybnd/krlyxd/issues/1) részben kész, nyitva marad a hiányzó F-35 fixture/mérés (#18) és a teljes view szerződés bizonyítékáig. **26 issue összesen: 22 nyitott, 4 lezárt (#2, #17, #19, #21).**

Mindkét worker befejezett és leállt. Nincs commit/push; a `.ctx/ctxe` felhasználói változásai érintetlenek. Következő szelet: komplex fixture/minőség/performance és a B-rep snapshot/view szerződés migrációja. Az alábbi részek történeti állapotokat rögzítenek.

## Hatodik multi-agent kör — integrálva, 2026-11-14

A [#2 socket/render feladat](https://github.com/krlybnd/krlyxd/issues/2) kapcsolatbontási része elkészült: a teljes disconnect megszakítja a helyi render/mesh lekérdezést, a következő kliens használhatja a változatlan modellt. A normál write half-close továbbra is megengedi a válasz fogadását. A legacy műveletazonosító/eseményprotokoll még hiányzik, ezért **#2 nyitva marad**; a peer forwarding továbbra is #16.

### Integrált változások

- **Leibniz / Sol high:** teljes socket hangup figyelése blokkoló poll-lal, külön leállító jelzéssel és minden kilépéskor joinnal; Endpoint → Joined → Session kontrolltovábbítás; lekérdezésenkénti Context, változatlan legacy válaszformátum.
- **Leibniz / Sol high:** megszakadó batch után a dokumentum/modell és a teljes mark-lista helyreáll. A korábban elfogyasztott mark és a hibás későbbi batch-elem regressziója is lefedett. Gyerekfolyamatban valódi render/mesh, mindkét legacy dialektus, sorban álló következő kliens és half-close tesztelve; megtelt kimeneti socket és monitor-cleanup külön tesztelve.
- **Lorentz / Sol high:** kooperatív ellenőrzések a meglévő bounds/probe/render algoritmusban, 64 sugaras kötegek és pixelfestési ellenőrzések, PNG/base64 kódolás és kép-lekérdezések. A közös bounds algoritmus egy helyen maradt. Determinisztikus cancel/deadline, azonos újrapróbált kép, párhuzamos sorsorrend, kódolási byte-egyezés, késői siker/decline kizárása és félkész tree-cache kizárása.
- **Integrátor:** diff-review, közös kapuk, kompatibilitás, issue-k és struktúraszabályok. Az elavult első indítási terv a [.tmp archívumba](../.tmp/README.md) került; az [aktív terv](MULTI-AGENT-DEVELOPMENT-PLAN.md) a mostani sorrendet követi. A költöztetési kapuk a [termék-handoffban](PRODUCT-ARCHITECTURE-HANDOFF.md#struktúraváltás-és-elavult-fájlok) és a [#26 issue-ban](https://github.com/krlybnd/krlyxd/issues/26) szerepelnek.

### Ellenőrzés és korlátok

- `cargo test --workspace`: **883 passed, 0 failed, 0 ignored**, 51 suite, sikeres doctest-szakasz; **27 új teszt** az ötödik körhöz képest.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`: sikeres.
- Friss legacy CLI és mesh adapter build: **8/8 legacy JSON-RPC válasz változatlan**, stderr üres; a hivatalos SDK-próba **4 CLI/MCP geometriai paritásesettel** és a teljes meglévő életciklus/hibaellenőrzéssel sikeres.
- A kontroll a Rust API-ban határidőt is fogad, de nincs új legacy wire-timeout paraméter vagy Started/Progress eseményfolyam. A külön mesh v2 adapterek meglévő protokollja változatlan.
- A már futó módosító műveletek és a távoli peer-hívások nem preemptálhatók. A batch rollback a dokumentumot/modellt/markokat állítja vissza, nem külső pluginhatásokat vagy minden cache-t. Az egyedi geometriai hívás, rendezés, allokáció és felszabadítás továbbra sem kemény időkorlátos; a megszakítás kooperatív.

**23 issue nyitott, 3 lezárt.** #2 és #26 frissítve a tényleges eredménnyel és a hátralévő scope-pal. Mindkét worker befejezte a munkát és leállt. A teljes Cargo-kapuk egymás után futottak. Nincs commit vagy push; a felhasználó `.ctx/ctxe` módosításai és a gitlink-pin változatlanok. Következő nagyobb szelet: #1 gyors mesh-render, előtte a view/B-rep csomaghatár újraértékelése; #2 legacy eseménybinding a #26 szerződésmigrációval összehangolva.

## GitHub issue-audit — 2026-11-14, az ötödik kör után

A [#21 API–CLI–MCP pilot](https://github.com/krlybnd/krlyxd/issues/21) eredeti, egy működő pilotra vonatkozó elfogadási feltétele teljesült, ezért lezárva. A korábbi értelmezés tévesen minden domén migrációját is a lezárás feltételévé tette. A hátralévő doménmigrációt és a további eseménytípusokat az új [#26 követőfeladat](https://github.com/krlybnd/krlyxd/issues/26) őrzi meg.

Kilenc meglévő issue (#1, #2, #3, #13, #16, #18, #20, #21, #24) frissült: megmaradt az eredeti leírás és elfogadási feltétel, külön audit rögzíti a kész részeket, bizonyítékokat, függőségeket és hiányokat. Következő fejlesztési prioritás a [#2 legacy socket/render megszakítás](https://github.com/krlybnd/krlyxd/issues/2), majd a [#1 gyors view-render](https://github.com/krlybnd/krlyxd/issues/1).

**26 issue összesen, 23 nyitott, 3 lezárt (#17, #19, #21).** Egy lezárás és egy új követőfeladat miatt a nyitott darabszám változatlan. A GitHub-frissítések megtörténtek; a kód és dokumentáció továbbra is helyben, commit és push nélkül van. Az issue-k ezt kifejezetten jelzik. A bizonyíték az ötödik kör 856 sikeres tesztje és adapterellenőrzése; ebben a dokumentációs körben nem futott újra a teljes tesztcsomag. Az alábbi körbeszámolók történeti állapotokat is tartalmaznak.

## Ötödik multi-agent kör — integrálva, 2026-11-14

Elkészült a mesh pilot teljes önálló CLI/MCP útvonala a [0003-as döntés](decisions/0003-mesh-transport-adapters.md) szerint. Használat, példák és klienskonfiguráció: [mesh adapterek](MESH-ADAPTERS.md). Ez a #21 mesh-pilotjának adapterrésze; a többi domén szerződése és migrációja még nyitott.

### Integrált változások

- **Dewey / Sol high:** külön `krly3d-mesh-cli` csomag, `krly3d-mesh` bináris. Natív modell és kanonikus v2 kérés, `--describe`, NDJSON Started/terminal, SIGINT, 0/1/130 kilépési kódok. Bemeneti korlátok és megszakítás lezárt/blokkolt stdout mellett; korlátos, best-effort stderr. **15 folyamatteszt**.
- **Peirce / Sol high:** külön `krly3d-mesh-mcp` stdio-szerver, MCP 2025-11-25. `mesh`, `operation_events`, `operation_cancel`, `operation_release`; nem blokkoló műveletkezelés, közös descriptor-séma és kanonikus eredmények. Strict JSON, duplikált kulcsok elutasítása, független MCP/domain azonosítók. **10 egységteszt és 8 folyamatteszt**.
- **Zeno / Astra medium:** szűk, olvasási transport-review. A talált MCP backpressure és CLI stderr blokkolási hibák javítva és regresszióval védve. A leválasztott kimeneti szál saját fájlleírót használ, hogy a runtime stdout-lock takarítása se akadályozza a kilépést.
- **Jason / Luna medium:** használati referencia és MCP-konfigurációs példa.
- **Integrátor:** közös `MeshEngine::from_k3d`, meglévő parser/plugin-regiszter és expectation-ellenőrzés, **2 modellbetöltési regresszió**; nyers kernel/API/engine függőségi határellenőrzés; hivatalos SDK-s klienspróba és CLI/MCP összehasonlítás.

### Működési határok

Mindkét adapter egy induláskor betöltött, változatlan modellt szolgál ki. Modellkorlát 16 MiB; CLI request/MCP bemeneti üzenet 1 MiB. A CLI stdin EOF-ja a kérés végét jelenti. Az MCP stdin EOF-ja a session végét jelzi: előbb megszakítja a saját munkáit, majd legfeljebb egy másodpercet enged a sorban álló válaszok kiírására. Az MCP kimeneten nyolc válasz várhat az aktív írón túl; telítődéskor lezárja a kapcsolatot és megszakítja a munkát. Unix alatt az idle stdout lezárását is figyeli. Ezek transportkorlátok, nem teljes processz-memória- vagy solverleállási garanciák.

Továbbra is csak Started és egy terminális esemény van; nincs progress vagy részleges mesh. A régi Unix-socket listener/render megszakítása és half-close kezelése **még #2 feladat**; a peer forwarding **#16**. A #21 eredeti pilot-elfogadása teljesült; a többi domén migrációja és a további eseménytípusok a #26 követőfeladatban maradnak nyitva.

### Integrációs ellenőrzés

- `cargo test --workspace`: **856 passed, 0 failed, 0 ignored**, doctestekkel együtt; **35 új teszt** a negyedik körhöz képest.
- `cargo clippy --workspace --all-targets -- -D warnings`: sikeres; a produkciós kód lintelnyomás nélkül, a tesztsegédek a repó szokásos `unwrap`/`panic` engedményeivel.
- `cargo fmt --all -- --check` és `git diff --check`: sikeres.
- Friss binárisok: **8/8 legacy CLI JSON-RPC válasz változatlan**, stderr üres.
- [SDK-s smoke](../tests/mesh_adapter_smoke.py): hivatalos Python `mcp==2.3.0`, `jsonschema==4.26.0`, MCP 2025-11-25. **4 CLI/MCP geometriai paritáseset**: alapértékek, verziózott háromszöges modell, pontos float-token, gömb. Közös schema és eseményvalidálás; hibás/verzióhibás bemenet, duplikált request ID, timeout 0, ismételt cancel, release és további ping sikeres. A gömbpróba r=5, tolerance=0.1 modellunit; a térfogathiba a `4πr²·tolerance` határon belül.

A teljes Cargo-kapuk egymás után futottak; a végső SDK-próba a friss builddel is sikeres. Minden agent befejezte a munkát és leállt. Nincs commit, push vagy új issue-lezárás; a felhasználó `.ctx/ctxe` módosításai és a gitlink-pin változatlanok. Következő nagyobb kör: a régi socket/render megszakítás végigvezetése, majd a közös mesh szerződésre épülő gyors view-render.

## Issue-lezárási audit — 2026-11-14

A felhasználó kérésére az elfogadási feltételek ellenőrzése után lezárva:

- [#17 — dokumentáció](https://github.com/krlybnd/krlyxd/issues/17): a négy handoff/spec újraolvasva és összevetve a jelenlegi kóddal. A SPEC elavult runner/cancellation állításai javítva; a NURBS-projekció javítása és regressziója külön dokumentálva.
- [#19 — clean clone](https://github.com/krlybnd/krlyxd/issues/19): a rögzített sikeres recursive clone és a megőrzött ellenőrző checkout tisztasága/SHA-ja visszaellenőrizve. Elfogadott policy a jelenlegi pontos gitlink-pin megtartása, automatikus branchkövetés nélkül; submodule-módosítás nem szükséges.

**23 issue maradt nyitva.** A #3 content/version cache-kulcsa, a #20 teljes raw szerződése és a #21 domain CLI/MCP adapterei még hiányosak; részeredmény miatt nem zártuk le őket. A többi issue teljes elfogadási bizonyítéka sem áll rendelkezésre.

A lezárásokhoz GitHub-komment rögzíti az ellenőrzést és azt, hogy a fejlesztési/dokumentációs módosítások továbbra is helyben, commit és push nélkül vannak. A `.ctx/ctxe` felhasználói módosításai változatlanok. Ez a kör dokumentációt módosított; `git diff --check` sikeres, új teljes tesztfuttatás nem történt. Az alábbi körbeszámolók történeti állapotok.

## Negyedik multi-agent kör — integrálva, 2026-11-14

A [műveleti életciklus döntése](decisions/0002-mesh-operation-lifecycle.md) alapján elkészült a külön [2.0-s operation API és háttérfuttató](MESH-OPERATIONS.md). A szinkron 1.0 szerződés és descriptora változatlan; ugyanaz a számítás szolgálja ki mindkét API-t. A teljes #21 még nincs kész: a domain CLI/MCP adapterek hátravannak.

### Integrált változások

- **Volta / Sol high:** típusos v2 kérés/esemény, verzióellenőrzés, strict codec és descriptor. A v1 raw meződekódert és eredménykódolást használja újra; 17 új API-teszt és 11 korábbi contractteszt sikeres.
- **Einstein / Sol high:** `MeshRunner` start/next/wait_next/cancel/release, snapshoton futó háttérszálak, kapacitás és bytekorlát, terminális versenyek, pánik/határidő kezelése. 18 célzott runner-teszt sikeres.
- **Carson / Astra medium:** szűk, olvasási concurrency review. A cache-destruktorok és a runner leállítási zárolásának kockázata javítva; panic/deadline döntés teszttel rögzítve.
- **Epicurus / Luna medium:** az aktuális kódból készült műveleti API referencia.
- **Integrátor:** leválasztott `MeshTask`, az engine megszűnése után is élő számítás; v1/v2 eredmény- és minőségparitás; cache-értékek felszabadítása mindkét zároláson kívül, célzott regresszióval. A célzott context-, task- és paritástesztek sikeresek.

### Működési határok

A pilot kizárólag Started (`seq=0`) és Completed/Failed/Cancelled (`seq=1`) eseményt küld. Nincs progress vagy korai részleges mesh. Timeout 0 elfogadható v2 kérés, amely deadline hibával zárul; v1-ben továbbra is elutasított mező.

Alapkorlátok runnerenként: **4 élő worker, 64 megtartott művelet**, műveletenként legfeljebb **2 sorban álló esemény és 16 MiB kanonikus kódolt eseményadat**. A request ID legfeljebb 256, az operation ID 64, a bemeneti node 4096 UTF-8 byte. Ezek kezdeti működési korlátok, nem hardverre hangolt teljesítményígéretek; nem korlátozzák a solver és az ideiglenes kódolás teljes memóriaigényét.

Megszakított, de még futó worker nem szabadít kapacitást. A request ID a record felszabadításáig foglalt; csak a ténylegesen befejezett számítás `release` hívása engedi az újrafelhasználást. Nincs automatikus eviction vagy tartós replay. Drop kooperatív leállítást kér, nem vár korlátlanul egy pluginra. A hálózati disconnect és a normál socket half-close kezelése még adapterfeladat.

### Integrációs ellenőrzés

- `cargo test --workspace`: **821 passed, 0 failed, 0 ignored**, a doctest-szakasz is sikeres; **38 új teszt** a harmadik körhöz képest.
- `cargo clippy --workspace --all-targets -- -D warnings`: sikeres, lintelnyomás nélkül.
- `cargo fmt --all -- --check` és `git diff --check`: sikeres.
- Friss CLI-build: **8/8 rögzített JSON-RPC válasz változatlan**, stderr üres.

A kapuk egymás után futnak, közben nincs más Cargo-build. A két Sol, Astra és Luna feladata befejeződött; minden worker leállt. Nincs commit, push vagy issue-lezárás. Következő szelet: domain CLI/MCP adapterek a v2 descriptorra és eseményekre építve, valós klienssel ellenőrzött MCP binding, disconnect/half-close tesztek.

## Harmadik multi-agent kör — integrálva, 2026-11-14

A [kontrollált mesh végrehajtás](MESH-API.md#local-controlled-execution) számítási alapja elkészült. Ez az ADR PR3 első szelete; a teljes #21 továbbra is nyitott.

- **Bohr / Sol high:** típusos `Cancelled`/`DeadlineExceeded` hibák, hívásonkénti context közös registryvel/cache-sel, megszakítható tesszelláció, publikálás és cancel összehangolása.
- **Aristotle / Luna high:** a terület, térfogat és nyitott élek számítása megszakítható, ugyanazzal az algoritmussal és összegzési sorrenddel; a régi metódusok delegálnak.
- **Integrátor:** `MeshEngine::execute_controlled`, kontrollált eredménycsomagolás, snapshot/határidő/cache regressziók, kompatibilitás és dokumentáció.

### Viselkedés és bizonyíték

A tesszellátor a hosszú ciklusokban és rekurzióban 64 munkaelemenként ellenőrzi a kontrollt; a statisztikák és az eredmény másolása legfeljebb 256 elemenként. A megszakítás és határidő nem geometriai decline, ezért nem kerül negatív cache-be. Ha a cancel megelőzi a publikálási őr megszerzését, a késői cache-írás/eredményközlés nem fut le; a cancel előtt elkészült cache-bejegyzés továbbra is érvényes.

Valódi gömbmeshelés közben, befecskendezett órával és szálak közötti szinkronizálással ellenőrzött cancellation/deadline, majd sikeres újraszámolás és cache reuse. Külön teszt állítja meg az élmintavételezést, a layout belső ciklusát, az artifact másolását és a sikeres tesszellálás utáni cache-publikálást. A worker 149 célzott library tesztje és célzott Clippy-ellenőrzése sikeres.

A worker egy mérésében a kérés és a számoló szál joinja között kb. 87 µs (cancel), illetve 92 µs (deadline) telt el; ez schedulinget és cleanupot is tartalmazó fixture-mérés, nem késleltetési garancia. Egyedi pluginevaluációk, kollekciós műveletek, allokáció/felszabadítás és cache-lock munka nem preemptálható.

### Integrációs ellenőrzés

- Végső, külön futtatott `cargo test --workspace`: **783 passed, 0 failed, 0 ignored**, a doctest-szakasz is sikeres. Ez **18 új teszt** a második körhöz képest.
- Az első futás összes kódtesztje sikeres volt, de rustdoc `E0463` buildfüggőségi hibával állt meg a közben futtatott CLI-build után. A változatlan kód önálló teljes újrafuttatása sikeres; további buildet nem indítottunk vele párhuzamosan.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` és `git diff --check`: sikeres.
- Friss CLI-build: **8/8 JSON-RPC válasz változatlan**, üres stderr.

### Hátralévő munka

A szinkron JSON API/protokoll 1.0 és descriptora változatlan; wire timeoutot továbbra sem fogad el. A Rust `execute_controlled` külön, additív belépési pont. Az operation runner, korlátozott eseménysor, terminal-state versenyek, háttérszámítás közbeni session-módosítás, disconnect-kezelés és CLI/MCP adapterek még hiányoznak.

Mindkét worker befejezte a munkát és leállt. A módosítások helyben vannak; nincs commit, push vagy issue-lezárás.

## Második multi-agent kör — integrálva, 2026-11-14

A [0001-es ADR](decisions/0001-kernel-and-domain-contract.md) első két implementációs szelete elkészült helyben. A teljes #20 és #21 issue még nem tekinthető késznek.

- **Hilbert / Sol high:** [raw kernel seed](RAW-KERNEL.md), kompatibilis core re-exportok, primitív szerződéstípusok és call control. Típusazonossági, egyetlen implementációt és tranzitív függőségeket ellenőrző tesztek.
- **McClintock / Sol high:** [verziózott, típusos mesh API és szinkron engine bridge](MESH-API.md). A legacy lekérdezés és az új API közös számítási utat használ. A bridge rögzíti a modellsnapshotot és a provenienciát; idegen engine-ből származó előkészített hívást elutasít.
- **Beauvoir / Luna medium:** raw kernel referencia és migrációs határok dokumentálása.
- **Integrátor:** integráció, séma/validáció illesztése, dokumentáció és kompatibilitási ellenőrzés.

### Ellenőrzött eredmény

- `cargo test --workspace`: **765 passed, 0 failed, 0 ignored**, 37-tel több sikeres teszt az első integrált körhöz képest.
- `cargo clippy --workspace --all-targets -- -D warnings`: sikeres, lintelnyomás nélkül.
- `cargo fmt --all -- --check` és `git diff --check`: sikeres.
- A migráció előtt rögzített **8 valódi CLI JSON-RPC válasz** az új buildben JSON-érték szerint változatlan: alap/triangles/verziózott mesh és hibás bemenetek.
- A raw/core célzott ellenőrzés külön is sikeres: **111 teszt**.

### Hátralévő határok

A mesh API 1.0 szinkron: nem hirdet eseményeket vagy megszakítható meshelést, és elutasítja a timeoutot. A raw call control primitív önmagában nem teszi megszakíthatóvá a tesszellátort. Következő szelet az ADR szerinti kontrollált végrehajtás, majd a közös szerződést használó domain CLI/MCP adapterek. Ezekhez külön verziózási döntés kell; a jelenlegi szinkron szerződés nem változhat észrevétlenül eseményalapúvá.

Mindhárom worker befejezte a munkát és leállt. Az eredmény a fő munkafában van, commit és push nélkül; GitHub issue nem lett lezárva. A felhasználó `.ctx/ctxe` módosításait megőriztük. Az alábbi első kör történeti beszámoló, nem az aktuális tesztszám.

## Első multi-agent kör — 2026-11-14

Kiinduló commit: `1aac484a594f1e1f7176fd0309d9d5e04f88ae0b`.
Kiosztás és következő hullámok: [multi-agent terv](MULTI-AGENT-DEVELOPMENT-PLAN.md).
Termékdöntések: [architektúra-handoff](PRODUCT-ARCHITECTURE-HANDOFF.md).

### Kiosztott munkák

- **Gauss / Astra high — #20, #21:** kernel- és doménszerződés-döntés, meglévő műveletre épülő pilotterv. Saját branch: `agent/wave1-architecture`; kizárólag új döntési dokumentum.
- **Tesla / Sol medium — #3:** determinisztikus tesszellációs decline cache és célzott regressziók. Saját branch: `agent/wave1-cache`; kernel/mesh cache és tesztek.
- **Poincare / Luna medium — #17:** kóddal ellenőrzött dokumentációjavítás. Saját branch: `agent/wave1-docs`; specifikációk és handoffok.
- **McClintock / Luna medium — #19:** távoli clean clone és submodule-verifikáció. Saját branch: `agent/wave1-submodule`; ellenőrzési jelentés.
- **Lovelace / Luna medium — lint baseline javítás:** a #19 ellenőrzés után felszabadult helyen indult, külön `agent/wave1-lint` branchen; meglévő Clippy-diagnosztikák minimális, viselkedést őrző javítása.

Az agentek külön worktree-ben dolgoznak. A fő checkoutban az integrátor ellenőrzi és összefésüli az eredményeket. A meglévő módosított `.ctx/ctxe` és a felhasználó más módosításai nem részei ennek a körnek.

### Kiinduló ellenőrzések

- Toolchain: `rustc 1.99.0 (b940084d7 2026-09-28)`.
- `cargo test --workspace`: **721 passed, 0 failed, 0 ignored**.
- `cargo fmt --all -- --check`: sikeres.
- `cargo clippy --workspace --all-targets -- -D warnings`: a változtatások előtt hibás. Első akadály: NURBS `needless_range_loop`; ennek ideiglenes diagnosztikai engedélyezése után a vision kódban `items_after_test_module` és `redundant_closure` is előkerült. A végső kapu nem használ lintelnyomást.
- #19 clean clone: sikeres, a gitlink `cac12f38e5cef691199a81302810cdcd1cd31146` visszaállt; [részletes bizonyíték](CTXE-SUBMODULE-VERIFICATION.md). Az integrátor a clone tisztaságát és submodule SHA-ját külön is visszaolvasta. A policy-javaslat a jelenlegi pontos commit-pin megtartása, nem automatikus branchkövetés.

### Integrációs kapu

1. Változás előtti workspace teszt, clippy és format baseline.
2. Agentenként fájlscope- és diffellenőrzés, célzott bizonyítékok.
3. Kódváltozás után releváns tesztek és workspace ellenőrzések.
4. A kész részek és nyitott acceptance feltételek elkülönített rögzítése; részeredmény miatt nem zárunk teljes issue-t.

### Integrált eredmények

- **#3, első implementációs szelet:** determinisztikus tesszellációs budget-decline memo, face/tolerance/indok megőrzése, publikus Rust hit/miss/entry statisztika. Az új tesztek igazolják, hogy az ismételt hívás nem tesszellál, a változó tolerancia/test/context elkülönül, az átmeneti és layout/plugin hibák újrapróbálhatók. A budget-regresszió valódi layout után injektálja az elutasítást; nem épít 400 000 háromszöget. A teljes issue nyitott része a külön allokációk közötti tartalomazonos cache és a formális API/solver/quality verziókulcs; ezek a contract-migrációval folytatódnak.
- **#20/#21, döntési szelet:** [0001 — kernel és mesh contract pilot](decisions/0001-kernel-and-domain-contract.md). Négy következő változtatás: raw seed, típusos mesh eredmény, megszakítható végrehajtás, CLI/MCP adapterek közös tesztszerződéssel. Ezek tervek, nem elkészült csomagok.
- **#19:** a clean-clone bizonyíték integrálva; a meglévő pontos gitlink-pin megtartását javasoljuk. A felhasználó submodule-munkafája változatlan.
- **#17:** a gyökér HANDOFF/SPEC, a krlyvis SPEC és a termék-handoff javításai integrálva. A stateful vision, peer/likeness/claim/profile viselkedést, az új cache-t és a megvalósítás/célarchitektúra különbségét dokumentálják; az elavult issue-rendezési utasítások kikerültek.
- **Lint baseline:** négy fájl kis módosítása, lintelnyomás nélkül; NURBS iteráció, likeness párok, datum helper-sorrend és PNG callback.
- **Végső kódellenőrzés:** `cargo test --workspace` **728 passed, 0 failed, 0 ignored**; `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` és `git diff --check` sikeres. A hét új teszt a decline cache viselkedését védi.

Az eredmények helyben, a fő munkafába integrálva állnak rendelkezésre. Ebben a körben nincs commit, push vagy issue-lezárás.
Az első kör összes workere befejezte a feladatát és le lett állítva; nem maradt háttérben futó fejlesztő agent.

### Az első kör után kijelölt lépések (történeti)

- #20 raw seed, majd #21 pilot implementációja: a második körben elkészült a raw és a szinkron mesh szelet. A kontrollált végrehajtás és adapterek még nyitottak; Astra csak felmerülő szemantikai döntések review-jára szükséges.
- #1 gyors render a stabil mesh interfészen, a migrációval ütköző fájlok kizárásával.
- #18 F-35/737/moai fixture-ek: a repóban jelenleg csak pegboard, plate-with-hole és tesseract `.k3d` található. A korábbi scratch modelleket még vissza kell szerezni; a `/tmp` és projektkönyvtár vizsgált fájllistájában nem kerültek elő. Kitalált modellel vagy méréssel nem pótoljuk a referenciát.
