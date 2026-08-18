# Assay — ADR-indeks og arkitekturplan
### Headless stat-resolver og patch-differ for Dark and Darker
*Udkast, 18. august 2026. Baseline-datasæt: Patch 6.12 / Hotfix #123, build `0.17.150.9384`.*

> ⚠️ **PLANLÆGNINGSDOKUMENT — delvist forældet.** Bevaret for beslutningshistorik.
> Gældende ADR'er: `ADR-000-010.md` + `ADR-rev2-amendments.md`. Afvigelser i dette dokument:
> - §2 viser fire crates og en grep-baseret renhedstest → erstattet af fem crates og `no_std`-gate (ADR-000 rev 2)
> - §3 angiver ADR-003 som åben → **lukket**: hybrid A+C, håndgodkendt JSON som eneste sandhedskilde
> - §1/§5 nævner milli-enheder → erstattet af mikro-enheder 1e-6 (ADR-001 rev 2)

---

## Navn

**Assay** — en assay er den analytiske bestemmelse af et metals faktiske sammensætning og renhed. Værktøjet gør præcis det: bestemmer hvad et build reelt er værd, i modsætning til hvad tooltip'et påstår. Passer til domænet uden at være sødt.

Crates: `assay-core`, `assay-data`, `assay-diff`, `assay-cli`.

---

## 1. Sprogvalg — anbefaling: **Rust**

Ikke fordi det er nyt, men fordi domænets primære fejlklasse er *enhedsforveksling*.

Dark and Darker har mindst fem forskellige størrelser der alle ser ud som "et tal med procent bagefter", og som opfører sig fundamentalt forskelligt:

| Størrelse | Lag | Eksempel |
|---|---|---|
| Armor Rating | Rå værdi → kurve → PDR | Barbuta Helm 26–38 |
| Physical Damage Reduction | Resultat af kurven, cappet | −22% ved 0 AR, cap 75% med Defense Mastery |
| **PDR Mod** | Multiplikativt *oven på* PDR | Lethal Mark −30% |
| Armor Penetration | Reducerer modstanderens AR | Thrust 15% |
| True Physical Damage | Omgår hele kæden | Dagger Mastery +1 |

I C#, Dart eller Python er alle fem en `double`, og en forveksling giver et plausibelt men forkert tal — den værste type fejl for et analyseværktøj, fordi den ikke crasher, den *overbeviser*. I Rust gør newtype-mønstret det til en compile error:

```rust
pub struct ArmorRating(i32);
pub struct PdrPercent(Milli);      // resultat af kurven
pub struct PdrMod(Milli);          // multiplikativt lag
pub struct ArmorPen(Milli);
pub struct TrueDamage(Milli);
// Ingen From/Into mellem dem. Ingen implicit aritmetik.
```

Sekundære argumenter:
- **serde** gør versionerede datasæt og strukturel diff til et løst problem frem for et projekt.
- **wasm-target** giver en gratis vej til web-frontend senere uden at røre kernen. Det er den eneste af de tre kandidater der har det.
- v1 er et bibliotek plus en CLI. Flutter er forkert værktøj til en CLI; C# ville give nul fordel her, da der ikke er nogen Unity-kobling overhovedet — og det ville sløre grænsen mod IronCore mentalt.

Fravalgt: **C#** (kendt, men bidrager intet teknisk til dette problem), **Dart** (rigtigt hvis UI var v1 — det er det ikke), **Rust som "læringsprojekt"** er en bivirkning, ikke begrundelsen.

---

## 2. Crate-arkitektur

```
assay/
├── assay-core/     Ren domænelogik. Ingen I/O. Ingen netværk. Ingen filsystem.
│                   Newtypes, resolution pipeline, damage model.
├── assay-data/     Versionerede datasæt + schema + loader + validering.
│                   Kender til filsystem. Kender ikke til beregning.
├── assay-diff/     Strukturel diff mellem to datasætversioner.
├── assay-cli/      Binær. Eneste crate der må printe.
└── fixtures/       Golden tests: håndberegnede loadouts med forventede tal.
```

**Håndhævelse (samme mønster som Vinkelværk):** en live test der fejler hvis `assay-core` importerer `std::fs`, `std::net`, `reqwest` eller `assay-data`. Renheden er et testet krav, ikke en konvention.

---

## 3. ADR-indeks

Nummerering følger dit vanlige mønster. Status angivet som *Proposed* indtil du låser.

| # | Titel | Beslutning der skal træffes | Status |
|---|---|---|---|
| **000** | Sprog, crate-layout og renhedskontrakt | Rust; fire crates; core-renhed håndhævet som test | Proposed |
| **001** | Numerisk repræsentation og determinisme | Fixed-point (`i64` milli-enheder) vs `f64` | Proposed |
| **002** | Typesikkerhed for statstørrelser | Newtype pr. størrelse, ingen implicit konvertering | Proposed |
| **003** | **Datastrategi og -provenienss** | Scraping / håndholdt / hybrid + licensforhold | **Åben — se §4** |
| **004** | Datasæt-schema og patch-versionering | Nøgling på build-id, entitets-identitet på tværs af patches | Proposed |
| **005** | **Resolution pipeline — rækkefølge** | Den eksakte ordre attributter → derived → mods | **Kritisk — se §5** |
| **006** | Damage-applikationsmodel | Rækkefølgen base → scaling → power → pen → PDR → PDR Mod → true | Proposed |
| **007** | Repræsentation af usikkerhed | `Known` / `Unverified` / `Unknown` og propagering | Proposed |
| **008** | Patch-diff semantik | Add/remove/modify/rename-detektion, output-format | Proposed |
| **009** | Loadout-format | Brugerredigerbart, versionsstabilt, diff-venligt | Proposed |
| **010** | Teststrategi | Golden fixtures, property tests, invarianter | Proposed |
| **011** | *(Deferred til v2)* TTK-solver og counter-matrix | Ikke i v1-scope | Deferred |

---

## 4. ADR-003 — Datastrategi (åben, med kræfterne lagt frem)

Dette er projektets største enkeltrisiko. Tre veje:

**A. Ren håndholdt versioneret JSON**
- ✅ Fuld kontrol, ingen ekstern afhængighed, ingen parser der knækker torsdag aften
- ✅ Ingen licens- eller ToS-spørgsmål
- ❌ Vedligeholdsomkostning hver patch. Realistisk 1–3 timer pr. hotfix hvis du vil have hele item-tabellen
- ❌ Skalerer ikke til det fulde datasæt (hundredvis af items × 7 rarities × modifier-ranges)

**B. Fuld scraping**
- ✅ Frisk automatisk
- ❌ Wiki-HTML er ikke et API. Tabber-tags, manuelle community-noter blandet med auto-genererede tal, "needs further testing"-markører midt i celler
- ❌ Du ejer ikke opdateringstakten og opdager først et brud når tallene er forkerte
- ❌ Wiki'en er CC BY-SA — kræver attribution og share-alike hvis datasættet distribueres

**C. Hybrid — scraper foreslår, mennesket reviewer**
- Scraper kører mod wiki/DarkerDB, producerer en **diff mod det committede datasæt**, ikke en overskrivning
- Du reviewer diffen som en pull request og accepterer eller afviser felt for felt
- Datasættet i repo'et er altid håndgodkendt og dermed pålideligt
- ✅ Frisk *og* kontrolleret. Scraper-brud giver støjende diffs, ikke stille korruption
- ✅ Genbruger `assay-diff` — samme motor som patch-diff-featuren. Du bygger komponenten én gang og bruger den to steder
- ❌ Mest arbejde op front

**Min anbefaling:** C, men med A som fallback-garanti — datasættet skal kunne vedligeholdes i hånden hvis scraperen dør, altså skal formatet være menneskeredigerbart (JSON/TOML, ikke en binær cache). Attribution til wiki'en i repo'et uanset hvad.

Bemærk at C har en elegant egenskab: **scraper-diffen og patch-diffen er det samme problem.** Det er ét stykke maskineri, to anvendelser.

---

## 5. ADR-005 — Resolution pipeline (kritisk)

Rækkefølgen skal låses eksplicit, fordi en forkert ordre giver tal der ser rigtige ud. Foreslået ordre:

```
1. Klassens base-attributter                    Rogue: STR 9, AGI 25, DEX 20, VIG 6 …
2. + attributter fra gear-ruller                Dark Leather Leggings: DEX 1–5
3. + attributter fra perks/skills/party         Jokester +2 alle, Fortified Ground +3 alle
   └─ FØRST HER er attributsummen endelig
4. Attributter → derived stats (kurver)         STR → Physical Power Bonus
                                                AGI → Action Speed, Move Speed
                                                VIG → Health
5. + flade adds                                 Move Speed Add, Buff Weapon Damage
6. + procentbonusser                            Dagger Mastery 15%, Back Attack 30%
7. Defensiv kæde                                Armor Rating → PDR-kurve → cap
                                                (Defense Mastery hæver cap til 75%)
8. Situationelle mods holdes SEPARAT            PDR Mod, healing mods, debuff-varighed
```

**Trin 3 før 4 er ikke til forhandling.** Fortified Ground og Jokester giver +5 All Attributes samlet; hvis de lægges på efter kurveopslaget, får du et systematisk forkert Physical Power Bonus for hele duo-analysen — og det er præcis den synergi guiden hviler på.

**Trin 8 separat:** Lethal Mark er ikke en del af defenderens statblok. Den er en modifier på *udvekslingen*. Bland dem, og du kan ikke længere svare på "hvad er denne spillers PDR" uafhængigt af hvem der skyder.

---

## 6. ADR-007 — Usikkerhed som førsteklasses begreb

Kildedataene er ufuldstændige. Wiki'en markerer eksplicit felter som uverificerede, og der er mindst seks åbne mekanikspørgsmål der påvirker beregninger direkte (Ambush' buff-forbrug, Trickster på ikke-knive, Dual Strikes on-hit-dobbeltapplikering, Shield Masterys buffvindue, Victory Strikes reset-semantik, Shadowsteps gyldige mål).

Derfor skal værdier bære deres tillidsgrad:

```rust
pub enum Confidence<T> {
    Verified(T),          // bekræftet i patch notes eller testet
    Unverified(T),        // wiki-værdi, community-noteret, ikke bekræftet
    Unknown { assumed: T, note: &'static str },
}
```

Og den skal **propagere**: hvis ét led i en TTK-beregning er `Unverified`, er resultatet det også. Outputtet mærkes tilsvarende. Et analyseværktøj der ikke kan skelne "dette tal er rigtigt" fra "dette tal er et gæt" er værre end intet værktøj, fordi det får dig til at handle på gæt med selvtillid.

---

## 7. v1-scope (låst)

**Med:**
- Loadout-definition: klasse, 4 perks, 2 skills, våben, 6 rustningsslots, accessories
- Fuld stat-resolution → komplet statblok, sammenlignelig med spillets character sheet
- Datasæt for Hotfix #123 som baseline, plus mindst én tidligere version (#122 eller Patch 12) så diff kan demonstreres
- `assay diff <build-a> <build-b>` → hvilke items/perks/skills ændrede sig, og **hvad det gjorde ved et givet sæt loadouts**
- CLI-output: tabel og JSON
- Golden test-fixtures med håndberegnede tal

**Ude af scope for v1:**
- TTK-solver, counter-matrix, Nash-analyse (ADR-011, v2)
- Sensitivitetsanalyse (v2 — kræver TTK)
- Web-/Flutter-frontend (v3 — wasm-vejen ligger klar)
- Monstre, bosser, PvE-beregning
- Marketplace-prisdata

**Definition of done for v1:** du kan bygge Rogue- og Fighter-loadoutsene fra duo-guiden i værktøjet, få tal ud der matcher spillets character sheet inden for afrundingsfejl, og køre `assay diff hotfix-122 hotfix-123` og se præcis hvad Flanged Mace-nerfen og Sprint-trimmen gjorde ved dem.

Det sidste er hele pointen: **næste torsdag ved du på ti sekunder om dit build stadig holder.**

---

## 8. Næste skridt

1. Bekræft navnet og sprogvalget
2. Luk ADR-003 (datastrategi) — det blokerer schema-designet i ADR-004
3. Jeg skriver de fulde ADR-bodies for 000–010 til review
4. Skeletprojekt: crates, renhedstest, ét håndkodet item som end-to-end vertical slice

---

*Datagrundlag: Dark and Darker Wiki (spellsandguns), CC BY-SA 4.0. Patch notes 6.12 via officielle Ironmace-udgivelser.*
