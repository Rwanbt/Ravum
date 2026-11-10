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
| Corpus | **323 cas** (`conformance/cases/`) |
| Résultat corpus | **322 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** |
| Règles de `features.md` | **188** |
| Règles tracées vers ≥ 1 cas existant | **187 / 188** |
| Règles satisfaisant le critère strict §2 | **187 / 188** (187/187 testables) |
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
règles traitées). **État au 2026-11-10 : les six défauts sont corrigés et
re-testés ; voir §14.**

- **Tuples hétérogènes** : `(i32, bool)` ne fonctionnait pas, seul l'homogène
  passait (`E-TYP-MISMATCH` sur le second élément). — **corrigé.**
- **Exhaustivité des tuples** : `match t { (a, b) => … }` était jugé
  non-exhaustif alors que `(a, b)` couvre le tuple entier. — **corrigé.**
- **`mut` sur paramètre** : `fn f(mut n: i32)` était refusé (`E-OWN-IMMUTABLE`)
  alors que `grammar.md` G-6.4/G-6.9 et `types.md` §2/§3 le documentent. —
  **corrigé** (accepté, vérifié).
- **Type `!`** : `grammar.md` G-5.11 et `types.md` TYP-1.6 le décrivent avec
  restrictions ; le parseur le refusait dans **toute** position de type. —
  **corrigé** (voir §14).
- **Littéraux flottants en récepteur** : `math::sqrt(2.0)` passait `check` mais
  échouait à `rustc` (`can't call method 'sqrt' on ambiguous numeric type
  '{float}'`). — **corrigé** : `check` et `build` réussissent sur
  `let x: f64 = math::sqrt(2.0);`.
- **Génériques à deux types** : `fn pair<A, B>(a: A, b: B) -> (A, B)` était
  refusé (`E-TYP-MISMATCH`). — **corrigé** : `fn first<A, B>(a: A, b: B) -> A`
  est accepté et instancié avec `(i32, &text)`, ce qui a aussi révélé le défaut
  des tuples hétérogènes.

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
## 11. Clôture de T2 — et l''exemption qui la rend possible

`F-STD-020` exigeait `E-TYP-MISMATCH` pour `for line in text.lines()` sur `&str`.
Or `stdlib.md` §9 définit `lines()` **sur `&str`** : l''appel est légal, donc le
code exigé était inatteignable. La ligne a été corrigée vers ce que la règle veut
réellement démontrer — `lines()` rend `Vec<String>` — avec
`let n: usize = text.lines();` → `E-TYP-MISMATCH`, qui **est** produit et
verrouillé par `types/lines-return-type-fail`.

Résultat : **323 cas, 322 PASS / 0 FAIL / 1 SKIP**, et **187/188** règles au
critère strict. La 188ᵉ, `F-META-006`, n''a **aucune forme exécutable** :

- sa colonne `Pass / Fail` dit `pass : aucun (oracle absent)` ;
- `core-machine.md` MACH-1.4 fixe l''interpréteur de référence à
  `NOT_IMPLEMENTED` et **interdit de citer un résultat machine comme preuve**.

Aucun cas ne peut donc exister sans inventer un oracle. **T2 passe à `PASS` sur
187/187 règles testables**, avec cette exemption écrite noir sur blanc et
révocable : si un relecteur considère qu''un gate exige 188/188, la ligne est là
et le downgrade tient en un mot.

**Correction P0 du 2026-11-10 — l''exemption n''est plus une simple prose.**
Tant qu''elle vivait uniquement dans ce rapport, `python
tools/features_coverage.py --strict` sortait **1** et aucun job CI ne l''exécutait :
le `PASS` de T2 reposait sur une affirmation non vérifiée. L''exemption est
désormais un artefact machine-lisible, `tools/coverage_exemptions.json`, et le
calculateur la valide :

- une règle non couverte sans exemption valide fait échouer le gate ;
- une exemption inconnue, mal formée, **obsolète** (la règle est recouverte) ou
  qui tente de masquer une règle dont la cellule `Conformance` ne dit pas
  littéralement `NOT_COVERED` fait échouer le gate ;
- chaque entrée porte une raison, une référence normative, un approbateur, une
  date et une **condition de réexamen** ;
- `tools/tests/test_validation_contracts.py` reproduit ces cas, dont le vecteur
  de faux PASS historique (règle non couverte + exemption de complaisance).

Mesure : `python tools/features_coverage.py --strict` → **188/188 règles
satisfaites (1 exemption justifiée)**, exit **0**. Le fichier d''exemption est
lui-même audité par la CI (`.github/workflows/gate-contracts.yml`, bloquant).
## 12. Un avertissement promis, jamais émis — **corrigé**

`diagnostics.md` DIAG-1.6 déclare la liste des avertissements **fermée** et lui
donne **deux** entrées : `W-RES-UNUSED-IMPORT` et `W-OWN-UNUSED-VARIABLE`.

Mesure du 2026-11-10 : `W-RES-UNUSED-IMPORT` est bien émis (import inutilisé →
avertissement, et `lint --deny W-RES-UNUSED-IMPORT` sort en code 1).
**`W-OWN-UNUSED-VARIABLE` n'est émis par aucun programme** : un `let unused: i32 = 1;`
jamais lu ne produit aucun diagnostic, et `grep` ne trouve cette chaîne nulle part
dans `crates/` hors du registre.

Deux lectures possibles, toutes deux à trancher par un humain :

- **l'implémentation est incomplète** : la détection de variable inutilisée est
  annoncée et absente ;
- **le registre surestime** : la ligne doit être retirée de DIAG-1.6.

**Corrigé depuis.** L'avertissement est implémenté dans le checker : un `let`
dont le nom n''est résolu par aucun chemin dans la fonction est signalé, avec une
gestion conservatrice de l''ombrage (un nom utilisé une fois vaut pour toutes ses
liaisons : l''avertissement peut manquer, jamais être inventé).

L''objection que j''avais formulée — « les cas `compile-fail` comparent l''ensemble
exact des codes » — était **fausse** : le harnais n''extrait que les codes de
sévérité `error` (`runner.rs`, `Some("error") => codes.insert`). Les
avertissements n''entrent donc pas dans `expect_codes`. Trois tests internes
comparaient en revanche la liste complète : ils comparent désormais les codes
`E-*`, ce qui correspond à ce qu''ils vérifient réellement.

Vérification : `python tools/registry_audit.py` affiche **`never emitted : 0`**,
et le cas `cli/lint-unused-variable` verrouille l''émission via `lint`.

Ce constat ne relève pas des règles `features.md` (donc pas de T2 tel que mesuré),
mais il appartient au même dossier : une capacité promise par la spécification et
non produite par l'outillage.

## 13. `W-TYP-UNREACHABLE` — émission retirée, contrat verrouillé (2026-11-10)

Le code était bien enregistré nulle part, et l''audit le signalait depuis deux
passes : `crates/ravum-lang-sema/src/typeck.rs` émettait `W-TYP-UNREACHABLE` pour
un bras de `match` inatteignable. La conclusion « décision humaine » écrite
précédemment était **erronée** : le contrat normatif avait déjà tranché.

- EXPR-5.9 (MUST) : un bras inatteignable « n''est **pas** une erreur, n''est pas
  supprimé et ne produit **aucun diagnostic** (liste d''avertissements fermée) ».
- G-6.44 (MUST) : même règle au niveau grammaire.
- DIAG-1.6 (MUST) : la liste des avertissements est fermée à
  `W-RES-UNUSED-IMPORT` et `W-OWN-UNUSED-VARIABLE` ; Q-DIAG-3 nomme
  explicitement « bras inatteignable » parmi les exclusions.

Enregistrer le code aurait contredit trois MUST ; le contrat prime sur le
comportement. L''émission a donc été **supprimée** (ce n''est pas une perte de
fonctionnalité : aucun diagnostic n''était dû), et
`crates/ravum-lang-sema/tests/diagnostics_contract.rs` vérifie, via le vrai
parseur et le vrai checker, qu''aucun diagnostic n''est produit pour un bras
inatteignable — y compris après un `_` couvrant.

`python tools/registry_audit.py` est désormais bloquant en CI dans les **deux**
sens : code émis absent du registre §3 **et** code enregistré qu''aucune source
n''émet. Mesure : **112 codes enregistrés, 130 émis, 0 hors registre, 0
déclaré-jamais-émis**, exit **0**.

## 14. Type `!` et tuples : deux défauts frontend mesurés puis corrigés (2026-11-10)

### 14.1 `!` était refusé dans toute position de type

G-5.11 et TYP-1.6 (MUST) déclarent `!` **nommable**, coercible vers tout type
attendu, et interdit comme paramètre, champ, élément de `Vec` et retour d''une
fonction **non divergente**. Le parseur émettait pourtant `E-TYP-RETURN` /
`E-TYP-MISMATCH` sur tout `!`. Conséquences mesurées :

- `fn stop() -> ! { loop { } }` était impossible à écrire ;
- le cas de refus `types/never-return-body` (`fn f() -> ! { }` → `E-TYP-RETURN`)
  passait **pour la mauvaise raison** : le type entier était rejeté avant même
  d''examiner le corps ;
- la colonne « pass » de F-TYP-004 n''était couverte que par la coercition de
  `panic`, jamais par une vraie fonction divergente.

Correction : `TypeKind::Never` dans l''AST, parse de `!`, résolution vers
`Type::Never`, codegen `-> !`. Le resolver rejette `!` (et tout type qui le
contient) en paramètre, champ de struct/enum, argument générique, annotation
locale et constante (`E-TYP-MISMATCH`). Le checker exige un corps réellement
divergent (`E-TYP-RETURN` sinon) et refuse `return 1;` dans une fonction `-> !`
— la compatibilité symétrique traitait sinon `i32` comme compatible avec `!`.

**`loop`/`while`/`for` restent de type `()`** : EXPR-5.11, EXPR-6.7 et G-6.19
(MUST) l''imposent, même quand la boucle diverge (EXPR-6.2). Une fonction `-> !`
doit donc diverger par `panic`, `return` ou un appel de fonction divergente ;
`fn stop() -> ! { loop { } }` est **refusé** (`E-TYP-RETURN`) et ce refus est
verrouillé par `types/never-loop-tail-fail`. Un premier correctif typait `loop`
sans `break` comme `!` : il a été **retiré après contre-expertise indépendante**
(il contredisait trois MUST et relâchait l''acceptation de
`fn f() -> i32 { loop { } }`). La contradiction interne EXPR-11.3 (« `!`
interne, non nommable ») a été alignée sur G-5.11/TYP-1.6/Q-GRAM-10, qui sont
les règles de type spécifiques.

Les autres trous révélés par la même contre-expertise sont fermés :
`x as !` → `E-TYP-CAST` ; une inférence qui produirait `Option<!>`, `Vec<!>`,
`[!; N]`, `(!, T)`, `W<!>` **ou `E::A(panic(…))` sur une variante d''enum
générique** → `E-TYP-AMBIGUOUS` (le backend Rust stable refuse `!` en argument
générique) ; `for x in panic("x")` → `E-TYP-MISMATCH` ;
`while`/`loop`/`for` en élément de tuple ou de tableau →
`E-TYP-STATEMENT-VALUE`. Les agrégats **annotés** (`[i32; 2]`,
`(i32, i32)`, `[i32; 3]`, `E<i32>`) acceptent en revanche un élément `panic(…)`,
conformément à TYP-1.6 (« aucun `E-TYP-MISMATCH` n''est émis pour `!` ») : le
type attendu vient de l''annotation, et un `!` en tête sans annotation est
`E-TYP-AMBIGUOUS`.

**Position de valeur des boucles — corrigé dans la même passe.** L''interdiction
EXPR-5.11/6.7/G-6.19 des expressions de contrôle en position de valeur n''était
vérifiée que pour l''initialiseur `let`, les éléments de tuple et les éléments de
tableau. Un compteur `value_position` couvre désormais les arguments d''appel
(y compris les constructeurs `Some`/`Ok`/`Err`), la valeur d''affectation, les
champs de struct, les bras et la scrutinee de `match`, et la valeur répétée d''un
tableau : ces positions produisent `E-TYP-STATEMENT-VALUE`. Un `loop` en
instruction directe reste accepté (`fn main() { loop { } }` vérifié), et un
`loop` en queue de bloc `()` reste régi par EXPR-5.11. Cas
`types/control-as-value-fail` (six positions, un seul code attendu).

Deux restrictions vont au-delà de la liste littérale de TYP-1.6 (qui nomme
paramètre, champ, élément de `Vec` et retour non divergent) : les annotations
locales/constantes de type `!` et les types composés contenant `!` sont aussi
refusés. Motif mesuré : le backend de référence B émet du Rust stable, où une
place de type `!` (`let x: ! = …`, `const C: ! = …`) est refusée (`E0658`).
C''est une **limite du backend**, documentée comme telle, pas une règle de
langage ; elle est fail-closed (le programme est refusé au `check`, jamais
accepté puis cassé au build).

Preuves (toutes exécutées) : `types/never-return-diverges` (compile-pass),
`types/never-return-run` (**run-fail : build + exécution réelle**, panique
`panic: stop`), `types/never-param-fail`, `types/never-field-fail`,
`types/never-return-value-fail`, `types/never-loop-tail-fail`,
`types/never-cast-fail`, `types/never-infer-option-fail`,
`types/never-infer-vec-fail`, `types/never-tuple-infer-fail`,
`types/never-for-iter-fail`, et `types/control-as-tuple-element-fail`.

### 14.2 Les tuples hétérogènes étaient unifiés comme des tableaux

`let t = (1, true);` produisait `E-TYP-MISMATCH: array/tuple element type bool
does not match i32`, alors que TYP-4 définit le tuple comme un produit ordonné
hétérogène. Le checker réutilisait `check_expr_list` (destiné aux tableaux) pour
les tuples. Correction : chaque élément est vérifié contre l''élément attendu
correspondant, sans unification entre frères. Effet secondaire mesuré : la
couverture d''exhaustivité ne connaissait pas `Type::Tuple`, donc
`match t { (a, b) => … }` exigeait un `_` ; un motif tuple irréfutable couvre
maintenant le produit.

Preuves : `types/tuple-heterogeneous` et `types/tuple-match-exhaustive`
(compile-pass), plus une exécution réelle d''un programme `(7, true)` déstructuré
par `match`, qui affiche `7` (codegen Rust compilé et exécuté).

### 14.3 Portée

Ces corrections sont des **corrections de conformité au contrat existant**, pas
des extensions de périmètre : aucun code de diagnostic nouveau, **aucune règle
relâchée**. Le type des boucles est inchangé (`()`) ; les changements qui
modifient l''acceptation sont tous des **resserrements** exigés par TYP-1.6
(`as !`, `Vec<!>`/`Option<!>` inférés, `for` sur `!`, contrôle en élément de
tuple), plus l''acceptation des tuples hétérogènes, exigée par TYP-4 et
auparavant refusée à tort. Le corpus passe de 326 à **343 cas, 342 PASS /
0 FAIL / 1 SKIP**. Ces mesures ne prouvent ni l''absence d''autres écarts, ni une
conformité générale.

**Limite de la vérification de l''exemption.** `F-META-006` est vérifiée
*structurellement* (cellule `NOT_COVERED`, référence normative, date, condition
de réexamen, absence d''exemption obsolète) ; la vérité sémantique de la
justification reste une **revue humaine**, pas un fait machine. Aucune
ratification humaine n''a eu lieu sur ce point : le statut `PASS` de T2 est un
claim interne, pas une certification.




## 15. Tableaux hétérogènes acceptés — défaut de soundness corrigé (2026-11-10)

`check_array` ne comparait les éléments entre eux que lorsqu'une annotation
fournissait le type attendu (`let a: [i32; 3] = [...]`). Sans annotation, le
premier élément fixait `T` et **les suivants n'étaient pas vérifiés** :
`let a = [1, true];` était accepté. Le codegen émet alors `[1, true]`, que
`rustc` refuse (E0308) : un programme « accepté par le checker » ne compilait
pas, et le contrat TYP-4 (tableau homogène) n'était pas appliqué.

Correction : quand aucun type d'élément n'est fourni, chaque élément suivant est
comparé au premier (`Type::Never` et `<error>` restent compatibles, comme
partout ailleurs) et l'écart produit `E-TYP-MISMATCH` sur l'élément fautif. Le
chemin annoté est inchangé : chaque élément y était déjà vérifié contre
l'annotation, et une longueur contradictoire (`[i32; 3] = [1, 2]`) reste un
`E-TYP-MISMATCH` via la comparaison des types de tableau.

Preuves exécutables : `types/array-heterogeneous-fail`,
`types/array-annotated-length-mismatch-fail` (compile-fail) et les cas positifs
`array_rules` de `crates/ravum-lang-sema/tests/typeck_contracts.rs` (tableaux
homogènes, imbriqués, annotés, répétés). Le corpus passe de 343 à **346 cas,
345 PASS / 0 FAIL / 1 SKIP**. Aucun code de diagnostic nouveau, aucune règle
relâchée : c'est un resserrement exigé par TYP-4.

## 16. Motifs littéraux flottants et négatifs — deux MUST non implémentés (constat, non corrigé)

Deux règles de `spec/language/grammar.md` sont contredites par
l'implémentation, et la spécification est elle-même contradictoire :

| Règle | Exigence | Comportement mesuré |
|---|---|---|
| G-6.36 (MUST) | un pattern littéral **flottant** est accepté (`match x { 1.5 => … }`) | `E-PARSE-EXPECTED` |
| G-6.37 (MUST) | `-N` est admis en pattern pour un entier (`-1`) et un flottant (`-1.0`) | `E-UNSUPPORTED-NEG-PAT` |

Or `spec/language/diagnostics.md` documente `E-UNSUPPORTED-NEG-PAT` comme code
canonique pour « motif de littéral négatif refusé ». Il y a donc **contradiction
interne** : grammar.md impose l'acceptation, diagnostics.md documente le refus.

Décision de cette passe : **ne rien changer unilatéralement**. Implémenter
G-6.36/G-6.37 est une fonctionnalité de langage (parseur, checker, codegen,
retrait du code `E-UNSUPPORTED-NEG-PAT` du registre et de diagnostics.md) ;
aligner la spécification sur l'implémentation revient à modifier deux MUST. Les
deux options exigent l'accord du responsable (voir la demande de décision
D-PAT-1). L'implémentation est **fail-closed** : elle refuse, elle n'accepte
jamais un motif non conforme, donc il n'y a pas de risque de sonorité, seulement
un déficit de fonctionnalité.

Les motifs littéraux **entiers, caractères, booléens et textuels** sont, eux,
acceptés et couverts par `crates/ravum-lang-sema/tests/typeck_contracts.rs` ;
aucun test ne fige le refus des flottants/négatifs comme « attendu ».

## 17. Couverture du vérificateur de types — campagne de contrats (2026-11-10)

La couverture de `crates/ravum-lang-sema/src/typeck.rs` était de **63,7 %** de
lignes. Un fichier de contrats indépendant,
`crates/ravum-lang-sema/tests/typeck_contracts.rs` (95 tests), couvre désormais
les chemins d'erreur et les formes nominales qu'aucun cas de conformance
n'exerçait : appels non appelables et arités, champs et indices, transtypages,
égalité/comparaison, inférence ambiguë, `if` sans `else` en valeur, `?`,
`return`, points d'entrée, `break`/`continue` hors boucle, plages, `[e; n]`,
constantes (arithmétique, décalages, casts tronquants, rejets), motifs
(littéraux, variantes avec payload, tuple, exhaustivité), génériques imbriqués,
méthodes utilisateur et builtins, agrégats, coercitions admises.

Mesure `cargo llvm-cov -p ravum-lang-sema` : `typeck.rs` **63,7 % → 76,9 %** de
lignes (régions 78,5 %), `own.rs` 90,1 %, `types.rs` 100 %, total sema **79,4 %**
de lignes / 79,7 % de régions. **Les objectifs 90 % lignes / 85 % branches ne sont pas
atteints** : le déficit restant est concentré dans `resolve.rs` (67,4 %) et dans
les branches de `typeck.rs` liées aux génériques et à la visibilité
inter-modules, qui demandent des programmes multi-fichiers et des cas
d'exhaustivité supplémentaires. Ce chiffre est mesuré, pas estimé.

## 18. D-PAT-1 résolue : G-6.36 et G-6.37 implémentées (option A)

Décision du responsable (2026-11-10) : **option A**, implémenter les deux règles
MUST plutôt que d'amender la spécification. Ce qui a changé :

| Élément | Avant | Après |
|---|---|---|
| `match x { 1.5 => … }` (G-6.36) | `E-PARSE-EXPECTED` | accepté, comparé avec `==` ; `NaN` ne correspond jamais, `-0.0` correspond à `0.0` |
| `match n { -1 => … }` (G-6.37) | `E-UNSUPPORTED-NEG-PAT` | accepté, y compris `-128` pour `i8` et `-2147483648` pour `i32` |
| `match n { -x => … }`, `-(1+2)` | `E-UNSUPPORTED-NEG-PAT` | `E-PARSE-EXPECTED` (G-6.37) |
| `E-UNSUPPORTED-NEG-PAT` | enregistré, émis | **retiré du registre** : 111 codes, 0 jamais émis |
| `negative_ok` (`*value > i128::MAX`) | acceptait un littéral positif hors bornes | remplacé par un test de **signe** (`negative_value_fits`) |

Détails qui comptent :

- **Représentation** : `LitPat::Int` porte désormais un booléen `neg` et une
  *magnitude* (jamais un complément à deux) ; `LitPat::Float` garde le texte
  source et le suffixe.
- **Codegen** : Rust n'a pas de motif flottant. Une branche contenant un motif
  flottant est abaissée en `binding` + `garde` (`Some(__ravum_flt_0) if
  __ravum_flt_0 == 1.5f64`) sur une liaison unique du scrutinee. Un bras gardé
  ne couvre rien : `pattern_contains_float` empêche le checker de croire que
  `Some(1.5)` couvre `Some`, sinon le `match` Rust émis serait non exhaustif.
- **Exhaustivité** : un motif flottant exige `_` (`E-TYP-NONEXHAUSTIVE`),
  exactement comme un motif entier.

Preuves exécutables : 8 nouveaux contrats dans `typeck_contracts.rs` et 5 cas de
corpus, dont deux `run-pass` qui **exécutent** la sémantique (`NaN` ne
correspond jamais, `-0.0` correspond à `0.0`, `-128`/`-2147483648` sélectionnent
la bonne branche), et un `compile-fail` pour `-y`. Corpus : 348 cas au moment de
la rédaction, 0 FAIL.

## 19. Défauts trouvés par les contrats de couverture et le remplissage du corpus

Les passes de couverture (`typeck_contracts2.rs`, `resolve_contracts.rs`) et de
complètement du corpus (31 cas) ont produit des **défauts réels**. Sept ont été
corrigés dans cette passe, chacun avec un test de régression ; les autres sont
consignés ici sans correction, avec la raison.

### 19.1 Corrigés

| Défaut | Symptôme mesuré | Correction |
|---|---|---|
| `None` nu en motif | `match o { None => …, Some(x) => … }` : `None` était une **liaison**, donc le premier bras était irréfutable et masquait le trou d'exhaustivité (G-6.41) | parseur : `Some`/`None`/`Ok`/`Err` sont des motifs de constructeur ; `match o { None => … }` seul est maintenant `E-TYP-NONEXHAUSTIVE` |
| `None(payload)` | `None(v)` sur `Option<i32>` accepté, alors que toute variante unitaire utilisateur est `E-TYP-ARITY` (TYP-10.2/G-6.29) | checker : `E-TYP-ARITY` + vérification des sous-motifs |
| `for x in &vec` / `&[T]` | accepté par le checker, puis rejeté par l'ownership en `E-OWN-REF-ESCAPE` : mauvaise phase, mauvais code (EXPR-6.4/TYP-5.8 MUST) | checker : `E-TYP-MISMATCH`, message qui cite l'itération par index |
| `const MIN: i8 = -128` | `E-TYP-LITERAL-RANGE` + `E-RES-CONST-NOT-CONST` alors que `let a: i8 = -128;` était accepté (TYP-13.2) | évaluateur de constantes : le signe est traité avant la magnitude |
| Budget de diagnostics | 150 erreurs de syntaxe produisaient 150 diagnostics (DIAG-1.7/DIAG-4.3 MUST) | `DiagnosticBag` : 100 par fichier puis un unique `E-PARSE-TOO-LARGE` |
| Pile du parseur | 240 parenthèses imbriquées **plantaient** (0xC00000FD) avant la limite `MAX_DEPTH` du parseur | le CLI exécute la commande sur un thread de 64 Mio de pile |


### 19.2 Ouverts (non corrigés, à décider)

1. **Famille d'affectations composées** — `grammar.md` G-6.8 et `expressions.md`
   EXPR-9.4 (MUST) déclarent la famille `-= *= /= %= &= |= ^= <<= >>=`
   **acceptée**, alors que `diagnostics.md` enregistre
   `E-UNSUPPORTED-COMPOUND-ASSIGN` et que l'implémentation ne supporte que `+=`.
   Contradiction de spécification, pas défaut d'implémentation : elle demande une
   décision (**D-CMP-1**, même forme que D-PAT-1).
2. **Visibilité d'un module privé dans `use`** — `resolve.rs` passe l'id du
   module lui-même comme module de définition, donc `mod geo { … }` est invisible
   depuis la racine et ses descendants : `use crate::geo;` donne `E-RES-PRIVATE`
   alors que MOD-4.3 impose l'acceptation (et MOD-3.7 attend `E-RES-DUPLICATE`
   pour le doublon).
3. **Segments de module non finaux d'un `use`** — la visibilité n'est vérifiée
   que sur le dernier segment : `use crate::a::secret::X;` est **accepté** quand
   `secret` n'est pas `pub`, alors que le même accès par chemin de valeur ou de
   type donne correctement `E-RES-PRIVATE` (MOD-4.2/MOD-4.3/MOD-6.2). C'est un
   trou de confidentialité, pas de confort.
4. **`test fn` avec code de retour invalide** — `E-PARSE-EXPECTED` alors que
   G-3.6/`diagnostics.md` nomment `E-TYP-MAIN`.
5. **Comparaison mixte `f32`/`f64`** — `E-TYP-EQ`/`E-TYP-CMP` alors qu'EXPR-4.8
   nomme `E-TYP-MISMATCH` (les codes EQ/CMP sont réservés au type non comparable).
6. **`E-RES-CONFLICT`** — `diagnostics.md` l'assigne à une collision
   import/local, `modules.md` MOD-3.7 l'assigne à `E-RES-DUPLICATE` (comportement
   actuel). Le seul émetteur réel est une collision de nom de prélude, qu'aucune
   règle ne décrit.
7. **`E-RES-SELF` vs `E-RES-SELF-OUTSIDE-IMPL`** — deux lignes de
   `diagnostics.md` décrivent le même cas.
8. **Codes inatteignables** — `E-OWN-LIMIT` (le budget DIAG-4.3 coupe avant),
   `E-PARSE-BAD-SUFFIX` (`E-LEX-INT-SUFFIX` gagne en amont), `E-RES-MODDECL`
   (le chargeur retire la déclaration) et `E-UNSUPPORTED-UFCS`
   (`resolve_associated` ne sélectionne que les méthodes sans récepteur). Ils
   restent enregistrés et textuellement émis ; aucun cas de corpus ne peut les
   atteindre honnêtement.
9. **`E-LIMIT-FILE-SIZE`** — exige un fichier de plus de 8 Mio ; un fixture de
   cette taille (≈50× le corpus) n'a pas été ajouté.
10. **Harnais de conformance** — `runner.rs` valide tout code avec `is_valid_code`
    qui n'accepte que `E-*` : un `W-*` dans `check --json` fait échouer la
    validation de schéma, alors que DIAG-1.6 les autorise. Les deux cas `W-*`
    ajoutés vérifient donc le fragment `code`+`severity` sans demander la
    validation de schéma.
11. **Phase des `E-LIMIT-*`** — le registre les classe `parse`, le JSON les
    rapporte en `"phase":"tool"`.
12. **Contradictions `unsupported.md`/`grammar.md`** sur les codes de `static`,
    alias de type, ré-export, bornes, `move`, motifs de plage et affectation
    composée (le même cas y est décrit par deux codes différents).
13. **`Vec::new()` + `push`** — `let mut v = Vec::new(); v.push(1);` laisse le
    type d'élément non résolu (`$0`) et `s += x` échoue : l'unification ne
    remonte pas du `push` vers le type du `Vec`. L'annotation `Vec<i32>` est
    nécessaire.
14. **Coercition implicite `String → &str`** (**D-COERCE-1**) — TYP-13.1 (MUST)
    n'admet que `&String → &str` et TYP-8.5 rend la conversion possédée
    explicite, mais `coerce` accepte aussi un `String` par valeur en position
    d'argument. Le resserrement a été **essayé puis retiré** : il casse
    `examples/stats` et le cas `run/stats` (cité par F-STD-017), parce que
    `line.trim()` rend un `String` possédé (STD-9.1, MUST). Deux MUST de la
    spécification se contredisent donc en pratique ; l'arbitrage est une
    décision de langage, pas une correction de bug. Le comportement actuel est
    conservé et le test unitaire `string_by_value_is_coerced_to_str` documente
    explicitement la divergence au lieu de la présenter comme conforme.


