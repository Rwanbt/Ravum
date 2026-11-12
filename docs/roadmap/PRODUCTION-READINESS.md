# Ravum Core 1.0 — cible « production ready » : définition, écart, plan

> **Décision D2 (2026-11-11)** : la cible n'est plus « MVP » mais **production
> ready**. Ce document dit ce que cela veut dire *concrètement* pour un
> compilateur, ce qui est déjà acquis, ce qui manque, et dans quel ordre le
> travail se fait. Ce n'est **pas** une déclaration de disponibilité : aucune
> ligne ci-dessous n'est cochée sans la commande, le SHA et la sortie qui la
> ferment.
>
> **État au commit `0c2ceb8`** (Windows 11 x64, `rustc 1.95.0`) : conformance
> **456 cas : 455 PASS / 0 FAIL / 1 SKIP**, périmètre build **195/195**,
> `build_verified: true` ; workspace **113 suites, 1537 passés / 0 échec** ;
> **0 forme `check`-pass / `build`-fail connue** ; gates **4 PASS / 7 PARTIAL /
> 2 BLOCKED**.

## 1. Ce que « production ready » change dans la barre d'acceptation

Une démo verte prouve qu'un programme marche sur une machine, un jour. La mise en
production d'un compilateur exige davantage, et la différence est mesurable :

| Dimension | Barre « MVP » (ancienne) | Barre « production » (décidée) |
|---|---|---|
| Conformité | le corpus passe | le corpus passe **et** aucune divergence connue, **et** chaque règle normative est tracée vers un cas ou une exemption machine-lisible |
| Sûreté | pas de miscompilation observée | borrow checker **audité indépendamment**, frontière FFI sous contrat écrit, code généré sans `unsafe` |
| Reproductibilité | builds identiques sur une machine | builds identiques **entre machines et toolchains documentées**, artefacts signés |
| Qualité mesurable | couverture sur un crate | couverture **sur tous les crates à logique**, seuils ratchet, **score de mutation** sur les modules critiques |
| Interfaces | CLI qui marche | contrats versionnés (CLI, JSON, LSP, codes de diagnostic) avec politique de dépréciation |
| Distribution | installateur testé | installateurs Windows/Linux, mise à jour, provenance vérifiable |
| Confiance | « nos tests sont verts » | audit externe humain, limites écrites, aucun `PROVED` |

Conséquence immédiate : **le verdict T4/T12 ne peut pas être contourné** par la
décision D2. On peut avancer, fusionner et durcir ; on ne peut pas *revendiquer*
la production avant l'audit (D4) et la signature (D9).

## 2. Critères de sortie (ce qui ferme chaque point)

Chaque critère a une preuve attendue. Tant qu'elle manque, le critère est
`PARTIAL`/`NOT_RUN`, jamais « fait ».

| # | Critère | Fermé par |
|---|---|---|
| **R1** | Sémantique ratifiée | D2 appliquée : contrat de backend + socle normatif ratifiés ; `spec/` sans contradiction interne (D11) |
| **R2** | Aucune divergence `check`/`build` connue | `--verify-build` vert **et** registre des formes vides (fait au 2026-11-11) |
| **R3** | Traçabilité complète | métrique FeatureIds 188/188 **et** métrique MUST publiée (463/807) avec plan pour les 344 (D10) |
| **R4** | Sûreté du frontend | audit externe du borrow checker (D4) ; corpus adversarial étendu ; Miri sur la lib et les contrats |
| **R5** | Frontière FFI sûre | programme D7 ci-dessous : contrats écrits + refus outillés + tests négatifs + `pure` qui la refuse |
| **R6** | Code généré sûr | `#![forbid(unsafe_code)]` dans tout fichier généré (déjà) ; aucun `unsafe` hors du module d'interop audité ; revue du prélude runtime |
| **R7** | Reproductibilité inter-machines | même digest sur **deux runners distincts** (aujourd'hui : local et CI diffèrent, c'est documenté et non revendiqué) |
| **R8** | Qualité mesurable | couverture de **tous** les crates à logique + seuils ratchet ; mutation sur `sema/own.rs`, `sema/typeck.rs`, `codegen/emit.rs` |
| **R9** | Performance sous budget | `check`/`build`/`watch` sur machine de référence 16 Go, au repos, p50/p95 publiés, absence de régression suivie |
| **R10** | Interfaces stables | CLI `--help` = contrat, schémas JSON versionnés, registre de codes figé, LSP testé ; politique de dépréciation écrite |
| **R11** | Distribution | installateur Windows **et** Linux, désinstallation propre, provenance + SBOM attachés à l'artefact |
| **R12** | Artefacts signés | D9 : clé hors ligne, `allowed_signers` publié, vérification documentée |
| **R13** | Audit externe | D4 : rapport humain indépendant sur borrow checker, codegen, prélude |
| **R14** | Documentation sans fausse affirmation | audit de véracité rejoué à chaque passe ; guide utilisateur ; notes de version |

## 3. État mesuré, critère par critère

**Plateformes (D5, 2026-11-11)** : Tier 1 = **Windows x86-64 + Linux x86-64**
(`ubuntu-latest`). **macOS a désormais son job** : `.github/workflows/macos-qual.yml`
(`macos-latest`, Apple Silicon arm64 : tests workspace + conformance build-vérifiée)
— niveau **Tier 2**, informatif, non requis pour fusionner. Promotion en Tier 1
après 30 jours de CI verte rattachée aux SHA. macOS x86-64, ARM Linux, Android,
iOS restent `NOT_RUN`.

| Critère | État | Preuve / manque |
|---|---|---|
| R1 | `PARTIAL` | 43 ADR `PROPOSED` ; contrat de backend écrit (`RFC-D2`), non ratifié ; D11 corrigé le 2026-11-11 |
| R2 | **fait sur le périmètre** | 456 cas, 0 FAIL, 0 forme connue ouverte ; ne prouve pas l'absence de formes inconnues |
| R3 | `PARTIAL` | 188/188 FeatureIds ; 463/807 MUST ; D10 = deux métriques + plan pour les 344 |
| R4 | `BLOCKED` | audit externe absent ; Miri Linux vert en CI, Windows partiel |
| R5 | **ouvert** | inventaire FFI réel (`RFC-D7`), aucun contrat ABI/lifetime/unwinding écrit, tests négatifs partiels ; programme §5 étendu aux **wrappers sûrs**, modules `trusted`, effets déclarés sur l'`extern` et isolation |
| R6 | **fait** | tout fichier généré porte `#![forbid(unsafe_code)]` ; le runtime est en Rust sûr |
| R7 | `PARTIAL` | local `847DF0D1…` ≠ runner `C281A035…` : digest lié à l'environnement, non revendiqué |
| R8 | `PARTIAL` | couverture **sema + codegen** depuis le 2026-11-11 (88,54/87,86 totaux ; `emit.rs` 87,02/86,02) avec seuils ratchet par fichier ; **objectif ratifié : 100 % lignes ET régions** — `coverage_report.py --gap` liste **11 fichiers sous l'objectif** (~2 100 lignes non couvertes : `emit.rs` 757, `loader.rs` 581, `typeck.rs` 274, `own.rs` 182, `resolve.rs` 138, `check.rs` 131) ; mutation ciblée (passe 5) non rejouée ; `mutation.yml` est désormais dans `main`, son premier run reste à observer |
| R9 | `PARTIAL` | mesures de `RFC-D2` sous charge déclarée ; pas de campagne au repos |
| R10 | `PARTIAL` | CLI et JSON documentés et testés ; politique de dépréciation absente |
| R11 | `PARTIAL` | installateur Windows CI 16/16 ; Linux installé par la CI de qualification mais pas de contrat d'installateur public |
| R12 | **différé** | mécanisme qualifié (refus d'un binaire altéré) ; **aucune clé** (D9 différée) |
| R13 | `BLOCKED` | trois revues LLM reçues (Claude, ChatGPT, DeepSeek) mais non versées au dépôt ; un audit humain indépendant reste requis |
| R14 | `PARTIAL` | quarante affirmations fausses corrigées en passe 8 ; guide utilisateur absent |

## 4. Plan de travail, par ordre de dépendance

### Vague 1 — ce qui débloque le reste (semaines)

1. **D11 (fait)** : contradictions de spec corrigées (`effects.md` 9→10,
   `features.md` FEAT-12.5 6→8, collisions de `rule_id` documentées Q-GRAM-11).
2. **D2** : ratifier le contrat de backend + le socle normatif prouvé par la
   conformance. Livrable : `docs/adr/README.md` mis à jour + décision écrite.
3. **R8 — couverture réelle** : mesurer `ravum-lang-codegen` (en cours),
   `ravum-lang-syntax`, `ravum-lang-cli`, `tools/conformance` ; ajouter des
   seuils ratchet par crate ; porter la mesure dans `coverage.yml` (aujourd'hui
   `-p ravum-lang-sema` seulement). C'est le point le plus faible des chiffres
   publiés.
4. **R5 — FFI sûre** : écrire les contrats (ABI, lifetimes, unwinding,
   callbacks, ressources), les refuser au `check` quand ils ne sont pas
   respectés, ajouter les tests négatifs, documenter la politique des
   bibliothèques C. Voir §5.
5. **R9 — performance de référence** : campagne au repos, machine 16 Go,
   p50/p95 publiés, seuil de non-régression en CI.

### Vague 2 — durcissement mesurable

6. **R8 — mutation** : campagne complète sur `sema/own.rs`, `sema/typeck.rs`,
   `codegen/emit.rs` ; survivants triés en inéquivalents/inatteignables ; le
   workflow `mutation.yml` doit devenir déclenchable (il est absent de la
   branche par défaut).
7. **R4 — fuzz long** : campagne `cargo-fuzz` versionnée (corpus, nightly
   épinglé) au lieu du seul smoke.
8. **R4/R13 — audit externe** : mandater la revue humaine ; verser les trois
   revues LLM comme *entrées* et trier leurs constats (voir §6).
9. **R10** : politique de dépréciation, gel du registre de codes, contrat LSP.

### Vague 3 — distribution et preuve

10. **R7** : deuxième runner (ou deuxième machine) pour un digest identique,
    ou documentation explicite de la dépendance à l'environnement.
11. **R11** : installateur Linux avec le même contrat que Windows.
12. **R12/D9** : clé hors ligne + `allowed_signers`, signature de l'artefact.
13. **R14** : guide utilisateur, notes de version, audit de véracité final.

## 5. Programme D7 — rendre la FFI sûre

**Ce qui existe** : `extern "C"` + `unsafe`, signatures primitives uniquement ;
`rustc` ne reçoit **aucun** `-L`/`-l`/`--extern` ; `ffi_safe()` n'admet ni
pointeur ni buffer ; le profil `pure` refuse l'effet `ffi`/`unsafe`.

**Ce qui manque, et que « rendre safe » veut dire ici** :

| Élément | Contrat à écrire / à outiller |
|---|---|
| ABI | types admis dans une signature `extern` (entiers, flottants, `bool`?, `*const`/`*mut` interdits ?), alignement, `#[repr(C)]` exigé sur les structs traversant la frontière |
| Durée de vie | **aucune** référence `&T`/`&mut T` ne traverse ; aucune donnée empruntée ne sort d'un appel ; le buffer doit être possédé |
| Déroulement (unwinding) | panique interdite à travers la frontière (abort ou `catch_unwind` explicite, décision à écrire) |
| Callbacks | interdits en Core 1.0, ou restreints à un pointeur opque + `unsafe` documenté |
| Ressources | qui alloue, qui libère, double-free interdit par construction (pas de `free` exposé) |
| Threads | aucune garantie `Send`/`Sync` à travers la frontière sans contrat écrit |
| Effets | `ffi` et `unsafe` inférés et déclarés ; profil `pure`/`rt` les refuse |

**Ce que « rendre safe » ne peut pas vouloir dire** : rendre sûres les
bibliothèques C appelées. Nous pouvons rendre la **frontière** explicite,
minimale, auditée et refusante ; nous ne pouvons pas prouver la mémoire d'un code
que nous ne compilons pas. Cette limite doit être écrite dans la doc utilisateur.

**Comment Rust fait — et pourquoi la solution existe quand même.** Rust prouve la
mémoire **à l'intérieur de Rust** : le borrow checker est une preuve locale sur du
code qu'il voit. À la frontière FFI, Rust ne prouve rien du C : `unsafe` signifie
exactement « le compilateur délègue l'obligation de preuve au programmeur ». La
solution n'est donc pas de prouver le C, elle est de **rendre la frontière
étroite, contractuelle et auditable**, ce que la bibliothèque standard fait avec
des wrappers documentés (`// SAFETY:`) et ce que les projets sérieux complètent
par de l'**isolation** quand le contrat ne peut pas être tenu. Le programme D7
suit ce modèle en quatre étages :

1. **Confinement** : `unsafe` et `extern "C"` seulement dans des modules marqués
   `trusted`, listés dans un inventaire versionné ; la CI compare l'inventaire à
   chaque PR. Aucun `unsafe` ailleurs (le code généré porte déjà
   `#![forbid(unsafe_code)]`).
2. **Contrat d'effets** : un `extern` déclare ses effets (`effects(net)`), et un
   appel FFI produit `{unsafe, ffi}` **∪** les effets déclarés — sinon il est
   refusé hors du profil `core`. C'est ce qui empêche une FFI de contourner le
   système d'effets (constat des quatre revues).
3. **Wrappers sûrs** : une fonction sûre encapsule l'`unsafe`, porte les effets
   déclarés mais **plus** `unsafe` : l'`unsafe` est encapsulable et non
   contagieux, ce qui rend l'audit tractable (une bibliothèque = un module
   wrapper, aucun type Ravum brut à la frontière).
4. **Isolation** : pour les bibliothèques dont le contrat ne peut pas être borné
   ou revu (callbacks incontrôlables, effets inconnus), l'exécution dans un
   composant isolé (processus séparé ou WASM) fait porter la sûreté mémoire à la
   frontière d'isolation au lieu de l'analyse statique. C'est le seul moyen de
   tenir une garantie quand la preuve locale est impossible.

**Livrables** : `docs/ffi/SAFETY.md` (contrats + limites), refus outillés au
`check`, tests négatifs (pointeur, buffer, callback, unwinding, ABI fausse),
`// SAFETY:` obligatoire sur chaque `unsafe` du runtime, revue du module
d'interop, et une passe Miri sur les cas FFI exécutables.

## 6. Programme D4 — ce que les revues LLM peuvent et ne peuvent pas

**Elles ne bloquent pas le code.** Rien dans D4 n'empêche d'écrire, de tester, de
fusionner ou de durcir : D4 ne conditionne que **le droit de dire
« production ready »**.

**Ce qu'elles apportent** : des constats à trier. Trois modèles qui relisent le
même dépôt trouvent des choses différentes ; c'est utile comme *entrée* de
triage, à condition de vérifier chaque constat au lieu de le croire.

**Ce qu'elles ne remplacent pas** : un humain indépendant qui endosse la
responsabilité de la revue (c'est la définition de T4/T12, et la raison d'être de
`docs/audit/AUDIT-BRIEF.md`).

**Procédure appliquée aux trois revues** (la même qu'aux cinq revues de la passe 6) :

1. verser le texte brut dans `docs/audit/2026-11-reviews/` (provenance : modèle,
   date, prompt) ;
2. extraire chaque constat en une ligne vérifiable ;
3. le vérifier indépendamment (programme minimal, commande, SHA) ;
4. classer : `CONFIRMÉ` → correctif + test ; `FAUX` → noté comme tel avec la
   preuve ; `NON VÉRIFIABLE` → documenté ;
5. publier un tableau de triage : constats, verdict, correctif, test.

**Ce qu'il me faut de toi** : les trois sorties d'audit (texte, fichier ou lien).
Sans elles, je ne peux pas les trier — et je ne vais pas inventer leurs constats.

## 7. Programme D10 — traçabilité et couverture

Deux mesures différentes, souvent confondues :

- **Traçabilité** : chaque règle normative est-elle reliée à un cas qui la
  satisfait ? FeatureIds **188/188** ; MUST **463/807** — 344 énoncés sans cas.
  Ce n'est **pas** un blocage de code : c'est une dette de preuve, décidée en D10
  (deux métriques officielles + plan écrit pour les 344).
- **Couverture** : le code est-il exercé par les tests ? Aujourd'hui **sema
  seulement** : 88,94 % lignes / 88,46 % régions, marge la plus mince
  `check.rs` 80,26 % pour un seuil 80. Le **codegen n'est pas mesuré** — c'est
  le trou réel dans les chiffres publiés, et il est en cours de mesure.

Fait le 2026-11-11 : le **codegen est mesuré et sous seuil** (première mesure :
87,04 % lignes / 86,05 % régions ; totaux sema+codegen **88,54/87,86**), et
`coverage.yml` couvre désormais les deux crates. **Objectif ratifié le
2026-11-11 : 100 % de lignes ET de régions sur chaque fichier mesuré.** Le
mécanisme est en place : `tools/coverage_thresholds.json` porte l'objectif, et
`python tools/coverage_report.py --input target/cov-all.json --gap` liste, fichier
par fichier, les lignes non couvertes. État de départ : **11 fichiers sous
l'objectif, ~2 100 lignes** — `emit.rs` 757, `loader.rs` 581, `typeck.rs` 274,
`own.rs` 182, `resolve.rs` 138, `check.rs` 131, `builtins.rs` 35, `facts.rs` 28,
`effects.rs` 25, `types.rs` 9, `lib.rs` 3. La trajectoire : fermer les écarts
module par module, et **relever le seuil du fichier à 100 dès qu'il l'atteint**,
pour que la couverture ne puisse plus redescendre. Reste aussi à étendre la mesure
à `ravum-lang-syntax`, `ravum-lang-cli` et `tools/conformance`. Et à compléter
par la **mutation** (mesure plus honnête pour un compilateur, car un `match`
géant peut être « couvert » sans être testé) : 100 % de couverture ne prouve pas
100 % de correction.

## 8. Ce que ce document ne prétend pas

- Il ne prouve pas que le langage est production-ready : il liste ce qui manque.
- Il ne transforme aucun `PARTIAL` en `PASS` ; les gates restent dans
  [`CORE-1.0-GATE-MATRIX.md`](CORE-1.0-GATE-MATRIX.md).
- Il ne remplace pas l'audit externe (D4) ni la signature (D9) : ce sont des
  critères de sortie, pas des tâches d'agent.
- Les estimations de durée sont volontairement absentes : elles n'ont pas été
  mesurées, et un chiffre inventé vaut moins que pas de chiffre.
