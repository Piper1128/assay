# Assay — ADR-amendments, revision 2

**Dato:** 18. august 2026
**Omfang:** ADR-000, ADR-001, ADR-010 erstattes i deres helhed af nedenstående revision 2.
**Anledning:** review mod nano-chains arkitekturgates. Tre mønstre derfra er strengt bedre end det oprindelige sæt, og ét af dem lukker en reel defekt.

### Ændringslog

| ADR | Ændring | Årsag |
|---|---|---|
| 000 | `assay-core` bliver **`no_std + alloc`**. Renhed håndhæves af compileren, ikke af en lint | En grep-test kan omgås ved uheld; en manglende `std` kan ikke |
| 000 | Schema-typer flyttes ind i `assay-core`; `assay-data` holder kun I/O | Undgår femte crate og gør no_std-gaten til dependency-gate |
| 001 | **Forbud mod randomiseret-hash-collections.** `BTreeMap`/`BTreeSet` overalt hvor rækkefølge kan nå outputtet | **Defekt i rev 1:** kravet om byte-identisk output var uforeneligt med `HashMap`'s randomiserede hasher. Ville have givet flaky diffs |
| 001 | Kanonisk encoding defineres eksplicit som diff-grundlag; fuzz-target på dataset-decoder | Diff skal sammenligne kanoniske former, ikke tilfældige serialiseringer |
| 010 | **Uafhængigt Python-spejl** af resolution pipeline, skal være enigt på hele vektorkorpuset | Én implementering kan ikke fange at pipeline-rækkefølgen er forkert, hvis fixturen er beregnet med samme forkerte antagelse |
| 010 | **Negativ probe-disciplin:** hver gate skal bevises i stand til at fejle | En gate der aldrig er set fejle er ikke en gate |

---

## ADR-000 (rev 2) — Sprog, crate-layout og renhedskontrakt

**Status:** Accepted — erstatter rev 1

### Kontekst
Uændret fra rev 1: Rust vælges fordi domænets dominerende fejlklasse er enhedsforveksling mellem fem numerisk identiske, semantisk uforenelige statstørrelser.

**Nyt i rev 2:** rev 1 håndhævede kernens renhed med en CI-test der søgte efter `std::fs`, `std::net` og `assay-data` i `assay-core`. Det er en lint, ikke en grænse. Den fanger kendte strengmønstre og misser resten — en transitiv afhængighed der trækker `std::time` ind, en `HashMap` med randomiseret hasher, en tråd. Compileren kan håndhæve hele klassen på én gang.

### Beslutning

**`assay-core` er `no_std + alloc`.**

Ikke for embedded-support, men fordi `#![no_std]` mekanisk forbyder præcis det sæt der ødelægger en ren, deterministisk beregningskerne:

| Forbudt af no_std | Hvorfor det er godt her |
|---|---|
| `std::fs`, `std::net` | Kernen kan ikke læse datasæt selv — den får dem udleveret |
| `std::time` | Ingen skjult tidsafhængighed i en ren funktion |
| `std::thread` | Ingen rækkefølgeafhængighed |
| **`std::collections::HashMap`/`HashSet`** | **Findes ikke uden `std`.** Randomiseret iterationsrækkefølge er dermed umulig i kernen — se ADR-001 |
| `std::process` | Ingen udgang |

**Bonusegenskab:** fordi `assay-data`, `assay-diff`, `assay-scrape` og `assay-cli` alle er `std`, vil enhver afhængighed fra `assay-core` til nogen af dem **bryde no_std-buildet**. Renhedsgaten er dermed også dependency-gaten. Én mekanisme, to invarianter.

**Revideret crate-layout:**

```
assay/
├── assay-core/     [no_std + alloc]  Domænetyper, schema-typer, newtypes,
│                                     resolution pipeline, exchange-model.
├── assay-data/     [std]             Filsystem, parsing, validering, versionsopslag.
├── assay-diff/     [std]             Strukturel diff + impact-diff.
├── assay-scrape/   [std]             Forslagsværktøj. Uden for tillidsgrænsen (ADR-003).
├── assay-cli/      [std]             Binær. Eneste crate der printer.
├── mirror/         [python]          Uafhængig referenceimplementering (ADR-010).
└── fixtures/                         Vektorkorpus + golden values + negative prober.
```

**Schema-typerne flytter ind i `assay-core`.** Rev 1 havde dem i `assay-data`, hvilket ville have tvunget en core→data-afhængighed eller en femte crate. I stedet: `assay-core` definerer typerne (rene `alloc`-strukturer med `serde`-derives — serde er `no_std`-kompatibelt med `default-features = false, features = ["alloc", "derive"]`), og `assay-data` gør udelukkende I/O og afleverer ejede strukturer ind i kernen.

**Traits ejet af kernen.** Alt hvad kernen behøver fra omverdenen passerer via traits defineret i `assay-core` og implementeret udenfor:

```rust
pub trait DatasetSource {
    fn class(&self, id: &ClassId) -> Option<&ClassDef>;
    fn item(&self, id: &ItemId)   -> Option<&ItemDef>;
    fn curve(&self, id: &CurveId) -> Option<&Curve>;
}
```

Kernen ved ikke at der findes filer.

**Fejlhåndtering:** `core::error::Error` (stabil siden Rust 1.81). Ingen `std::error::Error`, ingen `anyhow` i kernen — `thiserror` med `no_std` eller håndrullede enums.

**Negativ probe (obligatorisk).** Gaten er ikke accepteret før den er bevist i stand til at fejle:

```bash
# Skal bygge grønt:
cargo build --target thumbv7em-none-eabi -p assay-core

# Probe: indsæt `use std::fs;` i assay-core → skal fejle med E0433.
# Kørt af CI mod probes/no_std_violation/ (se ADR-010).
```

**Sekundær grep-test bevares** — ikke for `std`-symboler, som no_std allerede dækker, men for det no_std *ikke* fanger: at `assay-data` aldrig afhænger af `assay-scrape` (begge er `std`, så compileren er ligeglad). Den ensrettede tillidsgrænse fra ADR-003 kræver stadig en eksplicit test.

### Konsekvenser
- ✅ Renhed er en compile-time-egenskab, ikke en konvention med et sikkerhedsnet
- ✅ Dependency-retningen håndhæves gratis af samme mekanisme
- ✅ `HashMap` er udelukket fra kernen uden yderligere værktøj
- ✅ Kernen kan senere køre i wasm og bare-metal uden ændring
- ❌ Enkelte bekvemmeligheder forsvinder: `format!` kræver `alloc::format`, `Box<dyn Error>` bliver klodset, nogle crates skal `default-features = false`
- ❌ Tvinger disciplin i valg af dependencies. Det er tilsigtet

---

## ADR-001 (rev 2) — Numerisk repræsentation og determinisme

**Status:** Accepted — erstatter rev 1

### Kontekst
Uændret: fixed-point `i64` i mikro-enheder (1e-6), fordi patch-diff kun er brugbar hvis "ændret" betyder ændret, og fordi Rogue'ens base Action Speed på **7,8125%** kræver fire decimaler.

**Defekt i rev 1:** kravet om byte-identisk output på tværs af kørsler og platforme blev formuleret uden at adressere den mest almindelige kilde til det modsatte. `std::collections::HashMap` bruger `RandomState` — en hasher seedet forskelligt pr. proces. Enhver iteration over en `HashMap` hvis rækkefølge når serialiseringen, giver **forskelligt output i to kørsler af samme program på samme maskine**. Det ville have produceret fantomdiffs der forsvinder når man kigger på dem. Den værste fejlklasse i projektet, indført af projektets eget kravdokument.

### Beslutning

**1. Numerik (uændret fra rev 1).**
Fixed-point `i64`, skala 1e-6. Addition og subtraktion eksakt. Multiplikation runder med banker's rounding og dokumenteret rundingspunkt. Division kun via navngivne funktioner med eksplicit rundingsregel. `f64` er tilladt i `assay-cli`, aldrig i `assay-core`.

**2. Deterministiske collections.**
Ingen randomiseret-hash-collection må have indflydelse på output.

- **I `assay-core`:** automatisk. `no_std` udelukker `HashMap`/`HashSet` (de findes kun i `std`). `alloc::collections::BTreeMap`/`BTreeSet` er de tilgængelige, og de itererer i sorteret nøglerækkefølge.
- **I `std`-crates:** `clippy.toml` med `disallowed-types` for `std::collections::HashMap` og `HashSet`, kørt under `-D warnings`.

```toml
# clippy.toml
disallowed-types = [
  { path = "std::collections::HashMap",
    reason = "randomiseret hasher bryder determinismekravet i ADR-001; brug BTreeMap" },
  { path = "std::collections::HashSet",
    reason = "samme; brug BTreeSet" },
]
```

Undtagelse kræver eksplicit `#[allow]` med kommentar der begrunder hvorfor rækkefølgen beviseligt ikke kan nå outputtet.

**3. Kanonisk form som diff-grundlag.**
Diff sammenligner ikke serialiserede tekststrenge. Den sammenligner **kanoniske former**:

- Nøgler sorteres leksikografisk (gratis via `BTreeMap`)
- Ingen flydende komma i den kanoniske form — kun `i64`-mikroenheder
- Ingen whitespace-varians, ingen valgfrie felter der emitteres inkonsistent
- Fravær og nulværdi er distinkte og repræsenteres forskelligt

Kanonisk encoding er defineret i `assay-core` og er den eneste form `assay-diff` opererer på.

**4. Robusthed mod misdannede data.**
`assay-data`'s decoder skal aldrig panikke på input. Fuzz-target (`cargo-fuzz`) mod dataset-deserialisering. Misdannet JSON skal give en typet fejl, ikke et crash — relevant fordi datasæt kommer fra en scraper-forslagsproces (ADR-003).

**5. Determinismetest (se ADR-010).**
Resolvér hele fixture-korpuset to gange i samme proces og én gang i en ny proces; alle tre kanoniske outputs skal være byte-identiske. Kørt på mindst to platforme i CI.

### Konsekvenser
- ✅ Determinismekravet er nu faktisk opnåeligt
- ✅ `BTreeMap` giver sorteret output gratis — diffs bliver også mere læsbare
- ✅ Fuzzing af decoderen beskytter tillidsgrænsen fra ADR-003
- ❌ `BTreeMap` er langsommere end `HashMap` ved store datasæt. Irrelevant i denne størrelsesorden
- ❌ Kræver `Ord` på nøgletyper. Alle entitets-ID'er er strenge eller newtypes over strenge, så det er gratis

---

## ADR-010 (rev 2) — Teststrategi

**Status:** Accepted — erstatter rev 1

### Kontekst
Værktøjets eneste værdi er korrekthed, og den farligste fejl er et forkert tal der ser rigtigt ud.

**To huller i rev 1:**

1. **Golden fixtures kan ikke fange en forkert pipeline-rækkefølge.** Hvis fixturens forventede værdier er håndberegnet med samme fejlagtige antagelse som koden, består testen. ADR-005's hele risiko — at party-attributter anvendes efter kurveopslaget i stedet for før — er præcis den type fejl der overlever en golden test skrevet af samme person med samme misforståelse.

2. **En gate der aldrig er set fejle er ikke en gate.** Rev 1 specificerede en renhedstest uden at kræve bevis for at den kan afvise noget.

### Beslutning

Syv testlag.

**1. Compiler-gates (ADR-000).**
`cargo build --target thumbv7em-none-eabi -p assay-core` skal være grøn. Grep-test for `assay-data → assay-scrape`.

**2. Clippy-gates (ADR-001).**
`cargo clippy --workspace --all-targets -- -D warnings` med `disallowed-types` for `HashMap`/`HashSet`.

**3. Uafhængigt Python-spejl.**
En anden implementering af resolution pipeline (ADR-005) og damage-modellen (ADR-006), i `mirror/`. Den skal være enig med Rust på **hele vektorkorpuset**, felt for felt.

**Den afgørende regel: spejlet skrives fra ADR'erne og wiki-dataene — aldrig fra Rust-koden.** Et spejl afledt af implementeringen arver dens misforståelser og er værdiløst. Hvis Rust og spejl er uenige, er begge under mistanke indtil ADR'en afgør hvem der har ret; hvis ADR'en er tvetydig, er *den* fejlen.

Vektorkorpus i JSON, delt mellem begge: input-loadout + datasætversion + forventet kanonisk statblok.

**4. Negative prober.**
Hver gate skal bevises i stand til at fejle. `probes/` indeholder bevidst brudte varianter; CI asserter at hver af dem **fejler**:

| Probe | Bryder | Forventet fejl |
|---|---|---|
| `no_std_violation` | `use std::fs;` i `assay-core` | E0433 på thumbv7em |
| `hashmap_violation` | `HashMap` i `assay-diff` | clippy `disallowed_types` |
| `pipeline_order` | Party-attributter anvendt efter kurveopslag (ADR-005 trin 3↔4) | Spejl-uenighed + fixture-fejl |
| `pdr_mod_additive` | `PdrMod` lagt til i stedet for ganget (ADR-006 trin 7) | Fixture-fejl |
| `true_damage_pre_reduction` | True Damage lagt på før reduktion (ADR-006 trin 8) | Fixture-fejl |
| `scaling_ignored` | Skill scaling coefficient hardkodet til 100% | Sneak Attack-fixture fejler |
| `confidence_not_propagated` | `Confidence` returnerer max i stedet for min (ADR-007) | Propageringstest fejler |

Proberne er projektets egentlige forsvar. `pipeline_order` og `scaling_ignored` er de to der beskytter de indsigter hele Rogue/Fighter-analysen hviler på.

**5. Golden fixtures.**
Håndberegnede loadouts verificeret mod spillets character sheet. Minimum:
- Naked baseline pr. klasse — wiki'ens publicerede base-statblokke er autoritative og gratis
- Begge duo-builds fra Rogue/Fighter-analysen
- Ét loadout med Fortified Ground + Jokester aktive (dækker ADR-005 trin 3)
- Ét Sneak Attack-exchange fra Hide (dækker 0%-scaling-mekanikken)

**6. Property tests** (proptest):
- Monotonicitet: øget Armor Rating sænker aldrig PDR
- Cap-invariant: PDR overstiger aldrig gældende cap (60%, eller 75% med Defense Mastery)
- Pipeline-ordre: et loadout med party-buffs har strengt højere Physical Power Bonus end uden, når STR-kurven er ikke-flad
- Confidence-propagering: resultatets tillidsgrad ≤ minimum af inputs
- Determinisme: to kørsler i samme proces og én i ny proces → byte-identisk kanonisk output

**7. Data- og versionstests.**
Schema-validering pr. datasætversion. Alle `renamed_from` peger på eksisterende ID'er i forgængeren. Hvert fixture-loadout resolverer i hver datasætversion eller fejler med forventet, eksplicit fejl. Fuzz-target på decoderen.

**Dækningskrav:** branch coverage på alle pipeline-stadier og alle ni damage-trin. Ingen numerisk kode uden mindst én golden fixture *og* spejl-dækning.

### Konsekvenser
- ✅ Pipeline-rækkefølgen — projektets største korrekthedsrisiko — er dækket af to uafhængige implementeringer plus en dedikeret probe
- ✅ Ingen gate accepteres på tro
- ✅ Wiki'ens base-statblokke giver autoritative fixtures uden håndberegning
- ❌ Spejlet er reelt en anden implementering at vedligeholde. Accepteret: det dækker kun `assay-core`'s rene funktioner, ikke I/O, diff eller CLI
- ❌ Prober kræver at CI kan asserte at noget fejler. Løses med en `probes/run.sh` der inverterer exit-koden

---

## Note til ADR-011 (deferred)

nano-chains attack-lab er strukturelt identisk med Assays counter-matrix: **angreb er konfiguration, ikke særkode.** Når v2 bygges, skal builds være konfiguration ind i samme kodevej, ikke special-casede grene — og lab-kernen skal være headless-testet med nul domænelogik i frontend-laget. Samme mønster, samme begrundelse. Noteret her så det ikke skal genopfindes.

---

*Erstatter ADR-000, ADR-001 og ADR-010 rev 1 i `ADR-000-010.md`. Øvrige ADR'er uændrede.*
