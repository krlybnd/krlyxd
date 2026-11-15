# D5 porting audit

Dátum: 2026-11-15. Döntés: történeti Rust forrás nem törölhető a most igazolt
bizonyíték alapján. A `porting/README.md` feltétele az állítások átvitele,
nem pusztán egy azonos nevű új modul vagy zöld új tesztcsomag létezése.

## Konkrét megfeleltetések és hiányzó bizonyíték

- `src/section/{mod,rules,tests}.rs`: mai helyettesítő a
  `crates/std/src/section/` és a kernel section contract. A régi
  `a_plane_cuts_a_cylinder_in_a_circle_an_ellipse_or_rulings` teszt ferde sík
  és henger ellipszisét is követeli, döntött hengeren is. A mai
  `plane_cylinder.rs` ezt kifejezetten Declined eredménnyel kezeli, amit saját
  `an_oblique_plane_is_declined_and_a_tangent_plane_touches_along_a_ruling`
  tesztje elvár. Ez bizonyított funkcionális rés: a régi fájlok megmaradnak.
- `src/naming/{mod,tests}.rs`: mai rokonok a topo névkezelés, a
  `crates/std/src/tool/blend.rs` és queries tesztek. A chamfer/fillet és egymás
  utáni blend tesztek léteznek, de nem igazolt a teljes régi edge-name
  sorrend, több közös él midpoint-alapú sorszámozás és inherit_from provenance
  megfelelése. E részfájlok teljes kiváltását nem állítom.
- `src/kernel/{mod,probe,visit,tests}.rs`: mai Probe port, exhaustive/box-tree
  implementációk és szerződéstesztek léteznek. A régi teszt 405 konkrét sugár
  azonosságát, több látogató egy walk alatti számlálóit, és layer refresh
  szerződést ellenőriz. Ezek együttes átviteli bizonyítása nem készült el.
- `src/perception/{mod,index,probe}.rs`: a queries tesztek valódi here/ray/find/
  index/rollback fogyasztók, de ez nem teljes fájlszintű szemantikus ekvivalencia.
- `src/boolean/{mod,legacy,tests}.rs`: az új port/plugin architektúra rokon,
  de a régi adapter külön MultiToolOutcome és elnevezési szerződése miatt
  nem elégséges érv a fájlok törléséhez a mai boolean képesség megléte.
- `src/protocol/mod.rs`, `src/bin/krly3d.rs`: a mai CLI plain dialect és
  program tesztek stdin/stdout működést fednek; a történeti create/world-graph
  hívások teljes protokoll-paritása nincs igazolva. A régi belépési pontot
  sem töröltem csak azért, mert rövid és jelenleg nem fordul.
- `corinthian2.rs`: teljes geometriai demonstrátor, nem puszta adapter;
  azonos modellre vonatkozó mai összehasonlító QA nélkül megmarad.

## Ellenőrzés

`cargo test -p krly3d-std --lib section:: --locked --offline -q`: 19 sikeres.
`cargo test -p krly3d-std --test queries --locked --offline -q`: 18 sikeres.
Ezek a mai működésről adnak bizonyítékot; a régi fájlokat nem fordítják és nem
bizonyítják az összes történeti állítást. Törléshez nincs teljes paritási kapu.

## Archívum, sorelszámolás és restore

A helyi és remote-tracking refek között nincs `legacy-opengeometry`.
A README állítását pontosítottam, távoli ág létezéséről nem tettem állítást.
Az eredeti README: `.tmp/refactor/d5-porting/porting/README.md` (8 fizikai sor).
Rust: bruttó törölt 0, hozzáadott 0, nettó csökkenés 0 fizikai sor.
README: 1 régi sor helyett 3 új sor; nettó növekedés 2 fizikai sor.
Ledger: 54 új dokumentációs sor; a kódleltár változása 0 sor, a dokumentációé +56 sor, archívum +8.
Restore a repository gyökeréből:
`cp .tmp/refactor/d5-porting/porting/README.md porting/README.md`.
Csak az audit visszavonásához e ledger eltávolítható; történeti Rust restore nem kell.
