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

## 7. État mesuré

| Élément | Valeur |
|---|---|
| Date (Europe/Paris) | 2026-11-11, ≈12:49–12:57 |
| Commit | `5d88ecb` **plus l'arbre de travail** (correctifs codegen/sema d'autres agents, non commités au moment de la mesure) |
| Hôte | Windows 11 x86-64, `rustc 1.95.0`, `cargo 1.95.0` |
| Toolchain mesurée | `target\debug\ravum-lang.exe`, `ravum-lang 0.1.0 (edition core-1.0-draft.1)` — SHA-256 `5816ACDDCE0A81A768F9CE5620CCE0C786B342058BA8D7398D77725C157F661D` (hash identique avant et après le run : un seul binaire pour tout le run) |
| Harnais mesuré | `target\debug\ravum-conformance.exe`, `ravum-conformance 0.1.0` (schema 1) — SHA-256 `7DF6345C68284731E3D72BFF44B31639BE88EC4325C4548535ED88AA4BE4CCCF` |
| Commande | `target\debug\ravum-conformance.exe run --verify-build --json target\final-conformance.json` |
| Code de sortie | **0** |
| Cas | `total=424 pass=423 fail=0 skip=1 not_run=0` |
| Dimension build | `perimeter=163 covered=163 verified=101 implicit=62 failed=0 environment=0 not_measured=0` |
| `build_verified` | **`true`** |
| Rapport | `target\final-conformance.json` (artefact local non versionné) |

C'est la commande de la porte finale demandée par le Lead. Les hashes des deux
binaires ont été relevés **avant et après** le run et sont identiques : aucun
rebuild n'a eu lieu pendant la mesure. Les runs intermédiaires de la session ont
dû, eux, utiliser des copies figées (`target\p0a-conformance-bin\`) parce que
l'arbre était édité et rebuildé en parallèle par les autres agents ; la porte
finale ci-dessus est celle d'un arbre stabilisé.

Le seul `SKIP` est `security/dependency-absolute-unix`
(`skip_on: ["windows"]`) : c'est un `compile-fail`, **hors** du périmètre
build. Il est rapporté `SKIP` (jamais `PASS`) et n'affecte pas
`build_verified`, qui porte sur les 163 cas du périmètre : 101 `build`
explicites (`compile-pass` + `fmt-idempotent`) et 62 builds implicites
(`run-pass`/`run-fail` exécutés).

Historique honnête des runs intermédiaires de cette session :
`ownership/array-element-projection-move-pass` a d'abord échoué au lien
(`LNK1181`) parce que le harnais copiait une arborescence dont le chemin
dépassait `MAX_PATH` ; après correction du préfixe temporaire, une mesure
complète a montré 28 échecs du même type dus à la **disposition de build
content-addressed** du codegen (chemin `.ravum\build\<64 hex>\<pid>-0\out\…`),
puis la version finale du harnais (préfixe court + copie sans `.ravum/`) a
donné le run vert ci-dessus. Ces échecs étaient des artefacts de longueur de
chemin Windows, pas des divergences sémantiques Ravum ; ils sont tracés ici
pour que la mesure verte ne soit pas lue comme un hasard.


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

Un troisième point initialement observé — `struct W { r: &i32 }` accepté alors
que la spec exige `E-OWN-REF-ESCAPE` — a été **corrigé pendant la session** par
P0-F et est désormais figé par le cas négatif
`ownership/ref-field-escape-fail`.

## 9. Divergences connues — statut OPEN (hors corpus, non attendues comme échecs)

Trois formes sont `check`-pass / `build`-fail à la toolchain mesurée. Elles sont
**licites** selon OWN-5.3/OWN-5.4 (MUST) : l'échec est un `E0508` **bruyant** de
rustc (jamais un miscompile silencieux, aucune sémantique modifiée). Elles ne
sont **pas** dans le corpus : les ajouter en `compile-fail` serait faux (la spec
les accepte) et en `compile-pass` rendrait la porte rouge pour un élément ouvert
déjà identifié. Disposition : **« ouvert, nécessite un abaissement général des
déplacements par place ; non couvert par cette porte »**. C'est le complément
honnête du dénominateur : le corpus build-vérifié **ne prouve pas** l'absence de
divergences en dehors de lui.

### (a) chaîne de tableaux imbriqués

```rv
fn f(a: [[String; 2]; 2]) -> String {
    return a[0][1];
}

fn main() {
}
```

Mesuré : `check` exit 0 ; `build` échoue, rustc `E0508`.

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
`([String; 2], i32)` a la même allure et le même statut (mesuré).

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
bruyant, jamais silencieux.

## 10. Interprétation

Un run vert signifie : **sur le corpus listé, à ce commit et avec cette
toolchain, aucun cas accepté dont le `check` passe n'a échoué au `build`, et
tous les cas du périmètre ont été réellement mesurés**. Ce n'est ni une preuve
formelle, ni une garantie sur d'autres programmes, ni une promesse de
non-régression future : c'est une observation reproductible, outillée et
bloquante.

Mesure du 2026-11-11 (section 7) : `build_verified=true`, 424 cas,
163/163 cas du périmètre construits, 0 échec, 1 SKIP hors périmètre ; le corpus
ne montre aucune divergence sur ce périmètre, et les trois divergences connues
de la section 9 restent **ouvertes et hors corpus**. Cette observation est liée
au binaire `5816ACDD…` ; tout rebuild de la toolchain invalide la re-mesure tant
que la commande n'est pas rejouée.
