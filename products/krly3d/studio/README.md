# KRLY Studio

Helyi 3D munkatér: React/TypeScript és Three.js felület, protobuf/gRPC-Web
kapcsolat, Deno/TypeScript szolgáltatás és kódban definiált LangGraph munkafolyamat.
A geometria a meglévő Rust termékekben készül.

## Indítás

A repó gyökerében előbb `cargo build --workspace`. A Studio könyvtárában:

```sh
npm ci
npm run build
npm run service
```

A kész felület ekkor a http://127.0.0.1:4311 címen érhető el. Fejlesztéshez külön
terminálban `npm run dev`, majd http://127.0.0.1:4310. A szerkesztőben a
`Studio: Deno service`, `Studio: React UI`, `Studio: build` taskok is használhatók.
Előfeltétel: Node 22.12+, Deno 2, a natív Rust binárisok; protokoll-generáláshoz Buf.
Chromium/Azure hanghoz ffmpeg; a LiteLLM hangút közvetlenül fogadja a felvételt.

A demó és az **Építés** mód AI-kulcs nélkül is működik. A bal oldali munkatér
forgatható, nagyítható, eltolható; F: modellillesztés, 1/2/3: nézetek, nyilak:
forgatás, Shift+nyilak: mozgatás. A finom XY rács léptéke a nézettel együtt változik,
a Z tengely felfelé mutat. A jobb oldalon a tényleges műveleti események jelennek meg.

### Típusos CAD és helyi review

A **Típusos CAD** panel a `.typ` matematikai szerződésből, a külön telepített
`krlysci` CLI-ból és a `krly3d-cad-check` natív ellenőrzőből dolgozik; AI-kulcsot
nem igényel. Az alapmodell `#cad_spec` és `#cad_target`, a későbbi javítás
`#cad_review` hívást tartalmaz. A review csak az aktív modell és revízió
ellenében fogadható el. A panel a megváltozott feltételeket, az építési tervet,
az ellenőrzéseket és a natív hálót mutatja. Az elfogadott előzmény helyben
megmarad, visszavonható és újraalkalmazható; a böngésző az utolsó nyolc
munkamenet azonosítóját jegyzi meg. A mért próbák `tested` állapota nem
teljes geometriai vagy globális optimalitási bizonyítás.

A natív CLI-t a gyökérben `cargo build -p krly3d-cli --bin krly3d-cad-check`
fordítja. Az adjacens publication-as-code checkoutban a `krlysci` CLI-t is
le kell fordítani, és útvonalát `KRLYSCI_BIN` változóval megadni. A
`KRLY3D_CAD_BIN` az alapértelmezett `target/debug/krly3d-cad-check` útvonalat,
a `STUDIO_CAD_DIR` a helyi előzmények gyökerét írhatja felül. Példák:
[`solid-ring.typ`](../../../cad-knowledge/cases/solid-ring.typ) és
[`solid-ring-fill-hole.typ`](../../../cad-knowledge/cases/solid-ring-fill-hole.typ).
Az integrációs próba a Studio könyvtárából a `KRLYSCI_BIN` beállítása után
`deno test --allow-read --allow-write --allow-run --allow-env service/cad_test.ts`.
Ez a felület explicit `.typ` review-t fogad; a természetes nyelvű javítás
automatikus formalizálása és a geometriai előtte/utána overlay még nyitott.

## AI és hang

A szerver beállításait a Gitből kizárt `.env` tartalmazza; mintája
[config/env.example](config/env.example). A helyi OpenAI-proxyhoz a külön,
szintén kizárt `.proxy.env` szükséges:

```dotenv
OPENAI_API_KEY=helyben-megadott-upstream-kulcs
LITELLM_MASTER_KEY=helyben-generalt-sk-gateway-kulcs
```

A szolgáltatás `.env` beállításai ehhez:

```dotenv
LITELLM_BASE_URL=http://127.0.0.1:4010/v1
LITELLM_API_KEY=ugyanaz-a-gateway-kulcs
LITELLM_CHAT_MODEL=studio-fast
LITELLM_VISION_MODEL=studio-fast
LITELLM_SPEECH_MODEL=studio-speech
LITELLM_EMBEDDING_MODEL=studio-embedding
STUDIO_SPEECH_PROVIDER=litellm
STUDIO_SPEECH_LANGUAGE=hu-HU
LITELLM_SPEECH_LANGUAGE_FIELD=languages
```

`docker compose up -d` elindítja a saját proxyt a 4010-es porton;
`docker compose down` leállítja. A konfigurációváltozás után a Deno szolgáltatást
újra kell indítani. A [proxy profil](config/litellm-openai.yaml) jelenlegi aliasai:
GPT-4.1 mini chat/kép, gpt-transcribe hang, text-embedding-3-small opcionális
embedding. Más meglévő LiteLLM is használható. A
[referenciaprofil és hangútvonalak](config/README.md) megőrzik az Osirsys
Bedrock/Grok és Chromium/Azure lehetőségeit. A böngésző nem kap provider-kulcsot.

A mikrofon gomb csak kattintásra kér engedélyt; második kattintás lezárja és
átírja a felvételt. A szöveg az üzenetmezőbe kerül, elküldés előtt szerkeszthető.
A rögzítés legfeljebb 60 másodperc / 8 MiB, a hardver leállításkor felszabadul.
A provider hívások díjkötelesek lehetnek; az automatikus tesztek nem hívják őket.
A Health a konfiguráció meglétét jelzi, nem tesz minden ellenőrzéskor fizetős hívást.

## Támogatott 3D folyamat

Tölts fel vagy húzz az üzenetmezőre egy sötét, kitöltött sziluettet világos
háttéren, PNG/JPEG formátumban. Add meg a magasságot, a modell-egység/pixel
léptéket és a hálótoleranciát, majd **Építés**. A natív út kontúrt, korai hálót,
szerkeszthető B-rep modellt, végleges mesh-t, PNG előnézetet és geometriai
ellenőrzést ad. A kész fájlok letölthetők. Chatben a PNG/JPEG rajzról kérdezhetsz,
vagy kérheted ugyanezt a kihúzást; a felületi paramétereket az AI nem írja át.
Az AI képbemenet legfeljebb 8 MiB, a közvetlen natív építésé 16 MiB.
A szolgáltatás RPC-ja PNM képet is fogad közvetlen építéshez.

Kép nélkül is leírhatod az alkatrészt az **Építés** módban. A szaki a releváns
szabványos méreteket előbb szöveges, hivatkozott forrásból ellenőrzi, majd egy
legfeljebb 16 csomópontos, nyolc testműveletes milliméteres feature-receptet épít.
Az íves XY-vázlat, kihúzás, külön vak/átmenő zsebkivágás és támogatott natív
lekerekítés mellett doboz, henger, gömb, tórusz és boolean is elérhető. A testműveleteket a
natív B-rep kernel validálja és hálózza; a `.k3d` recept letölthető és
újrafuttatható. A natív mérés után külön, képet és új keresést nem használó szöveges tervellenőrzés
veti össze a kérést a recepttel és a befoglaló méretekkel. Legfeljebb egy javító
kör engedélyezett; hibás próbálkozásból nem kerül ki mesh vagy letöltés.
Ez modellalapú vélemény, nem teljes geometriai vagy szabványmegfelelőségi bizonyítás.
A sikeres egyszerű építés így két AI-hívás (tervezés + ellenőrzés); legfeljebb
négy, ha egy szemantikai hiba után javítás és ismételt ellenőrzés is szükséges.
A fogyasztás mindegyik hívást tartalmazza. A különálló vázlatok nem kész testek;
a felület a natív solid snapshotok mellett görbékként jeleníti meg őket.
Részletes MCP/katalógus irány: [ADR 0016](../../../../docs/decisions/0016-cad-tool-discovery-and-verified-workflow.md).
Ha egy szabványt nem sikerült forrással ellenőrizni, a Studio nem találgatja ki
a méreteit. Képkeresés csak a natív kontúrkihúzáshoz szükséges rajznál indul; a
keresés egyetlen hívásban legfeljebb három nyilvános képjelöltet kér le. A
forráslinkek megjelennek a felületen. Csak nyilvános HTTPS PNG/JPEG tölthető le
(8 MiB-ig, átirányítás nélkül), a vision modell pedig a natív építés előtt
ellenőrzi, hogy a kép illik-e a kéréshez és megfelel-e a kitöltött felülnézeti
sziluett feltételének. Ha nincs megfelelő kép vagy képelemző, a Studio rajzot
kér; nem indít üres vagy találomra választott geometriát. A DWG/DXF/STEP webes
fájlok importja ettől külön fejlesztési feladat; a szöveges építés még nem állítja,
hogy ezeket a formátumokat már beolvassa.

A munkamenet legutóbbi képét a chat csak támogatott PNG/JPEG formátum, méret
és konfigurált vision-provider mellett küldi újra képelemzésre. A natív-only
rajz megmarad építési bemenetnek; nem akadályozza a szöveges folytatást.
Az újonnan, explicit csatolt kép továbbra is a szigorú képbemeneti ellenőrzésen megy át.

### Szaki, utánanézés és pontosítás

A chat első lépése a **szaki**: röviden ellenőrzi a kérést, a szokásos megoldást,
a támogatott műveleteket és a lényeges bizonytalanságokat. Egyszerű kérésnél egy
döntési hívás elég. Ha külső tények segítenek, előbb legfeljebb egy rövid
szöveges webes keresés következik, majd a szaki összeveti a forrásokat a
feladattal. A drágább képi keresés csak akkor következik, ha a szöveges grounding
után is raster kontúr kell a natív építéshez. A kereséshez
`STUDIO_RESEARCH_ENABLED=true` és web search képes `LITELLM_RESEARCH_MODEL`
alias kell. A képtalálatokhoz a proxy alapértelmezésben a külön
`studio-image-search` aliasát használja; a `LITELLM_IMAGE_SEARCH_MODEL` ezt
felülírhatja. A `LITELLM_VISION_MODEL` szükséges a jelölt ellenőrzéséhez. A képkeresési kör
kérésenként legfeljebb egy, legfeljebb három képjelölttel. Forráslink csak a
keresési válasz valódi hivatkozásaiból kerül a felületre.

Fontos bizonytalanságnál egy **A–D jelölőnégyzetes kérdés** jelenik meg.
Egy vagy több válasz választható; több kérdés egymás után érkezik. A válaszadás
ugyanazt a kérést folytatja az eredeti képpel, beállításokkal és korábbi döntésekkel.
A várakozás nem foglal számítási helyet. Egy új kérés felülírja a régi kérdést;
a kérdés 30 perc után lejár, szerverújraindításkor elvész. Legfeljebb hat
pontosítás után új, összefoglalt kérés szükséges. Ez alkalmazásszintű HITL,
nem tartós LangGraph checkpoint vagy geometriai javaslatelfogadás.

A válasz **Használat és költség** része lenyitható: bemeneti/kimeneti/cache tokenek,
keresések, hívásonkénti és összesített USD költség. A folytatások ugyanazon kérés
halmozott adatait mutatják, a pillanatképek nem adódnak össze újra.
A költség elsődlegesen a LiteLLM `x-litellm-response-cost` fejlécéből származik;
csak külön ellenőrzött, konfigurált tarifa alapján lehet becsült. Hiányzó adat
**ismeretlen**, nem nulla; a részleges összeg külön jelölt. A keresés költségét
nem becsüljük pusztán a tokenekből. A panel a művelet expert/research hívásait
mutatja, nem a diktálás vagy az egész alkalmazás számláját. Megszakított hálózati
hívás tényleges díja nem mindig ismert a kliens számára.

### Kirajzolás és demók

A munkatér a beérkezett valódi háromszöghálókat fokozatosan rajzolja ki. A
**Kirajzolt háromszögek** számláló az aktuális háló megjelenítését követi; a natív
számítás állapotát a jobb oldali műveleti események jelzik. A natív futás
befejezése után a kirajzolás még folytatódhat.

- **Kirajzolás szüneteltetése / folytatása**: a megjelenítés lejátszását vezérli;
  a háttérben futó számítás tovább dolgozik.
- **Kirajzolás újra**: az adott futásban már beérkezett geometriai lépéseket
  játssza újra a böngészőben, új natív vagy AI-hívás nélkül.
- **Legfrissebb teljes háló**: átugorja a kirajzolást, és azonnal megmutatja a
  legutóbbi beérkezett teljes hálót. A még be nem fejezett számítást nem állítja le.

A rendszer csökkentett mozgás beállítása esetén a teljes háló azonnal látszik,
az újrajátszás letiltott. Rejtett böngészőlapon vagy mobilon a másik munkatérfülre
váltva a kirajzolás automatikusan szünetel. A **Végrehajtás leállítása** külön
vezérlő, amely a tényleges futást szakítja meg.

A **Demó betöltése** az eredeti
[lyukas lemezmodellt](../../../models/plate-with-hole.k3d) használja. Az
**Összetett demó** a [natív konzolmodellt](../../../models/studio-bracket.k3d)
négy tényleges geometriai állapotban mutatja:

1. Alaplemez (`part@1`).
2. Négy átmenő rögzítőfurattal ellátott lemez (`part@5`).
3. Négy kiemelt támasztóbakkal bővített lemez (`part@9`).
4. Kész konzol hátfallal, két kör alakú kivágással és két merevítőbordával
   (`part@14`).

Mind a négy hálót a natív mesh CLI számítja ki ugyanazon k3d dokumentum rögzített
csomópontjaiból, egymás után. A három köztes `construction_step` eseményt a végső
`editable_proposal` követi; a felület ezeket a valódi állapotokat játssza le.
A kész k3d dokumentum letölthető. A böngésző kirajzolási animációja a már
kiszámított hálókat jeleníti meg, és nem generál új geometriát.

### Eredeti felbontás és kontúrtolerancia

A Studio az eredeti feltöltött képbyte-okat adja át a natív rekonstrukciónak,
átméretezés és alulmintavételezés nélkül. A **Képpontlépték** továbbra is egy
eredeti forráspixel mérete modellegységben. Például 512 pixel × 0,025 egység/pixel
és 64 pixel × 0,2 egység/pixel egyaránt 12,8 modellegység szélességet jelent.

A [Studio-adapter](service/native.ts) a rögzített
`STUDIO_OUTLINE_TOLERANCE_PIXELS = 1` konstans alapján, explicit
`--outline-tolerance 1` kapcsolóval kéri a natív, topológiát védő
kontúregyszerűsítést. A tolerancia **egy eredeti forráspixel**, nem egy
modellegység; a felületen megadott hálótoleranciától független. A raszter és a
megadott képpontlépték változatlan marad. Ez kontúrközelítési hibakeret, nem
igazolt fizikai mérési pontosság. A közvetlen natív CLI e kapcsoló nélkül
továbbra is az egyszerűsítés nélküli utat használja. A Studio indításához olyan
újrafordított rekonstrukciós bináris kell, amely támogatja ezt a kapcsolót.

A natív kimenet **összesen legfeljebb 256 kontúrcsúcsot** enged meg a külső
kontúr és a lyukak együttese számára. Ez az egyszerűsítés után is érvényes korlát:
ha az egy forráspixeles keret és a topológiai ellenőrzések mellett megmaradó
kontúr ezt túllépi, a rekonstrukció hibával leáll. Nem emeli automatikusan a
toleranciát, és nem kicsinyíti le a képet a korlát teljesítéséhez. Ezért egy
részletes, például sokfogú fogaskerékrajz továbbra is elérheti a 256-os korlátot.

Ez jelenleg kontúrkihúzás, nem tetszőleges fotó teljes térbeli visszafejtése.
A rajz nem szolgáltat fizikai mértékegységet vagy rejtett felületeket. A ma
kizárt folyadékszimuláció és SVG/DWG import nem része a felületnek. A chat
műveleti összefoglalót és eszközeseményeket mutat, privát modellgondolatmenetet nem.

## Szerkezet és határok

- [protocol/](protocol/) és [gen/](gen/): verziózott protobuf, generált TypeScript;
  `npm run proto` újragenerálja.
- [web/](web/): nézet, beszélgetés, kép- és hangbevitel; gRPC-Web kliens.
- [service/workflow.ts](service/workflow.ts): LangGraph expert → opcionális research
  → expert → clarify / geometry / respond, explicit döntési séma.
- [service/questions.ts](service/questions.ts): munkamenethez kötött, lejáró HITL folytatás.
- [service/native.ts](service/native.ts): fix natív binárisok, megszakítható
  child process, korlátos események és fájlok, ideiglenes munkatér törlése.
- [service/ai/](service/ai/): szerveroldali LiteLLM/STT adapterek.

A Rust rekonstrukció belül továbbra is a közös, immutable artifactokra épülő
runtime-ot használja. A Studio folyamatonkénti külső adapter: JSON/byte másolások
vannak a Rust → Deno → böngésző határon; nem állítunk folyamatokon át zero-copyt.
A Deno réteg nem új geometriai kernel és nem helyettesíti a domén API-kat.

Egyszerre két futás, munkamenetenként egy; a határidő 120 s. A kliens leállítása
végigadja az abortot a providerig és a natív childig. A memória és történet
korlátos; a fájlok/munkamenetek egy óra után lejárnak, szerverújraindításkor
elvesznek. A fájlokat ezért mentsd le. Tartós dokumentum/replay és többfelhasználós
hozzáférés későbbi munka. A kiszolgáló csak loopback címre figyel.

## Ellenőrzés

```sh
npm run check:service
npm run test:service
npm run build
npm run test:e2e
npx playwright test --config e2e/expert.config.ts
```

A szervertesztek valódi natív rekonstrukciót és gRPC-Web átvitelt is futtatnak;
a provider és mikrofon hibautakat kontrollált helyettesítők ellenőrzik.
A böngészőpróbákhoz Chromium és a fejlesztői felület/szolgáltatás szükséges;
a részleteket a Playwright konfiguráció tartalmazza.

Az összetett demó a képrekonstrukciótól függetlenül, a meglévő mesh binárissal is
ellenőrizhető a Studio könyvtárából:

```sh
deno test --allow-read --allow-write --allow-run --filter complex service/native_test.ts
```

A teszt a négy különböző geometriai állapotot, mindegyik nulla nyitott élét, a
végső térfogatot és befoglaló méreteket, a letöltött dokumentum egyezését és a
15 másodperces teljes futási keretet ellenőrzi.

API-hivatkozások: [Connect Node adapter](https://connectrpc.com/docs/node/implementing-services/),
[LangGraph streaming](https://docs.langchain.com/oss/javascript/langgraph/streaming),
[OpenAI kép/tool modell](https://developers.openai.com/api/docs/models/gpt-4.1-mini),
[hangátírás](https://developers.openai.com/api/docs/guides/speech-to-text).

## Térbeli figura: rögzített Pikachu-demó

A **Térbeli figura** gomb a **Stilizált Pikachu · térbeli mesh** próbát indítja,
AI-hívás és képfeltöltés nélkül. A rögzített [recept](../../../models/pikachu.field.json)
paraméteres térfogati mezőből készít teljes háromdimenziós figurát; nem képsziluett
kihúzása, nem tetszőleges kép → 3D vagy általános AI-szobrászat. A beszélgetés
szakértője ettől külön szöveges CAD-tervet is készít: doboz, henger, gömb, tórusz, egyesítés, kivágás és átmenő furat, legfeljebb nyolc művelettel.

Gyors karakterdemóhoz a repó gyökerében fordítsd le mindkét optimalizált binárist:

```sh
cargo build --release --bin krly3d-field --bin krly3d-print
```

A szolgáltatás a teljes release párt részesíti előnyben. Ha annak bármelyik tagja
hiányzik vagy nem szabályos fájl (például szimbolikus link), a teljes debug párt
választja. Egy futás két profilját soha nem keveri; végrehajtási hiba után nem
próbálkozik újra másik profillal. A Health ugyanígy ellenőrzi az elérhetőséget.
Nincs környezeti változós vagy tetszőleges útvonalas felülírás. A meglévő CAD
rekonstrukció/mesh binárisok továbbra is a debug könyvtárból futnak.

A szolgáltatás kizárólag a következő rögzített natív programokat hívja
(vagy mindkettőt a `target/debug/` könyvtárból):

```text
target/release/krly3d-field --recipe PRIVATE/pikachu.field.json --output PRIVATE/output
target/release/krly3d-print --mesh PRIVATE/output/mesh.json --output PRIVATE/output/pikachu.stl --report PRIVATE/output/print-report.json --units mm
```

A receptet saját ideiglenes könyvtárba másolja; nincs shell, örökölt környezeti
változó, külső útvonal vagy futtatható program felülírása. Ugyanaz a 120 másodperces
határidő, TERM/KILL megszakítás, folyamatbevárás és könyvtártakarítás védi, mint a
meglévő natív adaptert. A korlátok felvételi/adatátviteli korlátok, nem OS-szintű
CPU-, heap- vagy lemezkvóták: 250 000 csúcs, 500 000 háromszög, fájlonként 16 MiB,
összesen 64 MiB. A geometria kizárólag a natív field motorból származik; a TS adapter
az ellenőrzött háromszögekből csak felületmérést számol.

A `RunRequest.mode` új értéke `character`. A `HealthResponse.native_character`
külön jelzi a két program és a recept elérhetőségét. A field stdout pontosan a
`sampling`, `mesh`, `completed` NDJSON szakaszokat küldi. A felület ezekből tényleges
haladást mutat; nem készít mesterséges köztes alakokat. A végső `final_mesh` esemény
és a `Mesh.units = "mm"` különbözteti meg a B-rep útvonaltól. Csak mindkét sikeres
natív kilépés és ellenőrzött fájlolvasás után véglegesül a modell. A meglévő
háromszög-kirajzolás és visszajátszás működik a kapott hálón.

Letölthető az STL, a natív mesh JSON, a változatlan recept és a
`krly3d.mesh.print-report/1` formátumú jelentés. A jelentés a vizsgált topológiai
hibák darabszámát, összefüggőséget, térfogatot és float32 STL-visszaolvasást közli.
**Az STL-export nem teljes nyomtathatósági igazolás**: a jelentésben minden vizsgálat
saját státusza jelenik meg (`passed`, `failed`, `unknown`); a `print_ready` jelenleg
`false`. A natív önmetszésvizsgálat elérhető módszerét, közelítő jellegét és
vizsgált párszámát, munkakorlátait és korlátozásait a felület külön kiírja az
eredeti és a float32 STL-hálóhoz. Mindkettőn `passed` szükséges az exporthoz;
a keret kimerülése (`unknown`) blokkolja az exportot. A falvastagság továbbra is
ismeretlen; a numerikus kontaktvizsgálat nem egzakt geometriai bizonyítás. A szeletelőben ellenőrizni kell a
milliméteres méretet, orientációt, támaszokat és gyártási beállításokat. Az STL nem
kódol mértékegységet, ezért importáláskor millimétert kell választani. Elutasított
exportnál nincs végleges modell/STL, de az ellenőrzött hibajelentés letölthető.

A natív adapter és a demó workflow tesztjei a [natív tesztekben](service/native_test.ts)
vannak; a protokollt mockoló desktop/mobile UI-próba:

```sh
npm run build
npx playwright test --config e2e/expert.config.ts character.spec.ts
```

## Szöveges CAD és kutatási költség

Kép nélkül is építhető modell. Saját, teljes méretsor esetén egy szakértői
hívás készíti a validált műveleti tervet; a geometria a natív kernelben épül.
Az Építés módban felismert számozott szabványjelölés (például ISO 7089 M8)
először egy szöveges méretkeresést kap, majd egy tervezői hívást. A keresésbe
csak a nyilvános szabványazonosító és névleges méret kerül. Ha a beszélgetési
módban a modell kihagyná a megnevezett szabvány ellenőrzését, a host ugyanezt
a keresést közbeiktatja. Sikertelen kutatás után nincs újabb keresési kör,
és nem készül ellenőrizetlen szabványméretekből modell.

A sikeres kutatások folyamatszintű, 32 elemre és 1 órára korlátozott cache-be
kerülnek. Gateway, modell vagy kulcs változása külön partíció; megszakított
és hibás kutatást nem tárolunk. Cache-találatkor nincs új kutatási AI-hívás
vagy kitalált használati tétel. A felület külön jelzi az újrafelhasználást;
a tervezői hívás tényleges költsége továbbra is szerepel. A kereső kimenete
600, a tervezőé 1800 tokenre korlátozott; ez felső kimeneti korlát, nem teljes
bemeneti token- vagy dollárkeret.

A letölthető `design.recipe.json` tartalmazza a méreteket és műveletneveket,
a `design.k3d` a natív modellt, a `design.sources.json` a felhasznált forrásokat.
Az utóbbi nem szabványmegfelelőségi igazolás. Minden lépés az addigi receptből
épül; a végső modell pozitív térfogata és zárt háromszöghálója ellenőrzött.
A kézi, külső CAD/DWG/STEP-import továbbra sincs bekötve a tervező eszközébe.

Az átmenő furat külön `through_hole` művelet: a terv a céltestet, az XY
helyet és a sugarat adja meg. A vágóhenger mélységét a host számítja a céltest
Z-kiterjedéséből, mindkét oldali túlfutással. Így az AI-nak nem kell középpont
és alapsík között átszámolnia a furatot, és három furat négy logikai lépésből
építhető (alaptest + három furat). A furatok itt +Z irányúak.
