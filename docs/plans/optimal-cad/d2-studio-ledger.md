# D2 Studio: bizonyítottan halott kód audit

Dátum: 2026-11-15. Hatókör: `products/krly3d/studio/service` és `web/src`,
az importok, exportok, build és tesztfogyasztók ellenőrzésével.

## Döntés

Ebben a szűk auditban nem találtam biztonsággal törölhető szeletet. Az aktív
forrásokat nem módosítottam, és bizonyíték nélkül nem archiváltam vagy töröltem.
Az eredmény nem állítja, hogy minden ág szükséges; a modulgráf és a használati
ellenőrzések alapján most nincs igazolt eltávolítási jelölt.

## Bizonyíték

- A két belépési pontból (`service/main.ts`, `web/src/main.tsx`) követett relatív
  importgráf 37 vizsgált, nem tesztnevű TS/TSX fájlból 36-ot elér.
- Az egyetlen kívül maradó modul, `service/ai/test_support.ts`, többek között a
  provider, workflow, character, research és CAD review tesztek aktív függősége.
- A termelési TS/TSX exportok név szerinti, Studio-szintű keresése nem adott
  kizárólag saját deklarációjában előforduló exportot. Ez jelöltkeresés,
  önmagában nem teljes szemantikus elérhetőségi bizonyítás.
- A `RevealTimeline` és `createRenderGeometry` a Viewportból, a `validateMesh`
  a modelből használt. A CAD construction és reveal külön tesztfogyasztót is kap.
- A research cache, image search és CAD guidance a workflow/expert aktív
  függősége. A demo/complex/character módok az RPC és UI aktív útvonalai.
- A meglévő CAD E2E, valamint az e2e/studio.spec.ts a modellfa és replay
  viselkedést fogyasztja; ezekhez tartozó segédek eltávolítása nem halottkód-törlés.

## Ellenőrzés és fizikai sorelszámolás

Sikeres: frontend `tsc --noEmit -p web/tsconfig.json --noUnusedLocals
--noUnusedParameters`; `npm run build`; `deno check service/main.ts`;
`deno lint --rules-tags= --rules-include=no-unused-vars service` (45 fájl).
A teljes ajánlott Deno lint külön futtatása 22 meglévő require-await és
no-control-regex jelzést adott; ezek nem halottkód-bizonyítékok, nem javítottam őket.

Aktív kód: bruttó törölt 0, hozzáadott 0, nettó csökkenés 0 fizikai sor.
Archívum: 0 fizikai sor; nem készült másolat. E ledger új dokumentáció,
40 fizikai sor hozzáadás, nem kódcsökkentés. A kódleltár változása 0 sor; a dokumentációé +40 sor. Restore: nincs visszaállítandó forrásmódosítás; a ledger
eltávolítása elegendő kizárólag ennek az auditnak a visszavonásához.

## Későbbi P2 integráció

A működő típusos Studio-út után a review jelöltet már a natív elfogadó
egyetlen `cad-candidate` hívással kéri a publication-as-code CLI-tól. A Studio
nem futtatja ugyanezt előtte még egyszer, és nem ír `candidate.json` köztes
fájlt. A helyi előzmény acceptora 41 régi fizikai sort törölt és 30-at adott
hozzá (nettó −11 sor); a két CLI közti jelölt-azonosságot az egységes port
biztosítja. Ez bizonyítottan kivett munka, nem a nagyobb régi receptes Studio
ág eltávolítása: az a természetes nyelvű építéshez továbbra is aktív. A teljes
P2 szelet új szolgáltatást, UI-t, szerződést és teszteket is hozzáad, ezért a
repo nettó mérete ebben a körben nőtt; az új funkcionalitást nem könyveljük
megtakarításnak. A két régi út összevonása külön fogyasztói átállási kapu.
