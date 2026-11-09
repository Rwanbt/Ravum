# Conformance Ravum Core 1.0

Harnais déclaratif et corpus de cas pour Ravum Core 1.0.

> **Statut : corpus candidat.** Chaque cas est une **exigence** dérivée de
> `docs/architecture/RAVUM-CORE-DESIGN.md` et de `spec/language/`, pas un
> enregistrement du comportement actuel. Le compilateur étant en cours
> d'implémentation, la majorité des cas échouent aujourd'hui : c'est le
> résultat honnête, pas une régression du harnais.

Le harnais juge la toolchain, il ne la remplace pas : il ne prétend jamais
`PROVED`, ne masque aucun cas, et n'invente aucun succès. Un cas dont le
résultat ne peut pas être établi est `NOT_RUN`, jamais `PASS`.

## 1. Lancer

```sh
# Construire la toolchain (le binaire est cherché dans target/debug/)
cargo build -p ravum-lang-cli

# Inventaire déterministe du corpus
cargo run -p ravum-conformance -- list

# Exécution complète ; rapport JSON
cargo run -p ravum-conformance -- run --json target/conformance-report.json

# Sous-ensemble, toolchain explicite, timeout
cargo run -p ravum-conformance -- run \
  --filter run/ \
  --toolchain target/debug/ravum-lang \
  --timeout-ms 5000

# Tests du harnais lui-même
cargo test -p ravum-conformance
```

Options de `run` : `--cases DIR`, `--filter SUBSTR`, `--json OUT`,
`--toolchain PATH`, `--timeout-ms N` (1..=3 600 000, défaut 10 000),
`--keep-temp` (conserve les copies isolées pour l'inspection).
`list` accepte `--cases` et `--filter`.

Résolution de la toolchain, dans cet ordre :

1. `--toolchain PATH` ;
2. variable d'environnement `RAVUM_LANG` ;
3. `<racine du dépôt>/target/debug/ravum-lang` (`ravum-lang.exe` sous Windows).

La racine du dépôt est trouvée en remontant depuis le répertoire courant jusqu'au
`Cargo.toml` qui déclare `[workspace]`. Si le binaire est absent, chaque cas non
`SKIP` devient `NOT_RUN` avec la raison exacte — jamais `PASS`.

## 2. Codes de sortie

| Code | Signification |
|---|---|
| `0` | au moins un cas exécuté, aucun `FAIL`, aucun `NOT_RUN` |
| `1` | au moins un `FAIL` |
| `2` | usage/configuration invalide, corpus introuvable ou rapport non écrit |
| `3` | aucun `FAIL` mais au moins un `NOT_RUN` (toolchain ou environnement absent) |

Une exécution entièrement filtrée (`total=0`) est un succès explicite : le
filtre est une demande de l'utilisateur. En revanche, un corpus **non filtré**
vide est une erreur de configuration (code 2), jamais un vert silencieux.

## 3. Statuts

| Statut | Sens | Preuve |
|---|---|---|
| `PASS` | toutes les assertions du mode sont vraies | `expected`/`actual` + sorties |
| `FAIL` | au moins une assertion est fausse, ou le cas est invalide, ou la toolchain crashe/timeout | diff des codes, `expected`/`actual`, stdout/stderr |
| `SKIP` | le cas est explicitement inapplicable à cet OS (`skip_on`) ou une fixture d'environnement (symlink) ne peut pas être créée | raison explicite |
| `NOT_RUN` | aucun binaire de toolchain exécutable | raison explicite, compté comme non exécuté |

Un cas invalide (JSON malformé, champ inconnu, `case.json` absent, id dupliqué,
taxonomie inconnue, fichier parasite) est **toujours** un `FAIL` : il apparaît
dans `list` et dans le rapport. Un cas ne peut pas « disparaître » du corpus.

## 4. Arborescence

```
conformance/cases/<taxonomie>/<cas>/case.json
conformance/cases/<taxonomie>/<cas>/ravum.toml      # package sous test
conformance/cases/<taxonomie>/<cas>/src/main.rv     # ou src/lib.rv
conformance/cases/<taxonomie>/<cas>/...             # fixtures du projet
```

Taxonomies (liste fermée, ordre du rapport) :
`lexer, parser, resolve, types, ownership, effects, run, unsupported, modules,
packages, fmt, cli, security`.

Un fichier directement dans `conformance/cases/` ou dans une taxonomie, un
répertoire de taxonomie inconnu, ou un répertoire de cas sans `case.json` sont
des cas invalides (`FAIL`).

## 5. `case.json`

Champs requis :

| Champ | Type | Sens |
|---|---|---|
| `id` | string | identifiant unique, lisible (`run/fibonacci`) ; ≤ 200 octets |
| `mode` | string | `compile-pass`, `compile-fail`, `run-pass`, `run-fail`, `fmt-idempotent`, `cli` |

Champs optionnels :

| Champ | Type | Défaut | Sens |
|---|---|---|---|
| `project` | string | `"."` | racine du package à compiler, relative au cas, sans `..` ni absolu |
| `expect_codes` | `[string]` | `[]` | codes `E-*` exactement attendus ; **obligatoire** pour `compile-fail` |
| `args` | `[string]` | `[]` | args du programme (`run-*`, après `--`) ou de la commande (`cli`) |
| `stdin` | string | `""` | entrée standard fournie |
| `stdout_contains` | string | aucun | sous-chaîne exacte attendue sur stdout |
| `stderr_contains` | string | aucun | sous-chaîne exacte attendue sur stderr ; **obligatoire** pour `run-fail` |
| `exit_code` | integer | selon le mode | code de sortie attendu (0..=255) |
| `skip_on` | `[string]` | `[]` | `windows`, `linux`, `macos` |
| `unsupported` | bool | `false` | marque un cas d'exclusion Core 1.0 ; exige `compile-fail` + un code `E-UNSUPPORTED-*` |
| `notes` | string | `""` | justification/limite ; affichée par `list` |
| `expect_files` | `[string]` | `[]` | fichiers (relatifs au projet) qui doivent exister après l'exécution |
| `expect_file_contains` | `[{path, contains}]` | `[]` | contenu attendu d'un fichier après l'exécution |
| `expect_json_schema_version` | integer | aucun | au moins un objet JSON de stdout doit porter ce `schema_version` ; toute ligne non-JSON est un échec |
| `timeout_ms` | integer | timeout global | timeout spécifique au cas |
| `fixtures` | `[{kind, path, target}]` | `[]` | liens symboliques déclaratifs : `kind` ∈ `symlink-dir`, `symlink-file` ; `path` relatif sûr, `target` relatif |

Tout champ **inconnu**, toute clé **dupliquée**, tout `expect_codes` hors
`E-[A-Z0-9-]+`, tout `skip_on` inconnu, tout chemin absolu ou contenant `..`
(`project`, `expect_files`, fixtures, args CLI) est une erreur de cas — pas un
avertissement. Cette rigidité est volontaire : un champ mal orthographié
(`stdout_contain`) ne doit pas rendre un cas silencieusement vacuux.

### 5.1 Modes

| Mode | Commande exécutée | Assertions |
|---|---|---|
| `compile-pass` | `check <project> [args…]` | exit 0 |
| `compile-fail` | `check --json <project> [args…]` | exit **1**, **un seul** objet JSON racine (`DIAG-7.2`) avec `schema_version: 1` et `diagnostics[]`, **ensemble exact** de codes `E-*` = `expect_codes` |
| `run-pass` | `run <project> [-- args…]` | exit attendu, `stdout_contains`/`stderr_contains`, fichiers |
| `run-fail` | `run <project> [-- args…]` | exit attendu **et** message de panne (`stderr_contains`) |
| `fmt-idempotent` | `fmt <project>` ×2 puis `check <project>` | deux passages byte-identiques sur tous les `.rv`, puis check exit 0 |
| `cli` | `args` tels quels (cwd = `project`, `{project}`/`{case}` → `.`) | exit attendu, contains, fichiers, JSON schema |

`compile-fail` exige un `expect_codes` non vide : « exit 1, peu importe
pourquoi » n'est pas un test de conformité. Un code attendu manquant **et** un
code inattendu sont tous deux des `FAIL`, avec diff lisible.

`run-fail` exige un message (`stderr_contains` non vide) : le code de sortie
seul ne distingue pas un panic contrôlé d'un crash.

Un `SKIP` n'est jamais un `PASS` ; un `NOT_RUN` n'est jamais un `PASS`.

### 5.2 Isolation et exécution

Pour chaque cas, le harnais :

1. copie récursivement le répertoire du cas dans un répertoire temporaire
   unique (aucune écriture dans le corpus) ;
2. refuse les liens symboliques présents dans l'arbre du cas (ils se déclarent
   via `fixtures`) ;
3. matérialise les fixtures ; si le système refuse la création d'un lien
   (privilège Windows), le cas est `SKIP` avec la raison ;
4. vérifie que `project` reste sous la copie isolée ;
5. exécute avec stdout/stderr/stdin redirigés vers des fichiers (pas de pipe :
   une sortie volumineuse ne peut pas bloquer le processus) et un timeout qui
   tue l'enfant ; un timeout est un `FAIL`, jamais un `PASS` ;
6. capture au plus 256 Kio par flux ; une troncature est signalée dans la preuve.

Aucun réseau, aucune dépendance externe, aucun parallélisme : les cas
s'exécutent séquentiellement dans l'ordre taxonomie → chemin.

## 6. Rapport JSON (`schema_version: 1`)

```json
{
  "schema_version": 1,
  "toolchain": "target/debug/ravum-lang.exe",
  "toolchain_version": "ravum-lang 0.1.0 (edition core-1.0)",
  "started_at": "2026-11-09T14:25:11Z",
  "cases": [
    {
      "id": "run/hello",
      "taxonomy": "run",
      "mode": "run-pass",
      "status": "pass",
      "duration_ms": 42,
      "expected": "run exit 0; stdout contains \"hello, ravum\\n\"",
      "actual": "run: exit 0; stdout 13 B; stderr 0 B",
      "detail": ""
    }
  ],
  "summary": { "total": 1, "pass": 1, "fail": 0, "skip": 0, "not_run": 0 }
}
```

`status` ∈ `pass`, `fail`, `skip`, `not_run`. `cases` est dans l'ordre
déterministe du corpus. **Aucun chemin utilisateur absolu n'est écrit dans le
JSON** : racine du dépôt → `<repo>`, répertoire temporaire → `<tmp>`, home →
`<home>`, chemin absolu résiduel → `<path>`. Les preuves trop longues sont
tronquées avec un marqueur explicite (`... [N chars total, truncated]`).

`toolchain_version` est la sortie de `ravum-lang --version` ; elle vaut
`UNKNOWN` si la sonde échoue. `toolchain` vaut `<not found>` si le binaire est
absent.

Ce rapport est celui du **harnais**. Le rapport `--json` du **compilateur** est
un autre contrat, défini par [`spec/language/diagnostics.md`](../spec/language/diagnostics.md)
§7 : objet racine unique `{schema_version, tool, tool_version, spec_version,
command, status, exit_code, target, summary, diagnostics[]}`, un diagnostic par
entrée avec `code`, `severity`, `phase`, `message`, `primary`, `labels`,
`notes`. Le harnais exige ce schéma pour `compile-fail` (enveloppe minimale :
racine objet, `schema_version: 1`, `diagnostics[]`, `code`/`severity` par
diagnostic) et le valide **intégralement** dans `cli/check-json-schema`
(champs obligatoires, `status`, `target`, `summary`, `primary`, `labels`,
`notes`). Du JSON-lines (NDJSON) est refusé.

## 7. Politique d'ajout et de modification

1. **Tout bug ou écart observé devient un cas** (taxonomie la plus proche),
   avec le code de diagnostic attendu par la spec.
2. Un cas est une **exigence**, pas une photographie du comportement actuel :
   on n'écrit jamais un cas pour « faire vert » et on n'abaisse jamais une
   attente pour obtenir `PASS`.
3. Un changement de comportement du compilateur doit faire évoluer le corpus
   **seulement** si la spec change ; la modification doit citer la règle
   (`E-…`, `G-…`, `LEX-…`, `OWN-…`, `EFF-…`, `MOD-…`) dans `notes`.
4. Un cas `compile-fail` liste **exactement** les codes attendus. Si
   l'implémentation en produit d'autres, le cas échoue : soit le code est
   justifié (mettre à jour la spec et le corpus), soit c'est un défaut.
5. Aucun `SKIP` ne remplace un `FAIL` : `skip_on` ne sert qu'à une
   inapplicabilité de plateforme documentée (chemins absolus POSIX/Windows,
   privilège symlink).
6. Quand un code de diagnostic manque dans la spec, **on n'en invente pas** :
   le cas porte `notes: "GAP: …"` et l'écart est remonté (voir §9).

## 8. Limites explicites

- **Pas de fuzzing ici.** Le corpus est déterministe et ciblé ; le fuzz smoke
  (entrées hostiles, non-panic) relève de la gate T3 et d'un harnais dédié.
- **Pas de preuve de sûreté.** Un `PASS` d'ownership/effets signifie « le
  corpus négatif est refusé avec les codes attendus », pas un théorème.
- **Pas d'oracle indépendant** pour les flottants au-delà de la représentation
  la plus courte spécifiée (STD-7.4) ; les cas DSP n'assertent que des valeurs
  exactement représentables.
- **Une seule plateforme par exécution** : les cas `skip_on` non applicables
  sont `SKIP`, comptés comme tels.
- **Dépendance à la toolchain** : sans `ravum-lang` construit, tout est
  `NOT_RUN` (code 3). Le harnais ne compile ni ne reconstruit la toolchain.
- **Sorties volumineuses** tronquées à 256 Kio/flux ; les assertions portent
  sur les 256 premiers Kio.
- **Symlinks** : la création peut être refusée sur Windows sans privilège
  (mode développeur). Le cas est alors `SKIP`, pas `PASS`.

## 9. Inventaire et écarts de spécification observés

Corpus : **104 cas**, tous alignés sur le registre canonique
[`spec/language/diagnostics.md`](../spec/language/diagnostics.md) §3 (plus la
décision Lead pour `E-MOD-RESERVED-NAME`, absent du registre livré).

| Taxonomie | Cas | | Taxonomie | Cas |
|---|---:|---|---|---:|
| lexer | 12 | | unsupported | 8 |
| parser | 5 | | modules | 3 |
| resolve | 8 | | packages | 7 |
| types | 11 | | fmt | 2 |
| ownership | 10 | | cli | 5 |
| effects | 7 | | security | 6 |
| run | 20 | | **total** | **104** |

Écarts réels constatés (aucun n'est contourné par un faux vert) :

1. **Deux codes de la décision Lead manquent au registre livré.**
   `E-MOD-RESERVED-NAME` (noms réservés Windows) et `E-MOD-PATH-ESCAPE`
   (chemins absolus, `..` hors périmètre, profondeur) sont déclarés canoniques
   par le Lead et émis par le loader, mais `diagnostics.md` §3.3 ne liste que
   `E-MOD-PATH` (qui couvre les symlinks et les chemins hors `src/`). Les deux
   codes sont **distincts** : `E-MOD-PATH` pour un symlink de dépendance,
   `E-MOD-PATH-ESCAPE` pour un absolu/`..`/profondeur. Le corpus utilise les
   deux ; le registre doit ajouter `E-MOD-PATH-ESCAPE` et `E-MOD-RESERVED-NAME`.
2. **BOM contradictoire.** `diagnostics.md` DIAG-5.1 et `lexical.md` LEX-2.2
   disent que le BOM est refusé ; la décision Lead et le lexer canonique
   l'acceptent et l'ignorent. Le cas est `lexer/bom-accepted` (compile-pass) ;
   les deux documents doivent être corrigés.
3. **Double racine contradictoire.** `modules.md` MOD-1.8 autorise
   `src/main.rv` **et** `src/lib.rv` ; `diagnostics.md` §3.3 (E-MOD-BOTH-ROOTS,
   Q-MOD-1 résolue) les interdit. Le corpus suit le registre
   (`packages/both-roots`) ; `modules.md` doit être aligné.
4. **Noms de codes divergents entre documents et registre.** Les anciens
   documents emploient encore : `E-RES-MODULE-NOT-FOUND/AMBIGUOUS`,
   `E-RES-MAIN-MISSING`, `E-MOD-SYMLINK`, `E-MOD-MANIFEST-KEY`,
   `E-MOD-DEP-NOT-LIB`, `E-OWN-MOVE-BORROWED`, `E-EFF-UNKNOWN-GROUP`,
   `E-UNSUPPORTED-ATTRIBUTE`, `E-UNSUPPORTED-RAW-PTR`, `E-TYP-CONST-EVAL`,
   `E-UNSUPPORTED-RECURSIVE-TYPE`, et les codes `E-PARSE-*` fins de
   `grammar.md`. Le registre canonique les replie respectivement sur
   `E-MOD-MISSING/AMBIGUOUS`, `E-MOD-MANIFEST/DEPENDENCY`,
   `E-TYP-MAIN-MISSING`, `E-OWN-MOVE-OUT-OF-BORROW`, `E-EFF-UNKNOWN`,
   `E-UNSUPPORTED-ATTR/TOKEN/UNSAFE`, `E-TYP-CONST/RECURSIVE`,
   `E-PARSE-EXPECTED/UNEXPECTED/UNCLOSED/DEPTH/TOO-LARGE` (voir aussi
   l'écart 1 pour les codes de chemins). Les documents doivent citer les codes
   du registre.
5. **Avertissements fermés.** `diagnostics.md` DIAG-1.6 limite les `W-*` à
   `W-RES-UNUSED-IMPORT` et `W-OWN-UNUSED-VARIABLE` ; `effects.md`,
   `lexical.md` et `modules.md` mentionnent encore `W-EFF-*`, `W-FMT-*`,
   `W-MOD-LOCK-STALE`. Non canoniques.
6. **Enveloppe JSON du CLI : conforme depuis l'alignement.** `check --json`
   émet désormais `status`, `tool`, `tool_version`, `spec_version`,
   `exit_code`, `target`, `summary` et les diagnostics avec
   `primary`/`labels`/`notes` ; `cli/check-json-schema` le vérifie. Le harnais
   exige une racine unique, `schema_version: 1` et `diagnostics[]` pour tout
   `compile-fail`, et refuse le NDJSON.
7. **`E-LEX-INT-SUFFIX` pas encore émis** : le registre le déclare canonique
   et le cas `lexer/int-suffix` l'exige, mais le lexer courant ne valide pas
   les suffixes (il les transporte). Défaut d'implémentation, pas de corpus.
   `E-LEX-NUMBER` n'a pas de cas isolé : la reprise après littéral invalide
   (LEX-11.3) n'est pas encore assez spécifiée pour figer la cascade
   lexer→parser.
8. **`E-UNSUPPORTED-DROP-IMPL`** est canonique mais Core 1.0 n'offre aucune
   syntaxe de `Drop` utilisateur (`impl Drop` est un `impl` sur type étranger,
   et les traits sont refusés) : aucun cas n'est exprimable sans préciser la
   reconnaissance syntaxique attendue.
9. **`effects.md` §4** : l'exemple « profil pure positif » contient `assert`,
   alors que `panic` est interdit sous `pure` (EFF-4.2). Le Lead a confirmé
   que l'exemple est faux ; le corpus n'encode que des scalaires sans panic
   (`effects/pure-main-ok`).
10. **Exemples vs grammaire/stdlib** (les exemples ont servi de source aux cas
    `run`) : `test fn … -> Result<(), Error>` est absent de `G-3.6` ;
    `for line in lines` (itération sur `Vec`) est absent de `G-6.24` (qui ne
    prévoit que `for x in a..b`) ; `Vec::with_capacity`, `String::starts_with`,
    `String::lines`, `String::trim`, `String::to_uppercase` et `.to_string()`
    sont utilisés par les exemples alors que `stdlib.md` §8/§9 les déclare
    inexistants ; les constructeurs `Some/None/Ok/Err` non qualifiés et les
    coercitions `&String → &str` / `&[T; N] → &[T]` sont admis par le Lead mais
    contredits par `G-6.41`/`TYP-13.1`. Les cas suivent la consigne du Lead
    (exemples normatifs) ; la spec doit être alignée.
11. **Divergences d'implémentation observées** (relevées par le corpus, à
    corriger côté compilateur, pas côté corpus) : `1e9` lu comme entier à
    suffixe `e9` (`E-PARSE-BAD-SUFFIX` au lieu d'un flottant) ; codes émis
    `E-MOD-DEPENDENCY` pour un `..` qui sort du périmètre (attendu
    `E-MOD-PATH-ESCAPE`) ; `E-TYP-CONST-EVAL` / `E-UNSUPPORTED-RECURSIVE-TYPE`
    / `E-TYP-METHOD` là où le registre impose `E-TYP-CONST` / `E-TYP-RECURSIVE`
    / `E-RES-UNKNOWN` ; `use-after-move` classé `E-OWN-DOUBLE-MOVE` (3 cas) ;
    affectation d'une liaison immuable acceptée ; champ privé non refusé en
    `E-RES-PRIVATE` ; assignation acceptée dans une condition ; `#[...]`
    produit un `E-LEX-CHAR` parasite ; recovery parseur incomplète (un seul
    diagnostic sur deux) ; `run` échoue en codegen
    (`missing resolution fact for path`).
12. **Shorthand de champ : décision Lead non reflétée par `grammar.md`.**
    Le Lead a décidé que `Point { x, y: 0.0 }` est **accepté** en Core 1.0 ;
    `grammar.md` G-4.4, G-6.40 et la table §7 disent encore qu'il est refusé
    (`E-PARSE-EXPECTED`). Le cas `parser/field-shorthand` est `compile-pass`
    conformément à la décision ; la grammaire doit être mise à jour.

