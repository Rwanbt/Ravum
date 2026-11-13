# GOAL « production ready » — rapport final (Core 1.0)

> **Ce rapport n'est pas une déclaration de disponibilité.** Il dit ce qui a été
> **exécuté** dans cette session, avec la commande et le SHA, ce qui ne l'a pas
> été (`NOT_RUN`), ce qui est bloqué par une décision humaine (`BLOCKED`,
> `PENDING_HUMAN_APPROVAL`). Aucun `PROVED`, aucune maturité « production » n'y
> est écrit. Le label appartient au responsable.
>
> Branche : `goal/production-ready` (depuis `test/coverage-batch-1`, lui-même
> `dev` + 3 commits de couverture). SHA final mesuré : voir §2.8.
> Machine : Windows 11 x64 10.0.26100, i7-13620H 10C/16T, 16 Go,
> `rustc 1.95.0`.

## 1. Résumé en dix lignes

1. Le corpus officiel est vert : **471 cas, 470 PASS / 0 FAIL / 1 SKIP**,
   `build_verified: true`, périmètre build **202/202**, aucune régression.
2. Le workspace est vert : **99 groupes, 1 587 passés / 0 échec / 4 ignorés**
   (les 4 ignorés sont les campagnes longues explicitement lancées à part).
3. Un **oracle différentiel** neuf a mesuré **3 600 programmes** bien typés :
   `check` accepte ⇒ `rustc` compile, **0 divergence** ; la configuration
   nominale (3 × 10 000) est `NOT_RUN` localement (≈ 3 h) mais outillée en CI.
4. La boucle `.rv` est **mesurée** : `check` p95 à froid **1,3-8,1 ms**,
   après édition **11,0-49,8 ms** ; `build` p50 **179-331 ms** dont `rustc`
   **97,1-99,3 %**.
5. `-C incremental` a un gain **nul ou négatif** : implémenté en opt-in,
   **désactivé par défaut**, chiffres publiés.
6. Le LSP **ne re-parse plus** un fichier inchangé : p95 d'analyse après édition
   **1,362 ms** avec cache contre **1,836 ms** sans (hors transport).
7. Les `NodeId` sont **déjà stables par fichier** (2 tests) : la migration
   envisagée par l'audit est **sans objet** ; la règle « aucun cache indexé par
   `FileId` » est écrite.
8. La gouvernance périmée est corrigée (`STATUS.md`, `START_HERE.md`,
   `AGENTS.md` en `PENDING_HUMAN_APPROVAL`, statut du langage en un seul état).
9. **Non fait** : phases 4.3-4.5 (`OwnFacts`), 5 (traps `rt`), 8 (bloc DSP) ;
   Miri, mutation et campagne longues complètes : `NOT_RUN` avec raisons.
10. **Rien n'est prouvé** au sens formel. T4 (audit externe) et T12 (Go/No-Go)
    restent `BLOCKED` par construction ; D9 (clé) reste différée.

## 2. Tableau par phase

| Phase | Statut | Commits | Tests ajoutés (positifs / négatifs) | Commande de preuve et résultat brut |
|---|---|---|---|---|
| **0** Baseline | `TESTED` | `c931fdf`, `e82ed14`, `ded0e60` | — | `cargo test --workspace --all-targets -- --test-threads=2` : 94 groupes, 1574 / 0 / 2 ; conformance 471 / 470 / 0 / 1 ; smoke 43/0/0 ; docs PASS ; registre 108/126 ; 188/188 ; fmt 0. Détail : `docs/reports/GOAL-BASELINE.md` |
| **1** Gouvernance et base de confiance | `TESTED` (+1 `PENDING_HUMAN_APPROVAL`) | `41851db`, `fac8a8c`, `c04f33e`, `dddfd32`, `b129024`, `85e993e`, `0ea05b7` | +2 gardes codegen (positifs) avec preuve de rouge ; +1 garde HashMap (positif, rouge prouvé) ; +5 tests d'hygiène racine | docs PASS ; gardes prouvées rouges puis vertes ; `C`+U+F03A supprimé après preuve (0 fichier, 6 répertoires vides) |
| **2** Oracle `check ⇒ build` | `PARTIAL` (mode long nominal `NOT_RUN`) | `2539d5c`, `272ece3`, `825aea1` | +2 tests de non-vacuité (rustc rejette un fichier invalide ; deux graines distinctes) ; générateur 34 gabarits | `cargo test -p ravum-lang-codegen --test differential_oracle` : 600 programmes, **600 acceptés, 0 divergence** ; long local 3 × 1 000 = **3 000, 0 divergence** ; nominal 3 × 10 000 `NOT_RUN`. Détail : `T2-FINDINGS.md` §25 |
| **3** Mesure et incrémental | `TESTED` | `4262569`, `84eaa08`, `3b6eb75`, `d186e3d`, `1265de2` | +3 contrats `--timings` (JSON additif, absence du champ sans flag, phases build) | `scripts/lang-timings.ps1 -Runs 20` : chiffres §3 ; `repro-check.ps1` PASS ; tests contrat 3/3 |
| **4** `OwnFacts` | `PARTIAL` (4.1-4.2), `NOT_RUN` (4.3-4.5) | `e91ecc3` (RFC) | — | Inventaire des décisions d'`emit.rs` (journal) ; RFC-OWN-FACTS-1 ; aucune modification d'`emit.rs` |
| **5** Traps `rt` | `NOT_RUN` | `43fbd68` (RFC) | — | RFC-RT-TRAPS-1 `PROPOSED`/`PENDING_HUMAN_APPROVAL` ; aucun code, aucun cas |
| **6** `NodeId` | `TESTED` (sans objet) | `0595b9f`, `7916840`, `91d3621` | +2 tests (stabilité inter-fichiers, layout 8/24 bits) | `cargo test -p ravum-lang-sema --test node_id_stability` : 2/2 ; `util.rv` inchangé après édition de `main.rv` |
| **7** Cache de parse LSP | `TESTED` (analyse) ; bout en bout `NOT_RUN` | `a0a7c26`, `2d432be`, `5860ef3`, `6257b84` | +2 tests (réutilisation exacte hits/misses ; INV-07) +1 mesure `#[ignore]` | `cargo test -p ravum-lang-lsp --test parse_cache` : 2/2 ; mesure p50/p95 §3 ; `docs/qualification/LSP.md` §7 |
| **8** Bloc DSP | `NOT_RUN` | `bb3edea` (RFC) | — | ABI lue (8.1) ; RFC-RV-DSP-BLOCK-1 (8.2) ; aucun wrapper, aucun test de swap |
| **9** Qualification | `PARTIAL` | ce rapport et les sections datées des registres | — | Voir §4 ; couverture 89,21/88,61 (seuils tenus), fuzz long rejoué, mutation/Miri `NOT_RUN`, PR brouillon |

### 2.8 SHA final mesuré

Le SHA exact du rapport est celui du commit qui l'ajoute ; toutes les commandes
de la section 4 ont été exécutées sur l'arbre `faf65cb` + ce rapport (aucun
changement de code entre les deux). Les mesures de phase 3 citent leur propre
SHA (`3b6eb75`) parce qu'elles ont été produites avant les phases 6-7.

## 3. Comparaison baseline (phase 0) → état final

Aucune régression : chaque chiffre est identique ou meilleur. Les éventuelles
dégradations auraient été placées ici en tête.

| Mesure | Phase 0 (`c931fdf`) | Final (`faf65cb`+) | Écart |
|---|---|---|---|
| Conformance | 471 cas, 470 PASS / 0 FAIL / 1 SKIP | **identique** | 0 |
| Périmètre build | 202/202, vérifiés 121, implicites 81, échecs 0 | **identique** | 0 |
| Workspace | 94 groupes, 1 574 / 0 / 2 | **99 groupes, 1 587 / 0 / 4** | +5 groupes, +13 tests, +2 ignorés (campagnes longues) |
| `ravum-lang-sema` | 683 / 0 | 685 / 0 (2 tests NodeId) | +2 |
| Registre | 108 / 126 / 0 / 0 | **identique** | 0 |
| Traçabilité FeatureId | 188/188 (1 exemption) | **identique** | 0 |
| MUST | 807, 463 atteignables | **identique** | 0 |
| Couverture sema+codegen | 89,08 / 88,39 (docs, non re-mesurée en phase 0) | **89,21 / 88,61**, seuils tenus | +0,13 / +0,22 pt |
| Fichiers sous l'objectif 100 % | 10 (2 098 lignes, vue instanciations) | **10 (2 103 lignes)** | +5 lignes (code de cache LSP, mesuré hors tests LSP) |
| Smoke exemples | 43 / 0 / 0 | **identique** | 0 |
| Fuzz syntaxe | non rejoué en phase 0 | **249 054 entrées, 0 échec**, digest `0x9b09fc61e761b87e` | nouvelle preuve |
| Fuzz sema | non rejoué en phase 0 | **100 000 itérations, 0 échec**, 4 660/s | nouvelle preuve |
| LSP client | non rejoué en phase 0 | **11 / 0** | nouvelle preuve |
| Reproductibilité release | non mesurée en phase 0 | **PASS**, deux racines identiques, sha256 `19743DC29F3937E09E3765A485FFAB545B412D4DF22E2B8FBB2A2CD0BECD10B6` | nouvelle preuve |
| Installateur Windows | `scripts/install.ps1 -Prefix <tmp> -SkipBuild` puis `-Uninstall` deux fois | install exit 0 (manifeste sha256 `3c52d353…`, commit `faf65cb`), désinstallation idempotente **0 / 0** | nouvelle preuve |
| SBOM | `SOURCE_DATE_EPOCH=0 pwsh -File scripts/sbom.ps1` | SPDX-2.3, **64 paquets / 170 relations**, sha256 `5119e194…`, déterministe | nouvelle preuve |
| Oracle différentiel | inexistant | **3 600 programmes, 0 divergence** | nouvelle preuve |

## 4. Critères PRODUCTION-READINESS et gates T0–T12

États au SHA final. Les commandes sont dans les sections précédentes ou dans le
journal ; un `PARTIAL` n'est jamais arrondi en `PASS`.

### 4.1 Critères R1-R14

| # | Critère | État | Preuve / manque |
|---|---|---|---|
| R1 | Sémantique ratifiée | `PARTIAL` | ADR `PROPOSED`, spec DRAFT ; aucune ratification par un agent |
| R2 | Aucune divergence connue | **fait sur le périmètre mesuré** | 471 cas 0 FAIL + 3 600 programmes générés 0 divergence ; ne prouve pas l'absence de forme inconnue |
| R3 | Traçabilité complète | `PARTIAL` | 188/188 FeatureIds ; **463/807 MUST**, 344 non mappés (`must_audit.py`) — D10 |
| R4 | Sûreté du frontend | `BLOCKED` | audit externe absent (T4) ; Miri `NOT_RUN` (nightly non garanti hors CI) |
| R5 | Frontière FFI sûre | `PARTIAL` | inventaire et refus outillés ; contrats ABI/lifetime/unwinding non écrits ; D7 ouvert |
| R6 | Code généré sûr | **fait** | `forbid(unsafe_code)` sans `extern` ; `deny` + `allow` local au wrapper ; 2 tests de garde, rouges prouvés |
| R7 | Reproductibilité inter-machines | `PARTIAL` | deux racines identiques sur **cette** machine ; pas de deuxième machine |
| R8 | Qualité mesurable | `PARTIAL` | couverture 89,21/88,61, seuils tenus ; objectif 100 % non atteint ; **mutation `NOT_RUN`** (cargo-mutants absent, aucun ajout de dépendance sans RFC) |
| R9 | Performance sous budget | `PARTIAL` | p50/p95 publiés (`LANG-TIMINGS.md`) ; pas de campagne sous charge ni de seuil de non-régression CI |
| R10 | Interfaces stables | `PARTIAL` | JSON/CLI/diagnostic testés ; politique de dépréciation toujours absente |
| R11 | Distribution | `PARTIAL` | installateur Windows rejoué (install + 2 désinstallations idempotentes) ; installateur Linux hors contrat ; `wasm`/macOS non couverts |
| R12 | Artefacts signés | `différé` | mécanisme qualifié, **aucune clé** (D9) |
| R13 | Audit externe | `BLOCKED` | quatre revues LLM triées ; audit humain indépendant requis |
| R14 | Documentation sans fausse affirmation | `PARTIAL` | gouvernance corrigée ; guide utilisateur et notes de version absents |

### 4.2 Gates T0-T12

| Gate | État | Ce qui a changé dans cette session |
|---|---|---|
| T0 | `PASS` | inchangé (aucune modification du socle) |
| T1 | `PARTIAL` | inchangé (voie C toujours gelée, décision humaine) |
| T2 | `PARTIAL` | **preuve renforcée** (oracle 3 600 programmes, 0 divergence) ; le verdict reste `PARTIAL` pour D10 (344 MUST non mappés) |
| T3 | `PASS` | fuzz long rejoué (249 054 entrées, 0 échec) |
| T4 | `BLOCKED` | inchangé : audit humain absent. Aucun agent ne peut le lever |
| T5 | `PASS` | inchangé (smoke 43/0/0, binaires natifs) |
| T6 | `PARTIAL` | inchangé (FFI scalaire expérimentale) |
| T7 | `PASS` | LSP : cache de parse et latence mesurée ; client LSP 11/11 |
| T8 | `PARTIAL` | redémarrage, pas de préservation d'état ; le bloc DSP `.rv` reste `NOT_RUN` (phase 8) |
| T9 | `PARTIAL` | inchangé (10 verticales, pas de DSP hôte `.rv`) |
| T10 | `PARTIAL` | couverture/fuzz/LSP mesurés ; Miri, mutation et 30 000 programmes : `NOT_RUN` ; audit externe absent |
| T11 | `PARTIAL` | reproductibilité release rejouée (`19743DC2…` sur deux racines) ; SBOM déterministe (64/170) ; installateur rejoué ; aucune clé (D9) |
| T12 | `BLOCKED` | Go/No-Go humain |

## 5. Éléments `PENDING_HUMAN_APPROVAL`

1. `AGENTS.md` — révision du mandat (fait périmé corrigé) : commit `fac8a8c`.
2. `RFC-OWN-FACTS-1` — extension de `Facts` (interface gelée) et étapes B/C.
3. `RFC-RT-TRAPS-1` — règle de langage `rt` (traps, formes explicites) ; deux
   points ouverts : décalages/négation, sort de `pure`.
4. `RFC-NODE-ID-1` — règle « aucun cache indexé par `FileId` ».
5. `RFC-RV-DSP-BLOCK-1` — ABI de bloc généré, `unsafe` confiné, digest de schéma.
6. FFI expérimentale (D7) — contrats ABI/lifetime/unwinding toujours non ratifiés
   (préexistant à cette session).

## 6. `BLOCKED` et `NOT_RUN` (raison et tentatives)

| Élément | Statut | Raison | Tentatives |
|---|---|---|---|
| Audit externe T4/T12 | `BLOCKED` | décision humaine | — (par construction) |
| Clé de signature D9 | `PENDING_HUMAN_APPROVAL` | aucune clé hors ligne fournie | — |
| Phase 4.3-4.5 (`OwnFacts` A/B/C) | `NOT_RUN` | refonte de `own.rs` + suppression du `MovePlanner`, trop risquée sans le filet d'une session dédiée ; aucun demi-changement laissé | 1 tentative d'analyse (inventaire 4.1) |
| Phase 5 (traps `rt`) | `NOT_RUN` | règle de langage à ratifier avant implémentation | RFC écrite |
| Phase 8 (bloc DSP) | `NOT_RUN` | ABI + protocole + codegen, hors budget de session | RFC écrite, ABI lue |
| Mode long nominal de l'oracle (30 000) | `NOT_RUN` | ≈ 3 h sur cette machine ; job CI hebdomadaire fourni | 3 000 exécutés |
| Miri | `NOT_RUN` | toolchain nightly non garantie sur cette machine ; preuve CI Linux existante non rejouée | — |
| Mutation | `NOT_RUN` | `cargo-mutants` absent ; l'installer serait une nouvelle dépendance (R5) | — |
| Couverture à 100 % | `NOT_RUN` | 2 103 lignes non couvertes (vue instanciations) ; aucun test « pour la ligne » ajouté | mesure + gap |
| Astreinte LSP de bout en bout | `NOT_RUN` | mesure en processus seulement (transport exclu) | latence d'analyse mesurée |
| Découpage des monolithes (9.7) | `NOT_RUN` | conditionné à « tout le reste terminé » | — |

## 7. Décisions humaines restantes

1. **T4** : mandater un auditeur humain indépendant (borrow checker, codegen,
   prélude). Dossier prêt : `docs/audit/AUDIT-BRIEF.md`.
2. **T12** : Go/No-Go humain.
3. **D9** : custody de la clé de signature et publication d'`allowed_signers`.
4. **Ratifier ou refuser** les quatre RFC de cette session (ordre de priorité :
   `RFC-OWN-FACTS-1`, `RFC-RV-DSP-BLOCK-1`, `RFC-RT-TRAPS-1`, `RFC-NODE-ID-1`).
5. **D10** : dénominateur MUST (807 vs 463 atteignables).
6. **D7** : contrats de la frontière FFI.
7. **`AGENTS.md`** : approuver la révision du mandat.

## 8. Ce qui n'a pas été exécuté (`NOT_RUN`) — liste consolidée

Voir le tableau §6. En plus : aucun test sous charge concurrente, aucune mesure
mémoire/CPU, aucune deuxième machine, aucun macOS/ARM, aucun WASM, aucun
`ravum-lang install` Linux, aucun dogfood indépendant.

## 9. Limites

- **Une seule machine** (Windows 11 x64) : les chiffres de performance et de
  reproductibilité sont liés à cette machine et à `rustc 1.95.0`.
- **Corpus fini** : 471 cas + 3 600 programmes générés sont une observation,
  pas une preuve sur tous les programmes. L'oracle lui-même ne couvre qu'un
  sous-ensemble documenté de Core 1.0.
- **Aucune preuve formelle** : pas de Kani, pas de Miri cette session, pas
  d'audit externe. La sûreté mémoire repose sur `rustc` sous `forbid(unsafe_code)`
  (voie B), pas sur une preuve Ravum.
- **Un audit mené par un agent reste un audit interne** : T4 et T12 ne peuvent
  pas être levés par ce rapport.
