# P1: review-hoz kötött elfogadás

Dátum: 2026-11-15. A CLI helyi előzményében a 0. revízió az alapállapot;
újabb revízió elfogadásához típusos `cad-review` eredmény kell. Ez a szelet
nem változtatja meg a Studio jelenlegi receptes tervezőútvonalát.

## Követelmény és szerződés

- Egy revízió csak az aktív, már elfogadott modell közvetlen folytatásaként
  vehető fel. A modellazonosító, a revíziószám és a specifikáció hash-e
  egyezzen.
- A review `before` specifikációja és `before_target` célja pontosan az aktív
  elfogadott szerződéshez tartozzon. A review `after` és `after_target` pontosan
  a jelölt specifikációja és célja legyen. A terv lehet új vagy szintetizált.
- A publication-as-code validálja a típusos revíziós rekordot, a hash-eket és
  a delta tartalmát. A natív oldalon minden jelölt újra `tested` státuszt
  érjen el. A véges próba nem alakazonossági bizonyítás.
- A review az elfogadott bejegyzés része maradjon, és betöltéskor is
  ellenőrizhető legyen a szülő és a jelölt közötti kötés.

## Technikai határ

A [helyi előzmény](../../../crates/cli/src/cad_check/cad_history.rs)
`--accept-reviewed <history> <review-state> <krlysci-bin> [budget]`
parancsa a pontosan beolvasott review-t stdin-en adja át a megadott
publication-as-code CLI `cad-candidate -` parancsának. Így nem fordítási idejű
függőség a másik repository, és a validátor ugyanazokat a bájtokat vizsgálja,
amelyeket a napló megőriz. A [CLI stdin-portja](../../../../../orplex/repos/orplex-kalyx/crates/krlysci-cli/src/main.rs)
legfeljebb 256 KiB állapotot vesz át. A natív folyamat egyezteti a validátor
eredményét mint egyetlen jelöltet, a review előtte/utána állapotát és az aktív naplóbeli szülőt;
utána korlátos keresést végez, ha a jelöltben nincs terv. A sima `--accept`
csak a 0. revízióra használható. A `--show` válasz `reviewed` mezője jelzi a
revíziós kötést.

Az explicit validátor-útvonal a két repository közötti futásidejű port. A
validátor futtatható állománya és az alapmodell eredete a helyi környezet
bizalmi határa. A JSON-napló nem aláírt; a kötés adatkonzisztenciát ellenőriz,
nem szerzőséget. Régi, review nélküli naplóbeli bejegyzések olvashatók maradnak,
de a CLI már nem hoz létre új, review nélküli revíziót.
Ez a parancs jelenleg csak olyan review-t fogad el, amely maga tartalmazza az
új geometriai célt; a pusztán skaláris review-hoz külön jelöltcél-validáló port
kell. Ezt nem kerülhetjük meg a cél nélküli review hallgatólagos elfogadásával.

## QA

A [gyűrűt tömörítő review](../../../cad-knowledge/cases/solid-ring-fill-hole.typ)
alapján a két CLI-val végigfutott az alapelfogadás, revíziós elfogadás,
`--show`, undo és redo. Az el nem látott revízió `--accept` alatt elutasított;
a megváltoztatott cél-hash-t a típusos validátor utasította el. Az egységteszt
az eltérő szülőcélt és a naplóban utólag megváltoztatott review-t is elutasítja.
A publication-as-code workspace 70 tesztje, a krlyxd CLI 130 tesztje, a
célzott Clippy és a formázásellenőrzés átment.

Reprodukció a krlyxd gyökeréből, a két CLI fordítása után:

```sh
science=/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci
native=target/debug/krly3d-cad-check
"$science" cad-contract cad-knowledge/cases/solid-ring.typ > /tmp/cad-base.json
"$science" cad-review cad-knowledge/cases/solid-ring.typ cad-knowledge/cases/solid-ring-fill-hole.typ > /tmp/cad-review.json
"$native" --accept /tmp/cad-history.json /tmp/cad-base.json
"$native" --accept-reviewed /tmp/cad-history.json /tmp/cad-review.json "$science"
"$native" --show /tmp/cad-history.json
```

Ebben a szeletben produktív kódot nem töröltünk: a napló kötése és a CLI
stdin-portja új kód. A korábbi teljes kódmérlegből ezért nem vonunk le újabb
törölt sort. A következő Studio-kapu külön, típussal jelölt review-műveletet,
előtte/utána diffet és aktív előzménykurzort igényel; a meglévő `CadRecipe`
AI-review-t addig nem lehet biztonságosan eltávolítani.
