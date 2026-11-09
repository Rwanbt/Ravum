# MVP-0c — Vérification sémantique : reclassement des échecs transitoires (L4.6)

Statut : **IMPLÉMENTÉ / TESTÉ** (correctif approuvé) — pas `PROVED`, pas une
release, pas une acceptation d'ADR.
Branche : `feat/mvp0c-hardening`. Base de l'investigation amont : section 9 de
[`MVP-0c-build-performance.md`](MVP-0c-build-performance.md) et preuve
[`../evidence/mvp0c-verification-transient-2026-11-09.json`](../../evidence/mvp0c-verification-transient-2026-11-09.json).
Preuve de ce correctif :
[`../evidence/mvp0c-verification-retry-2026-11-09.json`](../../evidence/mvp0c-verification-retry-2026-11-09.json).

## 1. Problème corrigé

L'investigation L4.5 a établi une lacune de classification dans le vérificateur
`rustc` du coordinateur : une erreur de **spawn** devient `Unavailable`
(`MANUAL_REQUIRED`, correct), mais un processus `rustc` qui sort **non-zéro**
devenait toujours `Failed` → **`REJECT` définitif**, même lorsque les diagnostics
ne contenaient **aucun code d'erreur** (`E####`) — cas réel observé sous charge :
« only metadata stub found for `rlib` dependency `std` … »,
« cannot resolve a prelude import ». Un incident transitoire d'infrastructure
devenait donc une décision définitive sur la validité d'un patch.

## 2. Correctif appliqué

### 2.1 Règles de classification (fail-closed, inchangé sur le fond)

| Cas observé (metadata_check) | Classification | Décision |
|---|---|---|
| `rustc` réussit (exit 0, aucun diagnostic d'erreur) | `Passed` | `AUTO_SAFE` si les autres préconditions tiennent (`SEMANTICALLY_CHECKED`) |
| échec **avec au moins un code d'erreur** (`E####`) | `Failed` (définitif) | `REJECT` (conservé) |
| échec **sans aucun code d'erreur** | tentative 2 (retry borné, état neuf) | — |
| retry réussi | `Passed` + résumé mentionnant le retry | `SEMANTICALLY_CHECKED` comme avant |
| retry en échec **avec code d'erreur** | `Failed` (définitif) | `REJECT` |
| échec persistant **sans code** (2 tentatives) | `Unavailable` | `MANUAL_REQUIRED` — **jamais `REJECT`** |
| erreur de spawn / timeout d'exécution des tests | `Unavailable` (inchangé) | `MANUAL_REQUIRED` |
| aucune vérification réussie | — | **aucune intégration** (fail-closed) |

### 2.2 Détails d'implémentation

Fichier `crates/ravum-workspace-coordinator/src/bin/common/mod.rs` :

- `RustcMergeVerifier::new` : seam de test `RAVUM_RUSTC`, appliqué **uniquement**
  quand l'appelant n'a pas résolu de compilateur spécifique (valeur par défaut
  `rustc`) ; un `--rustc`/`RUSTC` explicite gagne toujours. Sans la variable, le
  comportement est inchangé.
- `scratch(snapshot, attempt)` / `materialize(..., attempt)` : la tentative 2
  utilise un répertoire d'état **distinct** (`<id>-retry1`) et rematérialise le
  snapshot — le retry démarre sur un état neuf (aucun artefact périmé réutilisé).
- `Probe` porte désormais `exit_code` et `stderr_excerpt` (borné à
  **2 Kio**, une ligne, troncature explicite).
- `metadata_attempt(...)` construit le `MergeCheck` et son résumé :
  - réussite au retry : résumé avec « succeeded on bounded retry 1 of 2 … first
    exit code …, first stderr excerpt: … » (journalisé via la raison de décision) ;
  - échec avec code : résumé avec nombre d'erreurs et exit code ;
  - échec sans code : résumé « no compiler error code after attempt N/2 … ».
- `verify(...)` : classification ci-dessus, bornée à **1 retry**
  (`MAX_METADATA_ATTEMPTS = 2` tentatives au total), puis raisons journalisées :
  - `definitive_compile_reason(...)` : « merged snapshot … does not compile with
    rustc 1.95.0 (exit code …; stderr excerpt: …) » ;
  - `transient_unavailable_reason(...)` : « … produced no compiler error code …
    after 2 attempt(s) with fresh state … (attempt 1: exit code …, stderr
    excerpt: … | attempt 2: …); no integration without a successful
    verification ».
- `bounded_stderr_excerpt(...)` : ≤ 2 Kio, caractères de contrôle aplatis,
  troncature marquée.

Le schéma de `MergeCheck` (crate `records.rs`) n'est **pas** modifié (hors
périmètre de ce correctif) : l'exit code et l'extrait stderr sont conservés dans
le **résumé du check** et dans la **raison de vérification**, tous deux
journalisés durablement. Aucun secret n'est introduit : l'extrait ne contient que
le stderr de `rustc` (chemins/toolchain), borné à 2 Kio.

Le mapping du coordinateur (`coordinator.rs`) n'a pas eu besoin d'être modifié :
`Failed → REJECT` et `Unavailable → MANUAL_REQUIRED` étaient déjà corrects ; le
correctif ne change que **ce que le vérificateur renvoie**.

## 3. Tests déterministes (faux rustc piloté par `RAVUM_RUSTC`)

Nouveau fichier `crates/ravum-workspace-coordinator/tests/transient_verification.rs`
(3 tests, aucun test existant modifié). Le faux `rustc` est un petit programme
Rust compilé au setup du test (déterministe sous Windows, aucun réseau, aucun
second toolchain) :

- délègue `-vV` au vrai `rustc` (identité de toolchain réelle) ;
- mode `transient-once` : échoue le **1er** appel de compilation avec les deux
  diagnostics JSON sans code de l'incident (schéma plat `$message_type`) puis
  délègue le retry ;
- mode `ecode` : échoue toujours avec un diagnostic `E0061` (code d'erreur réel) ;
- mode `permanent-transient` : échoue toujours avec les diagnostics sans code ;
- compte les appels de compilation (fichier compteur) et journalise les arguments
  (fichier log) pour prouver le nombre exact de tentatives.

Chaque test exécute le **vrai binaire** `ravum-collab integrate --verify --json`
avec `RAVUM_RUSTC=<faux rustc>`, puis vérifie décisions, niveaux d'évaluation,
`accepted_changes`, raisons du journal durable et nombre d'appels compilateur.
Le détail brut de chaque scénario est écrit dans
`target/ravum-tests/transient-verification/<scénario>/report.json` et repris dans
la preuve JSON.

### 3.1 Résultats observés (exécution réelle)

| Scénario | Décisions | Niveaux | Appels `rustc` | `accepted_changes` | Journal |
|---|---|---|---|---|---|
| (a) `transient-once` | `AUTO_SAFE`, `AUTO_SAFE` | `SEMANTICALLY_CHECKED` ×2 | **3** (1 échec + 1 retry + 1 normale) | 2, vérification `passed` | retry mentionné + extrait « only metadata stub found » |
| (b) `ecode` | `REJECT`, `REJECT` | `REJECTED` ×2 | **2** (aucun retry) | 0 | `E0061`, `exit code 101`, `stderr excerpt` |
| (c) `permanent-transient` | `MANUAL_REQUIRED`, `MANUAL_REQUIRED` | `TEXTUALLY_APPLICABLE` ×2 | **4** (2 propositions × 2 tentatives) | 0 | « no compiler error code », « 2 attempt(s) », exit codes 101, extraits des 2 tentatives |

Aucun `AUTO_SAFE` dans les scénarios (b) et (c) ; aucune intégration sans
vérification réussie. Durées des scénarios : 508 ms / 304 ms / 282 ms (incluent
la compilation réelle du faux rustc et les vérifications).

Vérifications explicites codées dans les tests :

- (a) jamais `REJECT` ; niveau `SEMANTICALLY_CHECKED` (pas `PROVED`/`TESTED`) ;
  exactement 1 retry (compteur = 3) ; extrait de la 1re tentative présent dans le
  journal ;
- (b) `REJECT` conservé ; pas de retry (compteur = 2) ; code `E0061` et exit code
  journalisés ; zéro changement accepté ;
- (c) `MANUAL_REQUIRED`, jamais `REJECT` ni `AUTO_SAFE` ; compteur = 4 ; deux
  tentatives avec état neuf ; exit codes et extraits bornés journalisés ;
  longueur des raisons journalisées bornée.

### 3.2 Suite existante (non-régression)

```text
cargo test -p ravum-workspace-coordinator -- --test-threads=1
  lib (76), collab1 (2), collab2 (3), collab3 (10), collab5 (5),
  git_publication (16), git_publication_crash (15 — fichier untracked d'un autre
  worker, exécuté aussi), git_writer (2), journal_durability (3),
  merge_strategies (2), transient_verification (3)
  TOTAL : 137 passed / 0 failed / 0 ignored
```

Interprétation : **119 tests existants** du périmètre committé + **3 nouveaux**
= 122 verts ; les 15 tests du fichier `git_publication_crash.rs` (non suivi par
Git, autre worker) étaient déjà présents et passent aussi — total observé 137/0.
Aucun test supprimé, ignoré ou modifié ; `0 warning` dans le log de build/tests.

## 4. Fail-closed préservé

- Aucune intégration sans vérification **réussie** (`Passed`), dans les trois
  scénarios : (b) et (c) n'acceptent aucun changement.
- Un vrai échec de compilation garde son `E-code` et reste `REJECT`.
- Un échec persistant d'infrastructure devient `MANUAL_REQUIRED` : la décision
  est terminale (rien n'est intégré) mais elle **n'affirme pas** que le patch est
  invalide.
- Aucun statut `PROVED` n'est produit ; le retry réussi reste
  `SEMANTICALLY_CHECKED` (niveau inchangé).

## 5. Limites et non-claims

1. **Le faux rustc n'est pas l'incident OS réel** : il reproduit le *motif*
   (exit non-zéro, diagnostics JSON sans E-code, schéma plat) de façon
   déterministe, mais pas la cause physique (pression I/O/mémoire sur le
   sysroot). Le correctif traite la *classification* de ces échecs.
2. **Retry unique borné** : un incident persistant plus long que 2 tentatives
   reste `MANUAL_REQUIRED` (jamais `REJECT`) ; aucun backoff, aucune file
   d'attente.
3. **Risque de reclassement** : un échec de compilation sans aucun E-code
   (diagnostic exotique) serait reclassé `MANUAL_REQUIRED` au lieu de `REJECT`.
   C'est un arbitrage assumé : l'incident L4.5 montre que l'inverse (traiter un
   incident transitoire comme définitif) est plus dangereux ; rien n'est jamais
   intégré sans succès, et un `E-code` reste décisif.
4. **Champ d'évidence** : `MergeCheck` n'a pas été étendu (schéma hors
   périmètre) ; exit code et extrait stderr vivent dans le résumé du check et la
   raison de vérification, tous deux journalisés. Une extension de schéma dédiée
   reste souhaitable pour une lecture structurée.
5. **Pas de timeout ajouté** : le cas « timeout » mentionné dans la consigne est
   celui de l'exécution des tests (déjà `Unavailable` inchangé) ; la compilation
   metadata reste bloquante sans budget propre (limite connue du vérificateur).
6. Les tests lancent de vrais processus (`ravum-collab`, compilation du faux
   rustc) : ils doivent être exécutés avec `--test-threads=1` sur la machine
   partagée ; ils ne sont pas des tests unitaires purs.
7. Aucun résultat de ce document n'est une preuve de correction générale du
   coordinateur, ni une acceptation d'ADR ; aucun `PROVED`.

## 6. Reproduire

```powershell
# build + tests complets (sérialisé sur la machine partagée)
cargo build -p ravum-workspace-coordinator --bins
cargo test -p ravum-workspace-coordinator -- --test-threads=1

# seulement les 3 scénarios du correctif
cargo test -p ravum-workspace-coordinator --test transient_verification -- --test-threads=1
```

Preuve : [`../evidence/mvp0c-verification-retry-2026-11-09.json`](../../evidence/mvp0c-verification-retry-2026-11-09.json)
(rapports bruts des 3 scénarios, résultats de la suite complète, provenance Git).

Fichiers du correctif :

- `crates/ravum-workspace-coordinator/src/bin/common/mod.rs` (classification,
  retry, exit code + extrait stderr, seam `RAVUM_RUSTC`) ;
- `crates/ravum-workspace-coordinator/tests/transient_verification.rs` (nouveau,
  3 tests déterministes) ;
- `docs/mvp/MVP-0c-verification.md` (ce document) ;
- `evidence/mvp0c-verification-retry-2026-11-09.json` (preuve d'exécution) ;
- `crates/ravum-workspace-coordinator/src/coordinator.rs` et `verifier.rs` :
  **non modifiés** — le mapping existant (`Failed → REJECT`,
  `Unavailable → MANUAL_REQUIRED`) était déjà correct.
