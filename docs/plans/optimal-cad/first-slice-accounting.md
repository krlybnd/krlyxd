# Kódkivételi mérleg: első és második kör

Dátum: 2026-11-15. Az összehasonlítás alapja a munka kezdetén rögzített [forrás-pillanatkép](source-baseline.json), nem a már akkor is eltérő Git HEAD. Ugyanazokat a követett és nem ignorált, létező `.rs`, `.ts`, `.tsx`, `.py`, `.css`, `.proto`, `.typ` fájlokat számoltuk újra. A fizikai sorokba az üres és komment sorok is beletartoznak; a `.tmp`, `.ctx`, `node_modules`, `target`, `dist` és `__pycache__` kizárt. A forrás és a teszt besorolása a baseline meglévő fájljainál változatlan; az új `domIds.typecheck.ts` tesztnek számít. Ez **repo-kódfájl-leltár**, nem kizárólag a jelenleg fordított futtatókód: a megőrzött [porting/](../../../porting/) történeti, nem forduló Rust forrásai 2804 sora is benne van mindkét állapotban, így a változás értékét nem befolyásolják.

| Kategória | Előtte | Most | Változás |
| --- | ---: | ---: | ---: |
| Termelő forrás | 71 024 | 70 723 | −301 |
| Tesztforrás | 28 814 | 28 951 | +137 |
| Generált forrás | 926 | 926 | 0 |
| **Összes aktív kód** | **100 764** | **100 600** | **−164** |

A táblázat az első kör lezárását mutatja; a második kör és a jelenlegi összeg alább szerepel. Az „aktív” itt a nem ignorált kódfájlokat jelenti a fenti leltározási szabály szerint, nem kizárólag a fordított runtime-ot.

A kódból négy duplikált MCP JSON-parserfájl, összesen 474 sor került ki. Egy 118 soros, közös implementáció váltotta őket; a **bruttó termelő kódkivétel 356 sor**. A többi termelő változás összege +55 sor: végrehajtható numerikus `.typ` példa (+15), típusos DOM-azonosítók (+36), Studio-fogyasztók (+4). Így a nettó termelő csökkenés 301 sor. A tesztek +137 sora a közös parser (+43), DOM typecheck és E2E segéd (+28), Studio E2E (+38), függőségi határteszt (+12) és rekonstrukciós regressziós javítások (+16) összege. Más meglévő fájlok tartalma változott, de a fizikai sorszámuk nem.

A [D1 ledger](parser-dedupe-ledger.md) más nézetet ad: az adott parserkivétel Rust-tesztekkel, manifestekkel és hozzá rendelhető lockfile-metaadattal **277 nettó aktív fizikai sor** megtakarítás. A fenti 164 sor az *első kör összes* kódváltozására vonatkozik, és nem számolja a manifestet vagy a lockfile-t; a két számot nem szabad összeadni. Dokumentáció és helyi archívum egyik kódsorösszegben sincs. A második körrel együtt mért teljes eredmény alább szerepel.

Az eredeti négy parser [`.tmp/refactor/d1-parser/`](../../../.tmp/refactor/d1-parser/) alatt, hash-ekkel és visszaállítási leírással van. A `.tmp` [ignorált](../../../.gitignore), egyetlen ottani fájl sincs követve; ez csak helyi visszaállítási anyag. A későbbi nagyobb refaktorhoz új baseline és külön, elfogadás utáni mérleg kell. A jelen mérleg nem állítja, hogy a még csak tervezett solver, review-javítás vagy további duplikációk már kikerültek.

Ellenőrzés: a teljes Rust workspace `cargo test --workspace --locked --offline -q --no-fail-fast` futása sikeres; a Studio 17 célzott Deno workflow/RPC tesztje, buildje és a DOM-pilot két Playwright-környezete is sikeres. A teljes natív CAD/E2E folyamat és az Inventor külön tervezett QA-kapu.

## Második kör: célzott újabb kivétel

Az első kör utáni, ignorált `.tmp/refactor/second-pass-baseline.json` 574 kódfájlt és 100 600 fizikai sort rögzít hash-ekkel. A [D4 közösítés](d4-rust-ledger.md) a mesh- és view-runnerből összesen 68 sort vett ki, és 41 sort adott hozzá a meglévő közös Rust rétegben. A két eddigi publikus `RunnerLimits` útvonal megmaradt; a második kör **nettó 27 Rust-kódsor csökkenés**. Sem teszt, sem generált kód, sem manifest/lockfile nem nőtt. Az eredetik az ignorált `.tmp/refactor/d4-rust/` alatt vannak.

A [Studio-audit](d2-studio-ledger.md), a [rekonstrukciós process-pilot auditja](d3-reconstruction-ledger.md) és a [történeti porting-audit](d5-porting-ledger.md) egyenként **0 igazoltan törölhető kódsort** talált. A 611 soros Python process-pilotot ma CLI és tesztek futtatják; a porting források egyes régi geometriai állításaihoz még nincs paritás. Ezeket a sorokat nem könyveltük megtakarításnak. A `porting/README.md` pontatlan történeti ághivatkozását javítottuk; a dokumentációs változás kívül esik a kódleltáron.

**Jelenlegi kódleltár:** termelő forrás 70 696 sor, külön tesztforrás 28 951 sor, generált forrás 926 sor, összesen **100 573 sor**. Ez az első kör utáni állapothoz képest −27, az eredeti 100 764 soros baseline-hoz képest **−191 fizikai kódsor**. A második körben teljes fájlt nem töröltünk: 68 sor került ki két megtartott runnerfájlból, egy közös típus és érvényességi feltétel váltotta fel őket. A két kör bruttó kivétele 474 korábbi parserfájlsor és 68 runner-sor, azaz 542 sor; az új közös implementációk, DOM-pilot, numerikus példa és regressziós tesztek együtt 351 sort adtak hozzá. Ez adja a 191 soros nettó csökkenést.
