# Contrat `check ⇒ build` du corpus de conformance (P0-A)

> **Statut : contrat outillé et bloquant, mesure honnête.** Ce document définit
> ce que le drapeau `--verify-build` mesure, sur quel périmètre, et ce qu'il ne
> prouve **pas**. Aucun chiffre n'est estimé : la section « État mesuré » cite
> la commande exacte, le commit, la toolchain et le rapport JSON produits.

## 1. La règle

Un programme Ravum **accepté par le `check`** doit aussi être **accepté par le
`build`** (génération Rust + `rustc`). Une divergence `check` vert / `build`
rouge est un défaut du compilateur, pas un passe-droit : le harnais de
conformance la classe en **FAIL** et publie les deux sorties (celle du `check`
et celle du `build`) comme preuve.

Le harnais applique cette règle sans liste d'exemption, sans silence et sans
alignement sur le comportement observé :

| Situation mesurée | Statut du cas | Dimension `build` |
|---|---|---|
| `check` exit 0 puis `build` exit 0 | **PASS** | `verified` |
| `check` exit 0 puis `build` exit ≠ 0 (sauf 3) | **FAIL** | `failed` |
| `check` exit ≠ 0 (mode accepté) | **FAIL** | `not_attempted` |
| `check` exit 0 puis `build` exit 3 (`rustc` absent) | **NOT_RUN** | `environment` |
| `build` impossible à démarrer, tué ou hors délai | **NOT_RUN** | `environment` |
| Cas `run-pass`/`run-fail` dont l'exécution attendue a eu lieu | **PASS** | `implicit` |
| Cas ignoré sur cet OS (`skip_on`) | **SKIP** | `not_attempted` |
| Cas `compile-fail` ou `cli` | selon le mode | `not_applicable` |
| Rapport produit **sans** `--verify-build` | inchangé | `not_requested` |

**Un `SKIP` ou un `NOT_RUN` n'est jamais un `PASS`.** Un environnement
incomplet (pas de `rustc`) ne peut donc pas rendre la porte verte : il la rend
`NOT_RUN` (code de sortie 3).

## 2. Commande exacte

Mesure locale (Windows 11 x86-64, toolchain figée ci-dessous) :

```
cargo build -p ravum-conformance
target\debug\ravum-conformance.exe run --verify-build --json target\conformance-report.json
```

Ou directement via Cargo :

```
cargo run -p ravum-conformance -- run --verify-build --json target/conformance-report.json
```

Codes de sortie du harnais (inchangés) : `0` aucun échec et aucun cas non
exécuté, `1` au moins un FAIL, `2` erreur d'usage/configuration ou rapport non
écrivable, `3` au moins un NOT_RUN (et aucun FAIL).

La porte CI est le job `conformance` de
`.github/workflows/linux-qual.yml` : l'étape s'appelle désormais
« Run conformance corpus (check ⇒ build contract) » et passe
`--verify-build`. Tout cas accepté dont le `check` passe et dont le `build`
échoue fait échouer le job ; un `rustc` absent produit un NOT_RUN (exit 3),
donc un job rouge lui aussi, jamais un faux vert. Aucun job, étape ou artefact
existant n'a été supprimé ni affaibli.

## 3. Dénominateur : quel corpus est couvert, et pourquoi

Le périmètre est **le corpus de conformance du dépôt**
(`conformance/cases/**`, cas déclaratifs `case.json`), et non « tous les
programmes Ravum possibles ». Le dénominateur du contrat `check ⇒ build` est
l'ensemble des cas dont le mode **produit un programme constructible** :

| Mode | Dans le périmètre | Pourquoi |
|---|---|---|
| `compile-pass` | oui, `build` explicite | le programme doit être accepté et produit ; c'est la cible directe de la règle. |
| `fmt-idempotent` | oui, `build` explicite après le `check` final | le formatage ne doit pas changer la constructibilité ; le `check` final est suivi du même `build`. |
| `run-pass` | oui, implicite | `run` compile **puis** exécute : l'exécution attendue prouve que le `build` a réussi. |
| `run-fail` | oui, implicite | `run` compile puis exécute ; le message attendu (`panic:`) prouve que le binaire a bien été construit et lancé. |
| `compile-fail` | non | programme invalide **par construction** : le succès du `build` contredirait le cas. La dimension est `not_applicable`. |
| `cli` | non | pilote des commandes arbitraires (`lint`, `doc`, `cache`, `fmt --check`, …) ; le succès du `build` n'est pas son contrat. |

Le `build` explicite est lancé sur **la même copie isolée** que le `check`,
avec la commande `build <project>` (le `--profile` est un drapeau de `check`
seul ; les `args` du cas ne sont donc pas recopiés). Les cas sont copiés dans un
répertoire temporaire par cas : aucun cas du corpus n'est modifié, aucun
réseau n'est utilisé, et les artefacts (`.ravum/`) restent dans la copie. Deux
précautions de déterminisme : la copie **exclut le cache généré `.ravum/`**
(un cas ne doit pas dépendre d'un cache laissé par un build in-place, même
partiel) et le répertoire de copie porte un nom court (`0000`, ordre de
découverte) pour rester loin de la limite `MAX_PATH` de Windows, où MSVC
`link.exe` échoue avec un `LNK1181` opaque.

Les 37 cas signalés par la revue externe (Claude) **ne sont pas disponibles**
dans ce dépôt : ils ne sont donc ni confirmés ni infirmés en tant que tels. La
matrice de sondage du Lead (7 divergences sur 19 sondes, commit `5d88ecb`) est
reconstruite par les cas de régression listés en section 6.

## 4. Ce que le rapport prouve exactement

Le rapport JSON (`schema_version: 1`, champs **additifs** ; le numéro de schéma
n'est pas changé pour ne pas casser les lecteurs existants) porte :

* `verify_build` : le drapeau a-t-il été passé ;
* `build_verified` : `true` **seulement si** `--verify-build` a été passé,
  qu'au moins un cas est dans le périmètre, et que **tous** les cas du
  périmètre ont été mesurés et ont construit (`verified` ou `implicit`), sans
  `failed`, `environment` ni `not_measured` ;
* `build_summary` : `perimeter`, `verified`, `implicit`, `failed`,
  `environment`, `not_measured` ;
* `filter` : le filtre `--filter` effectif (`<none>` sans filtre) ; le verdict
  ne porte que sur les cas présents dans le rapport ;
* par cas : `build` (`not_requested`, `not_applicable`, `not_attempted`,
  `verified`, `implicit`, `failed`, `environment`) et `build_detail` (les deux
  sorties sur divergence, la raison d'environnement sinon) ;
* le `summary` habituel (`total/pass/fail/skip/not_run`).

`build_verified: true` est un énoncé **sur la dimension build** : il ne dit pas
que le run entier est vert. Un cas `compile-fail` qui passerait à tort fait
échouer le run (`fail` > 0) sans changer `build_verified` : les deux champs
doivent être lus ensemble.

Un rapport produit **sans** `--verify-build` porte toujours
`verify_build: false`, `build_verified: false` et `build: not_requested` pour
chaque cas : il **ne doit jamais être cité** comme preuve de correction du
`build`. Le texte de résumé l'écrit explicitement :

```
build_verified: false — --verify-build was not passed; this report is NOT evidence of build correctness
```

Les champs additifs sont documentés ici plutôt que par un bump de
`schema_version` : un consommateur qui les ignore lit exactement le même
`cases`/`summary` qu'avant. Le prix de cette compatibilité est explicite : un
lecteur qui ne connaît pas les nouveaux champs ne doit pas interpréter un
rapport sans drapeau comme une preuve de build.

## 5. Ce qui n'est PAS couvert

* **Les programmes hors corpus.** Le contrat est une mesure sur le corpus de
  conformance du dépôt, à un commit et une toolchain donnés. Il ne prouve pas
  la correction de programmes arbitraires, ni l'absence de divergence sur du
  code non écrit dans `conformance/cases/**`.
* **Les cas `compile-fail` et `cli`** : check-only par construction (voir
  section 3). Leur `build` n'est ni attendu ni exécuté ; ils ne comptent pas
  dans le dénominateur.
* **Les cas `skip_on` de l'OS hôte** : `SKIP`, donc `not_measured` ; ils
  n'apparaissent pas comme construits et empêchent `build_verified: true` sur
  ce run (le corpus complet n'est alors pas mesuré).
* **Le profil d'effets.** `build` ne prend pas `--profile` ; un cas vérifié en
  `check --profile rt|pure` est construit avec le profil `core` du chemin
  `build`. Ce point est une limite du CLI actuel, pas du harnais.
* **La qualification Linux réelle.** La commande ci-dessus tourne sur Windows ;
  la porte Linux n'est prouvée que par le job CI `conformance` sur
  `ubuntu-latest`, à un run et un commit donnés.
* **La longueur de chemin sous Windows.** Le harnais raccourcit sa propre part
  (préfixe temporaire court, répertoire de cas `0000`) mais ne contrôle ni le
  nom de paquet déclaré dans `ravum.toml`, ni la disposition interne des
  répertoires de build du codegen (chemins content-addressed). Un chemin
  d'objet dépassant `MAX_PATH` produit un échec de liaison MSVC
  (`LNK1181: cannot open input file '.obj'`) classé FAIL : c'est une limite
  d'environnement/outillage à diagnostiquer, pas un défaut Ravum démontré. Ce
  cas s'est produit pendant P0-A et a été corrigé côté harnais ; il peut
  réapparaître si la disposition de build s'allonge.
* **La reproductibilité bit-à-bit, les performances, le temps réel, la
  sémantique complète** : hors sujet ici (voir les autres documents de
  `docs/qualification/`).

Autrement dit : **le corpus mesuré ne montre aucune divergence `check ⇒ build`
sur son périmètre** ; il ne prouve rien au-delà.

## 6. Corpus de régression P0-A (reconstruit)

Les familles de divergence signalées sont reconstruites par des cas dédiés.
Les codes attendus viennent de `spec/language/*.md`, **jamais** du comportement
observé. Les positifs sont dans le périmètre et doivent être `build=verified` ;
les négatifs restent des `compile-fail` (check-only) et figent le code que la
spécification impose.

### 6.1 Familles rendues par le Lead (sondes gelées, commit `5d88ecb`)

| Cas | Mode | Attendu (spec) | Rôle |
|---|---|---|---|
| `types/param-mut-binding-pass` | `compile-pass` | build OK (OWN-7.1) | liaison `mut x` de paramètre ; le codegen émettait l'assignation sans `mut` (rustc E0384). |
| `ownership/str-field-pass` → **étendu** dans `ownership/static-str-stored` | `compile-pass` | build OK (OWN-6.2, OWN-6.4) | champ `&str`, retour `&str`, usage/comparaison dans l'instruction ; le codegen émettait `&str` sans lifetime (rustc E0106). Aucun cas dupliqué : le cas existant a été étendu en programme complet. |
| `ownership/array-element-move-pass` → **étendu** dans `ownership/partial-move-tuple-ok` | `compile-pass` | build OK (OWN-5.4, TYP-5.4) | déplacement `a[1]` d'un `[String; 2]` ; rustc E0508. Le cas existant est étendu pour exécuter réellement les deux déplacements. |
| `ownership/array-element-projection-move-pass` | `compile-pass` | build OK (OWN-5.4) | déplacement `a[1].0` (projection de champ d'élément de tableau). |
| `ownership/move-after-unwrap` | `compile-fail` | `E-OWN-DOUBLE-MOVE` (OWN-5.2) | `unwrap` consomme le receveur puis `let t = o` déplace la même place sans usage entre les deux : OWN-5.2 impose DOUBLE-MOVE, pas USE-AFTER-MOVE. |
| `ownership/unwrap-consumes-receiver` | `compile-fail` | `E-OWN-DOUBLE-MOVE` (OWN-5.2, STD-11.2/11.3) | matrice des trois receveurs consommés (`Option::unwrap`, `Option::unwrap_or`, `Result::unwrap`) avec un second déplacement. |
| `ownership/unwrap-use-after-move` | `compile-fail` | `E-OWN-USE-AFTER-MOVE` (OWN-5.1) | compagnon du précédent : après consommation, la place est **lue** (`is_some`/`is_none`/`is_ok`), pas déplacée. |
| `ownership/move-in-loop-from-outer` | `compile-fail` | `E-OWN-USE-AFTER-MOVE` (OWN-5.6) | déplacement d'une valeur déclarée hors de la boucle : la spec nomme explicitement USE-AFTER-MOVE. |
| `ownership/ref-field-escape-fail` | `compile-fail` | `E-OWN-REF-ESCAPE` (OWN-6.1) | `struct W { r: &i32 }` : une référence non statique ne peut pas être stockée dans un champ (exemple négatif de la spec). Ce défaut du checker a été corrigé pendant la session ; le cas le fige. |
| `types/owned-string-where-str-expected-fail` | `compile-fail` | `E-TYP-MISMATCH` (TYP-8.5, TYP-13.1) | un `String` possédé passé où `&str` est attendu exige une conversion explicite ; seul `&String → &str` est implicite. Le checker l'acceptait et rustc le refusait (E0308). |
| `types/trim-string-where-str-expected-fail` | `compile-fail` | `E-TYP-MISMATCH` (STD-9.1, TYP-8.5) | `trim()` rend un `String` possédé (STD-9.1), donc `takes(line.trim())` est un mismatch ; la forme correcte est `line.trim().as_str()`. |
| `types/array-annotated-element-mismatch-fail` | `compile-fail` | `E-TYP-MISMATCH` (TYP-4) | l'annotation `[i32; 2]` ne doit pas masquer `[1, true]`. |
| `types/nested-match-tuple-payload-nonexhaustive-fail` | `compile-fail` | `E-TYP-NONEXHAUSTIVE` (EXPR-5.8) | `Some((n, true))` laisse `false` non couvert. |
| `types/nested-option-nonexhaustive-fail` | `compile-fail` | `E-TYP-NONEXHAUSTIVE` (EXPR-5.8, TYP-10.4) | `Some(Some(_))` + `None` laisse `Some(None)` non couvert. |
| `types/nested-enum-payload-nonexhaustive-fail` | `compile-fail` | `E-TYP-NONEXHAUSTIVE` (EXPR-5.8) | `E::A(Some(_))` + `E::B` laisse `E::A(None)` non couvert. |

### 6.2 Contrôles positifs (le correctif ne doit pas sur-rejeter)

| Cas | Attendu (spec) | Pourquoi |
|---|---|---|
| `types/str-param-accepts-as-str-pass` | build OK | un paramètre `&str` accepte une vue non statique (`" a ".trim()` puis `as_str()`). Garde contre un correctif global `&'static str` qui casserait les arguments non statiques. |
| `types/nested-option-exhaustive-pass`, `types/nested-enum-exhaustive-pass` | build OK | les motifs imbriqués **exhaustifs** doivent rester acceptés. |
| `types/array-element-copy-projection-pass` | build OK | projection `Copy` (`a[0].1`) : ne pas la confondre avec un déplacement. |
| `types/array-element-copy-read-pass` | build OK | éléments `Copy` : `let x = a[0]; a[1]` reste légal ; contrôle pour que le correctif des déplacements multi-éléments n'interdise pas les lectures `Copy`. |
| `types/string-as-str-explicit-ok` | build OK | conversions explicites acceptées : `takes(&owned)`, `takes(owned.as_str())`, `takes(line.trim().as_str())`. Contrôle du durcissement `String → &str`. |
| `ownership/struct-partial-then-other-pass` | build OK | déplacer `d.a` puis lire `d.b` est légal (OWN-5.3). |
| `ownership/borrow-block-argument-ok`, `ownership/borrow-if-argument-ok` | build OK | emprunt temporaire construit par bloc (`&{...}`, `{ &v }`) ou par `if` en argument (EXPR-5.1, EXPR-7.10, OWN-6.4). |
| `ownership/tuple-element-move-ok`, `ownership/unwrap-move-ok`, `ownership/static-str-in-vec-ok`, `ownership/generic-str-instr-ok` | build OK | déplacements/usages légaux des familles ci-dessus. |

### 6.3 Familles déjà couvertes, non dupliquées

* `&str` stocké/retourné : `ownership/static-str-stored` (étendu).
* déplacement d'élément de tableau : `ownership/partial-move-tuple-ok` (étendu).
* tableaux hétérogènes déjà corrigés : `types/array-heterogeneous-fail`
  (négatif) plus le nouveau `types/array-heterogeneous-string-fail`
  (`String` vs `&str`).
* `run/stats` a été **migré** de `parse_f64(trimmed)` à
  `parse_f64(trimmed.as_str())` (le `trim()` rend un `String`, STD-9.1) : c'est
  exactement l'exemple de `spec/language/stdlib.md`, et aucune sortie attendue
  n'a été modifiée. Sans cette migration, le durcissement `String → &str` aurait
  rendu le cas rouge pour une raison légitime.

### 6.4 Déplacements multi-éléments de tableau (ajoutés après levée du hold)

Quatre formes de déplacements multi-éléments étaient `check`-pass/`build`-fail
à un état intermédiaire ; le correctif codegen (P0-B) a été livré et vérifié.
Elles sont désormais des cas `compile-pass` build-vérifiés, ainsi que l'index
dynamique :

| Cas | Vérifie |
|---|---|
| `ownership/array-two-elements-move-pass` | `let x = a[0]; let y = a[1];` sur `[String; 2]` : deux déplacements par composant. |
| `ownership/array-projection-two-moves-pass` | `a[0].0` + `a[1].0` sur `[(String, i32); 2]` : projection puis déplacement par composant. |
| `ownership/array-move-then-other-element-pass` | déplacer `a[0]` puis lire `a[1]` (et le `x` déplacé) reste légal. |
| `ownership/array-move-into-builtin-arg-pass` | `v.push(a[0])` : déplacement d'un élément dans un argument prédéfini. |
| `ownership/array-dynamic-index-move-pass` | `a[i]` : index dynamique produisant un élément par déplacement (abaissé via un itérateur `nth`). |

Contrôles maintenus verts : `types/array-element-copy-read-pass`,
`types/array-element-copy-projection-pass`,
`ownership/struct-partial-then-other-pass`,
`ownership/tuple-element-move-ok`, et le déplacement d'un seul élément
(`ownership/partial-move-tuple-ok`, étendu).

### 6.5 Cas routés : verticale de stockage fichier (`file-store`)

Trois cas couvrent la verticale de stockage adossée à des fichiers (pas de SQL,
pas de service), avec le paquet de `examples/file-store` copié **verbatim**
(`ravum.toml` + `src/main.rv`, sans `.ravum/`, `store/` ni `ravum.lock`) :

| Cas | Mode | Vérifie |
|---|---|---|
| `run/file-store-put` | `run-pass` | `put greeting` avec `hello world` sur stdin : `stored greeting v1`, et les fichiers `index` (`greeting|1`) et `greeting.v1` (`hello world`) existent avec le bon contenu. |
| `run/file-store-missing-key` | `run-fail` | `get ghost` : `Err` typé, pas un panic ; exit 1 et `error: no such key: ghost` sur stderr. |
| `security/file-store-key-traversal` | `run-fail` | `get ../etc/passwd` : la frontière clé → nom de fichier rejette la traversée **avant** tout appel fs ; `invalid key`, exit 1. |

Ce sont des modes à build implicite (`run` compile puis exécute) : ils comptent
dans le périmètre `check ⇒ build` comme `implicit`.

### 6.6 Round 3 — axes nouvellement sondés

Le script `scripts/p0a-probe-round3.ps1` sonde systématiquement l'invariant
`check` ⇒ `build` sur **47 programmes × 11 axes** qui n'étaient pas couverts par
les rounds 1–2. Il écrit `target/p0a-probe-round3.json` (statut, codes Ravum et
rustc, sorties, extrait du programme), échoue (exit 1) sur toute divergence,
et refuse (exit 2) une matrice contenant un programme que le `check` rejette.

| Axe | Programmes | Cas ajoutés au corpus (verts check+build) |
|---|---|---|
| `match-move` (payload d'enum/`Option`, patterns imbriqués) | 4 | `ownership/match-option-string-move-pass`, `ownership/match-enum-struct-payload-move-pass` |
| `nested-aggregate` (struct dans struct/tuple, struct dans array) | 6 | `ownership/nested-struct-field-move-pass`, `ownership/nested-tuple-in-struct-move-pass` |
| `array-borrow` (`&`/`&mut` sur élément, index constant/dynamique) | 5 | `ownership/array-element-shared-borrow-pass`, `ownership/array-element-mut-borrow-pass`, `ownership/array-element-dynamic-mut-borrow-pass` |
| `reinit-after-move` (réaffectation de champ/élément) | 4 | `ownership/struct-field-reinit-after-move-pass`, `ownership/array-element-reinit-after-move-pass` |
| `loop-move` (`while`/`loop`/`for`, valeur locale du corps) | 4 | `ownership/while-body-local-move-pass`, `ownership/for-vec-body-element-move-pass` |
| `if-move` (retours multiples déplaçant un élément) | 3 | `ownership/if-branch-array-element-move-pass`, `ownership/if-else-same-array-element-pass` |
| `method-receiver` (`self`/`&self`/`&mut self`) | 7 | `ownership/method-self-consumes-struct-pass`, `ownership/method-mut-self-receives-element-pass` |
| `generic-array` (`T` non-`Copy` avec tableaux) | 4 | `ownership/generic-array-element-move-pass` |
| `enum-array-payload` | 3 | `0` — la forme « déplacer un élément hors d'une liaison de payload de `match` » est rouge (F2, section 9) |
| `nested-unwrap` (`Option<Vec<T>>`, `Option<Option<T>>`) | 5 | `ownership/option-vec-unwrap-pass`, `ownership/option-option-unwrap-pass` |
| `static-str-array` (`&'static str` élément de tableau) | 2 | `types/array-static-str-element-compare-pass` (comparaison en instruction) |

Limites tracées du sondage :

* **`for x in [T; N]` n'est pas exprimable en Core 1.0** : `for` itère une range
  ou un `Vec<T>` possédé (EXPR-6.4) ; le `check` rejette la forme tableau. L'axe
  « `for` sur tableau » du brief est donc remplacé par `for x in Vec<T>`.
* Les formes `[&str; 2]` de **stockage** et de **retour** d'élément sont
  rejetées par le `check` (`E-OWN-REF-ESCAPE`) : hors matrice, signalées en
  section 8 (sema, routé).
* Les 4 contrôles D12 (section 9) ne sont pas dans la matrice ; au binaire
  mesuré ils construisent désormais.

Les 19 cas ajoutés sont `compile-pass` et sont donc des `build` explicites dans
le périmètre du contrat. Le script se relance avec :

```
pwsh -File scripts/p0a-probe-round3.ps1
```

## 7. État mesuré

### 7.1 Mesure finale, arbre commité (passe 7)

| Élément | Valeur |
|---|---|
| Date (Europe/Paris) | 2026-11-11, ≈14:45–14:55 |
| Commit | `c6b4537` (code `0f73738`), **arbre propre** — plus aucune mesure sur arbre de travail |
| Hôte | Windows 11 x86-64, `rustc 1.95.0`, `cargo 1.95.0` |
| Toolchain mesurée | `target\debug\ravum-lang.exe`, `ravum-lang 0.1.0 (edition core-1.0-draft.1)` — SHA-256 `5DDF8E5F602D6B657C3644BBE22584B67A655799188E5511491B8202A705C499` |
| Harnais mesuré | `target\debug\ravum-conformance.exe`, `ravum-conformance 0.1.0` (schema 1) — SHA-256 `9F21CFF01629458776E4F66ED0E0587E3A7166F184F197FC5F2014100600DF58` |
| Commande | `target\debug\ravum-conformance.exe run --verify-build --json target\final-pass2\conformance.json` |
| Code de sortie | **0** |
| Cas | `total=448 pass=447 fail=0 skip=1 not_run=0` |
| Dimension build | `perimeter=187 covered=187 verified=120 implicit=67 failed=0 environment=0 not_measured=0` |
| `build_verified` | **`true`** |
| Rapport | `target\final-pass2\conformance.json` (artefact local non versionné) |

Le seul `SKIP` reste `security/dependency-absolute-unix`
(`skip_on: ["windows"]`), un `compile-fail` **hors** périmètre build : il est
rapporté `SKIP` (jamais `PASS`) et n'affecte pas `build_verified`, qui porte sur
les 187 cas du périmètre.

### 7.2 Historique des runs de la passe (conservé pour ne pas relire le vert comme un hasard)

| Run | Base | Cas | Divergences |
|---|---|---|---|
| pré-passe 7 | `5d88ecb` | 386 | — |
| round 1–2 | `5d88ecb` | 424 | 0 FAIL (2 divergences trouvées hors corpus, corrigées) |
| round 3, arbre en cours | `ab51b17` + arbre | 446 | 0 FAIL / 4 divergences hors corpus (F1, F2) |
| round 3, commit seul | `aac9743` | 443 | 0 FAIL |
| **final** | **`c6b4537`** | **448** | **0 FAIL** |

Les échecs de lien (`LNK1181`) vus plus tôt dans la session étaient des artefacts
de longueur de chemin Windows sur la disposition *content-addressed*
(`.ravum\build\<id>\<pid>-0\out\…`), corrigés par un préfixe temporaire court et
une copie sans `.ravum/` ; ils sont tracés ici pour que la mesure verte ne soit
pas lue comme un hasard.


## 8. Divergences spec ↔ implémentation rencontrées pendant P0-A (hors build)

Ces points ont été observés en construisant le corpus. Ils ne sont **pas**
ajoutés comme cas pour éviter d'attribuer au contrat build un défaut de
checker ; ils sont signalés au propriétaire du checker (P0-F) :

1. `fn m(t: (String, i32)) -> i32 { let s = t.0; return t.1; }` est marqué
   NÉGATIF `E-OWN-PARTIAL-MOVE` dans l'exemple de `ownership.md` §5, alors que
   la règle OWN-5.4 elle-même dit que les autres éléments restent lisibles. Le
   checker suit la règle (accepte) ; l'exemple de la spec est contradictoire.
2. `let t = s.text;` où `s: S { text: &'static str }` est POSITIF (OWN-6.2,
   `&'static str` Copy et stockable) mais le checker émet
   `E-OWN-REF-ESCAPE`. Par conséquent l'extension du cas
   `ownership/static-str-stored` n'ajoute **pas** ce stockage : elle garde les
   usages dans l'instruction, et le cas reste un contrôle honnête du périmètre.
3. Élément de tableau `&'static str` (round 3) : `let s = a[1];` sur
   `let a = ["zero", "one"];` et `fn f(a: [&str; 2]) -> &str { return a[1]; }`
   sont rejetés par le `check` avec `E-OWN-REF-ESCAPE`, alors qu'OWN-6.2 (MUST)
   fait de `&'static str` une valeur Copy, stockable et retournable ; la
   comparaison en position d'instruction (`a[0] == "zero"`) est, elle, acceptée.
   Signalé à `sema-owner` ; si l'ambiguïté est normative, elle doit finir en
   dossier de décision, pas en changement silencieux. Non ajouté au corpus.

Un troisième point initialement observé — `struct W { r: &i32 }` accepté alors
que la spec exige `E-OWN-REF-ESCAPE` — a été **corrigé pendant la session** par
P0-F et est désormais figé par le cas négatif
`ownership/ref-field-escape-fail`.

## 9. Divergences connues — état au commit `7696e2a`

Le corpus build-vérifié **ne prouve pas** l'absence de divergences en dehors de
lui. Cette section liste les formes `check`-pass / `build`-fail connues.

**Les cinq formes (a)–(e) de cette section sont fermées** (commit `0f73738`).
Les **quatre formes restantes de la passe 7** — index dynamique dans un tableau
imbriqué, racine de chaîne qui n'est pas une place, plusieurs accès dans un bras
de `match` en corps d'expression, récepteur *by value* d'une méthode builtin —
sont fermées par `5268838`. Les **deux formes signalées à la revue de la passe
8** — plusieurs descentes de projection sur des éléments différents du même
tableau, base relue après la descente — sont fermées par `d693162`. Toutes
passent `check` **et** `build`, ont un test `rustc`-vérifié dans
`crates/ravum-lang-codegen/tests/backend_audit.rs` et un cas de corpus
`conformance/cases/run/array-move-*`. Les reproducers restent ci-dessous comme
archive de la mesure rouge et de la preuve de fermeture.

**Deux formes restent ouvertes** au commit `7696e2a`, toutes deux `check` exit 0 /
`build` exit 1 avec un `E0508` **bruyant** (jamais de miscompile silencieux,
aucune sémantique modifiée). Elles ne sont pas dans le corpus : les ajouter en
`compile-fail` serait faux (la spec les accepte) et en `compile-pass` rendrait la
porte rouge pour un élément ouvert déjà identifié. Aucun test ne verrouille le
défaut.

| Forme restante | Repro | Raison |
|---|---|---|
| descentes de projection dans **deux branches distinctes** (`if`/`else`) | §9 ter (a) | le hiss partagé exige une instruction du bloc du binder qui précède *tous* les usages ; deux branches n'ont pas d'ancêtre commun qui les domine |
| deux descentes dans un **même bloc imbriqué** | §9 ter (b) | le plan ne suit pas l'identité de bloc : il ne peut pas prouver qu'un ancêtre domine un usage d'une autre branche, donc il n'ouvre pas de groupe pour un bloc imbriqué |

C'est la raison pour laquelle `P0-A` reste `PARTIAL` : la porte est mesurée et
bloquante sur son périmètre (**453 cas, `perimeter 192`, `build_verified: true`**
au commit `7696e2a`), mais elle ne couvre pas ces deux formes.

### (a) chaîne de tableaux imbriqués

```rv
fn f(a: [[String; 2]; 2]) -> String {
    return a[0][1];
}

fn main() {
}
```

Mesuré au binaire `5816ACDD…` (13:32) : `check` exit 0 ; `build` échoue,
rustc `E0508`. Construit désormais avec `923D413D…` (13:49:46).

### (b) tableau atteint par projection de champ ou de tuple

```rv
struct H {
    items: [String; 2],
}

fn f(h: H) -> String {
    return h.items[0];
}

fn main() {
}
```

Mesuré : `check` exit 0 ; `build` échoue, rustc `E0508`. La forme `t.0[0]` sur
`([String; 2], i32)` a la même allure et le même statut (mesuré). Les deux
construisent désormais avec `923D413D…` (correctif D12).

### (c) liaison de tableau dont le nom est ombré dans le corps

```rv
fn f(a: [String; 2]) -> String {
    let x = a[0];
    {
        let a = 2;
        let b = a;
    }
    return x;
}

fn main() {
}
```

Mesuré : `check` exit 0 ; `build` échoue, rustc `E0508`. Le planificateur
abandonne conservativement (pas d'abaissement par place) ; l'échec reste
bruyant, jamais silencieux. Construit désormais avec `923D413D…` (correctif D12).

### (d) récepteur de méthode par valeur sur un élément de tableau — F1 (OPEN)

```rv
struct H {
    s: String,
}

impl H {
    fn take(self) -> String {
        return self.s;
    }
}

fn f(a: [H; 2]) -> String {
    return a[0].take();
}

fn main() {
}
```

Mesuré avec `923D413D…` : `check` exit 0 ; `build` échoue, rustc `E0508`
(`cannot move out of type [H; 2]`) ; le Rust généré contient
`return a[1usize].take();`. Même échec avec un index dynamique (`a[i].take()`).
Contrôles qui construisent : la même méthode sur un élément de tuple
(`t.0.take()`), sur un champ de struct (`o.h.take()`), et une méthode `&self`
sur un élément de tableau. Cause : l'abaissement place-move des tableaux est
appliqué aux positions `let`/`return` mais pas au **récepteur** d'un appel de
méthode `self`. Statut OPEN, routé vers codegen-pipeline.

### (e) déplacement d'élément depuis une liaison de payload de `match` — F2 (régression)

```rv
enum E {
    Items([String; 2]),
    Empty,
}

fn f(e: E) -> String {
    match e {
        E::Items(a) => {
            return a[1];
        }
        E::Empty => {
            return String::from("empty");
        }
    }
}

fn main() {
}
```

Mesuré avec `923D413D…` : `check` exit 0 ; `build` échoue, rustc `E0508` sur le
Rust généré `return a[1usize];`. La forme construisait avec le binaire du 13:32
et lors du round-3 de 13:47 ; elle casse depuis l'arbre de travail non commité
de codegen-pipeline (13:49:40). C'est une **régression collatérale** du travail
D12, routée ; elle reste hors corpus jusqu'au correctif. Le cas
`ownership/enum-array-payload-move-pass` a été retiré pour cette raison.

## 9 ter. Les deux formes encore ouvertes (mesurées le 2026-11-11)

Reproduites sur `7696e2a` (Windows 11 x64, rustc 1.95.0) : `check` exit 0,
`build` exit 1, `error[E0508]: cannot move out of type
[([String; 2], i32); 2], a non-copy array`.

### (a) descentes de projection dans deux branches distinctes

```rv
fn f(c: bool, a: [([String; 2], i32); 2]) -> String {
    if c { let x = a[0].0[1]; return x; } else { let y = a[1].0[0]; return y; }
}

fn main() {
}
```

Le code généré conserve `let x: String = a[0usize].0[1usize];` (et
symétriquement `a[1usize].0[0usize]`) : le hiss partagé n'est ouvert que pour des
descentes en instructions **directes** du bloc du binder, parce qu'il doit être
émis avant le premier usage. Aucune instruction ne précède les deux branches.

### (b) deux descentes dans un même bloc imbriqué

```rv
fn f(a: [([String; 2], i32); 2]) -> String {
    if true { let x = a[0].0[1]; let y = a[1].0[0]; return x; }
    return String::from("none");
}

fn main() {
}
```

Un ancêtre valide existerait (première instruction du bloc `if`), mais le plan ne
suit pas l'identité de bloc et ne peut donc pas prouver qu'il domine les usages ;
le formulaire indexé reste et rustc le rejette.

Fermer ces deux formes demande de suivre une identité de bloc dans le *walker*
puis de valider la dominance (estimation : une quinzaine de lignes plus les
tests) ; ce n'était pas dans le budget de la passe 8. Les deux formes sont
bruyantes : jamais de valeur fausse, seulement un refus de `build` après un
`check` accepté.

## 10. Interprétation

Un run vert signifie : **sur le corpus listé, à ce commit et avec cette
toolchain, aucun cas accepté dont le `check` passe n'a échoué au `build`, et
tous les cas du périmètre ont été réellement mesurés**. Ce n'est ni une preuve
formelle, ni une garantie sur d'autres programmes, ni une promesse de
non-régression future : c'est une observation reproductible, outillée et
bloquante.

Mesure du 2026-11-11, round 3 (section 7) : `build_verified=true` sur l'arbre
de travail (446 cas, 185/185 du périmètre) et sur le corpus du commit seul
(443 cas, 182/182, 0 échec, 1 SKIP hors périmètre). Les 19 cas ajoutés sont
verts en `check` **et** en `build`. Le corpus ne montre aucune divergence sur
son périmètre ; les formes (a)–(c) de la section 9 construisent avec le binaire
mesuré, mais restent hors corpus jusqu'à la re-mesure du correctif commité ;
F1 (récepteur de méthode) et F2 (payload de `match`, régression) restent
ouvertes et hors corpus. Cette observation est liée au binaire `923D413D…` ;
tout rebuild de la toolchain invalide la re-mesure tant que la commande n'est
pas rejouée.
