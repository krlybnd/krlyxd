# Multi-agent fejlesztési terv

Aktuális irány: 2026-11-14, [közös memóriás runtime](SHARED-RUNTIME.md), [ADR 0011](decisions/0011-shared-runtime.md). A helyi belső út típusos Rust API, immutable artifact és revízióellenőrzött publikálás; a CLI/MCP külső adapter. A [fejlesztési napló](DEVELOPMENT-STATUS.md) külön tartja a kész szeletet és a még process-piloton futó képességeket. Aktuális ellenőrzés: 1134 workspace teszt / 103 suite, 14 process-flow próba, négy shared/process, hat vision és három B-rep adapter-paritáseset sikeres. Minden worker befejezett, nincs aktív háttérmunka. A korábbi ellenőrzési számok történeti adatok.

## Termékfolyamat az elsődleges mérföldkő — felhasználói döntés

A következő eredmény egy kipróbálható **rajz → geometriai javaslat → előnézet → összehasonlítás** folyamat (#24). A tesztszám és az issue-darabszám nem termékkészültségi százalék. Új infrastruktúrát és scene/pick/likeness bővítést csak a konkrét folyamat hiányai indokolnak.

Első támogatott eset: síkbeli, egy összefüggő külső kontúrt tartalmazó műszaki rajz, explicit pixel/méretegység aránnyal és megadott vastagsággal. Befoglaló durva mesh előbb, kontúrból extrudált szerkeszthető javaslat utána, végül előnézet és mérési riport. Rejtett felületek és mérési bizonytalanság explicit; nem általános fotórekonstrukció.

A különálló munkák párhuzamosíthatók, a közös szerződéseknek egy írójuk van. Régi implementáció csak működő fogyasztói átállás után kerül `.tmp/` alá. A kész körökhöz rendezett Git-mentési pontok szükségesek; a jelenlegi teljes, több körös munkafa keverékét nem címkézzük egyetlen új feature commitnak. A mentési bontás a további integráció része; push még nincs.

## Történeti állapot — tizenkettedik kör

Az integrátor a [view v2 életciklust](VIEW-OPERATIONS.md) megvalósította a meglévő termékstruktúrában. A worker-kvóta miatt nincs aktív párhuzamos agent. A következő külön munkaszeletek a standalone view scene/paint/pick/likeness lefedése és a B-rep konverziós bridge fogyasztóinak átállítása; a shared API-t előbb rögzíteni kell.

## Történeti párhuzamos kör — tizenegyedik kör

- **Archimedes / Sol high — #3:** tartalom- és konfigurációazonos decline memo, biztonságos fallback, invalidáció és valós triangulációs bizonyíték.
- **Sartre / Sol high — #7:** lokális u/v állomásriport, explicit pixel-lépcsőtűrés, kontúrbizonyíték és per-station matches.
- **Integrátor — #20 és összefésülés:** közös raw metadata/quality alaptípusok, domain ownership döntés, központi metadata/dependency gate; dokumentáció, issue-k és teljes integráció. #20 célzott ellenőrzés után lezárva.

Mindkét worker kvótakorlát után leállt; az integrátor befejezte és ellenőrizte a munkát. Nincs aktív worker. 1004 workspace teszt és a szigorú Clippy sikeres.

A meglévő helyi változások az integrált alap részei; nem írhatók felül. Külön fájltulajdonosok dolgoznak ugyanazon munkafában. Teljes workspace-ellenőrzés az integráció után, egy futtatóval. Nincs automatikus commit/push.

## Következő sorrend

1. #24/#26: a natív hétlépcsős egyrajzos út megvan; következő a sheet/részleges hibák és a teljes CLI lifecycle átvezetése a hostra, azonos kimeneti viselkedéssel.
2. #24: bizonyítékalapú refinement, tartós parancsnapló és replay, valós rajzokhoz fidelity küszöbök. A részletes sorrend a [runtime tervben](SHARED-RUNTIME.md).
3. #15: revízióhoz kötött proposal accept/discard, AI-trigger/watch és PMI-anchor. A host frissességellenőrzése alap, nem kész AI-réteg.
4. #22: CTM capability registry, assembly és kényszerek. Field/SubD/szimuláció a saját doménfeladatok szerint. #16 külső peer async/cancel nem blokkolja a belső helyi számítást.
5. Rendezett Git-mentési pontok a meglévő több körös változások áttekinthető bontásával. A felhasználói submodule-változások külön maradnak.

A név szerinti F-35/737/moai tesztek továbbra is elvetve; #18 nem új blokkoló. #1/#2/#3/#7/#20/#21 lezárása nem a teljes termék elkészültét jelenti.

## Legutóbbi blokk munkamegosztása és folytatás

Hilbert (Sol high): típusos megosztott krlyvis evidence és wire adapter. Helmholtz (Sol high): retained B-rep/export és célzott review. Averroes (Luna high): kis peer hibaparitás-javítás. Integrátor: host revízió/dependency/admission/esemény, CLI, közös Cargo/kapuk, integráció és ticketek. Az architektúradöntés és a közös szerződések egy kézben; külön fájltulajdonosok párhuzamosan dolgoznak. Astra nem szükséges minden szelethez.

A tizennyolcadik blokkban Hume (Sol high) a közvetlen snapshot-rendert és a coarse
lépést, Socrates (Sol high) a natív likeness szerződést és bridge-et, Turing (Luna high)
a stream és review ellenőrzését készítette. Az integrátor a host control/review állapotát,
a hétlépcsős CLI-t, a függőségi kapukat és az összevont ellenőrzést kezelte.

## Agentválasztás

- **Luna medium:** rögzített szerződés melletti dokumentáció, fixture-metaadat és mechanikus adaptermunka.
- **Sol medium/high:** alapértelmezett implementáció, Rust, tesztek; concurrency és numerika high.
- **Astra:** konkrét nehéz szemantikai/numerikus döntés vagy célzott review; nem szükséges minden feladathoz.

Legfeljebb négy párhuzamos worker, ebből legfeljebb egy Astra. A közös fájloknak egy írójuk van; a workspace manifest és lockfile módosításait az integrátor koordinálja. Modellváltás konkrét elakadás vagy nehéz döntés alapján, nem buildkörnyezeti hiba miatt.

## Struktúra és félretétel

A [migrációs szabályok](PRODUCT-ARCHITECTURE-HANDOFF.md#struktúraváltás-és-elavult-fájlok) szerint egy domén költöztetése a stabil szerződéshez és paritástesztekhez kapcsolódik. A használatban lévő régi engine és kompatibilitási réteg nem elavult attól, hogy a célarchitektúra más. Bizonyítottan kiváltott anyagok a gyökér [.tmp](../.tmp/README.md) mappába kerülnek, eredetük és visszaállítási helyük rögzítésével. Végső törlés a projekt befejezésekor, külön takarítási lépésben.
