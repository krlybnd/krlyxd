# Review-javítás: jelenlegi határ és következő függőleges szelet

Állapot: 2026-11-15, forráskód-audit. Ez a dokumentum megvalósítási kapu; nem állítja, hogy a review-javítás már működik.

Későbbi P0 állapot: a [típusos review-delta](p0-review-delta.md) a krlysci-ben
már fut a specifikációs szinten. A lent felsorolt Studio-kijelölés, natív
újraépítés, geometriai diff és undo ettől továbbra is hiányzik.

Újabb P0 állapot: a [natív mérési szelet](p0-native-measurements.md) egy
CLI-fixture-ben review után újraépítette és mérte a gyűrűtestet. A Studio
kijelöléshez kötött újraépítés, a képernyős geometriai diff és a Studio undo
továbbra is hiányzik.

P1 CLI-állapot: a [strukturális review és helyi előzmény](p1-structural-review-history.md)
név szerinti geometriai célcserét, natív újrakeresést és fájlban megőrzött
elfogadott revíziók undo/redo műveleteit futtatja. A
[review-hoz kötött elfogadás](p1-reviewed-acceptance.md) a típusos rekordot is
az aktív szülőhöz egyezteti. A lenti Studio-kijelölés,
látható geometriai diff és szerveres többfelhasználós revíziókezelés továbbra
is megvalósítási feladat.

## Ami ma ténylegesen megvan

- A Studio modellfa egy futás `Construction.features` listáját és a hozzá tartozó natív mesh-lépéseket jeleníti meg ([CadFeatureTree.tsx](../../../products/krly3d/studio/web/src/CadFeatureTree.tsx), [Viewport.tsx](../../../products/krly3d/studio/web/src/Viewport.tsx)). A kijelölt feature ID a Viewport lokális React-állapota.
- A kliens `ModelState` egy előnézetet csak sikeres terminális esemény után erősít meg, hibánál visszaállítja az előző megerősített snapshotot ([model.ts](../../../products/krly3d/studio/web/src/model.ts)). Ez jó tranzakciós alap, de nem modellrevízió/undo-napló.
- A szerver ellenőrzi az új, teljes `CadRecipe` terv natív végrehajtását, majd egy AI-review elfogad/elutasít ítéletet kér a szöveg, válaszok, recept és mért befoglaló/térfogat/felület alapján ([workflow.ts](../../../products/krly3d/studio/service/workflow.ts), [cad-review.ts](../../../products/krly3d/studio/service/ai/cad-review.ts)). Az elutasítás egy célzott újratervezési kör bemenete; nem típusos, már meglévő modellre alkalmazott követelménydelta.
- A szerveres `SessionStore` legfeljebb 12 üzenet/48 000 karakter előzményt tart, inaktív munkamenetet idővel töröl; a `BlobStore` fájljai is lejárnak ([store.ts](../../../products/krly3d/studio/service/store.ts)). Ez nem tartós, elfogadott modell- és korrekciótár.

## Hiányzó szerződés a felhasználó által kért javításhoz

Az aktuális [RunRequest](../../../products/krly3d/studio/protocol/krly/studio/v1/studio.proto) mezői között nincs `model_id`, `base_revision`, `selected_feature_id` vagy kijelölt geometriai referencia. A kliens [execute](../../../products/krly3d/studio/web/src/App.tsx) jelenleg szöveget és futási beállításokat küld; a [Viewport](../../../products/krly3d/studio/web/src/Viewport.tsx) kijelölése nem jut el ehhez a kéréshez. A szerver ezért az „ezt tedd vastagabbá” típusú mondatot nem kötheti biztonságosan az éppen kiválasztott feature-höz. A promptbeli chat history nem helyettesíti a pontos modellt, a régi feltételt és a megőrzendő relációkat.

A `CadRecipe` mai műveletsora és a `Construction` megjelenítési rekordja nem azonos a tervezett típusos `DesignSpec`-kel. A specifikáció, terv, natív geometria, ellenőrzés és elfogadás közti stabil, verziózott kapcsolat hiányzik. Emiatt a `domIds` pilot önmagában csak a UI-célzást teszi stabilabbá; a review-funkciót nem teljesíti.

## Minimális, ellenőrizhető sorrend

1. **Állapotazonosság.** A szerver mentse a validált `DesignSpec`-et, `FeaturePlan`-t, natív eredményhash-t és ellenőrzést egy modellrevízióhoz. A klienshez `model_id`, `revision`, stabil feature-/datum-/selection-hivatkozások érkezzenek. Azonosítók feloldása csak a konkrét modellben és revízióban történjen; stale kérés hiba.
2. **Review-kérés.** Külön parancs vagy változatdiszkriminált Run bemenet hordozza az új felhasználói mondatot, a bázisrevíziót és a kijelölést. A `question_id`-s pontosítás nem kezelhető rejtett modellfrissítésként. Egy kérésből legfeljebb egy új jelöltrevízió készülhet.
3. **Típusos delta.** A nyelvi értelmező `add | replace | retract | preserve` műveleteket javasolhat követelményazonosítókra, hatókörrel és a kiválasztott modellhivatkozással. A validator ellenőrzi az egységet, a régi feltétel létezését, a megőrzendő tételeket és a revíziót. A `replace` ténylegesen lecseréli a régi értéket, nem ad mellé ellentmondó második feltételt.
4. **Végrehajtás és ellenőrzés.** A validált új specifikációból a korlátos kereső/tervező készít jelöltet. A szükséges részgráf újraépül, és a teljes eredmény kötelező invariánsai is ellenőrződnek. `pass`, `fail`, `unknown` és `not_applicable` külön marad. Hibás vagy megszakított jelölt nem válik elfogadott állapottá.
5. **Jóváhagyott helyi előzmény.** A képernyő diffet mutat, a korábbi revízió megmarad és undo/redo elérhető. Csak elfogadott korrekció kap később újrahasználható precedensrekordot; egy adott modellen elfogadott méret nem válik más modellek általános szabályává.

Az első tesztpár: ismert paraméter cseréje kijelölt feature-en megőrzött további dimenzióval, majd szándékos régi revíziós kérés elutasítása. Következő esetek: kijelölt pattern mozgatása, két modell azonos feature ID-val, újranyitás, undo/redo, és egy valóban kétértelmű geometriai irány célzott tisztázása. A sikerkritérium a kért és megőrzendő relációk mérhető teljesülése, nem egy sikeres szöveges válasz.

## Kódkivételi következmény

A jelenlegi [expert.ts](../../../products/krly3d/studio/service/ai/expert.ts), [cad-recipe.ts](../../../products/krly3d/studio/service/ai/cad-recipe.ts) és [cad-review.ts](../../../products/krly3d/studio/service/ai/cad-review.ts) aktív újmodell-útvonalat szolgálnak ki. Ebből ma nem következik biztonságos teljes törlés. Funkciónként kell kiváltani és a fogyasztókat átvezetni; a review-delta és általános szintézis kapuja előtt törölt planner csak működő funkciót venne el.
