# Közös memóriás futtatás és átállási terv

A helyi cél: egy folyamatban a doménmotorok közös, immutable artifact-hivatkozásokon
dolgoznak. A [döntés](decisions/0011-shared-runtime.md) rögzíti az ownershipet és a
publikálási szabályokat. A raw kernel matematika/alaptípus marad; a CTM assembly és
kényszerek gazdája. A rekonstrukciós host alkalmazási összekötés.

## Helyi útvonalak

`kép → krlyvis read_shared → megosztott krly2d Region → retained B-rep → mesh → host publish`

A [krly2d API](KRLY2D.md) külön domén. A vision a világkoordinátás régiót egyszer
állítja elő, majd ugyanazt az `Arc<Region>`-t kapja és tartja meg a B-rep és a host
modellje. A raszteres datum és a 2D régió külön reprezentáció marad; a régi wire
export kompatibilitása megmarad. Ez nem teljesen másolatmentes folyamat: a B-rep
providerhez jelenleg ideiglenes poligonok és profilszöveg készül. A natív 2D régió
egyenes határú, model-unit változatát építi a B-rep; a görbéket/metre egységet
explicit hibával utasítja el. Fizikai skálát nem talál ki.

A [runtime](../products/krly3d/reconstruction/runtime/src/lib.rs) típusos evidence-t,
B-rep owner/snapshotot és mesh eredményt tart. A számítási útvonal nem ment natív
dokumentumot és nem tölti vissza a mesh kedvéért. Az export a
[host CLI](../products/krly3d/reconstruction/host-cli/src/main.rs) végén történik.

```sh
cargo build -p krly3d-reconstruction-cli
target/debug/krly3d-reconstruct --image /absolute/drawing.png \
  --output /tmp/new-shared-proposal --pixel-unit 1 --height 3
```

Opcionális: `--seed X,Y`, `--tolerance 0.05`, `--timeout 120`. A kimeneti könyvtár szülője létezzen,
a könyvtár maga legyen új. Kimenet: evidence, szerkeszthető k3d, mesh, revíziós
eseménynapló, manifest hash/provenance/feltételezés/időadatokkal. Nincs fizikai
pontosság- vagy confidence-becslés: egy felülnézeti kontúr, adott méretarány és
egyenletes magasság, átmenő lyukak. Az alapértelmezett CLI-mód a teljes számítás után exportál; stdouton egyetlen
manifestet ad. A teljes, lépésenkénti út a `--stream` kapcsolóval kérhető. A fájlírás hibája új részleges
kimeneti könyvtárat hagyhat; kész manifest csak a többi artifact kiírása után készül.

## Hétlépcsős natív stream

```sh
target/debug/krly3d-reconstruct --stream --image /absolute/drawing.png \
  --output /tmp/new-streamed-proposal --pixel-unit 1 --height 3
```

A [stream adapter](../products/krly3d/reconstruction/host-cli/src/stream.rs) sorrendje:
`started → evidence → coarse_preview → editable_proposal → preview → validation → completed`.
Minden esemény külön flush után engedi elindulni a következő lépést. A stdout és az
`events.jsonl` azonos NDJSON rekordokat kap (`krly3d.shared-reconstruction/2`, seq 0–6,
flow ID, revision, stage/elapsed idő). Ez artifact-eseménynapló, nem tartós command replay.

A [coarse lépés](../products/krly3d/reconstruction/runtime/src/coarse.rs) befoglaló
prizmát készít a meglévő planar/mesh providerekkel. A kontúrmodell építése csak a
`finish` után kezdődik. A lyukakat és konkáv részeket még kitöltő közelítés explicit
`bounding_prism`, nem pontossági állítás. Az editable modell ugyanazon logikai
artifact második revíziója, az elsőt felváltja. A coarse native owner különálló;
a végső szerkeszthető dokumentumba nem kerül bele.

A [review](../products/krly3d/reconstruction/runtime/src/review.rs) ugyanabból a megtartott
B-rep snapshotból renderel és összehasonlít. A domainhatáron nincs k3d-visszatöltés,
peer vagy JSON. A natív render a meglévő view render-utat, a typed likeness az eddigi
station providert használja. Az eredmény a forrás snapshot identitását őrzi.
A likeness egy pixelnyi toleranciával, XY felülnézetben a mintavett külső szakaszhatárokat
hasonlítja; lyukakat, topológiát és fizikai mérési pontosságot nem igazol. Az uncertainty
és confidence továbbra is ismeretlen.

Új artifactok: `coarse-mesh.json`, `preview.png`, `likeness.json`. A validation esemény
az egyezés eredményét közli; a befejezés a feldolgozás sikerét jelenti, nem általános
geometriai tanúsítást. Számítási/artifact-hiba esetén az addigi fájlok megmaradnak és
best-effort `failed` esemény készül. Ismert journal/stdout íráshiba után nincs újabb
írási kísérlet; megkísérelt `completed` után nem kerülhet ellentmondó `failed` a naplóba.
Sikertelen futás visszavonja a sikermanifestet, amennyiben a fájlrendszer ezt engedi;
a törlési hibát az eredeti hibával együtt jelzi. A sikermanifest a számítás végén,
a completed kézbesítése előtt íródik. Crash vagy törlési hiba esetén ezért nincs
tranzakciós kézbesítési/fájl-publikálási garancia. A napló részleges utolsó sort vagy
csak az egyik kimenetre eljutott terminális kísérletet is tartalmazhat.
A stdout-írás visszanyomást ad és nem épít korlátlan sort. A Unix adapter nonblocking
descriptorral, rövid poll-várakozások között ellenőrzi a közös stop/deadline tokent,
így a megtelt pipe nem akadályozza meg a kooperatív leállítást.

A Rust host `replace_drawing_controlled`, `revise_controlled` és
`revise_if_current_controlled` belépői ugyanazt a caller-owned `CallControl` tokent
viszik végig. Megszakított vagy lejárt szerkesztési kérés a parancs elfogadásakor
sem invalidálhatja a már publikált állapotot; az admission slot ilyenkor felszabadul.
A hash/raszter/komponens/lyukkontúr és planar topológiai ciklusok, valamint a
mesh/render/likeness belső checkpointokat kapnak. A külső képdekóder előtt és után
ellenőrzünk; a dekóder belsejét ez nem teszi megszakíthatóvá. A coarse és végső host-publikálás ugyanazon stop
tokennel védett. Elkezdett, de félbemaradt review nem publikálható; a korábbi core-only
út továbbra is használható. A kontrollált vision/B-rep hívások ideiglenes contextet
használnak: a kész képforrás, evidence, modell és snapshot nem tartja meg a művelet
stop tokenjét, ezért egy későbbi cancellation nem mérgezi meg a kész eredményt.

## Közös CLI-életciklus

Mindhárom natív mód — alap, `--stream`, `--parts` — pozitív, véges `--timeout`
értéket fogad; alapértéke 120 másodperc. A határidő az argumentumok ellenőrzése után,
a bemeneti fájlok beolvasása előtt indul, és a teljes futásra közös. A SIGINT/SIGTERM
handler csak atomikus flaget állít; külön rövid életű figyelőszál továbbítja a stopot
a publikálásvédett tokennek. Leállításkor a folyamat nem nulla exitkóddal tér vissza.

A [CLI I/O](../products/krly3d/reconstruction/host-cli/src/io_control.rs) a bemenetet
korlátos darabokban olvassa, és csak reguláris fájlt fogad: FIFO-t nem vár meg.
A pipe-írás megszakítható; a hibaüzenet kiírása saját rövid best-effort határidőt kap,
így a megtelt stderr sem tartja életben a hibás folyamatot. A journal és stdout
kézbesítése továbbra sem tranzakció: cancellation/timeout után terminális stdout
esemény nem garantált. Részleges utolsó sor vagy csak naplózott terminális kísérlet
előfordulhat. Az ismert íráshibát követően nincs újabb kimeneti kísérlet.

A sheet a leállás okát tartalmazó részleges manifestet és a már kész részeket
megtartja, ha a fájlrendszer engedi. A single mód visszavonja a sikermanifestet
best-effort. Az artifactok együtt nem atomiak, crash-resume nincs.
A codec, egyes szinkron providerhívások, allokáció/destruktor és reguláris
fájlrendszeri read/write/sync/rename nem kapott kemény megszakítási garanciát.
Korlátos párhuzamos scheduler és teljes memória-/költségkeret továbbra is nyitott.

## Natív többalkatrészes sheet

```sh
target/debug/krly3d-reconstruct --image /absolute/drawing.png \
  --parts /absolute/parts.json --output /tmp/new-native-sheet \
  --pixel-unit 0.25 --timeout 120
```

A parts fájl 1–16 objektumot tartalmazó JSON tömb, például:

```json
[{"id":"Frame","seed":[3,3],"height":2},{"id":"Plate","seed":[23,5],"height":5}]
```

Az output kötelező és új könyvtár. Az alkatrésznevek legfeljebb 64 ASCII karakterből
állnak, betűvel kezdődnek, utána betű/szám/`_`/`-` megengedett. A parts bemenet
legfeljebb 1 MiB; ismeretlen/duplikált kulcs és név hibát ad. A single `--seed` és
`--height` itt tiltott; minden rész saját kijelölést és magasságot kap. A timeout
alapértéke 120 másodperc, és a teljes sheetre közös, nem alkatrészenként újrainduló.

A [Sheet runtime](../products/krly3d/reconstruction/runtime/src/sheet.rs) egyszer
dekódolja a képet, és ugyanazt a képet használja a kijelölésekhez. Sorosan épít,
legfeljebb 16 kész eredményt tart meg. A közös rajzkoordináták, lyukak és forráshash
megmaradnak. Azonos komponens második kiválasztása a második evidence-esemény előtt
hibát ad; későbbi hiba esetén a már kész részek megmaradnak, a további részek pendingek.
Ez explicit seed-alapú kiválasztás, nem automatikus szegmentálás vagy assembly.

A `krly3d.shared-sheet/1` NDJSON sorrendje: `started`, alkatrészenként öt
`part_event` (evidence/coarse/editable/preview/validation), `part_completed`, majd
`completed` vagy hiba. A gyökérmanifest alkatrészenként követi az állapotot és a
relatív artifactutakat; teljes ideiglenes fájl írása után, azonos fájlrendszeren
rename váltja le. Hiba esetén a sheet megtartja a részleges manifestet. Íráshiba
után nincs kimeneti újrapróbálás vagy ellentmondó terminális esemény. A fájlok és
a stdout együtt nem alkotnak tranzakciót, összeomlás utáni helyreállítás nincs.

Az eredeti képbyte-ok és a normalizált parts-lista mentése lehetővé teszi az új
kimeneti könyvtárba történő újrafuttatást. Ez még nem resume vagy tartós szerkesztési
parancsnapló. A [Python sheet](../products/krly3d/reconstruction/cli/SHEET.md) aktív
marad a külön folyamatokra épülő kompatibilitási út és annak ellenőrzése miatt.

## Adapterfelelősségek

A host CLI-ben az argumentumok az `options`, a közös jel/deadline kezelés a
`lifecycle`, a korlátos input és megszakítható pipe a `io_control` modulé;
az artifactnevek, közös provenance/feltételezés/volume metaadatok és a v1/v2 manifest
az `output` modulé. A `stream` kizárólag a lépéssorrendet és az esemény-kézbesítést
vezérli; a belépő választja a futási módot. A domainmotorok és a runtime nem függenek
egyik adaptermodultól sem. Ez a sheet-bővítés előtti célzott rendezés, nem új generikus
futtatókeret. A Python sheet/folyamat-kompatibilitási út továbbra is aktív.

## Revíziók, események, memória

- Új kép: új evidence és minden függő artifact érvénytelenítése.
- Magasságmódosítás: ugyanaz az evidence-allokáció, új B-rep és mesh.
- Mesh-tolerancia módosítás: ugyanaz az evidence és B-rep owner, új mesh.
- AI-javaslat a megfigyelt alaprevízióval adható be (`revise_if_current`); elavult kérés nem módosít újabb felhasználói állapotot.
- Minden job saját bemeneti revíziót visz; idegen host és elavult revízió publikálása hibás.
- Régi olvasói hivatkozások életben maradnak, de nem lesznek újra aktuálisak.
- Legfeljebb négy job/kiadatlan eredmény és 256 esemény a hostban. A lemaradt
  eseményolvasó gap hibát kap. Ez nem teljes RAM-kvóta: a decoderek, mesh-provider
  és külső olvasók külön memóriát tarthatnak. Nincs korlátlan belső előzménymásolat.

A sikeres natív mesh-cache testenként nyolc külön toleranciát tart FIFO sorrendben.
A kieső cache-hivatkozás felszabadul; a már kiadott olvasói handle tovább élhet.
Ez bejegyzésszám-korlát, nem teljes RAM-kvóta.

A host nem indít automatikusan szálat: a job áttehető caller-owned workerre.
Így az admission/publikálás szálütemezéstől függetlenül ellenőrizhető. Az események
elfogadott módosításokról és publikálásról szólnak; nem tartalmaznak teljes képet vagy
tartós command logot. AI-trigger és watcher a következő réteg, nem rejtett callback.

## Következő átállási egységek és ticketgazdák

1. **#24 / #26 — natív alapút:** typed vision, megtartott B-rep, közvetlen mesh,
   revízió/admission/esemény és kipróbálható host CLI; ez a jelenlegi szelet.
2. **#24 — teljes helyi product flow:** korai coarse preview, render és likeness
   a megtartott forrásból, továbbá a natív sheet és részleges hibakezelés elkészült. A process pilot
   csak azonos viselkedés/paritás után válik opcionális kompatibilitási belépővé.
3. **#24 / #26 — lifecycle:** a közös CLI deadline/jelkezelés, kooperatív
   vision/B-rep/mesh és megszakítható pipe elkészült. Következő a bounded scheduler,
   memória-/költségkeret és a fennmaradó blokkoló határok kezelése; tartós replay külön blokk.
4. **#15 — AI/PMI:** revízióhoz kötött proposal accept/discard, naplózott AI input/output,
   releváns dependency-watch, stabil anchorok. Az AI csak parancsot javasol.
5. **#22 — CTM:** legalább két domain capability regisztrációja és verzióellenőrzése,
   assembly/kényszerek; a host infrastruktúra újrafelhasználása mérhető igény alapján.
6. **#16 — külső peer:** megmaradó async/cancel/schema-lifetime munka, a helyi
   számítás nem függ a teljesítésétől. **#5:** multiview/section és fidelity továbbra is külön feladat.

## Ellenőrzés és visszavonulási szabály

`cargo test -p krly3d-reconstruction` ellenőrzi az élettartamot, függőség-újrafelhasználást,
fordított befejezési sorrendet, hiba- és kapacitáskezelést. A közös fixture-alapú
`tests/shared_runtime_smoke.py` összehasonlítja a helyi CLI-t a külön domainfolyamatokkal.
Az idő és a sorosított byte-ok független mért értékek; a byte-szám nem heap/RSS mérés.
Teljesítményígéretet vagy általános gyorsulási arányt ez a kis debug smoke nem igazol.

Aktív régi provider/futtató marad a helyén. Csak teljesen kiváltott egész fájl kerül
gyökér `.tmp/` alá, eredeti út és helyettesítő dokumentálásával. Most ilyen fájl nincs:
a külön process flow és CLI/MCP kompatibilitás továbbra is támogatott.
