# Décisions humaines indispensables — Ravum Core 1.0

> Ces décisions **n'appartiennent pas** aux agents. Ce dossier les rend explicites,
> avec recommandation, preuves disponibles, alternatives et conséquence d'une
> non-décision. Aucune n'est prise ici. Dernière mise à jour : 2026-11-11
> (passe 9 : D12 close, `P0-A` vert sur son périmètre mesuré).

## 0. Réponses du responsable — 2026-11-11 (soirée)

Les sections D1…D13 ci-dessous restent le **dossier d'instruction** (contexte,
options, preuves). Le tableau ci-dessous est l'**état des décisions** après les
arbitrages du 2026-11-11 ; la feuille de suite détaillée est
[`docs/decisions/ANSWER-SHEET-2026-11-11.md`](docs/decisions/ANSWER-SHEET-2026-11-11.md).

| ID | Réponse | Suite appliquée / attendue |
|---|---|---|
| **D2** | Cible **production ready**, pas MVP | `docs/roadmap/PRODUCTION-READINESS.md` (14 critères, état mesuré, plan) ; **ratification** du backend + socle normatif encore à signer |
| **D3** | **Fusion dans `main` autorisée**, aucune release | **Exécutée le 2026-11-12** : `dev` = `a2b8db5` (fusion de la PR #101), `main` = `94f3ac4` (fusion de la PR #104). Commits de fusion, aucun squash, **aucun tag, aucune release, aucune signature** ; rollback = `git revert -m 1 94f3ac4` |
| **D4** | Quatre revues LLM fournies le 2026-11-11 ; ne doit pas bloquer le code | **Triées et vérifiées** dans `docs/audit/2026-11-reviews/TRIAGE.md` : 16 constats, verdicts CONFIRMÉ / FAUX (preuve) / HISTORIQUE. Deux critiques déjà corrigées (code de sortie d'un crash, builds concurrents), une vraie (`F-META-006` auto-approuvée → marquée `PENDING_HUMAN_APPROVAL`). **Aucun blocage de code** ; un audit **humain** reste requis pour le label production |
| **D5** | Tests sur **Windows et Linux** ; puis « mets les runners `macos-latest` » | Tier 1 = Windows x86-64 + Linux x86-64 ; **`macos-qual.yml` ajouté** (`macos-latest`, Apple Silicon arm64 : tests workspace + conformance build-vérifiée), **Tier 2 non requis**. Promotion en Tier 1 après 30 jours de CI verte rattachée aux SHA |
| **D6** | OK — périmètre gelé | Périmètre écrit (`PRODUCTION-READINESS.md` §6) ; extension = 1.x |
| **D7** | « Rust prouve bien la mémoire, la solution existe » | **Modèle en 4 étages** (`PRODUCTION-READINESS.md` §5) : confinement `trusted`, effets déclarés sur l'`extern`, wrappers sûrs (`unsafe` encapsulable), isolation (processus/WASM) quand le contrat C n'est pas bornable. Même modèle que Rust : preuve locale + frontière contractuelle + isolation |
| **D9** | Sans intérêt avant la mise en production | **DIFFÉRÉE** : aucune clé générée, aucun mécanisme bloqué ; T11 reste `PARTIAL` sur ce seul point |
| **D10** | « Je vise une couverture de 100 % » | **Objectif 100 % lignes ET régions** inscrit dans `tools/coverage_thresholds.json` ; `coverage_report.py --gap` liste les lignes non couvertes : **11 fichiers, ~2 100 lignes** au départ. Deux métriques de traçabilité adoptées ; couverture étendue au codegen (87,04/86,05, seuils ajoutés) |
| **D11** | OK | **Appliqué** : `effects.md` 9→10, `features.md` FEAT-12.5 6→8, collisions `G-5.11`/`G-6.40` documentées (`Q-GRAM-11`) |
| **D13** | « Une preuve déterministe ? A/B me semblent bancales » | **Réponse : oui, et ce n'est ni A ni B.** `docs/rfc/RFC-D13-STATICITY.md` : règle d'origine (treillis littéral/copie/projection/agrégat) + règle de stockage déjà outillée ⇒ tout `&str` lu dans un agrégat est statique **par construction**. Décision : lancer l'implémentation (fait de staticité dans `own.rs` + matrice de tests) |

## D1 — Licence du code et gouvernance (#34) — **RÉSOLUE**

- **Décision appliquée** : **`MIT OR Apache-2.0`** (double licence, au choix du
  preneur). Fichiers [`LICENSE-MIT`](LICENSE-MIT) et
  [`LICENSE-APACHE`](LICENSE-APACHE) à la racine ; `license =
  "MIT OR Apache-2.0"` dans `[workspace.package]`, hérité par **les 23 membres**
  (`cargo metadata --locked`, SBOM régénéré : 23/23 `MIT OR Apache-2.0`).
- **Origine** : décision du responsable, appliquée sur `main` (`ca675497`) et
  `dev` (`fbaa506`, PR #103), puis répercutée sur la branche du langage (les 4
  crates `ravum-lang-*` et `ravum-conformance` n'avaient pas le champ).
- **Reste ouvert (outillage, pas décision)** : `THIRD-PARTY-NOTICES` dérivé du
  SBOM ; politique de contributions (DCO/CLA) ; marque « Ravum ».
- **Registre** : [`LICENSE-DECISION.md`](LICENSE-DECISION.md) (`DECIDED`).

## D2 — Ratification des ADR (000…041 + RFC de cette itération)

- **Recommandation** : ne ratifier que les ADR dont les tests de conformance
  prouvent la règle ; garder `PROPOSED` le reste.
- **Preuves** : `docs/adr/` (43 fichiers PROPOSED), `spec/language/` (DRAFT),
  `conformance/` (résultats réels).
- **Sans décision** : toute sémantique reste « candidate », jamais « stable ».

## D3 — Fusion dans `main` et publication de release

- **Recommandation** : aucune fusion `main` ni release stable avant audit
  indépendant et licence tranchée ; livrer une **release candidate interne non
  publiée** liée à un SHA, avec artefacts et checksums.
- **Preuves** : rapports T0-T11, `evidence/`, `docs/reports/LANGUAGE-CORE-STATUS.md`.
- **Sans décision** : `dev` reste la branche d'intégration ; aucun tag stable.

## D4 — Audit externe / revue indépendante de sécurité

- **Recommandation** : mandater un audit humain du borrow checker, du codegen et
  du prélude avant toute promesse de « production-ready ».
- **Pourquoi** : un agent ne peut pas certifier seul l'absence de failles d'un
  compilateur inédit ; les tests ne couvrent qu'un corpus fini.
- **Sans décision** : verdict plafonné à `CORE_BETA` même si tous les gates
  locaux passent.

## D5 — Cibles Tier 1 et qualification Linux

- **Recommandation** : Windows x86-64 = testé localement ; Linux x86-64 =
  **qualifié** par un runner GitHub Actions `ubuntu-latest` sur le SHA de la PR
  (machine indépendante), run
  [42675213985](https://github.com/Rwanbt/Ravum/actions/runs/42675213985) :
  workspace, conformance, smoke, install, Miri, fuzz et WASM verts ; TSan
  `NOT_APPLICABLE` documenté. Le WSL local de la machine de développement est
  hors service (service non réactif, pas d'admin) : il n'est **pas** une preuve.
- **Preuves** : `.github/workflows/linux-qual.yml`,
  [`docs/qualification/LINUX-QUALIFICATION.md`](docs/qualification/LINUX-QUALIFICATION.md),
  artefact `conformance-report-linux`.
- **Limite** : une seule image (`ubuntu-latest`) et un seul run ; pas de
  reproductibilité multi-images ni de qualification ARM/macOS.
- **Sans décision** : annoncer Linux Tier 1 seulement pour l'image testée, pas
  comme support universel.

## D6 — Périmètre des fonctionnalités universelles (T8-T11)

- Trajectoire : Core 1.0 RC limité au sous-ensemble documenté. **FFI minimale**
  (`extern "C"` + `unsafe { }`, signatures primitives) est désormais supportée et
  testée ; hot reload et WASM/WASI sont qualifiés séparément ; threads,
  GPU/mobile, hard-RT restent `UNSUPPORTED`.
- **Sans décision** : pas d'extension de promesse au-delà de ce qui est prouvé
  par la conformance et la CI.

## D7 — Ratification de la frontière `unsafe`/FFI

- **Recommandation** : considérer la FFI Core 1.0 comme **expérimentale** tant que
  (a) l'audit interne du codegen n'est pas revu humainement, (b) aucun audit
  externe n'a couvert la frontière C, (c) la politique de sûreté mémoire des
  bibliothèques C appelées n'est pas écrite. Le profil `pure` la refuse déjà.
- **Preuves** : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.7 bis, tests
  `crates/ravum-lang-sema/tests/ffi.rs`, cas `conformance/cases/run/ffi-abs`,
  `cases/types/ffi-bad-type`, `cases/types/ffi-outside-unsafe`.
- **Sans décision** : FFI utilisable en interne, jamais présentée comme certifiée
  mémoire, jamais dans un profil `pure`/`rt`.

## D8 — Codes de diagnostic hors spécification — **RÉSOLUE (auto-corrigée)**

- **Constat mesuré** (2026-11-09, `docs/qualification/T2-FINDINGS.md`) : la passe
  de traçabilité T2 a produit **268 cas** de conformance dont **30 échouent**
  parce que le compilateur ne fait pas ce que la spécification écrit. Dans
  **13 cas**, le code émis par le compilateur n'apparaît **dans aucun** document
  de `spec/language/` : `E-OWN-MOVE-BORROWED`, `E-OWN-MOVE-INDEXED`,
  `E-OWN-NOT-COPY`, `E-RES-IMPL-FOREIGN`, `E-RES-VARIANT-UNQUALIFIED`,
  `E-TYP-ARRAY-LEN`, `E-UNSUPPORTED-GLOB`, `E-UNSUPPORTED-LET-PATTERN`,
  `E-UNSUPPORTED-OR-PAT`, `E-UNSUPPORTED-REF-PAT`, `E-UNSUPPORTED-TRAIT-IMPL`,
  `E-UNSUPPORTED-TURBOFISH`, `E-UNSUPPORTED-TYPE`.
- **Correction** : la question ne se posait pas. `spec/language/diagnostics.md`
  §3 déclare : « La liste des codes ci-dessous est **canonique** : c'est celle
  que l'implémentation émet. **Tout code absent est interdit.** » (DIAG-1.1
  impose aussi que tout diagnostic porte un code de §3.) Les 13 codes fautifs
  violaient donc une règle déjà écrite ; ils ont été **remplacés par les codes
  enregistrés** que `features.md` exige déjà. **Mon classement initial en
  « décision humaine » était une erreur** : la spécification avait tranché.
- **Ce qu'une éventuelle décision humaine pourrait encore changer** (facultatif,
  non bloquant) : réintroduire des codes plus spécifiques en les **enregistrant**
  d'abord dans §3, ce qui donnerait des messages plus précis :

  - **(a) le nom émis gagne** : l'enregistrer dans `diagnostics.md` §3 (le code
    est plus spécifique) et mettre à jour la colonne `Pass / Fail` de
    `features.md` ;
  - **(b) le nom spécifié gagne** : renommer le code dans le compilateur et
    mettre à jour les tests qui l'assertent.
- **Pourquoi ce n'est pas une décision d'agent** : les deux options sont
  défendables, elles touchent une interface publique stable
  (`diagnostics.md` §3 se déclare registre canonique) et changent le contrat
  observable pour tout outil consommateur. Choisir revient à trancher un point
  d'API, pas à corriger un bug.
- **Conséquence d'une non-décision** : le corpus de conformance reste **rouge**
  (30 FAIL), le gate T2 reste `FAIL` et la CI de conformance reste rouge. C'est
  l'état exact, pas une régression : avant cette passe, les divergences existaient
  mais n'étaient pas mesurées.
- **Preuves** : `target/conformance-after-t2.json`,
  `python tools/triage_conformance.py target/conformance-after-t2.json`.

## D9 — Custody de la clé de signature de release

- **Constat** : le mécanisme de signature est implémenté et qualifié
  ([`docs/qualification/SIGNATURES.md`](docs/qualification/SIGNATURES.md)) :
  `scripts/attest-release.ps1` signe avec `ssh-keygen -Y sign` (Ed25519,
  namespace `ravum-release`) et **rejette** un artefact altéré d''un octet. Ce qui
  manque n''est pas le code, c''est l''**ancre de confiance**.
- **Question posée** : où vit la clé privée (secret CI, HSM, machine hors ligne),
  qui peut l''utiliser, et où est publié le fichier `allowed_signers` correspondant ?
- **Pourquoi ce n''est pas une décision d''agent** : une clé générée par l''agent
  qui signe, stockée à côté de l''artefact signé, n''atteste rien. Elle donnerait
  un `PASS` de forme et zéro garantie réelle — exactement ce que le mandat
  interdit.
- **Conséquence d''une non-décision** : T11 reste `PARTIAL` ; aucune release ne
  peut être présentée comme signée, ni la provenance comme vérifiable.

## D-COERCE-1 — Coercition implicite `String → &str` — **RÉSOLUE par conformité (2026-11-11)**

- **Constat mesuré** : `check` acceptait un `String` **possédé** là où `&str` est
  attendu (`takes(owned)`, `takes(line.trim())`), puis `rustc` échouait en E0308.
  TYP-8.5 (MUST) déclare les conversions texte explicites (`.as_str()`) et
  TYP-13.1 (MUST) n'admet que `&String → &str` : le `String` possédé n'est pas
  `&String`. L'implémentation violait donc deux MUST déjà ratifiés ; ce n'était
  pas une décision de langage ouverte.
- **Correction appliquée** : l'arm `String` possédé est retiré de `coerce` ;
  `&String → &str` et `&[T; N] → &[T]` restent. `examples/stats` et
  `conformance/cases/run/stats` migrent vers `trimmed.as_str()`, exactement ce
  que `stdlib.md` montre déjà. Un test qui documentait l'ancienne acceptation a
  été remplacé par un test de refus (aucun test supprimé).
- **Ce qui reste proposé** : le texte normatif et la matrice de 14 programmes
  ([`docs/rfc/RFC-D-COERCE-1-MATRIX.md`](docs/rfc/RFC-D-COERCE-1-MATRIX.md)),
  ainsi que la politique de migration pour du code utilisateur existant.
- **Preuve** : `cargo test -p ravum-lang-sema` 649 passés ; conformance
  `--verify-build` 424 cas, 0 FAIL ; seuls `examples/` et le corpus ont été
  balayés (433 paquets) et un seul programme a changé de verdict.

## D10 — Dénominateur de la traçabilité (FeatureIds vs MUST) — **OUVERTE**

- **Constat mesuré (2026-11-11)** : `features_coverage.py --strict` donne
  **188/188** FeatureIds ; `tools/must_audit.py` compte **807 énoncés MUST**
  (805 ids uniques), dont **463/807** seulement sont atteignables par un
  FeatureId — **344** ne le sont pas. Détail :
  [`docs/qualification/MUST-TRACEABILITY.md`](docs/qualification/MUST-TRACEABILITY.md).
- **Pourquoi ce n'est pas une décision d'agent** : le seuil de T2 est un contrat
  de qualification. Le corriger revient à trancher ce que « traçabilité de la
  spécification » signifie. L'agent a seulement **mesuré** les deux métriques et
  reclassé T2 en `PARTIAL` sans changer le seuil.
- **Options** : (a) deux métriques officielles, avec obligation de traiter les
  344 ; (b) périmètre écrit qui exclut les 344 ; (c) traçabilité complète.
- **Conséquence d'une non-décision** : T2 reste `PARTIAL` ; la phrase
  « 188/188 règles tracées » doit être lue comme « 188/188 FeatureIds ».

## D11 — Contradictions internes de spécification — **OUVERTE**

| Point | Contradiction mesurée |
|---|---|
| `effects.md` EFF-1.1 | « les 9 groupes » alors que son propre tableau et `facts.rs` en comptent **10** |
| `features.md` FEAT-12.5 | énumère **6** lignes `experimental`, alors que **8** lignes du tableau le sont (`F-STD-031`, `F-META-006`) |
| `grammar.md` | deux règles distinctes portent le même `rule_id` : **G-5.11** et **G-6.40** |

- **Pourquoi une décision** : corriger un compte est non normatif, mais
  renuméroter un `rule_id` modifie un identifiant contractuel (Q-DIAG-4) et
  FEAT-12.5 est une liste MUST qui définit ce qui peut changer sans édition
  majeure. Un agent ne peut pas trancher.

## D12 — Disposition des divergences `check → build` sur les moves de tableau — **RÉSOLUE (option (a), 2026-11-11)**

- **Formes initiales** : `a[0][1]` (chaîne d'index imbriquée) ; `h.items[0]` /
  `t.0[0]` (tableau derrière un champ/tuple) ; liaison de tableau masquée par un
  shadowing imbriqué. Chacune passait `check` et échouait à `build` avec E0508.
- **Contrat** : OWN-5.3/OWN-5.4 (MUST) autorisent le déplacement d'un élément.
- **Options** : (a) abaissement général des moves par place (correctif de fond) ;
  (b) refus au `check` — contredirait deux MUST, donc exigerait un amendement ;
  (c) statu quo documenté (E0508 bruyant, hors corpus, aucune miscompilation).
- **Décision appliquée : (a)**, sans amendement de règle. Le commit `0f73738`
  abaisse les trois formes **et** deux familles trouvées depuis par sondage
  systématique : récepteur `self` **par valeur** sur un élément (`a[0].take()`,
  F1) et payload de tableau dans un bras de `match` (F2, qui était une régression
  du correctif lui-même, interceptée avant intégration). Chaque forme a un test
  qui compile avec `rustc`, exécute le binaire et vérifie la valeur, plus un cas
  de corpus (`conformance/cases/run/array-move-*`). Détail :
  [`docs/qualification/T2-FINDINGS.md`](docs/qualification/T2-FINDINGS.md) §22.
- **Ce que la résolution ne couvre plus (mis à jour le 2026-11-11, passe 9)** :
  les quatre formes encore ouvertes au moment de la décision — index dynamique
  dans un tableau imbriqué, racine de chaîne qui n'est pas une place, plusieurs
  accès dans un bras de `match` en corps d'expression, récepteur *by value* d'une
  méthode builtin — ont été fermées par la passe 8 (`5268838`, `d693162`), et les
  **deux dernières** — descentes dans deux branches distinctes d'un `if`/`else`,
  deux descentes dans un même bloc imbriqué — par la passe 9 (`21cf7d4`). Le
  corpus est passé de 443 à **456 cas** et le périmètre build de 182 à **195**,
  sans qu'aucune règle MUST ne soit amendée. **Aucune forme `check`-pass /
  `build`-fail ouverte connue** ne reste : `P0-A` est vert **sur son périmètre
  mesuré** (l'invariant général reste une observation outillée, pas une preuve).
  Détail : `CONFORMANCE-BUILD-CONTRACT.md` §9-§9 ter, `T2-FINDINGS.md` §23-§24.

## D13 — Provenance de `&'static str` à travers les agrégats — **OUVERTE**

- **Question** : `let s = a[1];` et `fn f(a: [&str; 2]) -> &str { return a[1]; }`
  passent-ils en Core 1.0 ? Aujourd'hui non : `E-OWN-REF-ESCAPE` (règle visée
  OWN-6.1), pour toutes les extractions d'un `&str` depuis un agrégat (tableau,
  `Vec`, champ de struct, payload d'enum, `Option<&str>.unwrap()`, binding de
  `match`). En revanche `a[1] == "x"` passe, `enum Label { Text(&str) }` et
  `Label::Text("x")` passent, et une copie de locale littérale
  (`let t = "x"; return t;`) passe.
- **Les deux lectures** :
  - **A — accepter** : TYP-6.3 (MUST) dit qu'un `&str` est `Copy`, stockable et
    retournable ; OWN-2.1/2.3 (une copie préserve la valeur) ; OWN-6.1/6.2
    interdisent tout stockage non statique, donc un `&str` présent dans un
    agrégat est nécessairement statique. Refuser contredirait TYP-6.3.
  - **B — refuser** : OWN-6.2 nomme l'exception « `&'static str` (littéral de
    chaîne) » et OWN-6.3 ne nomme que `as_str` ; aucune règle ne définit la
    provenance à travers un champ, un index ou un pattern. `ownership.md` §1
    revendique un modèle « conservateur par construction ». Le refus est sûr ;
    l'acceptation ferait de l'invariant « tout `&str` stocké est statique » une
    hypothèse portante de la sûreté, qu'une extension future (FFI, lifetimes)
    peut casser.
- **Périmètre enum (non tranché séparément)** : OWN-6.2 ne promet l'exception que
  pour un champ de **struct**, TYP-6.3 dit « champ ». L'implémentation accepte
  aussi la déclaration et le littéral d'un payload d'enum, mais refuse un payload
  non statique. Il faut trancher ce périmètre en même temps.
- **Impact mesuré (passe 7)** : aucune régression actuelle — 0 programme refusé
  dans les examples (9/9 exit 0) et conformance 0 FAIL. La question porte sur des
  programmes qu'un utilisateur écrirait naturellement, pas sur du code existant.
- **Recommandation de l'agent** : **B** pour Core 1.0 (garder le refus
  conservateur) et reformuler TYP-6.3/OWN-6.2 pour dire que la staticité ne se
  propage qu'aux littéraux et aux copies de locales, pas à travers les agrégats
  (limitation documentée, `Q-OWN`). Si l'ergonomie prime, choisir **A** impose de
  financer la provenance par constructeur (traçage de la staticité par agrégat)
  plus les tests de l'invariant — c'est un choix de modèle, pas un correctif
  d'implémentation.
- **Ce qu'un agent ne peut pas faire** : trancher. Aucun changement n'a été fait.

