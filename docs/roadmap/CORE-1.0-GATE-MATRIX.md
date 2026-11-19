# Ravum Core 1.0 — registre unique des gates T0–T12

> **Registre de suivi, pas un certificat.** Un seul tableau fait foi pour l'état
> des gates. Aucun `PASS` n'y est écrit sans preuve exécutée, liée à un SHA et
> reproductible par la commande citée.
>
> États autorisés : `PASS`, `PARTIAL`, `FAIL`, `BLOCKED`, `NOT_RUN`.
> `SKIP` et `NOT_RUN` ne sont **jamais** convertis en `PASS`. Un `BLOCKED`
> n'est jamais converti automatiquement en `PASS`.

## État courant

**Mesure du 2026-11-15 (E1, branche `goal/production-plan-2026-11-15`, fusionnée
dans `dev` = `228abee`, Windows 11 x64, `rustc 1.95.0`), rejouée par la CI des
trois OS (8/8 runs verts)** :

> **Conformance courante (`run --verify-build`)** :
> **503 cas : 502 PASS / 0 FAIL / 1 SKIP**,
> `perimeter=222/222`, `verified=124`, `implicit=98`, `failed=0`, `build_verified: true`.

| Mesure | Résultat courant |
|---|---|
| Conformance (`cargo run -p ravum-conformance -- run --verify-build`) | **503 cas : 502 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN**, exit 0 ; `perimeter=222/222`, `verified=124`, `implicit=98`, `build_verified: true` |
| Workspace (`cargo test --workspace --no-fail-fast`) | **124 suites, 1740 passés / 0 échec / 8 ignorés**, exit 0 |
| Traçabilité FeatureId (`features_coverage.py --strict`) | **188/188** règles tracées, **188/188** satisfaites (1 exemption `F-META-006` approuvée) |
| Traçabilité MUST (`must_audit.py`) | **811** énoncés, **467** mappés, **344** non mappés (dette D10) |
| Registre des codes (`registry_audit.py`) | **108** déclarés, **126** émis, **0** hors registre, **0** jamais émis |
| Oracle différentiel rapide | **3 graines × 200 programmes, 0 divergence** ; la campagne nominale 3 × 2 000 reste l'étape E7 |

Aucun gate n'est promu `PASS` par E1 : T2 reste `PARTIAL` (dénominateur MUST,
D10), T4/T12 restent `BLOCKED` (audit externe, Go/No-Go humains).

**Sections datées = archives.** Les anciennes sections `§0 bis` à `§0 decies` et
`§1 bis` à `§1 sexies`, qui portaient les chiffres **de leur passe** (et que deux
relecteurs externes avaient prises pour l'état du HEAD), sont déplacées telles
quelles dans
[`docs/roadmap/archive/CORE-1.0-GATE-MATRIX-ARCHIVE.md`](archive/CORE-1.0-GATE-MATRIX-ARCHIVE.md).
Seul le présent fichier fait foi pour l'état courant, avec
[`LANGUAGE-CORE-STATUS.md`](../reports/LANGUAGE-CORE-STATUS.md) pour le détail des
commandes.

## 0. Base de référence

| Élément | Valeur |
|---|---|
| `main` (référence documentaire, pas une preuve de maturité) | `2f79a63` — fusion sans tag ni release (PR #101 → `dev` `a2b8db5`, PR #104 → `main` `94f3ac4`) |
| `dev` (branche d'intégration) | `228abee` — après E1 (fermeture divergence `check → build`) et E2 (garde-fous Clippy/deny/Dependabot/CODEOWNERS/CHANGELOG/MSRV) |
| Branche de travail E1/E2 | `goal/production-plan-2026-11-15`, fusionnée dans `dev` |
| Toolchain testée | `rustc 1.95.0`, `cargo 1.95.0` ; `rust-version = "1.95.0"` |
| Licence | `MIT OR Apache-2.0`, 23/23 membres du workspace |
| CI `dev` | **8/8 runs verts** sur `228abee` (Linux, macOS, Windows evidence, MVP, coverage, contracts, docs, VS Code) |

> **Demande de décision finale E10 :** [`E10-FINAL-DECISION-REQUEST-2026-11-19.md`](../decision/E10-FINAL-DECISION-REQUEST-2026-11-19.md).

## 2. Matrice

> **V1 (2026-11-19, responsable)** : D2 exige tous les points d'architecture prévus implémentés et validés ; A2 et A3 annulés ; gates T13–T19 ajoutés — [DECISIONS-2026-11-19](../decision/DECISIONS-2026-11-19-V1.md).

| Gate | État | Ce qui est prouvé | Ce qui manque / échoue | Blocage |
|---|---|---|---|---|
| T0 | `PASS` | Corpus de sécurité du socle vert (overlays, publication Git, reprise après crash, journal, concurrence, symlinks, chemins hostiles) | — | — |
| T1 | `PARTIAL` | Voies A et B réelles (`check→build→run`), parité A/B mesurée | **Voie C hors périmètre 1.0** ; pas de parité A/B/C ; choix final non ratifié | Décision humaine (ADR) |
| T2 | `PARTIAL` | **2026-11-19 (E10) : FeatureIds 244/244, MUST 876 dont 95 sans FeatureId (`must_audit.py`).** Mesure antérieure du même jour : **FeatureIds 236/236 sans exemption (F-META-006 révoquée, l'interpréteur de référence existe), MUST 720/817, 97 sans FeatureId ; comparaison différentielle oracle ↔ backend : 54/54 accords** (lots `lexical.md`, `grammar.md`, `diagnostics.md`, `compatibility.md`, `abi.md`, `modules.md`, `concurrency.md`, `stdlib.md`, `effects.md`, `ownership.md`, `features.md`, `unsupported.md`, `types.md` et `expressions.md`). Mesure antérieure : **FeatureIds : 188/188** tracés et satisfaits au critère strict (187 par cas + 1 exemption vérifiée) ; 108 codes enregistrés / 126 émis / 0 hors registre ; contrat `check → build` **mesuré et bloquant** (`--verify-build` : périmètre **222/222**, vérifiés 124, implicites 98, échoués 0), **aucune forme ouverte connue** (divergence `power-array-move-write-region-nested` fermée le 2026-11-15, `RFC-OWN-ARRAY-ELEMENTS.md`) | **La seconde métrique change le verdict** : 811 énoncés MUST, **467/811** seulement atteignables par un FeatureId, **344** sans FeatureId (`must_audit.py`). L'exemption `F-META-006` est **approuvée** (2026-11-12, réexamen 2027-02-12) mais sans oracle | Dette tracée (D10) : 344 MUST non mappés, pas un blocage de release |
| T3 | `PASS` | Lexer/parser/AST réels, **1129 tests**, fuzz complet rejoué (2 campagnes, 0 échec), Miri Linux rejoué en CI (164 tests, 0 échec) + Miri Windows partiel, formateur idempotent, 8 défauts frontend corrigés | — (divergences de codes restantes traitées sous T2/D8 ; plus aucun code hors registre) | — |
| T4 | `BLOCKED` | Typechecker, borrow checker, cas adversariaux internes | **Audit externe humain absent** | Audit externe |
| T5 | `PASS` | Binaires natifs réels Windows (smoke 43 PASS) et Linux (CI) ; WASI exécuté ; `build` publie une identité déterministe et un artefact atomique | ABI stable non revendiquée ; sanitizers non exécutés | — |
| T6 | `PARTIAL` | Stdlib core/io/fs/string/vec/math + FFI minimale scalaire (C-FFI-1 à 10 approuvés) ; threads et canaux (STD-18), TCP (STD-17), HTTP/1.1 (STD-19), bibliothèques natives et SQLite ; TSan bloquant sur le corpus concurrent (`.github/workflows/tsan.yml`, std reconstruite avec TSan) | `async` : TCP asynchrone, mais `connect` bloquant et pas d'HTTP asynchrone ; TSan vert en CI depuis #198 | Implémentation |
| T7 | `PASS` | CLI complète ; LSP réel (11 tests client) ; harnais VS Code stabilisé ; 10/10 au repos, 10/10 sous charge, job CI `vscode-e2e` vert | Flakiness E2E ordinaire d'une suite Electron ; Windows/macOS non couverts par ce job | — |
| T8 | `PARTIAL` | `watch` fail-closed ; overlays/snapshots ; oracle cold vs warm ; `watch --reload` avec préservation d'état ([`reload.md`](../../spec/language/reload.md), 6 tests « 0 perte » dans `crates/ravum-lang-cli/tests/hot_reload.rs`) ; bloc DSP `.rv` chargé par l'hôte (`build --emit dsp-block`, différentiel bit à bit, 0 allocation au callback sur 10 000 blocs et 20 remplacements, `tests/dsp_block.rs`) | État DSP limité à `{ z1, z2 }` (ABI 2 non ratifiée) ; vertical `examples/dsp-gain-iir` non branché ; fondu non mesuré sur un bloc `.rv` | Implémentation |
| T9 | `PARTIAL` | 10 verticales `.rv` exécutées, dont `examples/file-store` ; HTTP + SQLite (`packages/http-notes`, Linux, macOS, Windows) ; bloc DSP `.rv` dans l'hôte réel (`tests/dsp_block.rs`) | `examples/dsp-gain-iir` non branché sur l'hôte ; E2E navigateur absent (D-18-2 a annulé le report A2 du DSP) | Implémentation |
| T10 | `PARTIAL` | 1 740 tests workspace ; conformance 503 cas avec build vérifié bloquant ; smoke 43/0/0 ; contrats Python ; Miri + fuzz smoke en CI ; couverture sema+codegen ~90 % (objectif 100 % inscrit) ; mutation ciblée rejouée en CI | Audit externe absent ; couverture loin de 100 % sur `emit.rs`/`loader.rs` ; fuzz complet non rejoué ; aucune mesure de performance au repos | Audit externe |
| T11 | `PARTIAL` | Installateur rejoué en CI ; SBOM déterministe ; provenance émise/vérifiée ; licence réconciliée ; build bit-à-bit ; mécanisme de signature OpenSSH qualifié | Aucune clé de projet ni ancre de confiance ; reproductibilité inter-machines limitée | Décision humaine D9 (custody de clé) |
| T12 | `BLOCKED` | Dossier de RC interne, PR, rapports, matrice | Dogfooding indépendant, Go/No-Go humain | Décision humaine |
| T13 | `FAIL` | Parsing mis en cache par fichier ; **étape 1 (2026-11-19)** : planificateur d'invalidation fail-closed `ravum-lang-incr`, oracle 3 048 mutations / 0 plan non sûr, constat F1 (barrière de phase globale) — [rapport](../reports/T13-STEP1-INVALIDATION-PLANNER.md) | Typage/ownership/effets recalculés sur tout le paquet ; pas d'incrémental par fonction (ADR-012/020) | Implémentation (V1) |
| T14 | `PARTIAL` | **2026-11-19** : IR **v1.1** (`ravum-ir-1.1`) : scalaires + mémoire explicite (`Ptr`, emplacements de pile, `Load`/`Store`, `PtrAdd`) pour structs/tuples/tableaux `Copy`, `&`/`&mut` vers la mémoire, retours d'agrégats par adresse, bornes vérifiées ; schéma de `struct State` = `reload::schema` (HR-1.1) ; **2 consommateurs** (interpréteur, JIT) ; machine indépendante : corpus **44/59 identiques**, **300/300** programmes agrégats générés identiques, 0 divergence ; **IR v1.2** (`ravum-ir-1.2`) : méthodes `abs`, `wrapping_*`, `saturating_*`, `f64::{abs,sqrt,floor,ceil}`, **8 058/8 058** programmes aux bornes identiques à la machine ; enums `Copy`, `Option`, `match`, `checked_*` (**3 775** + **614** programmes identiques à la machine, 614 à champs nommés natif = interpréteur) ; `io::print*` de littéraux et de scalaires (sortie identique à la voie `rustc`) ; `math::*` STD-6.3/6.5 (817 programmes) et `Option::is_some/is_none/unwrap_or` ; méthodes utilisateur (`impl` non générique, identiques à la voie `rustc`) ; corpus **54/60** ; bug de la machine corrigé (`saturating_sub` non signé) — [rapport](../reports/T14-T15-IR-AND-JIT.md) | Pas d'enum ni d'`impl` génériques/Result/String/Vec/`f64::round/min/max`/`pow`/libm/`i128`/`u128` ; pas de références vers des scalaires ; message de bornes court (EXPR-7.6 demande la longueur et l'index) ; ADR-010 non ratifié | Implémentation (V1) |
| T15 | `PARTIAL` | **2026-11-19** : backend natif Cranelift 0.116 en processus (`ravum-lang-jit`) ; oracle différentiel 1 805 programmes générés + 42 du corpus, 0 divergence (natif = interpréteur = machine) ; build 100 fonctions ~17 ms vs ~370 ms par `rustc` (≈ ×21), 800 fonctions ~150 ms vs ~725 ms (≈ ×4,8) — [rapport](../reports/T14-T15-IR-AND-JIT.md) | Même sous-ensemble que T14 ; **`cargo-deny` rouge** (licence `Apache-2.0 WITH LLVM-exception` + 3 doublons) → décision du responsable ; `unsafe` site #7 non audité ; pas de validation de traduction formelle (ADR-018) | Décision licence + audit (V1) |
| T16 | `PARTIAL` | **2026-11-19** : hot swap **par fonction** en processus sur le backend natif (`ravum-lang-jit::hot`) : table d'indirection, seule la fonction modifiée est recompilée, publication à un point sûr par boîte aux lettres atomique, refus fail-closed (signature d'entrée), retour à la version précédente sur trap (HR-1.5) ; 20 éditions sous un thread « audio » (blocs de 64 à 48 kHz) : **0 allocation / 0 libération** au thread temps réel, édition→activation **p95 ≈ 2,9 ms** (release), code ancien récupéré (≤ 3 modules) — [rapport](../reports/T16-NATIVE-HOT-SWAP.md). Côté `rustc` : `RFC-E11-HOT-RELOAD`/`RFC-E11-DSP-BLOCK` (autre session) | **Bloc DSP avec état** (`struct State`, `init`, `process_sample(&mut State, f32)`) : sortie bit à bit = interpréteur = Rust de référence, état conservé au remplacement, schéma changé refusé (HR-1.3), écriture partielle annulée sur trap (HR-1.5), 0 alloc, p95 ≈ 1,4 ms — [rapport](../reports/T16-NATIVE-HOT-SWAP.md) §7. Restent : `migrate` (HR-1.7) non supporté sur cette voie ; pas d'hôte audio réel ; CLI : `ravum-lang watch --backend jit` (§8 du rapport) ; sites `unsafe` #7b/#8/#9 non audités ; aucune garantie hard-RT | Implémentation (V1) |
| T17 | `PARTIAL` | **2026-11-19 étape 1** : replay déterministe d'une session de hot swap sur le backend natif (`ravum-lang-jit::replay`) : session réelle sur thread temps réel (20 éditions, dont un trap avec retour arrière) rejouée **bit à bit**, **101/101** digests identiques, refus identiques au direct — [rapport](../reports/T17-STEP1-HOT-SESSION-REPLAY.md) | Profil déterministe **du langage** (horloge/aléa/ordonnancement virtualisés, replay par seed) non fait ; une plateforme ; `migrate` JIT en attente de décision | Implémentation + décision `migrate` (V1) |
| T18 | `NOT_STARTED` | `assert`/`panic` | Contrats `requires`/`ensures`/invariants avec statut honnête (ADR-004/028) | Implémentation (V1) |
| T19 | `PARTIAL` | Diagnostics JSON à codes stables | Liens diagnostic→contrat→preuve→test versionnés (ADR-027/040) | Implémentation (V1) |

## 3. Détail par gate

### T0 — Fondations et sécurité du socle — `PASS`

Inchangé. Suites `overlay`, `git_publication`, `git_publication_crash`,
`journal_durability`, `cycle_integration`, `merge_strategies`,
`transient_verification` vertes, plus le scénario
`publication → snapshot → même fichier → publication`.

### T1 — Architecture du compilateur et bootstrap — `PARTIAL`

Voie B réellement utilisée par `build`/`run`/`test` ; voie A opérationnelle ;
parité A/B antérieure `MATCH` sur 4 programmes. **La voie C (backend natif
direct) est hors périmètre 1.0** (décision A3) : le gate est évalué sur « voies A
et B comparées, B retenue pour 1.0 ». Le verdict reste `PARTIAL` : ce changement
de formulation ne prouve rien de nouveau. Décision humaine D2.

### T2 — Spécification formelle — `PARTIAL` (dénominateur MUST ouvert)

Le tableau de la §2 fait foi : `PARTIAL`, parce que le critère `188/188`
FeatureIds ne mesure pas les **811 énoncés MUST** (467 atteignables, 344 non
mappés) — c'est la décision humaine **D10**, pas un défaut de tests. Passe de
traçabilité complète dans
[`docs/qualification/T2-FINDINGS.md`](../qualification/T2-FINDINGS.md).

- **188/188** règles tracées vers un cas ou une exemption déclarée et
  structurellement vérifiée ; **1 règle non testable** (`F-META-006`), exemptée
  dans [`tools/coverage_exemptions.json`](../../tools/coverage_exemptions.json)
  avec raison, référence normative, approbateur, date et condition de réexamen.
- **Aucun cas n'échoue** : les codes émis hors registre ont été alignés sur
  `diagnostics.md` §3 ; les codes légitimes ont été enregistrés, pas remplacés.
- **Aucun cas assoupli** : `expect_codes` vient de la spec, jamais de la sortie
  du compilateur.

### T3 — Frontend — `PASS`

Lexer/parser/AST réels, récupération, garde-fous, formateur idempotent, corpus
négatif, interfaces LSP effectives. Fuzz complet rejoué (249 054 entrées lexer /
parser, 100 000 itérations sema, 0 échec). Miri rejoué en CI Linux. Aucun code
hors registre ne subsiste.

### T4 — Sécurité mémoire, ownership, effets — `BLOCKED`

Le contrat exige un **auditeur humain indépendant**. Un audit mené par un autre
agent reste un audit interne. Dossier prêt pour l'auditeur :
[`docs/audit/AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md).

### T5 — Codegen, ABI, exécution native — `PASS`

Binaires natifs réels Windows (smoke 43 PASS) et Linux (CI). Portée : Windows
x86-64 + Linux x86-64 sur le corpus et les exemples. Non revendiqué : ABI stable,
macOS, ARM, sanitizers.

### T6 — Runtime, stdlib, FFI — `PARTIAL`

`core/io/fs/string/vec/math`, FFI minimale scalaire (`extern "C"`, `unsafe`,
primitifs, effets `ffi`/`unsafe`, refus hors `unsafe`) ; frontière C-FFI-1 à
C-FFI-10 approuvée (D7, S4). Threads, réseau, async, temps réel dur explicitement
`UNSUPPORTED`.

### T7 — Environnement de développement — `PASS`

Extension réelle `tools/vscode-ravum`, LSP en stdio, 10 tests d'intégration dans
un VS Code réel, harnais stabilisé (budgets 300 s/180 s, barrière de
disponibilité LSP), 10/10 au repos et sous charge, job CI `vscode-e2e` vert.
Risque résiduel : flakiness E2E ordinaire, pas de couverture Windows/macOS du job.

### T8 — Compilation continue et hot reload — `PARTIAL`

`watch` recompile et redémarre ; pas de préservation d'état ; contrat
d'invalidation mesuré (36 éditions incrémentales). Le bloc DSP est reporté à la
1.1 (A2). T8 reste `PARTIAL`.

### T9 — Applications réelles `.rv` — `PARTIAL`

10 verticales exécutées, plus le service HTTP + SQLite `packages/http-notes`
(2026-11-19, D-19-8 ; `crates/ravum-lang-cli/tests/http_notes.rs`, Linux,
macOS et Windows). Bloc DSP `.rv` dans l'hôte réel : `tests/dsp_block.rs` ;
le vertical `examples/dsp-gain-iir` n'y est pas encore branché.

### T10 — Qualification intensive et audits — `PARTIAL`

Mesure courante (E1, 2026-11-15) : 1 740 tests workspace, 503 cas de conformance
(502 PASS / 0 FAIL / 1 SKIP), build vérifié bloquant, mutation ciblée rejouée en
CI, fuzz smoke vert. **Audit externe absent** — c'est ce qui empêche `PASS`.

### T11 — Performance, distribution, supply chain — `PARTIAL`

SBOM SPDX-2.3 reproductible, checksums, manifeste de provenance, licence
réconciliée, build bit-à-bit mesuré, reproductibilité sur deux runners
`ubuntu-latest` (même image, même toolchain), contrat d'installateur Linux,
mécanisme de signature OpenSSH qualifié. **Aucune clé de projet ni ancre de
confiance** : la custody reste D9. T11 reste `PARTIAL`.

### T12 — Dogfood, audit final, RC — `BLOCKED`

Dossier de RC interne complet. Absents : dogfooding par des agents n'ayant pas
développé le compilateur, et Go/No-Go humain. Verdict plafonné à
`CORE_RC_CANDIDATE`, jamais `RELEASED`.

## 4. Décisions humaines restantes

| ID | Sujet | État |
|---|---|---|
| D1 | Licence | **RÉSOLUE** : `MIT OR Apache-2.0` |
| D2 | Ratification des ADR / choix du backend | **PARTIELLE** : cible **production ready** actée ; les ADR restent `PROPOSED` (aucune ratification par un agent) |
| D3 | Fusion dans `main` et publication | **RÉSOLUE (2026-11-12)** : fusion sans release (PR #101 → `dev` `a2b8db5`, PR #104 → `main` `94f3ac4`) ; **aucun tag, aucune release, aucune signature** |
| D4 | Audit externe (T4/T10/T12) | **OUVERTE** — bloque T4 et T12 ; cadre de mission prêt : [`AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md) |
| D5 | Périmètre Tier 1 | **PARTIELLE** : Windows local + Linux CI ; macOS CI Tier 2 ajouté et vert |
| D6 | Périmètre des fonctionnalités universelles | **RÉSOLUE (2026-11-11)** : toute extension est une version 1.x ; A2/A3 (2026-11-14) précisent le périmètre 1.0 |
| D7 | Ratification de la frontière `unsafe`/FFI | **APPROUVÉE pour la frontière scalaire (2026-11-14, S4)** : `SAFETY.md` C-FFI-1 à 10 ; l'audit externe (D4) reste requis |
| D8 | Arbitrage des codes de diagnostic | **RÉSOLUE** : « tout code absent est interdit » |
| D9 | Custody de la clé de signature | **REPORTÉE** : procédure proposée [`KEY-CUSTODY.md`](../release/KEY-CUSTODY.md) |
| D10 | Dénominateur de la traçabilité MUST | **RÉSOLUE (2026-11-14, S5)** : objectif 100 % du code produit + mutation ciblée ; les 344 MUST non mappés sont une **dette tracée**, pas un blocage ; T2 reste `PARTIAL` |
| D12 | Disposition des divergences `check → build` | **RÉSOLUE (option (a))** : abaissement général par place, sans amendement de règle |
| D13 | Provenance de `&'static str` à travers les agrégats | **RÉSOLUE et IMPLÉMENTÉE (2026-11-12)** : la preuve (`RFC-D13-STATICITY.md` §6) |

## 5. Règles de mise à jour

- Un changement d'état exige une commande, une sortie et un SHA.
- Un test ignoré ou non exécuté n'est jamais compté comme réussi.
- Un `SKIP` lié à l'OS est exécuté sur l'autre plateforme avant d'être coché.
- Aucun `PASS` obtenu en modifiant la définition du gate.
- **Une exemption de couverture n'est valide que si elle est machine-lisible,
  justifiée, normativement référencée et révisable** (`tools/coverage_exemptions.json`,
  validé par `features_coverage.py --strict` et la CI). Une phrase dans un rapport
  n'exempte rien.
- **Un gate peut régresser** : une information nouvelle qui dégrade un état est un
  progrès de mesure, pas une régression de qualité.
- **Les sections datées sont des archives** : elles vont dans
  `docs/roadmap/archive/`, pas dans ce registre.
