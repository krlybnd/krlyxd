# 0014 — Térfogati figura, önálló mesh és STL

Dátum: 2026-11-14. Állapot: helyi pilot.

## Kiindulás

A felhasználó teljes térbeli, nyomtatáshoz használható Pikachu-figurát kért.
A korábbi sziluettkihúzás erre nem alkalmas. A mesh előállítását nem kötjük
kötelező B-rep köztes reprezentációhoz.

## Döntés

- A [field API](../../products/krly3d/field/api/src/lib.rs) verziózott,
  milliméteres, korlátos JSON-receptet fogad. A levélalakzatok ellipszoidok,
  kapszulák és forgatott, lekerekített dobozok; a műveletek unió,
  simított unió, különbség és metszet. A hivatkozások előre felépített DAG-ra
  mutatnak, végrehajtható kód nélkül.
- A [field motor](../../products/krly3d/field/engine/src/lib.rs) mintavételezi
  a mezőt, majd közös rácséleken összehegesztett marching-tetrahedra hálót
  ad a meglévő semleges `MeshData` szerződéssel. Nem függ a B-rep motortól.
  A Rust-hívó saját cancellation/deadline vezérlést adhat át.
- A [nyomtatási előellenőrzés](../../products/krly3d/mesh/api/src/print.rs)
  a mesh domén része. A [bináris STL-író](../../products/krly3d/exchange/mesh/src/lib.rs)
  külön exchange-csomag, explicit mm megadással és az STL-be kerülő float32
  koordináták ismételt ellenőrzésével. Nincs automatikus hálójavítás.
- A Studio külön `character` művelettel futtatja a
  [rögzített figurareceptet](../../models/pikachu.field.json), majd a natív
  ellenőrzőt és exportálót. A meglévő mesh/artifact adatút és kirajzolás
  használható; a végső fázis `final_mesh`, nem szerkeszthető B-rep javaslat.

## Pontosság és határok

A rácslépés mintavételi paraméter, nem igazolt geometriai hibahatár. Az
ellipszoid mezője távolságszerű közelítés, nem egzakt SDF. A rács alatti
részletek elveszhetnek; a tartomány szélét a rácsmintákon ellenőrizzük.

A nyomtatási jelentés elkülöníti a vizsgált hálótulajdonságokat az ismeretlen
gyártási feltételektől. Az önmetszésvizsgálat numerikus és korlátos, nem egzakt
bizonyítás. A falvastagság, a nyomtató anyaga/profilja és a támaszigény nem
vezethető le pusztán a zárt háló jelzéséből. Az STL önmagában nem tárol
mértékegységet; az exportáló és a jelentés rögzíti az mm-értelmezést.

Ez első paraméteres figura és általános field-alap, nem általános képből
karaktergeneráló AI, sculpt-rendszer vagy nyomtatóvezérlő. A figura kézzel
összeállított recept; a képen nem látható részei tervezési döntések.
A field CLI jelenleg helyi adapter; a teljes verziózott doménhívás/MCP,
dual contouring, szerkesztési eszközök és a tartós shared-runtime artifact
integráció további munka. A meglévő B-rep út továbbra is a CAD-feladatoké.

## Korlátok és futtatás

A recept legfeljebb 128 csomópontot, a rács 4 millió mintát, a kimenet
500 ezer háromszöget enged meg. A Studio külső folyamatadaptere a meglévő
határidő-, fájlméret- és megszakításkezelését alkalmazza.

```sh
cargo build -p krly3d-field-cli -p krly3d-mesh-exchange
target/debug/krly3d-field --recipe models/pikachu.field.json --output /tmp/pikachu
target/debug/krly3d-print --mesh /tmp/pikachu/mesh.json --output /tmp/pikachu/pikachu.stl --report /tmp/pikachu/print-report.json --units mm
```

A teljes #12 field, #13 mesh/exchange és #24 képrekonstrukció elfogadását
ez a pilot nem zárja le.
