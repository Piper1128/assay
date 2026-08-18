# Rogue + Fighter Duo — Fuld analyse
### Dark and Darker, Patch 6.12 / Hotfix #123 (build 0.17.150.9384, 13. aug 2026)
*Analysedato: 18. august 2026. Marketplace + Trading Post er åbne (siden 31. juli). High Roller entry-GS er sænket til 225.*

> **Rolle i Assay-projektet:** dette dokument er kilden til v1's golden fixtures (ADR-010 rev 2).
> De to duo-builds i §4 og de mekanikker der er fremhævet i §5 er præcis dem proberne
> `pipeline_order` og `scaling_ignored` skal beskytte.

---

## 1. Kernetese

De fleste spiller denne duo forkert. Standardmodellen er "Fighter tanker, Rogue burster fra stealth". **Den model er teknisk død efter Season 10.**

To patch-ændringer flytter hele arketypen:

1. **Hide-exit koster nu −30% Physical Power Bonus i 3s**, og **Ambush mistede sin damage-bonus helt**. Stealth er ikke længere en damage-opener — det er et *repositionerings- og disengage-værktøj*.
2. **Lethal Mark kan nu genapplikeres på samme mål** (Hotfix 123). Det gør Rogue til en *permanent debuff-platform* i stedet for en engangs-burst.

Den rigtige model i den nuværende patch:

> **Fighter er våbnet. Rogue er ammunitionen.**
> Rogue leverer ikke skaden — Rogue leverer *multiplikatorerne*, fra afstand, i sikkerhed, på repeat. Fighter konverterer dem.

Det er hele pointen, og det er den eneste duo i spillet der kan gøre det med rent fysisk damage.

---

## 2. Patch-baseline — hvad der faktisk ændrede sig

### Rogue (S10 → HF123)
| Ændring | Konsekvens |
|---|---|
| Exit fra Hide: −30% Physical Power Bonus i 3s | Hide-openere med rå dolkeslag er nerfet hårdt |
| Ambush mistede physical damage bonus (kun 10% MS tilbage) | Ambush er nu en flugt-/lukke-perk, ikke en damage-perk |
| Sneak Attack 10 → **15 base** (+10 hvis målet er out of combat), **0% scaling** | **0% scaling = immun over for −30%-straffen.** Dette er nu den korrekte hide-opener |
| Dagger Mastery 10% → **15%** + 1 True Physical Damage | True damage ignorerer armor — relevant mod plate |
| Lethal Mark: PDR-mod −15% → **−30%**, og (HF123) **kan genapplikeres** | Fra situationel til fundament |
| Trickster: 30% Action Speed, 10% MS, 50% Item Swap Speed | Kastekniv-loopet er nu reelt hurtigt |
| Veil of Shadows: stealth brydes ikke af bevægelse i 2s (HF120) | Ægte panic button |
| **Play Dead** (ny skill, 6s CD): heal 1 Recoverable (150% scaling) pr. 2s | Mid-fight reset + ambush-fælde |
| **Wall Ride** (ny perk): 10 skridt vægklatring + wall-jump | Vertikale flankeruter, nye vinkler |
| Shadowstep CD 30 → 24s | Gap-close på cooldown |

### Fighter (S10 → HF123)
| Ændring | Konsekvens |
|---|---|
| Weapon Guard: PDR fjernet → **+1 Impact Resistance** | Skifter fra defensiv stat til stagger-modstand |
| Perfect Block: 12s varighed / 8s CD (op fra 8s/7s) | Reelt uptime på +5 Impact Resistance |
| Last Bastion: 12s varighed, CD 48 → **32s** | Comeback-perken er nu pålidelig |
| Disarm 24 → 18s, Pommel Strike 18 → **12s**, Fortified Ground 18 → **12s** | Alle utility-CDs kortere |
| Combo Attack: stack #1 gives på **første** hit | Ingen ramp-up længere |
| Sprint: 10 → 15 → **13 MS add pr. stack** (HF121 buff, HF123 trim) | Stadig netto op fra sæsonstart |

### Riposte-systemet (S10 + HF120/121) — vigtigst for Fighter
- **Parry** er nu våbenspecifikt: kun **sword-type og daggers** kan parry, via våbnets sweet spot. Rammer du uden for sweet spot, blokerer du bare.
- **Block → Riposte** er tilføjet til de fleste ikke-parry-våben (uden bonusskade).
- HF120 udvidede parry-arealet og forbedrede defense recovery times.
- HF121: **Lantern Shield riposte-chain fik 0,4s post-attack delay fjernet** på andet hit → chain'en er nu markant hurtigere. Longsword- og Zweihander-parryarealer flyttet tættere på crosshair.

### Items (HF123)
- **Flanged Mace og Morning Star**: −1 damage, armor pen **15% → 10%**. **War Hammer**: −1 damage. Club: +1. Leviathan: +1. Longbow: action speed op.
- **Netto: blunt/armor-pen-metaet er trimmet.** Sværd er relativt stærkere lige nu — hvilket passer perfekt ind i Sword Mastery + parry-systemet.
- Unique items ruller nu **2 random modifiers** med op til **2× tidligere maksværdi**. Marketplace-jagt efter uniques er markant mere værd end sidste sæson.

---

## 3. Rolle-arkitektur

Duoen kører på tre lag. Hold dem adskilt i hovedet — det er dér, folk roder.

**Lag 1 — Buff-gulvet (passivt, altid oppe)**
- Fighter: **Fortified Ground** — banner, uendelig varighed, 12s CD, **+3 All Attributes til allierede inden for 33m**.
- Rogue: **Jokester** — **+2 All Attributes** til party inden for 6m.
- Kombineret: **+5 All Attributes på begge**. 33m dækker reelt et helt modul. Dette er den mest undervurderede ting i hele duoen: en gratis, permanent, party-wide stat-forøgelse for én skill-slot og én perk-slot.
- På en Rogue med 108,5 base HP og −14% Physical Power Bonus flytter +5 Strength/Agility/Vigor faktisk nålen.

**Lag 2 — Debuff-motoren (Rogue, fra afstand)**
- **Lethal Mark** (kastekniv): **−30% Physical Damage Reduction Mod i 8s**, genapplikerbar. Multiplikativ. Fighterens damage stiger direkte.
- **Poisoned Weapon**: 4 Neutral Magical Base Damage over 4s (50% scaling), **−10% incoming Physical Healing og −10% incoming Magical Healing**, **stacker 5×**. Fem stacks = **−50% modtaget healing**.
  → **Dette er duoens anti-sustain-teknologi.** Kastekniv applikerer det. Rogue kan stå 15m væk og lukke en Cleric ned uden at være i fare.
- **Weakpoint Attack** (melee): −30% Item Armor Rating Bonus i 3s. Anden mekanik end Lethal Mark — de overlapper ikke, de stacker i praksis.

**Lag 3 — Damage-konverteren (Fighter)**
Fighter står i frontlinjen og høster. Alt hvad Rogue applikerer, forstærker Fighterens sustained physical damage — den højeste i spillet uden magi.

---

## 4. Builds

### Build A — "Ambolten og Sylen" *(mainline, den du kører 80% af tiden)*

**Fighter — Ambolten**
- Perks: **Defense Mastery** · **Sword Mastery** · **Counterattack** · **Combo Attack**
- Skills: **Fortified Ground** · **Second Wind**
- Våben: Arming Sword + **Lantern Shield** (HF121-riposte-chainen er nu vinderen) — eller Longsword hvis du vil parry på sweet spot nær crosshair
- Rustning: tungt, Defense Mastery hæver PDR-cap til **75%**

**Rogue — Sylen**
- Perks: **Dagger Mastery** · **Poisoned Weapon** · **Lethal Mark** · **Jokester**
- Skills: **Sneak Attack** · **Weakpoint Attack**
- Våben: Rondel Dagger (armor pen) main, Castillon Dagger offhand, **Throwing Knives i utility — ikke til forhandling**

**Gameplan:** Fighter planter banner og engagerer i en dørkarm. Rogue åbner *ikke* med at gå ind — Rogue kaster knive: Lethal Mark → 5× Poison-stacks. Først når målet er markeret og healing-lukket, roterer Rogue til flanken for Weakpoint Attack + Back Attack.

**Hvorfor det virker:** Rogue tager nul risiko før fjenden er 30% blødere og 50% dårligere til at heale. Fighter behøver ikke vinde duellen — han skal bare overleve 8 sekunder ad gangen.

---

### Build B — "Dobbelt Blitz" *(høj tempo, høj varians — third-party og squire/lav-GS)*

**Fighter**
- Perks: **Slayer** · **Dual Wield** · **Swift** · **Adrenaline Spike**
- Skills: **Sprint** · **Victory Strike**
- Våben: Arming Sword + Short Sword (dual wield). Slayer = +5 Physical Buff Weapon Damage + 10 MS Add, men **ingen plate**.

**Rogue**
- Perks: **Dagger Mastery** · **Thrust** · **Back Attack** · **Ambush**
- Skills: **Shadowstep** · **Sneak Attack**

**Gameplan:** Ingen banner, ingen opsætning. I finder lyd, I sprinter ind, I dræber inden for 6 sekunder. Victory Strike resetter uden cooldown på player-kill — mod et tredje hold er det en kædereaktion.

**Bemærk:** Slayer-Fighter mister plate, hvilket er præcis derfor build'en fungerer sammen med Rogue: **bevægelseshastighederne matcher**. Se afsnit 7.

---

### Build C — "Bedemanden" *(innovativ — third-party-fælde og hold-genstart)*

Dette er den build ingen kører, og som HF121–123 gjorde levedygtig.

**Rogue**
- Perks: **Creep** · **Veil of Shadows** · **Wall Ride** · **Poisoned Weapon**
- Skills: **Play Dead** · **Caltrops** (eller Smoke Pot)

**Fighter**
- Perks: **Last Bastion** · **Barricade** · **Projectile Resistance** · **Defense Mastery**
- Skills: **Taunt** · **Second Wind**

**Gameplan:** I lader jer bevidst finde. Fighter tanker og trækker sig til lav HP — **Last Bastion** (12s, 32s CD) gør ham til en mur ved 33%. Rogue lægger Caltrops i tilbagetrækningsruten, **Play Dead**'er i et hjørne som lig, healer op mens fjenden looter, og rejser sig i ryggen.

**Play Dead er reelt undervurderet:** 6s cooldown, healer på hvileniveau (HF121), og en Rogue der ligger ned i et rum fuldt af rigtige lig er visuelt umuligt at parse i kampens hede. Kombineret med Veil of Shadows (auto-stealth under 30% HP, bevægelse bryder ikke i 2s) har Rogue **to** uafhængige "jeg er ikke død"-knapper.

**Advarsel:** Rogue har **−27% Health Recovery Bonus**. Play Dead healer langsommere for dig end for nogen anden klasse. Brug den til at *undslippe*, ikke til at fuldheale.

---

## 5. Synergimatrix — hvad der faktisk stacker

| Effekt | Kilde | Mekanik | Stacker med |
|---|---|---|---|
| −30% PDR Mod | Lethal Mark (Rogue) | Multiplikativ damage reduction mod | Alt — ægte multiplikator |
| −30% Item Armor Rating | Weakpoint Attack (Rogue) | Armor rating, ikke PDR mod | Ja, med Lethal Mark |
| −50% incoming healing | Poisoned Weapon ×5 | Healing mod | Uafhængigt lag |
| 15% Armor Pen | Thrust (Rogue, dolke) | Penetration | Ja — men **virker ikke på monstre** (negativ PDR) |
| 1 True Physical Damage | Dagger Mastery | Ignorerer armor helt | Altid |
| +5 All Attributes | Fortified Ground + Jokester | Attributter | Ja, additivt |
| +40% Physical Power | Victory Strike (Fighter) | Power bonus | Ja, og resetter på kill |
| Silence 2s / silence + slow | Cut Throat (R) / Pommel Strike (F) | CC | **Kæd dem** — 4s samlet lockdown |

**Den skarpeste kæde i patchen:**
`Kastekniv (Lethal Mark) → 5× kastekniv (Poison-stacks) → Fighter engagerer → Pommel Strike (silence + −50% MS) → Rogue Shadowstep bag → Sneak Attack → Weakpoint Attack`

Målet er nu: −30% PDR mod, −30% armor rating, −50% healing, silenced, slowed, og bliver ramt bagfra af en klasse der ignorerer armor. Der er ikke meget der overlever den sekvens.

**Sneak Attack-detaljen der afgør det hele:** Sneak Attack har **0% scaling** på sine +15 (og +10 hvis målet er out of combat). Det betyder at **−30%-straffen fra Hide-exit ikke rammer den**. Alle andre openere bliver straffet. Dette er ikke valgfrit — det er *den* korrekte hide-opener i Season 10.

Tilsvarende: **Back Attack (+30% Physical Power) og Hide-exit (−30%) ophæver stort set hinanden.** Det er tydeligvis tilsigtet fra Ironmace. Kommer du ud af hide og rammer forfra, slår du −30%. Rammer du ryggen, slår du neutralt. Derfor: Shadowstep (teleporterer dig *bag* målet ved hit) er nu en damage-perk i forklædning.

---

## 6. Counters — begge veje

### 6.1 Hvad duoen slår

| Modstander | Hvorfor I vinder | Udførelse |
|---|---|---|
| **Cleric-duos / sustain-comps** | Poisoned Weapon ×5 = −50% healing, appliceret fra afstand | Marker healeren først, ikke tanken. Cleric er nerfet gentagne gange (Vow of Silence 10→6→4% pr. stack, Sacred Flame coefficient 1.0 → 0.25, Radiant Blade 20 → 12, Aura of Awe 3m → 2m) — han er en mur uden en hammer |
| **Barbarian** | HF123: Whirlwind gør **10% mindre skade mod skjolde, vægge, døre**. War Cry 20% → 15% max HP, varighed 10 → 8s. Savage Roar CD 32s | Fighter med skjold i dørkarm. Rogue rører ham *aldrig* frontalt |
| **Wizard** | S10 nerfede alle kernespells (Fireball 35→30, Zap 20→15, Ice Bolt 30→20, Lightning Strike 30→25) | Spell Reflection (Fighter) + Cut Throat silence (Rogue). Teleport-distancen blev sænket 600 → 500 i HF122 — han kan ikke flygte lige så langt |
| **Isolerede solo-spillere** | To vinkler, én af dem usynlig | Standardprocedure |
| **Andre lette duos (Rogue/Ranger, Bard-comps)** | Fighterens PDR-gulv er uopnåeligt for dem | Fighter walker dem ned, Rogue lukker udgangen |

### 6.2 Hvad der slår duoen

**Den strukturelle svaghed: I er 100% fysisk damage, med nul ranged burst, nul heal-output og nul area-denial.**

| Trussel | Hvorfor det gør ondt | Modtræk |
|---|---|---|
| **Højt PDR-stack (plate Fighter, Cleric i tungt)** | Begge jeres damage-typer er den samme type. Én counter lukker jer begge | Thrust (15% armor pen) + Dagger Mastery True Damage. Flanged Mace/Morning Star fik pen nerfet til 10% i HF123 — de er ikke længere svaret |
| **Warlock** | Curses, DoT, Power of Sacrifice. Rogue har **1,5% Magic Resistance** ved base | **Aegis-artefaktet (+50 MR)** på Fighter. Rogue skal have MR-ruller på gear — Arcane Hood fik +30 MR i S10 |
| **Ranger** | Longbow fik +3 damage i S10 og action speed op i HF123. I har ingen ranged svar ud over kasteknive | Fighter: Projectile Resistance perk (10%, +10% i defensiv stance) + Last Bastion (30% projectile DR). Rogue: Wall Ride til vertikale ruter, Shadowstep til gap-close |
| **Sorcerer** | Buffet i HF123: Mana Flow 5% → **10%** magical damage bonus og magic penetration, varighed 4 → 5s. Flamethrower 20 → 25. Flamefrost Spear 30/30 → 35/35 | Silence-kæden er alt I har. Cut Throat → Pommel Strike. Ellers: bryd line of sight |
| **Døre og dørkarme** | Neutraliserer Rogue-flanken fuldstændigt | Wall Ride giver vertikale indgange. Ellers: accepter at Rogue bliver anden melee og ikke en flanker |
| **Lysdisciplin hos fjenden** | **Rogue kan ikke Hide mens han holder en lyskilde**, og potions i quickslots lyser i mørket | Afmonter potions fra quickslots inden mørk approach. Buff-abilities og Poison Weapon får dine våben til at gløde — timing betyder alt |
| **Lyd** | Våben- og item-swap larmer **selv i stealth** | Swap *inden* du gemmer dig, aldrig under |
| **Third-party under jeres setup** | Build A's opsætningsfase er langsom | Fighter skal kunne holde 8s alene mens Rogue roterer tilbage |

### 6.3 Counters mod hver enkelt

**Mod jeres Rogue:**
Alt der tvinger ham til at stå stille eller tage skade. Han har 108,5 HP, −22% PDR, 1,5% MR og −27% Health Recovery. Enhver AoE, enhver DoT, enhver Ranger med line of sight. Hans mest fatale fejl er at bryde Hide for tæt på — −30% Physical Power i 3s betyder at en mislykket åbning er en dødsdom, ikke en tabt tempofordel.

**Mod jeres Fighter:**
Kiting. Han kan ikke fange nogen. Sprint giver 3 stacks à 13 MS add, tabende 1 stack pr. 2s — det er 6 sekunder værdi på 28s cooldown. Mod Wizard med Fly eller Ranger med afstand er han et mål. Derudover: **magisk skade**, som omgår hele hans armor rating.

---

## 7. Bevægelseshastighed — duoens skjulte problem

Dette er dét der reelt splitter Rogue/Fighter-duos ad, og næsten ingen guides nævner det.

| | Base Move Speed |
|---|---|
| Rogue | **306 (102%)** |
| Fighter | **300 (100%)** |

Men efter gear og perks:
- Rogue i let kit med **Creep (+10% MS)**, **Ambush (+10% MS ved hide-exit)**, **Trickster (+10% MS ved kast)**, **Stealth (+3 MS Add pr. resterende skridt)** → nemt **330+**
- Fighter i fuld plate → ned mod **270**

**Det er over 20% forskel.** I rotationer, third-parties og escapes bliver duoen til to soloer. Tre løsninger:

1. **Fighter bygger til MS**: Swift (−20% armor speed penalty), Slayer (+10 MS Add, men ingen plate), Sprint. Koster PDR.
2. **Rogue leasher sig selv**: aldrig mere end ét rum foran. Kedeligt, men korrekt i Build A.
3. **Accepter splittet bevidst** (Build C): Rogue scouter og markerer, Fighter ankommer sent. Kun hvis I har voice comms og disciplin.

Vælg én. Improvisér ikke — det er dér holdet dør.

---

## 8. Gear guide

### 8.1 Stat-prioritering

**Rogue** (i rækkefølge)
1. **Additional Physical Damage / Weapon Damage** — dolke har lav base og høj swing rate; flad damage skalerer bedst
2. **Agility** — action speed + move speed, jeres primære skalering
3. **Move Speed** — overlevelse *er* bevægelse for denne klasse
4. **Strength** — Physical Power Bonus starter på −14%, det skal repareres
5. **Vigor** — 108,5 HP er ikke nok
6. **Magic Resistance** — 1,5% base er en åben flanke mod Warlock/Sorcerer
7. Resourcefulness — allerede 25 base; kun hvis I kører loot-fokus

**Fighter** (i rækkefølge)
1. **Armor Rating** — Defense Mastery hæver PDR-cap til 75%, gå efter det
2. **Strength** — direkte damage
3. **Additional Physical Damage**
4. **Vigor** — 125 base, byg videre
5. **Move Speed / Armor Speed Penalty-reduktion** — se afsnit 7
6. **Magic Resistance** — 7,8% base, jeres eneste modsvar mod caster-comps

### 8.2 Våben

**Fighter — anbefalet, HF123**
- **Arming Sword + Lantern Shield** — den stærkeste konfiguration lige nu. HF121 fjernede 0,4s post-attack delay på riposte-chainens andet hit og udvidede weak block-vinduet. Lantern Shield fik også impact resistance 4 → 5 i HF120.
- **Longsword** — parry sweet spot flyttet tættere på crosshair (HF121), +3 damage fra S10-animationsopdateringen. Bedste rene duelist-våben.
- **Undgå lige nu:** Flanged Mace, Morning Star, War Hammer — alle nerfet i HF123 (damage −1, armor pen 15% → 10% på de to første).
- Med **Sword Mastery**: +2 Physical Buff Weapon Damage, +5% Action Speed, +10 MS Add i defensiv stance. Sværd er mekanisk favoriseret i denne patch.

**Rogue**
- **Rondel Dagger** main hand (armor pen-profil, matcher Thrust)
- **Castillon Dagger** eller **Stiletto** offhand
- **Kris Dagger** hvis I vil have swing speed over penetration
- **Throwing Knives i utility — obligatorisk.** Uden dem har I ingen Lethal Mark og ingen ranged Poison-applikering, og hele build'en falder fra hinanden.

### 8.3 Gear-brackets

**Budget / Normal (grøn–blå)**
- Fighter: Ornate Jazerant (AR 70–82, MR 32–38 efter S10-justering), Barbuta Helm (AR 26–38, buffet i S10), Plate Pants hvis MS tillader
- Rogue: Rogue Cowl / Shadow Hood, Light Aketon, **Dark Leather Leggings** (AR blev nerfet 50–59 → 33–39, men MS-penalty faldt 5 → 4), Lightfoot Boots
- Køb 2× Throwing Knife-stacks. Køb bandager, ikke potions — Rogue har **−11% Buff Duration**, potions healer mindre for jer

**High Roller-indgang (~225–280 GS — bemærk: kravet blev sænket 280 → 225 i HF123)**
- Fighter: fuld Defense Mastery-optimering. Dark Plate Armor (MR 39–45, Vigor 1–5, men MS-penalty steg 11 → 14 — vej det)
- Rogue: **jagt uniques på Marketplace.** Uniques ruller nu **2 modifiers med op til 2× tidligere maksværdi** — det er det største værdi-pr-guld-skifte i S10
- Begge: Magic Resistance-ruller. Arcane Hood fik +30 MR tilføjet i S10 (mod lavere AR)

**BiS / Artefakter**
- **Aegis** (+50 Magic Resistance) — Fighterens svar på hele caster-metaet. Højeste prioritet af alle artefakter for denne duo
- **Grimveil Cloah** — droprate blev øget i HF121, mere realistisk at få nu
- **Windreaver** — first attack wind range 200 → 300 (HF122)
- **Titanium Pavise**, **Shadowbite**, **Tear of Hrímthurs**, **Soul-Devoted Folio** (rarity hævet Epic → Legendary i HF122)
- Undgå at overbetale for **Stillmend Boots** — nerfet i HF122 (nu out-of-combat i stedet for stationær, og 1 HP pr. 5s i stedet for pr. 2s)

### 8.4 Consumables
- **Rogue: bandager over potions.** −11% Buff Duration betyder potions varer og healer mindre for jer end for alle andre.
- Fighter: Second Wind er 40% max HP over 12s med 1 charge — genoplades ved campfire (Tier 5-rate). Planlæg jeres campfires.
- Fjern potions fra quickslots før mørke approaches — de gløder og afslører Rogue.

---

## 9. Eksekverings-playbook

**Rutine 1 — Standard engagement (Build A)**
1. Fighter planter Fortified Ground *inden* kontakt (12s CD, uendelig varighed — der er ingen undskyldning)
2. Rogue finder line of sight fra 12–15m, ikke fra flanken endnu
3. Kastekniv → **Lethal Mark**. Derefter 4–5 knive → **Poison ×5**
4. Fighter engagerer i dørkarm/flaskehals
5. **Først nu** roterer Rogue. Shadowstep på hit → lander bag målet
6. Sneak Attack (immun over for hide-straf) → Weakpoint Attack → Back Attack-vinkel
7. Ved dårlig udvikling: Rogue Hide (repositionering, ikke damage), Fighter Second Wind

**Rutine 2 — Silence-kæden mod castere**
Cut Throat (2s silence) → Fighter Pommel Strike (silence + 10 ekstra base damage hvis den afbryder en handling + −50% MS i 2s). Det er ~4s hvor en Wizard eller Cleric ikke er en klasse.

**Rutine 3 — Anti-heal-låsen**
Mod ethvert hold med en Cleric, Druid eller Bard: Rogue ignorerer frontlinjen fuldstændigt. Kasteknive går udelukkende ind i support-spilleren indtil 5 Poison-stacks er oppe. −50% incoming healing på begge damage-typer. Så først skifter I mål.

**Rutine 4 — Boss-looting (ændret i HF122)**
Loot-prioritet på boss-lig gives nu til **det hold der har gjort mest kumulativ skade**, ikke til det sidste hit. Hvis bossen resetter og healer, nulstilles den akkumulerede skade også. Det betyder: **stjæl ikke last-hit — commit til hele fighten eller lad være.** Duoens Firedeep-viability afhænger af det: Magma Wyvern fik movement speed, health og damage hævet i HF122, derefter health let sænket i HF123. Fire Colossus fik hitbox-rækkevidde og health let sænket i HF123.

---

## 10. Åbne punkter — hvad der bør testes

Disse er ikke afklaret i patch notes eller wiki, og de påvirker build-valg direkte. **De er også kilden til `Unverified`-markeringerne i Assay (ADR-007).**

1. **Ambush** — wiki'en angiver stadig at et succesfuldt melee-angreb fjerner buffen, selvom damage-bonusen er fjernet. Er det stadig tilfældet? Afgør om Ambush overhovedet er en slot værd.
2. **Trickster** — virker de 30% Action Speed på *alle* throwables, eller kun kasteknive? Påvirker Build A's utility-valg.
3. **Dual Strike** — hvilke on-hit-effekter tæller dobbelt? Hvis Poison eller Rupture applikeres to gange, er skillen kraftigt undervurderet.
4. **Shield Mastery** — er de 50% Action Speed en buff der forbruges på næste swing, eller et vindue på 1s?
5. **Victory Strike** — på reset: gengives buffen instant, eller nulstilles kun cooldown? Afgør om Build B's kædereaktion er reel.
6. **Shadowstep** — hvad tæller som gyldigt teleport-mål? Monstre? Deployables?

Test punkt 3 og 5 først — de er de to der kan flytte en hel build.

---

*Kilde: Dark and Darker Wiki (spellsandguns), CC BY-SA 4.0. Klassedata verificeret opdateret til Patch 6.12 Hotfix 123. Patch notes 6.12: Patch 12 (S10, 17. juli), Hotfix 120 (23. juli), 121 (30. juli), 122 (6. aug), 123 (13. aug).*
