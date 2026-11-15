# P1: típusos geometriai review és helyi elfogadott előzmény

Dátum: 2026-11-15. Ez a CLI-n futó függőleges szelet a már létező modell
geometriai céljának revízióját, az új terv keresését és a helyi undo/redo
állapotot kapcsolja össze. A Studio és a természetes nyelvi értelmező még nem
fogyasztja ezt az útvonalat.

A [review-olvasó](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/cad_review.rs)
név szerinti `geometry_replace`, `geometry_add`, `geometry_retract` és
`geometry_preserve` műveleteket kezel. A `change_geometry(old, new)` a régi
feltételt pontosan a báziscélhoz hasonlítja; hibás régi érték vagy védett
feltétel módosítása hiba. A teljes új cél a revíziós rekordba kerül. A
[szerződés](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/cad_contract.rs)
terve és geometriai célja külön `revision` mezőt kapott, mert csak geometriai
változásnál a skalár specifikáció hash-e változatlan maradhat. A natív
[ellenőrző](../../../crates/cli/src/cad_check.rs) a régi revíziójú tervet
ilyenkor is elutasítja. Mindkét repó `serde_json`-ja pontos lebegőpontos
körbeolvasást használ a revíziós cél és a helyi napló ismételt betöltéséhez.

A [topológiaváltó review](../../../cad-knowledge/cases/solid-ring-fill-hole.typ)
a gyűrű középső pontját ürességből anyaggá változtatja, és a térfogati célt
ennek megfelelően cseréli. A külső átmérő, magasság és falpont megmarad. A
`cad-candidate <review-state.json>` kézi új terv nélkül ad célhoz kötött
kontraktust. A korlátos kereső egy `disk(R)` + `extrude(h)` tervet talált az
első natív próbában; 5/5 feltétel teljesült. Mért térfogat:
`5.6548667764616275e-6 m³`. A régi annulus-terv és az új cél együttes
ellenőrzése `unknown` státusszal, 1-es kilépési kóddal elutasított.

A [helyi előzmény](../../../crates/cli/src/cad_check/cad_history.rs) csak
`tested` jelöltet fogad el. Az alapmodell `--accept` paranccsal, a következő
revíziók kötelezően `--accept-reviewed` paranccsal vehetők fel. Az utóbbi a
publication-as-code validátort futtatja, és az előző elfogadott állapotot,
a review előtte/utána specifikációját és geometriai célját, valamint a
jelöltet pontosan egyezteti. Egy fájl egy modellt tárol, legfeljebb 128
elfogadott bejegyzéssel. A revízió és modellazonosító ellenőrzött; újranyitás,
undo, redo és konkrét bejegyzésre checkout natív újraellenőrzést végez. Az
elfogadott bejegyzések elágazásnál sem törlődnek. A mentés azonos könyvtárban
ideiglenes, Unix alatt 0600 jogosultságú fájlon keresztül történik, majd
átnevezéssel vált aktívvá. A CLI jelenleg egy íróval használható; a napló
nem hitelesített provenance vagy többfolyamatos tranzakciós adatbázis.

Reprodukció a krlyxd gyökeréből, mindkét CLI fordítása után:

```sh
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-contract cad-knowledge/cases/solid-ring.typ > /tmp/cad-ring-base.json
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-review cad-knowledge/cases/solid-ring.typ cad-knowledge/cases/solid-ring-fill-hole.typ > /tmp/cad-ring-review.json
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-candidate /tmp/cad-ring-review.json > /tmp/cad-ring-revised.json
target/debug/krly3d-cad-check --synthesize /tmp/cad-ring-revised.json 2000
target/debug/krly3d-cad-check --accept /tmp/cad-ring-history.json /tmp/cad-ring-base.json
target/debug/krly3d-cad-check --accept-reviewed /tmp/cad-ring-history.json /tmp/cad-ring-review.json /home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci
target/debug/krly3d-cad-check --undo /tmp/cad-ring-history.json
target/debug/krly3d-cad-check --redo /tmp/cad-ring-history.json
target/debug/krly3d-cad-check --show /tmp/cad-ring-history.json
```

QA: publication-as-code workspace 70 teszt, krlyxd CLI 130 teszt; célzott
Clippy `-D warnings` mellett mindkét repóban átment. A teljes két-CLI útvonal,
az újranyitás, az undo/redo, a stale elfogadás elutasítása és a régi terv
revíziós elutasítása is lefutott. A megfigyelt siker továbbra is véges
mérésekre épül: `geometry_verified:false`, `optimality_proven:false`.

Kódmérleg: ebben a szeletben régi produktív kódot nem töröltünk. Az új helyi
napló 214 fizikai Rust-sor; a natív CLI-fájl 535-ről 592-re nőtt, a kereső
329-ről 331-re. A `.typ` szerződésolvasó 951-ről 975-re, a review-olvasó a
korábbi 450 soros ellenőrzőpontról 743-ra nőtt. Ez legalább 590 új fizikai
Rust-sor az érintett fájlokban, plusz CLI-illesztés és tesztek. A külső repónak
nincs Git HEAD-je, ezért teljes, megbízható added/deleted diff nem áll
rendelkezésre; a korábbi 191 soros nettó csökkenés másik szelethez tartozik.

Nyitott kapuk: a review-mondat célkijelöléshez kötött értelmezése, általánosabb
geometriai relációk és stabil datumok, részleges újraépítés, Studio-diff és
undo, elutasított javaslatok külön naplója, a típusos review-állapot Studio-ba
vezetése, valamint a teljes alakellenőrzés. A
provenance-kötés részlete és QA-ja a [következő szeletben](p1-reviewed-acceptance.md)
található. A napló nem kriptográfiailag hitelesített, az alapmodell JSON-ja
pedig külön bizalmi gyökér: `--accept` csak annak natív ellenőrzését végzi.
