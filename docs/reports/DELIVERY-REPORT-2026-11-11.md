# Ravum — rapport de livraison P0 / P1 / P2 (2026-11-11)

> **Nature du document.** Rapport de synthèse d'une passe d'**implémentation**
> (et non d'une nouvelle revue). Chaque chiffre est lié à une commande, un
> environnement et un état de l'arbre. Les statuts `NOT_RUN`, `SKIP`,
> `PENDING_HUMAN_APPROVAL` restent ce qu'ils sont : ils ne sont jamais convertis
> en `PASS`. Aucune revendication de production, aucune ratification d'ADR,
> aucune release.

| Élément | Valeur |
|---|---|
| Branche | `feat/lang-core-1.0-rc` |
| Base de la passe | `5d88ecbdb1839287aecd13fd6ba914b911b75f5b` |
| Commits de la passe | `8c6434b` (sema/codegen), `5daeb40` (contrat de conformance), `e0af6b6` (CLI), `9314d2f` (build), `9c163de` (P2), `a3a43b2` (P1), puis le commit de documentation qui porte ce fichier |
| Plateforme de mesure | Windows 11 x86-64, 16 threads logiques, ≈15,7 Go RAM, `rustc`/`cargo` 1.95.0, Node 26.13.0, Python 3.13.7 |
| Binaires épinglés | `ravum-lang.exe` `5816ACDD…C157F661D` ; `ravum-conformance.exe` `7DF6345C…4BE4CCCF` |
| PR | [#101](https://github.com/Rwanbt/Ravum/pull/101), toujours **draft**, base `dev`, non fusionnée |
| CI | **NOT_RUN sur les commits de cette passe** : aucun push n'a été effectué. Voir §H. |

---

## A — Executive Summary

**État réel.** Ravum Core 1.0 reste un sous-ensemble **DRAFT**, sans ADR ratifié,
sans release et sans audit externe. Cette passe ne change pas cette nature : elle
supprime des écarts de fiabilité mesurés et rend plusieurs contrats
**vérifiables par machine**.

**Progression depuis la baseline `5d88ecb`.**

| Indicateur | Baseline | Après la passe |
|---|---|---|
| Tests workspace | 89 suites / 1405 passés / 0 échec / 2 ignorés | **94 suites / 1525 passés / 0 échec / 2 ignorés** |
| Conformance | 386 cas / 385 PASS / 0 FAIL / 1 SKIP | **424 cas / 423 PASS / 0 FAIL / 1 SKIP** |
| Contrat `check → build` | non mesuré (79 cas acceptés ne passaient jamais par `rustc`) | **bloquant** : périmètre 163, couverts 163, vérifiés 101, implicites 62, **0 échec**, `build_verified: true` |
| Divergences `check`-pass/build-fail | 2 dans le corpus, 11 par sondage indépendant | **0 dans le périmètre** ; 3 formes ouvertes documentées, hors corpus |
| Smoke exemples | 31 PASS | **43 PASS / 0 FAIL** (dont 13 assertions `file-store`) |
| Traçabilité | 188/188 FeatureIds | 188/188 FeatureIds **et** **463/807 MUST** (nouvelle métrique) |
| Gates | 5 PASS / 6 PARTIAL / 2 BLOCKED | **4 PASS / 7 PARTIAL / 2 BLOCKED** (T2 rétrogradé par mesure) |

**Les quatre acquis principaux.** (1) Un programme accepté par `check` ne peut
plus échouer silencieusement à la compilation sur le périmètre mesuré.
(2) Les builds concurrents ont une identité déterministe, des sorties isolées et
une publication atomique ; un résultat obsolète ne peut plus remplacer un
artefact valide. (3) `watch` ne coupe plus la version active avant d'avoir une
nouvelle version valide. (4) Les codes de sortie et le runner de tests ne peuvent
plus transformer un échec en succès.

**Ce que cette passe ne prouve pas.** Elle ne prouve pas l'absence de bugs, ni la
sûreté générale du frontend, ni une conformité complète à la spécification. Elle
ne prouve pas non plus les performances au repos (mesures faites sous charge) et
n'a pas rejoué fuzz, Miri, couverture ou mutation sur les nouveaux commits.

---

## B — P0 : fiabilisation du compilateur

### B.1 Méthode

Les 37 cas de la revue externe (Claude) **ne sont pas dans le dépôt** : ils ne
sont ni confirmés ni infirmés en tant que tels. La passe a donc (a) balayé tout le
corpus accepté avec `check` puis `build`, et (b) reconstruit des contre-exemples
par famille signalée. Résultat : **11 divergences réelles** au lieu de 2.

### B.2 Anomalies, reproductions, correctifs, validations

| # | P0 | Défaut reproduit | Correctif | Validation |
|---|---|---|---|---|
| 1 | A | `struct S { text: &str }` et `fn lit() -> &str` : `check` 0, `rustc` E0106 | `&'static str` en position valeur ; `&str` élidé en paramètre (OWN-6.2, STD-9.3) | `backend_audit` compile et exécute ; cas `ownership/static-str-stored` |
| 2 | A | `a[1]` et `a[1].0` sur `[String; 2]` : E0508 | déstructuration hoistée + réécriture par node id (OWN-5.4) | 6 cas `compile-pass` + tests rustc réels |
| 3 | A | deux moves du même tableau, move après lecture, move en argument builtin | planificateur réécrivant tous les accès constants | 5 cas de conformance, contrôles `Copy` conservés |
| 4 | A | `fn bump(mut x: i32)` : E0384 | `mut x: T` émis | `types/param-mut-binding-pass` |
| 5 | A | `unwrap` puis move/lecture : E0382 | `unwrap|unwrap_or|ok_or|ok|err` consommateurs (STD-11.3) | `E-OWN-DOUBLE-MOVE` / `E-OWN-USE-AFTER-MOVE`, 3 cas + 39 contrats |
| 6 | A | `let a: [i32; 2] = [1, true]` accepté | vérification élément par élément (TYP-5.1/5.7) | `E-TYP-MISMATCH` |
| 7 | F | exhaustivité non récursive (`Option<(i32,bool)>`, `Option<Option<_>>`, enum à payload) : E0004 | moteur de couverture récursif (G-6.22/TYP-10.4) | `E-TYP-NONEXHAUSTIVE`, 5 cas positifs/négatifs |
| 8 | A | `String` possédé vers `&str` : E0308 | coercition retirée (TYP-8.5/13.1) ; `stats` migré | `E-TYP-MISMATCH`, 4 cas + contrôles `&String`/`as_str` |
| 9 | — | références stockées dans champ/enum/payload prélude ; `&mut self` sur place immuable | `E-OWN-REF-ESCAPE`, `E-OWN-IMMUTABLE` (OWN-6.1/6.2, TYP-3.2) | contrats positifs/négatifs |
| 10 | B | build partagé : deux builds se collisionnent, un build incomplet peut écraser un artefact valide | identité sha256, isolation `.ravum/build/<id16>/`, publication atomique, manifeste, garde d'obsolescence | 7 tests `build_isolation` |
| 11 | C | `watch` tue l'enfant avant de compiler | build d'abord, activation conditionnelle au snapshot, copie privée, repli, aucun orphelin | `watch-fail-closed-check.ps1` (PASS) |
| 12 | D | `clamp(0,255)` sur un code d'exception Windows négatif → succès | mapping pur, `0x%08X`, jamais 0 pour un échec | 8 tests unitaires + 6 E2E |
| 13 | E | `test` peut présenter une exécution partielle comme complète | validation compteurs/summary/exit/verdicts, `--filter` réel | 22 tests unitaires + 5 E2E |

### B.3 Divergences restantes (déclarées, hors corpus)

Trois formes échouent encore à `build` après un `check` vert, avec un **E0508
bruyant** (aucune miscompilation silencieuse) : chaîne d'index imbriquée
(`a[0][1]`), tableau derrière un champ/tuple (`h.items[0]`, `t.0[0]`), liaison de
tableau masquée par un shadowing imbriqué. OWN-5.3/5.4 autorisent ces moves ;
les fermer demande un abaissement général des moves par place. Disposition :
**D12**, documentée dans [`CONFORMANCE-BUILD-CONTRACT.md`](../qualification/CONFORMANCE-BUILD-CONTRACT.md).

**Gate de sortie P0** : les défauts reproduits ont un correctif ou une
disposition explicite ; le corpus différentiel est bloquant en CI ; les builds
concurrents sont isolés ; les résultats obsolètes ne sont pas publiés ; la
continuité de `watch` est testée ; les codes de sortie et le runner de tests ne
masquent plus d'échec. **P0 est atteint sur le périmètre mesuré**, avec les trois
formes ouvertes ci-dessus explicitement hors périmètre.

---

## C — P1 : stabilisation et décisions préparées

| Livrable | Fichier | État |
|---|---|---|
| D2 — contrat du backend de référence | [`RFC-D2-REFERENCE-BACKEND-CONTRACT.md`](../rfc/RFC-D2-REFERENCE-BACKEND-CONTRACT.md) | `PROPOSED` / `PENDING_HUMAN_APPROVAL` |
| D-COERCE-1 — matrice et texte normatif | [`RFC-D-COERCE-1-MATRIX.md`](../rfc/RFC-D-COERCE-1-MATRIX.md) | `PROPOSED` ; **l'implémentation est déjà conforme** (TYP-8.5/13.1) |
| D7 — FFI et effets | [`RFC-D7-FFI-EFFECTS.md`](../rfc/RFC-D7-FFI-EFFECTS.md) | `PROPOSED` ; FFI scalaire→scalaire seulement |
| Traçabilité MUST | [`MUST-TRACEABILITY.md`](../qualification/MUST-TRACEABILITY.md) + `tools/must_audit.py` | **mesuré** : 807 MUST, 463 atteints, 344 non |
| F-META-006 | [`F-META-006-EXEMPTION-DOSSIER.md`](../qualification/F-META-006-EXEMPTION-DOSSIER.md) | exemption maintenue, dossier complet |
| Plateformes | [`PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) | Windows/WASI mesurés ; macOS/ARM/mobile `NOT_RUN`/`UNSUPPORTED` |
| D3/D4/D9 — politiques | [`D3-D4-D9-POLICIES.md`](../release/D3-D4-D9-POLICIES.md) | propositions, aucun acte réservé au mainteneur |

**Contradiction majeure mesurée.** La couverture `188/188` porte sur des
**FeatureIds**, pas sur les **807 énoncés MUST** de `spec/language/` :
**463/807** seulement sont atteignables par un FeatureId, **344** ne le sont pas.
T2 ne peut donc plus être présenté comme `PASS` sans qualificatif : il repasse à
`PARTIAL` (décision **D10** sur le dénominateur). Le seuil n'a pas été modifié et
aucune règle n'a été touchée.

**Métriques du backend (mesurées, charge 100 %, à déclarer).** `check`
225 / 375 / 1 058 ms et `build` 2 027 / 2 758 / 6 109 ms pour 100 / 500 / 2 000
fonctions ; `rustc` représente **77–87 %** du build ; Rust généré
58 511 / 268 245 / 1 068 750 o ; binaires 184 832 / 345 600 / 947 200 o. Au repos :
**NOT_RUN** (machine partagée par six agents).

---

## D — P2 : capacités réelles et blocages décidables

**Livré et vérifié.** `examples/file-store` — magasin clé/valeur versionné en
Ravum pur (338 lignes, 21 fonctions) : `put`/`get`/`list`/`history`/`delete`
(pierre tombale), erreurs typées, refus de traversée de chemin, données
persistantes après redémarrage. Effets : `io`, `fs`, `alloc` ; profils `pure` et
`rt` le refusent. Couvert par 13 assertions de smoke et 3 cas de conformance.
**Ce n'est ni une base de données, ni du SQL, ni un service.**

**Preuve de liaison (mesurée).** La commande `rustc` n'a **aucun** `-L`, `-l`,
`--extern` ni `--cfg` ; `ffi_safe()` n'admet que des primitives (aucun pointeur,
aucun buffer) et `#[link]` est `E-UNSUPPORTED-ATTR`. Un `sqlite3_open_v2` passe le
type-check puis échoue à `link.exe` (LNK2019). Lier SQLite n'est donc pas de la
plomberie : c'est une décision **système de dépendances natives + modèle de
confiance** (D7).

| Verticale | État | Blocage |
|---|---|---|
| Stockage fichier (P2-E partiel) | **fonctionnelle** | — |
| TCP (P2-B) | `AWAITING_HUMAN_DECISION` | D6 : `net` est `UNSUPPORTED` ; toute extension d'effet est normative |
| HTTP (P2-C) | `AWAITING_HUMAN_DECISION` | dépend de `net` |
| SQLite (P2-D) | `AWAITING_HUMAN_DECISION` + `EXTERNAL_DEPENDENCY` | D7 + système de liaison natif |
| Compilation continue (P2-F) | conception (`RFC-P2-CONTINUOUS-COMPILATION`), P0-B/P0-C en place | reste : cache incrémental, annulation, hot-state |
| DSP temps réel (P2-H) | **PARTIAL mesuré** | 10 000 swaps, 0 allocation/libération sur le thread de rendu, 10,1 M blocs sans version mixte ; gigue max **6 579 µs** pour une période de 5 333 µs sous charge, 1 échéance manquée ⇒ **pas** une preuve hard-RT |

---

## E — Tests : résultats exacts

| Commande | Résultat | Environnement |
|---|---|---|
| `cargo test --workspace --all-targets` | **94 suites, 1525 passés / 0 échec / 2 ignorés**, exit 0 | Windows 11, rustc 1.95.0, binaires épinglés |
| `ravum-conformance run --verify-build` | **424 cas : 423 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build 163/163 couverts, 101 vérifiés, 62 implicites, 0 échec ; `build_verified: true` | idem |
| `scripts/lang-smoke.ps1 -SkipBuild` | **43 PASS / 0 FAIL / 0 NOT_RUN** | idem |
| `scripts/watch-fail-closed-check.ps1` | **PASS** (ancienne version survit, activation, 1 enfant, 0 orphelin, staging supprimé) | idem |
| `cargo test -p ravum-lang-sema` | **649 passés / 0 échec / 1 ignoré** | idem |
| `cargo test -p ravum-lang-codegen` | 29 lib + 28 backend_audit | idem |
| `cargo test -p ravum-lang-cli` | 129 passés (dont 7 `build_isolation`, 6 `exit_codes`, 5 `test_runner_contract`, 3 `watch_continuity`) | idem |
| `cargo test -p ravum-conformance` | 48 unitaires + 9 intégration | idem |
| `python tools/registry_audit.py` | **108 codes / 126 émis / 0 hors registre / 0 jamais émis / 0 W hors DIAG-1.6**, exit 0 | Python 3.13.7 |
| `python tools/features_coverage.py --strict` | **188/188** (187 cas + 1 exemption), exit 0 | idem |
| `python tools/must_audit.py` | **807 MUST / 463 atteints / 344 non atteints**, exit 0 | idem |
| `python -m unittest discover -s tools/tests` | **33 tests OK** | idem |
| `python tools/report_consistency.py`, `json_contract.py`, `validate_repo_docs.py` | PASS / PASS / PASS | idem |

**Non exécuté sur ces commits (à ne pas compter comme vert)** : fuzz complet,
Miri, couverture `llvm-cov`, mutation, build bit-à-bit, SBOM, installateur,
WASI, job VS Code, et toute CI. Ces preuves existent sur `ea7df46` (passe 5) et
restent liées à ce SHA.

---

## F — Gates T0–T12 (sans modification de critère)

| Gate | État | Ce qui change dans cette passe |
|---|---|---|
| T0 | `PASS` | inchangé |
| T1 | `PARTIAL` | voie B réellement utilisée ; voie C absente ; D2 proposée |
| T2 | **`PARTIAL`** | FeatureIds 188/188 **mais** MUST 463/807 : le critère ne couvre pas les règles MUST (D10). `F-META-006` sans oracle |
| T3 | `PASS` | +1 525 tests ; frontend inchangé sur les règles |
| T4 | `BLOCKED` | audit humain externe toujours absent |
| T5 | `PASS` | smoke 43, publication atomique, identité de build |
| T6 | `PARTIAL` | FFI scalaire ; réseau/threads/async `UNSUPPORTED` |
| T7 | `PASS` | inchangé |
| T8 | `PARTIAL` | `watch` **fail-closed** ; toujours **pas** de préservation d'état |
| T9 | `PARTIAL` | **10 verticales**, dont `file-store` ; HTTP/SQLite absents |
| T10 | `PARTIAL` | 1 525 tests, **build vérifié bloquant** ; audit externe absent ; fuzz/Miri/mutation non rejoués |
| T11 | `PARTIAL` | inchangé ; aucune clé, aucune release |
| T12 | `BLOCKED` | dogfooding indépendant et Go/No-Go humains absents |

Bilan : **4 PASS / 7 PARTIAL / 2 BLOCKED**.

---

## G — Architecture : ce qui a changé, interfaces préservées

- **Codegen** : l'émission est maintenant *position-dépendante* pour les
  références texte ; un planificateur de moves par place (déstructuration hoistée
  + réécriture par node id) remplace l'émission naïve des accès indexés. Le
  langage généré reste du Rust sûr, auditable, sans `unsafe` hors FFI.
- **Pipeline** : nouveau contrat d'identité de build (`BuildIdentity`), arbre
  isolé `.ravum/build/<id16>/`, `publish_rename` atomique, manifeste
  `<artefact>.build.json`, garde d'obsolescence par re-chargement. **Interfaces
  préservées** : `.ravum/gen/main.rs`, `.ravum/bin/<name>`, `.ravum/lib/<name>.rlib`
  et `Artifacts.binary` restent valides ; `Artifacts` gagne `identity`,
  `manifest`, `build_dir`, et `build` les affiche.
- **CLI** : `watch` distingue explicitement « continuité de service » de « hot
  reload » et de « préservation d'état » ; l'arrêt de l'ancien processus a lieu
  **après** le démarrage réussi du nouveau.
- **Conformance** : dimension build additive dans le rapport (`build_verified`),
  sans casser `schema_version: 1`.
- **Sémantique** : six resserrements et une correction d'acceptation, tous
  justifiés par des MUST existants ; **aucune règle du langage modifiée**.

---

## H — Git

| Élément | Valeur |
|---|---|
| Branche | `feat/lang-core-1.0-rc`, base `5d88ecb` |
| Commits | `8c6434b`, `5daeb40`, `e0af6b6`, `9314d2f`, `9c163de`, `a3a43b2`, + commit de documentation |
| Fichiers | ≈ 40 sources/tests, 38 cas de conformance, 10 documents, 4 scripts de preuve |
| Push | **non effectué** : les commits existent **localement** ; aucun historique réécrit, aucun merge, aucun tag, aucune release |
| CI | **NOT_RUN** sur ces commits. Le workflow `linux-qual` exécute désormais `--verify-build` et exige `build_verified: true` ; il n'a pas encore tourné sur ces SHA |
| PR #101 | ouverte, draft, base `dev`, non fusionnée ; **non mise à jour** par cette passe |
| `main` | intacte (`ca675497`) |

**Action laissée au mainteneur** : `git push origin feat/lang-core-1.0-rc` pour
déclencher la CI. Aucun agent ne fusionne ni ne publie.

---

## I — Blockers et éléments nécessitant une action humaine

| # | Élément | Classe | Conséquence |
|---|---|---|---|
| 1 | Push de la branche et observation de la CI | `EXTERNAL_DEPENDENCY` | aucune preuve CI sur ces commits |
| 2 | D4 — audit externe humain | `AWAITING_HUMAN_DECISION` | T4 et T12 restent `BLOCKED` |
| 3 | D2/D3/D6/D7/D9 — backend, fusion, périmètre, FFI, custody | `AWAITING_HUMAN_DECISION` | T1/T11 partiels ; pas de release signée |
| 4 | D10 — dénominateur de traçabilité (188 vs 807) | `AWAITING_HUMAN_DECISION` | T2 reste `PARTIAL` |
| 5 | D11 — contradictions de spécification (EFF-1.1 9/10, FEAT-12.5 6/8, G-5.11 et G-6.40 dupliqués) | `AWAITING_HUMAN_DECISION` | documentation normative incohérente sur des points précis |
| 6 | D12 — trois divergences `check → build` ouvertes | `NEEDS_EXPERIMENT` ou `AWAITING_HUMAN_DECISION` | E0508 bruyant, hors corpus |
| 7 | Fuzz, Miri, couverture, mutation, repro, SBOM, WASI, VS Code non rejoués sur cette passe | `NOT_RUN` | preuves liées à `ea7df46`, pas à ces commits |
| 8 | Aucune CI Windows alors que Windows est la plateforme primaire | `EXTERNAL_DEPENDENCY` | preuve Windows locale seulement |
| 9 | Ambiguïtés sémantiques A1–A8 (`types.md` exemples obsolètes, provenance `&'static str` via génériques, `take(&(a+1))`, double rapport `[i32;0]`) | `AWAITING_HUMAN_DECISION` | comportement conservateur conservé, aucune règle changée |

---

## J — Next steps (chemin critique)

1. **Pousser** `feat/lang-core-1.0-rc` et lire la CI réelle (Linux). Corriger tout
   écart du job `--verify-build` avant toute autre promesse.
2. **Rejouer sur ce SHA** les qualifications lourdes : `llvm-cov` + seuils,
   mutation ciblée, fuzz (lexer/parse + sema), Miri Linux, repro bit-à-bit, SBOM,
   installateur, WASI. Tant qu'elles ne sont pas rejouées, elles restent
   `NOT_RUN` pour ces commits.
3. **Décider D10** (dénominateur) puis, selon la réponse, tracer ou exclure par
   écrit les 344 MUST non atteints. C'est le seul chemin crédible vers un T2
   `PASS` non ambigu.
4. **Décider D11** (comptes de spécification et `rule_id` dupliqués) : corrections
   non normatives à valider, renumérotation à trancher.
5. **Poursuivre P0-A** : abaissement général des moves par place pour fermer les
   trois formes ouvertes (D12), puis étendre `--verify-build` aux cas
   multi-fichiers et aux paquets.
6. **P2** : obtenir D6 (`net` dans Core 1.x ?) puis l'ADR d'extension d'effets ;
   seulement ensuite le prototype TCP derrière une surface expérimentale
   fail-closed, puis HTTP en `.rv`. SQLite reste derrière D7 et un système de
   dépendances natives.
7. **Audit externe** (D4) : le dossier [`AUDIT-BRIEF.md`](../audit/AUDIT-BRIEF.md)
   est prêt ; il est le seul chemin vers T4/T10/T12.

---

## Statut de livraison honnête

`CORE_RC_CANDIDATE` — fiabilité P0 nettement améliorée et **mesurée**, contrats
P1 préparés sans ratification, une verticale P2 réelle livrée, le reste bloqué
par des décisions humaines et par des preuves lourdes non rejouées sur ce SHA.
Aucun `PROVED`, aucune release, aucun merge.

---

## Passe 7 — CI réelle et fermeture des places de tableau (2026-11-11, `c6b4537`)

Ajout après le rapport A–J, à la demande du mainteneur (« push et continue sur les
tâches restantes »). Cette section **prévaut** sur les chiffres A–J.

### Ce qui a été poussé et ce que la CI a dit

La branche `feat/lang-core-1.0-rc` comptait 7 commits locaux jamais poussés : la CI
n'avait donc **jamais** tourné sur ce code (`NOT_RUN` dans la section F). Après
push, **12 runs sur 12 sont verts** sur `c6b4537` — Linux qualification comprise,
qui n'avait jamais été exercée sur ces commits.

La CI a trouvé **trois défauts que le local Windows ne pouvait pas voir**, tous dans
nos propres tests de la passe 6, tous corrigés :

| Défaut | Preuve | Correctif |
|---|---|---|
| `cargo fmt` jamais exécuté — job bloquant rouge | job « Check formatting (rustfmt, pinned toolchain) » | `ab51b17` (formatage seul, 6 fichiers) |
| contrat d'`exit code` écrit depuis l'observation Windows seule | Linux : `300` n'est jamais observable (POSIX tronque à 8 bits), `256` devient `0` | `10459b8` + `c67f17e` (paire `cfg(windows)`/`cfg(unix)`, limitation documentée) |
| course `try_wait`/`kill` dans le test de build interrompu | Linux : le build finissait entre la sonde et le signal, le test traitait une publication **complète** comme une violation | `f10a12f` (attente du début de phase de compilation, deux états acceptés) |

### Ce que le langage a gagné

**Cinq formes `check`-pass / `build`-fail sont fermées** (`0f73738`) :

| Forme | Repro | Preuve |
|---|---|---|
| chaîne d'index imbriquée | `return a[0][1];` sur `[[String; 2]; 2]` | test `rustc`-vérifié + cas `run/array-move-nested` |
| tableau derrière champ/tuple | `h.items[0]`, `t.0[0]` | idem + `run/array-move-projected` |
| liaison masquée par shadowing | paramètre `a` + `let a = …` | idem + `run/array-move-shadowed` |
| récepteur `self` **par valeur** sur élément | `a[0].take()`, `a[i].take()` | idem + `run/array-move-self-receiver` |
| payload de tableau dans un bras de `match` | `E::Items(a) => { return a[1]; }` | idem + `run/array-move-match-payload` |

La dernière est une **régression introduite par le correctif lui-même**, détectée
par le sondage systématique round-3 (47 programmes × 11 axes) **avant**
intégration : c'est la démonstration que le sondage sert à quelque chose.

**Quatre formes restent ouvertes**, bruyantes (E0508), documentées et hors corpus :
index dynamique dans un tableau imbriqué (`a[0][i]`), racine de chaîne qui n'est pas
une place (`make()[0]`), plusieurs accès dans un bras de `match` en corps
d'expression, récepteur `by value` d'une méthode **builtin** (`a[0].unwrap()`).
Elles relèvent de la même option que D12 (abaissement général) : c'est du travail
d'ingénierie restant, pas une décision humaine. **P0-A reste donc `PARTIAL`.**

### Mesures de la passe 7

| Mesure | Résultat |
|---|---|
| Tests workspace | **113 suites, 1530 passés / 0 échec / 2 ignorés** |
| Conformance `--verify-build` | **448 cas : 447 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build **187/187** (120 vérifiés, 67 implicites, 0 échec) ; `build_verified: true` |
| Sondage round-3 | **47 programmes × 11 axes, 0 divergence** (sortait `1` avec 4 divergences avant) |
| Smoke / watch | **43 PASS / 0 FAIL / 0 NOT_RUN** ; **WATCH CHECK: PASS** |
| Registre / FeatureIds / MUST | **126 émis, 0 écart** ; **188/188** ; **463/807** (inchangés) |
| Contrats Python | **33 tests OK** |
| CI | **12/12 verts** sur `c6b4537` (Linux `43031394324`, Windows `43031394660`) |

### Ce qui reste non prouvé après cette passe

macOS/ARM/Android/iOS (`NOT_RUN`/`UNSUPPORTED`, D6) ; campagne de **mutation**
(non rejouée : `mutation.yml` est en `schedule`/`dispatch` et absent de la branche
par défaut, donc non déclenchable par `gh workflow run`) ; **fuzz complet**
(seul le garde-fou `fuzz_*` tourne en CI) ; **couverture** non re-mesurée après les
changements de la passe 7 (le ratchet CI reste vert) ; **reproductibilité bit-à-bit,
SBOM, provenance, installateur** non rejoués sur ce SHA ; **audit externe** (D4)
toujours absent, donc T4/T10/T12 restent bloqués.


## Passe 8 — preuves continues et véracité documentaire (2026-11-11, `7696e2a`)

Cette passe ne change pas les gates (`4 PASS / 7 PARTIAL / 2 BLOCKED`) ; elle ferme
des formes, rend cinq preuves continues et corrige la documentation.

**Fermé.** Les quatre formes `check`-pass / `build`-fail laissées ouvertes par la
passe 7 (index dynamique imbriqué, racine non-place, plusieurs accès dans un bras
de `match` en expression, récepteur builtin *by value*) et les deux signalées par
la revue (descentes multiples sur des éléments différents, base relue) sont
abaissées (`5268838`, `d693162`). Chacune a un test qui compile réellement, exécute
le binaire et vérifie la valeur, plus un cas de corpus : la conformance passe de
**448 à 453 cas** et le périmètre build de **187 à 192**.

**Encore ouvert.** Deux variantes bruyantes (`E0508`) restent : descentes dans
deux branches distinctes, et deux descentes dans un même bloc imbriqué.
Programmes minimaux et mécanisme dans `docs/qualification/CONFORMANCE-BUILD-CONTRACT.md`
§9 ter. **`P0-A` reste `PARTIAL`**, et aucun test ne verrouille le défaut.

**Continu.** Nouveau workflow `qualification-evidence.yml` : conformance Windows,
reproductibilité bit-à-bit, SBOM, provenance et installateur Windows sont rejoués
à chaque push et chaque PR. Runs `43064500916` (push) et `43064513146` (PR) :
`success`, conformance **453 cas / 452 PASS / 0 FAIL / 1 SKIP**, `build_verified`
vrai, installateur **16/16**, provenance **8/8**, SBOM déterministe, repro **PASS**
(`C281A035…` sur le runner, `847DF0D1…` en local — le digest est propre à
l'environnement, d'où la portée « même toolchain, même linker »).

**Deux défauts trouvés par la CI dans ce workflow, pas dans le langage.** Une
étape qui prouve un refus (`exit 1`) héritait de ce code et affichait rouge avec
8/8 assertions vertes ; et un `$code:` dans un message est une référence de
variable à portée, donc un bloc entier n'était jamais analysé. Les deux sont
corrigés (`7696e2a`, `abe7d88`) et un contrôle de syntaxe couvre désormais chaque
bloc `pwsh` du workflow.

**Véracité documentaire.** Sept commits corrigent une quarantaine d'affirmations
fausses dans `spec/` et `docs/` — LSP « UNSUPPORTED », WASM, `--emit-ir`,
FFI/`unsafe`, licence « à décider », signature « non implémentée »,
reproductibilité « non revendiquée », codes `E-TYP-RETURN` et `E-MOD-BOTH-ROOTS`,
inventaires de tests. Motif commun : un document écrit à une passe antérieure et
jamais revu quand le code avançait.

**Mesures.** `cargo test --workspace` : **113 suites, 1536 passés / 0 échec /
2 ignorés**. Couverture sema re-mesurée : **88,94 % lignes / 88,46 % régions**
(contre 89,2 % / 88,6 % publiés en passe 5 ; `typeck.rs` 92,74 % contre 93,8 %) —
la baisse est rapportée, aucun seuil n'a été abaissé et tous tiennent.
Traçabilité et registre inchangés (188/188 ; 108 codes, 126 émis ; MUST 463/807).

**Non prouvé, inchangé.** Audit externe (D4), signature (D9), release,
reproductibilité inter-machines, campagne de mutation non rejouée (workflow absent
de la branche par défaut), fuzz complet non rejoué, macOS/ARM, hard-RT.
