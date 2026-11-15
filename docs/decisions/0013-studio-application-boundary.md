# ADR 0013 — Studio alkalmazáshatár

Állapot: elfogadott első implementáció, 2026-11-14.

## Döntés

A felhasználó a mai 3D kipróbáláshoz React/TypeScript felületet, gRPC kapcsolatot,
Deno/TypeScript szolgáltatást, szerveroldali LangGraph vezérlést, LiteLLM-et és
kép-/hangbevitelt kért. A helye `products/krly3d/studio`: alkalmazáskompozíció,
nem a nyers kernel vagy egy geometriai domén kiterjesztése.

A `publication-as-code` és a korábbi `lunecue` szolgáltatáshatár-mintáját követjük:
verziózott protobuf, generált TypeScript, Connect Node adapter Deno alatt,
böngészőből bináris gRPC-Web. A `orplex-zeus`/Osirsys LiteLLM aliasai és a
működő laptopos beszédfelismerés mintái külön adapterben maradnak. A
referenciaprojektek fájljait nem módosítjuk.

## Végrehajtás

A kódban definiált LangGraph `expert → geometry → respond` munkafolyamatot
futtat. Az expert alternatívája a közvetlen válasz, egy pontosító kérdés, illetve
legfeljebb egy `research → expert` kör. A szolgáltatás a kérdést az eredeti
bemenettel és döntésekkel a munkamenetben tárolja; a válasz új RPC-ban folytatja
a gráfot. Egyszerre egy A–D multiselect kérdés aktív; várakozás közben nincs
foglalt végrehajtási hely. A memóriabeli kérdés 30 percig él, nem tartós checkpoint.
A demó és a közvetlen Építés determinisztikusan kihagyja az AI-t.
Felületi paramétert a modell nem írhat át. A szolgáltatás rövid műveleti
összefoglalót és valódi eszközeseményeket közvetít, nem rejtett gondolatmenetet.

A fix Rust CLI-t korlátos byte/NDJSON adapter indítja. A natív rekonstrukción
belül változatlanul az ADR 0011 közös runtime-ja dolgozik. A folyamatok és a
böngésző között másolunk; ez nem egy új peer-hálózat és nem folyamatokon át
megosztott memória. Tartós Rust daemon/FFI egy későbbi mérés alapján válthatja
ki ezt a külső adaptert. TypeScriptben nem implementálunk geometriát.

## Életciklus és adat

A szolgáltatás helyi, loopbackre kötött, egyszerre két műveletet enged;
munkamenetenként egyet. A producer/consumer sor, natív protokoll, képek,
hálóadatok, history és artifactok korlátosak. Abort/deadline eljut a providerhez
és a natív folyamathoz; a child reaping és temp-cleanup lelassult fogyasztó
mellett is működik. A korai mesh előzetes; sikertelen futás nem válhat kész
modellé a felületen.

A fájlok és sessionök memóriában élnek, egyórás lejárattal. Újraindítás után
nem állnak helyre. A letölthető k3d/PNG/riport nem tartós parancs-replay.
A #24 revízióbiztos refinement, tartós replay és teljes scheduler/budget
elfogadása ettől nem lesz teljesített.

## AI és hang

Az upstream kulcs csak a saját LiteLLM-proxy privát környezetében van; a Deno
csak proxykulcsot kap, a böngésző egyiket sem. A referencia Bedrock/Grok profil
mellett külön OpenAI-profil indulhat a 4010-es porton. Embedding csak explicit
igényre hívható, nincs automatikus embeddingköltség minden üzenetnél.

Az expert/research hívások tokenadatai és proxy által jelentett USD díjai külön
alkalmazásszerződésben érkeznek. Azonos hívásazonosítóval közölt pillanatképek
nem számlázódnak újra a felületen. A becslés és a hiányzó/részleges adat láthatóan
elkülönül; ismeretlen díj nem válik nullává. Keresés csak explicit konfigurált
Responses web search útvonalon történik, korlátos nyilvános lekérdezéssel.

A mikrofonfelvételt a böngésző MediaRecorderrel készíti, a szerver végzi az
átírást. LiteLLM, Chromium kompatibilitás és Azure választható. Engedélykérés,
leállítás, késői engedély, idő-/méretkorlát és megszakítás külön kezelt állapot.
A transcript előbb a szerkeszthető beviteli mezőbe kerül.

## Scope

Ma raszteres kontúr → uniform kihúzás → B-rep/mesh kipróbálás. SVG/DWG-import,
CFD, általános fotórekonstrukció és tartós többfelhasználós CAD-dokumentum nincs
beleértve. Indítás és aktuális korlátok: [Studio](../../products/krly3d/studio/README.md).
