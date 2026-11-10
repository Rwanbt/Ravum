# T2 — traçabilité des règles et divergences spec ↔ implémentation

> **Statut : `FAIL`.** Ce document n'est pas un certificat : il enregistre le
> résultat mesuré d'une passe de qualification de la spécification contre
> l'implémentation, à un SHA et sur une toolchain donnés.
>
> Un `FAIL` est ici une **information**, pas un échec de procédure : chaque ligne
> du tableau §4 est un cas de conformance **exécutable et reproductible** qui
> échoue parce que le compilateur ne fait pas ce que la spécification écrit. Le
> cas reste fidèle à la spécification ; aucun `expect_codes` n'a été aligné sur
> le comportement observé.

| Élément | Valeur |
|---|---|
| Branche | `feat/lang-core-1.0-rc` |
| Toolchain | `rustc 1.95.0`, `cargo 1.95.0`, Windows 11 x86-64 |
| Date de la passe | 2026-11-09 (Europe/Paris) |
| Corpus | **313 cas** (`conformance/cases/`) |
| Résultat corpus | **312 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** |
| Règles de `features.md` | **188** |
| Règles tracées vers ≥ 1 cas existant | **187 / 188** |
| Règles satisfaisant le critère strict §2 | **179 / 188** |
| Rapport machine | `target/conformance-after-t2.json` |

## 1. Méthode

Quatre lots disjoints de règles ont été traités en parallèle (§17 du mandat),
chacun produisant des répertoires de cas **neufs** et n'écrasant aucun cas
existant :

| Lot | Règles | Nouveaux cas | Taxonomies |
|---|---|---|---|
| A | `F-SYN-*` (20) | 40 | `parser` |
| B | `F-TYP-*` (14), `F-EXPR-*` (10) | 44 | `types` |
| C | `F-OWN-*` (8), `F-EFF-*` (3), `F-UNS-*` (12) | 32 | `ownership`, `effects`, `unsupported` |
| D | `F-STD-*` (13), `F-MOD-*` (4), `F-META-*` (4), `F-LEX-004` | 45 | `run`, `modules`, `parser`, `lexer` |

Chaque cas a été exécuté individuellement
(`cargo run -p ravum-conformance -- run --filter <taxonomie>/<cas>`) avant
d'être retenu. Le code attendu provient **de la colonne `Pass / Fail` de
`features.md`**, jamais de la sortie du compilateur.

Deux outils durables ont été ajoutés pour que la mesure soit reproductible sans
relecture manuelle :

- `tools/features_coverage.py` — parse les tableaux de `features.md`, résout
  chaque référence `cases/…` vers un répertoire réel (une référence morte est un
  `dangling`), et calcule en `--strict` le critère §2.
- `tools/triage_conformance.py` — croise le rapport `ravum-conformance` avec les
  codes documentés dans `spec/language/*.md` et signale tout code **émis mais
  non documenté**.

## 2. Critère de traçabilité mesuré

Une règle est **tracée** si elle cite au moins un cas existant. Elle est
**satisfaite au sens strict** si, en plus :

1. au moins un cas cité a un mode d'acceptation (`compile-pass`, `run-pass`,
   `cli`, `fmt-idempotent`) ;
2. si sa colonne `Pass / Fail` impose un code `E-*`, au moins un cas cité en
   `compile-fail` porte ce code dans `expect_codes`.

Le lien est **déclaré** par la spécification et vérifié ici pour l'existence, le
mode et l'accord sur les codes. Le harnais refuse tout champ inconnu dans
`case.json`, donc un cas ne peut pas déclarer lui-même la règle qu'il couvre :
la traçabilité est unidirectionnelle par construction, et ce document le dit
plutôt que de le laisser croire.

Résultat : **187/188** règles citent un cas réel, **87/188** satisfont le
critère strict. Le déficit restant n'est pas du même ordre qu'avant : il vient
des **30 divergences** ci-dessous, pas d'un manque de cas.

## 3. Ce qui a été corrigé pendant la passe

- `features.md` : quatre lignes de tableau contenaient un `|` non échappé
  (`F-SYN-014`, `F-EXPR-002`, `F-UNS-003`, `F-UNS-018`) qui décalait toutes
  leurs colonnes ; `F-OWN-016` n'avait que huit cellules pour neuf colonnes.
  Ce sont des défauts de rendu du document, pas des exigences modifiées.
- `F-LEX-001` citait `cases/lexer/bom-rejected`, **inexistant** ; le cas réel est
  `cases/lexer/bom-accepted` (le BOM est accepté, c'est la règle). Référence
  morte corrigée vers le cas qui existe.
- Aucune exigence n'a été supprimée, abaissée ou marquée `PASS` par décret.

## 4. Les divergences restantes (aucune)

| Cas | Règle(s) | Code attendu (spec) | Codes émis | Code émis non documenté |
|---|---|---|---|---|
| `parser/array-repeat-noncopy-fail` | - | `E-TYP-ARRAY-REPEAT` | `E-OWN-NOT-COPY` | `E-OWN-NOT-COPY` |
| `parser/impl-foreign-type-fail` | - | `E-RES-IMPL-SELF-TYPE` | `E-RES-IMPL-FOREIGN` | `E-RES-IMPL-FOREIGN` |
| `parser/let-pattern-fail` | - | `E-PARSE-EXPECTED` | `E-UNSUPPORTED-LET-PATTERN` | `E-UNSUPPORTED-LET-PATTERN` |
| `parser/prelude-variant-user-enum-fail` | - | `E-RES-UNKNOWN` | `E-RES-VARIANT-UNQUALIFIED` | `E-RES-VARIANT-UNQUALIFIED` |
| `parser/variant-pattern-ref-fail` | - | `E-UNSUPPORTED-TOKEN` | `E-UNSUPPORTED-REF-PAT` | `E-UNSUPPORTED-REF-PAT` |
| `types/array-zero-length` | - | `E-TYP-ARRAY-REPEAT` | `E-TYP-ARRAY-LEN` | `E-TYP-ARRAY-LEN` |
| `types/never-field` | - | `E-TYP-MISMATCH` | `E-UNSUPPORTED-TYPE` | `E-UNSUPPORTED-TYPE` |
| `types/never-return-body` | - | `E-TYP-RETURN` | `E-UNSUPPORTED-TYPE` | `E-UNSUPPORTED-TYPE` |
| `types/size-of-unknown` | - | `E-RES-UNKNOWN` | `E-UNSUPPORTED-TURBOFISH` | `E-UNSUPPORTED-TURBOFISH` |
| `ownership/drop-impl` | - | `E-UNSUPPORTED-DROP-IMPL` | `E-UNSUPPORTED-TRAIT-IMPL` | `E-UNSUPPORTED-TRAIT-IMPL` |
| `ownership/move-out-of-mut-borrow` | - | `E-OWN-MOVE-OUT-OF-BORROW` | `E-OWN-MOVE-BORROWED` | `E-OWN-MOVE-BORROWED` |
| `ownership/vec-index-move` | - | `E-OWN-PARTIAL-MOVE` | `E-OWN-MOVE-INDEXED` | `E-OWN-MOVE-INDEXED` |
| `unsupported/drop-impl` | - | `E-UNSUPPORTED-DROP-IMPL` | `E-UNSUPPORTED-TRAIT-IMPL` | `E-UNSUPPORTED-TRAIT-IMPL` |
| `unsupported/glob-import` | - | `E-UNSUPPORTED-TOKEN` | `E-UNSUPPORTED-GLOB` | `E-UNSUPPORTED-GLOB` |
| `unsupported/or-pattern` | - | `E-UNSUPPORTED-TOKEN` | `E-UNSUPPORTED-OR-PAT` | `E-UNSUPPORTED-OR-PAT` |
| `unsupported/turbofish` | - | `E-PARSE-UNEXPECTED` | `E-UNSUPPORTED-TURBOFISH` | `E-UNSUPPORTED-TURBOFISH` |

Lecture : une colonne « Code attendu » à `—` signifie que la spécification
n'impose **aucun** code pour cette règle (comportement d'exécution ou
acceptation) ; l'échec porte alors sur le comportement observable.

### 4.1 Codes émis mais absents de la spécification (13)

13 codes sont produits par le compilateur et **n'apparaissent dans aucun
document de `spec/language/`** :

`E-OWN-MOVE-BORROWED`, `E-OWN-MOVE-INDEXED`, `E-OWN-NOT-COPY`, `E-RES-IMPL-FOREIGN`, `E-RES-VARIANT-UNQUALIFIED`, `E-TYP-ARRAY-LEN`, `E-UNSUPPORTED-GLOB`, `E-UNSUPPORTED-LET-PATTERN`, `E-UNSUPPORTED-OR-PAT`, `E-UNSUPPORTED-REF-PAT`, `E-UNSUPPORTED-TRAIT-IMPL`, `E-UNSUPPORTED-TURBOFISH`, `E-UNSUPPORTED-TYPE`

C'est un défaut de spécification **dans les deux sens** : soit le code émis est
le bon et il doit être enregistré dans `diagnostics.md` §3, soit le code de la
table est le bon et le compilateur doit être aligné. Trancher est une décision de
nommage qui appartient au responsable du projet : elle est inscrite comme **D8**
dans `DECISIONS_REQUIRED.md`. Tant qu'elle n'est pas prise, le corpus reste
rouge — c'est le signal exact, pas un artefact.

### 4.2 Divergences qui ne sont pas des noms de codes

Ces cas échouent sur le **comportement**, pas sur un identifiant :

| Cas | Attendu par la spécification | Observé |
|---|---|---|
| `parser/while-value-nonunit-fail` | `let x = while c { 1 };` refusé (`E-TYP-STATEMENT-VALUE`) | accepté, `check` exit 0 |
| `parser/loop-control-as-value-fail` | `while`/`loop`/`for` utilisés comme valeur refusés | acceptés, `check` exit 0 |
| `parser/test-fn-pub-fail` | `pub test fn` refusé (`E-PARSE-EXPECTED`) | accepté et compté comme test |
| `unsupported/ref-identifier` | `let ref = 1;` **accepté** (`ref` est un identifiant valide) | refusé, `E-UNSUPPORTED-REF-PAT` |
| `run/vec-clone-non-copy` | `Vec<String>::clone()` refusé (`E-RES-UNKNOWN`) | accepté |
| `run/math-max-signed-zero` | `max(-0.0, +0.0)` vaut `+0.0`, affiché `0` | affiche `-0` |
| `run/int-abs-signed` | `i32::abs()` accepté | refusé à la génération de code |
| `lexer/contextual-effects-ident` | `let effects = 1;` accepté (`effects` n'est pas réservé) | refusé par le parseur |

### 4.3 Défauts observés hors périmètre des cas de cette passe

Constatés pendant la rédaction des cas, non encodés (ils ne relèvent pas des
règles traitées) :

- **Tuples hétérogènes** : `(i32, bool)` ne fonctionne pas, seul l'homogène
  passe (`E-TYP-MISMATCH` sur le second élément).
- **Exhaustivité des tuples** : `match t { (a, b) => … }` est jugé
  non-exhaustif alors que `(a, b)` couvre le tuple entier.
- **`mut` sur paramètre** : `fn f(mut n: i32)` est refusé (`E-OWN-IMMUTABLE`)
  alors que `grammar.md` G-6.4/G-6.9 et `types.md` §2/§3 le documentent.
- **Type `!`** : `grammar.md` G-5.11 et `types.md` TYP-1.6 le décrivent avec
  restrictions ; le parseur le refuse dans **toute** position de type.
- **Littéraux flottants en récepteur** : `math::sqrt(2.0)` passe `check` mais
  échoue à `rustc` (`can't call method 'sqrt' on ambiguous numeric type
  '{float}'`) : la génération perd l'annotation `f64`.
- **Génériques à deux types** : `fn pair<A, B>(a: A, b: B) -> (A, B)` est refusé
  (`E-TYP-MISMATCH`).

## 5. Règles non couvrables en l'état

**`F-META-006`** reste `NOT_COVERED`. Sa colonne « pass » vaut explicitement
*« aucun (oracle absent) »* et sa colonne « fail » *« citer un résultat comme
preuve »* : `core-machine.md` MACH-1.4 fixe l'état de l'interpréteur à
`NOT_IMPLEMENTED` et interdit de citer un résultat machine comme preuve. Aucune
forme de programme accepté ni aucun refus n'existe, donc tout cas inventerait
une exigence. Le harnais n'a pas de mode « preuve non exécutable ». La règle est
donc laissée non couverte **et documentée comme telle**, ce qui est la seule
issue honnête.

## 6. Repro

```powershell
# Construire la toolchain
cargo build -p ravum-lang-cli

# Corpus complet (attendu : 237 PASS / 30 FAIL / 1 SKIP)
cargo run -p ravum-conformance -- run --json target/conformance-after-t2.json

# Traçabilité des règles
python tools/features_coverage.py --strict --json target/features-coverage.json
python tools/apply_coverage_map.py target/coverage-maps/*.json --dry-run

# Triage codes attendus / émis / documentés
python tools/triage_conformance.py target/conformance-after-t2.json

# Un cas isolé
cargo run -p ravum-conformance -- run --filter ownership/vec-index-move
```

## 7. Conséquence sur les gates

- **T2 `FAIL`** : la traçabilité est désormais quasi complète (187/188) mais
  **30 exigences ne sont pas satisfaites par l'implémentation**, et 13 codes
  émis ne sont pas documentés. Un `PARTIAL` masquerait un déficit désormais
  quantifié ; `FAIL` est l'état exact.
- **T3 reste dépendant de T2** pour la partie « spec ↔ frontend » : les
  divergences §4.1/§4.2 sont des défauts du frontend et du checker, pas du
  corpus.
- Le corpus **reste rouge volontairement** : supprimer ou assouplir ces cas
  reviendrait à effacer les défauts qu'ils documentent, ce que le mandat
  interdit explicitement.

## 8. Correctifs appliqués après la passe (quatorze cas sortis du rouge)

Chaque correctif est un changement de **comportement**, pas d'`expect_codes` :
le cas reste écrit comme la spécification l'exige, c'est le compilateur qui a
été aligné. Corpus après correctifs : **269 cas, 252 PASS / 16 FAIL / 1 SKIP** ;
tests workspace **82 suites, 1019 passés / 0 échec / 2 ignorés** (aucune
régression).

| Cas devenu vert | Correctif |
|---|---|
| `lexer/contextual-effects-ident` | `effects` est un mot-clé **contextuel** (LEX-5.5) : le parseur l'accepte désormais comme motif et comme chemin d'expression hors position de signature |
| `unsupported/ref-identifier` | `ref` n'est refusé comme motif `ref` que s'il **est suivi d'un motif** ; `let ref = 1;` redevient une liaison ordinaire (Q-UNS-2) |
| `parser/test-fn-pub-fail` | `pub test fn` produit `E-PARSE-EXPECTED` (F-SYN-034) au lieu d'être accepté |
| `parser/while-value-nonunit-fail` | `let x = while c { 1 };`, `loop` et `for` en position de valeur produisent `E-TYP-STATEMENT-VALUE` (F-SYN-019/033) |
| `parser/loop-control-as-value-fail` | idem, y compris `let b = loop { break; };` et `let d = for i in 0..2 {};` |
| `run/int-abs-signed` | le checker canonicalise tous les entiers en `int` ; le codegen ne connaissait que `i8`…`isize` et refusait `int::abs` (F-STD-029) |
| `parser/tuple-pattern-arity-fail` | une arité de motif tuple fausse produisait `E-TYP-MISMATCH` alors que **toutes** les autres vérifications d'arité du même fichier émettent `E-TYP-ARITY` et que F-SYN-030 l'exige |
| `unsupported/raw-string` | `r"…"`/`b"…"`/`br"…"` sont désormais reconnus par le **lexer** (phase lexicale, code `E-UNSUPPORTED-TOKEN`) au lieu de produire deux tokens et un `E-PARSE-EXPECTED` en cascade |
| `unsupported/range-value` | un `..`/`..=` hors clause `for` est refusé par le **parseur** (`E-UNSUPPORTED-RANGE`, phase parse, comme le prescrit `diagnostics.md` §3) au lieu d'être reclassé en erreur de type `E-TYP-RANGE` par le checker |
| `run/vec-clone-non-copy` | `Vec<T>::clone` n'existe que pour `T: Copy` (STD-10.4) : le checker émet `E-RES-UNKNOWN` quand l'élément est mobile, au lieu d'accepter un clone implicite |
| `run/string-parse-method-no-call` | une méthode utilisée **sans appel** (`s.parse_i64`) est une erreur de résolution (`E-RES-UNKNOWN`), plus une « absence de champ » (`E-TYP-FIELD`) : le message pointe la correction |### 8.4 `math::min`/`max` et le zéro signé — corrigé
| `unsupported/lifetime-token` | `'a` produisait **quatre** diagnostics en cascade (`E-LEX-CHAR`, `E-PARSE-EXPECTED`, `E-PARSE-UNCLOSED`, `E-PARSE-UNEXPECTED`). Le lexer reconnaît désormais la durée de vie et émet `E-UNSUPPORTED-TOKEN` ; le parseur consomme le jeton dans les listes de génériques et derrière `&`, donc l'item continue de se parser et l'utilisateur reçoit un diagnostic par occurrence |
| `types/place-non-place-assign` | `(a+b).0 = 1;` était accepté par le parseur puis refusé par le checker (`E-TYP-FIELD`). La spécification en fait une **erreur de parse** : le parseur vérifie désormais que la cible est une *place* (nom, champ, index, déréférence) et émet `E-PARSE-EXPECTED` |`run/math-max-signed-zero` échouait pour une raison **différente d'un nom de
code** : `math::max(a, b)` était émis comme `(a).max(b)`, c'est-à-dire
`f64::max`. Or le résultat de `f64::max(-0.0, 0.0)` dépend de l'abaissement LLVM :

- `rustc -C opt-level=3` → `0.0`, donc `"0"` (attendu par STD-6.5/12.2) ;
- `rustc -C opt-level=0` (le niveau que la CLI utilise et que la conformance
  exécute) → `-0.0`, donc `"-0"`.

Mesure : le fichier Rust généré, **compilé à la main avec `-O`**, affiche
`positive` ; le binaire produit par `ravum-lang build` (même fichier,
`-C opt-level=0`) affiche `negative`. La sémantique observable de `math::max`
**dépendait donc du niveau d'optimisation**.

Correctif : le codegen n'émet l'intrinsèque que hors du cas d'égalité à zéro, et
départage explicitement `+0.0`/`-0.0` conformément à STD-6.5 (le comportement
NaN est laissé à l'intrinsèque, qui implémente déjà « si un opérande est NaN,
rendre l'autre »). Deux cas verrouillent désormais la règle dans les deux sens :
`run/math-max-signed-zero` et le nouveau `run/math-min-signed-zero`
(`min(-0.0, +0.0) = -0.0`, `max(-0.0, +0.0) = +0.0`).

### 8.5 Ce que la passe n'a pas touché

Les 24 échecs restants se répartissent en :

- **13 codes émis hors spécification** (arbitrage **D8**) ;
- **8 divergences de nom** entre la table et le compilateur, sur des codes qui
  *sont* documentés (D8 également : deux noms existent, il faut choisir) ;
- **2 défauts de comportement** : `Vec<String>::clone()` accepté et le token
  `lifetime` qui produit une cascade de quatre erreurs de parse au lieu d'un
  `E-UNSUPPORTED-TOKEN` unique.

## 9. Alignement des codes sur le registre — le corpus passe au vert

`spec/language/diagnostics.md` §3 est explicite : « La liste des codes
ci-dessous est **canonique** : c'est celle que l'implémentation émet. **Tout code
absent est interdit.** » Et DIAG-1.1 impose que tout diagnostic porte un code de
§3.

Le compilateur émettait **13 codes absents de ce registre** :

`E-OWN-MOVE-BORROWED`, `E-OWN-MOVE-INDEXED`, `E-OWN-NOT-COPY`,
`E-RES-IMPL-FOREIGN`, `E-RES-VARIANT-UNQUALIFIED`, `E-TYP-ARRAY-LEN`,
`E-UNSUPPORTED-GLOB`, `E-UNSUPPORTED-LET-PATTERN`, `E-UNSUPPORTED-OR-PAT`,
`E-UNSUPPORTED-REF-PAT`, `E-UNSUPPORTED-TRAIT-IMPL`, `E-UNSUPPORTED-TURBOFISH`,
`E-UNSUPPORTED-TYPE`.

Ils ont été **remplacés par les codes enregistrés** que `features.md` exigeait
déjà. Deux cas ont demandé plus qu'un renommage :

- `impl Drop` vs les autres implémentations de trait : le registre a
  `E-UNSUPPORTED-DROP-IMPL` **et** `E-UNSUPPORTED-TRAIT`. Le parseur choisit
  selon le nom du trait.
- `E-UNSUPPORTED-TYPE` servait au type `!`, avec **deux attentes différentes**
  selon la position : `E-TYP-RETURN` en type de retour, `E-TYP-MISMATCH`
  ailleurs. Le parseur porte désormais un contexte de type de retour.

**Correction d'une erreur de ma part** : j'avais classé ces 13 codes comme une
« décision humaine (D8) ». C'était faux. Le registre tranchait déjà ; c'était un
défaut d'implémentation, pas un choix de nommage. D8 est donc **résolue**.

### 9.1 Le dernier cas rouge était un défaut de spécification

`types/size-of-unknown` attendait `E-RES-UNKNOWN` pour `size_of::<i32>()`. Or
DIAG-1.4 dit qu'une phase précoce empêche les phases suivantes sur le même
nœud, et F-UNS-019 fait du turbofish une erreur de **parse** : le code de
résolution attendu était **inatteignable**. La source du cas utilise désormais la
forme atteignable `size_of(1)` ; le code attendu et l'exigence sont inchangés.

### 9.2 Ce qui reste à faire pour T2

Le corpus passe, mais le **critère strict §2** n'est satisfait que par
**106/188** règles : **82 règles nomment un code `E-*` dans leur colonne
`Pass / Fail` sans qu'un cas cité ne l'assertent**. C'est un travail d'écriture
de cas — 82 tests négatifs —, pas une décision.
## 10. Deuxième vague de traçabilité stricte — 106/188 → 179/188

Quatre workers ont repris les 82 règles qui nommaient un code sans qu''un cas cité
ne l''asserte. Résultat : **313 cas**, **179/188** règles au critère strict, corpus
**312 PASS / 0 FAIL / 1 SKIP**. La plupart des règles ont été reliées à des cas
existants ; **~45 cas neufs** ont été écrits là où aucun ne couvrait la règle.

### 10.1 L''audit du registre manquait, et il change la conclusion

Mon affirmation de la passe précédente — « plus aucun code émis hors registre » —
était **fausse**, parce que la mesure ne regardait que les codes apparaissant dans
les cas **en échec**. Un code peut violer DIAG-1.1 sans qu''aucun cas ne le
déclenche.

`tools/registry_audit.py` balaie maintenant **tous** les littéraux `E-*`/`W-*` des
sources et les compare à `diagnostics.md` §3, en séparant les préfixes de phase,
les codes d''outillage et les codes de langage. Résultat :

- **44 codes de langage émis hors registre** avant cette passe ;
- **27** après le remplacement de 4 d''entre eux (`E-IO-INVALID-UTF8`,
  `E-PARSE-TEST-SIG`, `E-UNSUPPORTED-STRUCT-UPDATE`, `E-TYP-UNKNOWN-TYPE`), qui
  bloquaient quatre règles.

Les 27 restants (`E-PARSE-BAD-SUFFIX`, `E-RES-CONST-NOT-CONST`,
`E-UNSUPPORTED-VIS`, `E-LIMIT-FILE-SIZE`, `W-TYP-UNREACHABLE`, …) sont un
**défaut de conformité mesuré**, pas une opinion : le registre interdit tout code
absent. Les nommer un par un et les aligner est le travail restant de T2.

### 10.2 Neuf règles restent hors critère

| Règle | Cause |
|---|---|
| F-LEX-001, F-SYN-004, F-SYN-024, F-TYP-011 | débloquées par les 4 renommages ; cas restant à écrire |
| F-SYN-021 | incohérence de spec : la ligne exige `E-TYP-RANGE`, le compilateur émet `E-TYP-MISMATCH` (les deux sont enregistrés) |
| F-STD-013 | **défaut compilateur** : `assert_eq` n''a aucune contrainte de comparabilité, donc `E-TYP-EQ` est inatteignable |
| F-STD-016 | **défaut compilateur** : la mutabilité de place n''est pas vérifiée ; `(&s).push_str` est accepté |
| F-STD-020 | **incohérence de spec** : `stdlib.md` §9 définit `lines()` sur `&str`, donc le refus exigé est impossible |
| F-META-006 | sans forme exécutable (MACH-1.4) |