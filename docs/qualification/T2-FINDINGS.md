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

> **Mise à jour (passe 5, 2026-11-10)** : les quatorze points ci-dessous ont été
> traités en **§20**. Ce paragraphe est conservé tel qu'il a été écrit pour que
> l'écart entre le diagnostic et le correctif reste lisible ; ne le citez pas
> comme l'état courant.

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


## 20. Passe 5 — traitement des quatorze points de §19.2 (2026-11-10)

Chaque point de §19.2 est soit **corrigé** (avec test de régression), soit
**tranché** (contradiction de spécification), soit **documenté comme restant**
avec la raison. Aucun test n'a été supprimé ; les deux fichiers de cas modifiés
sont signalés explicitement.

### 20.1 Défauts corrigés

| # | Défaut (avant) | Comportement après | Preuve |
|---|---|---|---|
| 2 | `use crate::geo;` sur un module privé → `E-RES-PRIVATE` alors que MOD-4.3 l'autorise depuis le module déclarant et ses descendants | accepté depuis le sous-arbre déclarant ; même-module → `E-RES-DUPLICATE` (MOD-3.7), plus de code trompeur | 4 contrats `resolve_contracts2` |
| 3 | `use crate::a::secret::X;` accepté alors que `secret` est privé (trou de confidentialité) | `E-RES-PRIVATE` citant le segment privé, pour chaque segment non final | 3 contrats (dont profondeur ≥ 3 segments) |
| — | **MOD-3.2** : `use util::helper;` depuis un sous-module → `E-RES-UNKNOWN` (programme valide refusé), repli racine jamais tenté | repli module courant → racine ; **double résolution → `E-RES-CONFLICT`** ; `crate::`/`self::`/`super::` inchangés | 9 contrats (repli, priorité locale, conflit, inconnu, 1 segment, même entité) |
| 4 | `test fn` avec paramètre ou retour invalide → `E-PARSE-EXPECTED` | `E-TYP-MAIN` (G-3.6, MOD-4.10, `diagnostics.md`) | 4 contrats + cas `parser/test-fn-param-fail` corrigé |
| 5 | `f32 == f64` / `f32 < f64` → `E-TYP-EQ`/`E-TYP-CMP` | `E-TYP-MISMATCH` (EXPR-4.8) ; EQ/CMP restent pour le **même** type non comparable/non ordonné (TYP-7.3/7.4) | 7 contrats (dont littéraux contextuels et texte mixte) |
| 13 | `let mut v = Vec::new(); v.push(1);` laissait `$0` et `v[0]`/`s += v[0]` échouaient | l'inférence est substituée en sortie de `check_expr` (cause réelle : `$0` brut, pas la contrainte de `push`) ; `Vec::new()` non contraint reste `E-TYP-AMBIGUOUS` | 5 contrats + exécution de bout en bout |
| 1 | famille d'affectations composées refusée (`E-UNSUPPORTED-COMPOUND-ASSIGN`, seul `+=`) | les **dix** opérateurs acceptés (G-6.8/EXPR-9.4), règles par opérateur, codegen `place op= e` (place évaluée une fois) | 9 contrats + cas `run/compound-assign` **exécuté** (imprime `22 2 false`) + cas `types/compound-assign-mismatch` |
| 8 | `E-OWN-LIMIT` inatteignable (borne 200 > budget fichier 100) | borne par fonction **50**, strictement sous le budget 100 ; 50 conflits réels puis un `E-OWN-LIMIT` | contrat `own_audit::per_function_budget_…` |
| 8 | `E-PARSE-BAD-SUFFIX` : doublon de `E-LEX-INT-SUFFIX`, jamais atteint | code **retiré** ; le parseur garde un chemin défensif qui émet `E-LEX-INT-SUFFIX`/`E-LEX-NUMBER` | `diagnostics.md` Q-DIAG-8 |
| 8 | `E-UNSUPPORTED-UFCS` : jamais atteint et contraire à EXPR-7.8 | code **retiré**, `resolve_associated` inchangé (`P::len(&p)` → `E-RES-UNKNOWN`, comportement canonique) | `diagnostics.md` Q-DIAG-11, `typeck_contracts2::ufcs_is_e_res_unknown` |
| 9 | `E-LIMIT-FILE-SIZE` sans preuve bout-en-bout | test CLI qui **génère** 8 Mio + 1 octet à l'exécution et attend ce code | `crates/ravum-lang-cli/tests/cli.rs` |
| 10 | le harnais rejetait tout code `W-*` comme invalide → les cas d'avertissement contournaient le schéma | `E-*` et `W-*` acceptés (DIAG-1.6) ; les 2 cas `W-*` font la validation complète | `tools/conformance/src/case.rs` + 2 `case.json` |
| 11 | phase JSON `tool` pour `E-LIMIT-*`, `parse` pour `E-LEX-*`, `sema` dans le registre | `phase_for_code` suit **exactement** le registre ; un test lit `diagnostics.md` et échoue sur toute dérive | `json::tests::every_registered_code_reports_its_registry_phase` |
| — | expression interdite dans un `const_expr` (bloc, appel, variable, champ, index, `?`, autre constante) → `E-RES-CONST-NOT-CONST` | `E-TYP-CONST` (G-5.11/G-5.13 MUST) ; `E-RES-CONST-NOT-CONST` reste réservé aux valeurs non représentables (constante flottante) | contrats `typeck_contracts` (bloc, appel, `String::from`), `mutation_gaps::const_block_is_refused_by_name` resserré |

### 20.2 Contradictions de spécification tranchées

| Décision | Contradiction | Arbitrage retenu | Raison |
|---|---|---|---|
| **D-CMP-1** | G-6.8 + EXPR-9.4 (MUST acceptées) vs §7 de `grammar.md` et §12 de `expressions.md` (refus) et `E-UNSUPPORTED-COMPOUND-ASSIGN` | **famille acceptée** ; ligne §7, ligne §12 et code retirés | Q-GRAM-3 (résolue par le Lead) dit « famille complète acceptée, **aucun `E-*` spécifique** » : les deux tables étaient des restes |
| **Q-DIAG-8** | `E-PARSE-BAD-SUFFIX` vs `E-LEX-INT-SUFFIX` | code parse retiré | LEX-8.3 (MUST) assigne le code au lexer ; le parse n'était qu'un doublon inatteignable |
| **Q-DIAG-11** | `E-UNSUPPORTED-UFCS` (`unsupported.md`) vs EXPR-7.8 (MUST, `E-RES-UNKNOWN`) | code dédié retiré | précédent identique : la règle MUST nomme le code, le catalogue nommait un code mort |
| **Q-DIAG-9** | phase `sema` (2 lignes) absente de §4 | `E-UNSUPPORTED-EFFECT` → `resolve`, `E-UNSUPPORTED-DROP-IMPL` → `parse` | phase = phase qui émet (resolve.rs / parser.rs) |
| **Q-DIAG-12** | `E-RES-SELF` décrivait aussi `Self` hors `impl`, et G-5.10 nommait `E-RES-SELF` | `E-RES-SELF` = `self` (paramètre/valeur) ; `E-RES-SELF-OUTSIDE-IMPL` = type `Self` ; G-5.10 aligné | le corpus `resolve/self-type-outside-impl` verrouillait déjà le code dédié |
| **Q-DIAG-13** | codes de `static`, alias de type, bornes, `move`, motifs de plage/`..`, ré-export : `unsupported.md`/`lexical.md`/`grammar.md` citaient `E-UNSUPPORTED-TOKEN` ou `E-PARSE-EXPECTED`, l'implémentation et le registre §3.1 nommaient les codes dédiés | tables alignées sur les **codes dédiés déjà enregistrés** (`E-UNSUPPORTED-STATIC`, `-TYPE-ALIAS`, `-BOUNDS`, `-MOVE`, `-RANGE-PAT`, `-REST-PAT`, `-REEXPORT`) | le registre §3.1 fait foi (DIAG-1.1) et ces codes étaient déjà ceux du corpus |
| **Q-DIAG-14** | `const_expr` interdit : G-5.11/G-5.13 (MUST, `E-TYP-CONST`) vs code émis `E-RES-CONST-NOT-CONST`, que `diagnostics.md` §3.1 réserve aux valeurs non représentables | le code suit la MUST : `E-TYP-CONST` pour les formes de grammaire interdites ; `E-RES-CONST-NOT-CONST` pour la constante flottante | une MUST explicite nomme le code ; le code générique était une erreur de classement, pas une règle |

Conséquences sur le registre : **111 → 108 codes** (`E-PARSE-BAD-SUFFIX`,
`E-UNSUPPORTED-COMPOUND-ASSIGN`, `E-UNSUPPORTED-UFCS` retirés, 0 code
déclaré-jamais-émis, 0 hors registre).

### 20.3 Ce qui reste ouvert, et pourquoi

1. **D-COERCE-1** — contradiction réelle entre TYP-13.1/TYP-8.5 et STD-9.1
   (`trim()` rend un `String`). Comportement conservé ; l'arbitrage est une
   décision de langage, pas un correctif. Un test documente la divergence.
2. **`E-LIMIT-FILES`** — atteignable en principe (4097 fichiers ou 200 modules
   chargés) mais aucun test dédié : le coût du fixture n'est pas justifié tant
   que la limite n'a pas bougé.
3. **Sites inatteignables restants** — `E-RES-SELF` (resolve.rs L559) et
   `E-RES-SELF-OUTSIDE-IMPL` (resolve.rs L2281) ne peuvent pas être produits :
   le parseur refuse `Self` en premier segment (`E-PARSE-UNEXPECTED`) et abaisse
   `Self` en `TypeKind::SelfType`. Deux contrats « parse-only » le prouvent au
   lieu de laisser croire à une couverture.
4. **D-COERCE-1, D2, D6, D9, D4** — décisions humaines (backend natif,
   périmètre universel, signature de release, audit externe).
5. **Lacunes de mutation : 0** — la campagne rejouée donne 195 mutants,
   **166 tués (85,1 %)**, 20 survivants : 14 inatteignables par construction
   (prouvé par le parseur) et 6 équivalents (prouvés par le code), 0 lacune
   de test. Voir `MUTATION-EQUIVALENCE.md` §8.
6. **Identifiant de règle dupliqué** — `grammar.md` porte **deux** règles
   `G-5.11` (type `!` et restrictions de `const_expr`). `rule_id` étant
   contractuel et stable (Q-DIAG-4), la renumérotation est une décision,
   pas un nettoyage : elle est signalée ici sans modification.

### 20.4 Mesures de la passe (Windows 11, rustc 1.95.0)

| Mesure | Valeur |
|---|---|
| Tests workspace | **89 suites, 1405 passés / 0 échec / 2 ignorés** |
| Conformance | **386 cas : 385 PASS / 0 FAIL / 1 SKIP** |
| Registre | **108 codes ; 126 émis ; 0 hors registre ; 0 jamais émis** |
| Mutation ciblée | **195 mutants ; 166 tués (85,1 %) ; 20 survivants = 14 inatteignables + 6 équivalents ; 0 lacune** |
| Nouveaux contrats | `resolve_contracts2` 31, `typeck_contracts3` 25, `own_audit` +1, CLI +1, `json` +1 |
| Corpus | +3 cas (`run/compound-assign`, `types/compound-assign-mismatch`, `cli/check-json-lexer-phase`), −1 obsolète (`unsupported/compound-assign`) |

### 20.5 Clôture de la passe (après §20.4)

Deux éléments ont été ajoutés en clôture, tous deux motivés par la campagne de
mutation et la relecture du registre :

1. **Mutant `check_struct_lit` L3939 tué.** L'assertion ajoutée à
   `struct_variant_literal_records_a_variant_resolution` interdit qu'un fait de
   résolution soit `Resolution::Error` après un littéral de variante résolu ;
   campagne ciblée : 18 mutants, **14 tués**, 3 équivalents (prouvés), 1
   invivable — 14/14 sur les mutants observables.
2. **Code de `const_expr` corrigé** (Q-DIAG-14) : bloc, appel, variable, champ,
   index, `?` et référence à un autre `const` émettent `E-TYP-CONST` (G-5.11 et
   G-5.13, MUST) au lieu de `E-RES-CONST-NOT-CONST`. Deux tests existants qui
   figeaient l'ancien code ont été corrigés avec la règle citée (aucun test
   supprimé) ; la constante flottante garde `E-RES-CONST-NOT-CONST`.

Après ces deux ajouts, les mesures de §20.4 sont inchangées (89 suites / 1405
tests, conformance 386/385/0/1, couverture 89,2 % / 88,6 %, registre 108/126/0/0)
et la mutation passe de 165 à **166 tués sur 195**.

## 21. Passe 6 — divergences `check → build` et seconde métrique de traçabilité (2026-11-11)

> **Base : `5d88ecb`** (les correctifs de cette passe sont les commits `8c6434b`,
> `5daeb40`, `e0af6b6`, `9314d2f`, `9c163de`, `a3a43b2`). Windows 11 x86-64,
> rustc/cargo 1.95.0. Mesures : `cargo test --workspace --all-targets` et
> `target\debug\ravum-conformance.exe run --verify-build`.

### 21.1 Ce qui a été mesuré avant correction

Le corpus de conformance ne vérifiait que `check` en mode accepté : un programme
pouvait donc être « conforme » et échouer à la compilation. La passe a mesuré le
contrat sur **deux jeux distincts** plutôt que de supposer le chiffre externe :

- balayage du corpus accepté (132 cas `compile-pass` + `run-pass`) :
  **132 check-pass / 130 build-pass / 2 divergences** ;
- sondes indépendantes reconstruites par famille (38 programmes) :
  **11 divergences** au total, dont 4 familles absentes du corpus.

Les 37 cas signalés par la revue externe ne sont **pas dans le dépôt** : ils ne
sont ni confirmés ni infirmés en tant que tels. Les familles ont donc été
reconstruites à partir de la liste de la revue.

### 21.2 Divergences reproduites puis corrigées

| # | Programme (résumé) | check | build avant | Cause | Correctif | Code/effet après |
|---|---|---|---|---|---|---|
| 1 | `struct S { text: &str }` | 0 | E0106 | codegen émettait `&str` sans lifetime | `&'static str` en position valeur (OWN-6.2) ; `&str` élidé en paramètre (STD-9.3) | build 0 |
| 2 | `fn lit() -> &str { "a" }` | 0 | E0106 | idem | idem | build 0 |
| 3 | `return a[1];` sur `[String; 2]` | 0 | E0508 | move d'élément de tableau non abaissé | déstructuration hoistée + réécriture par node id (OWN-5.4) | build 0 |
| 4 | `a[1].0` (projection après index) | 0 | E0508 | idem | idem, avec repli sur lecture simple si le champ est `Copy` | build 0 |
| 5 | `let x = a[0]; let y = a[1];` | 0 | E0508 | planificateur limité à une occurrence | réécriture complète par node id | build 0 |
| 6 | `let x = a[0]; return a[1].len();` | 0 | E0508 | idem | idem | build 0 |
| 7 | `v.push(a[0]);` | 0 | E0508 | argument d'appel builtin non traité | positions consommées étendues | build 0 |
| 8 | `fn bump(mut x: i32)` | 0 | E0384 | `mut` de liaison non émis | `mut x: T` | build 0 |
| 9 | `o.unwrap(); let t = o;` | 0 | E0382 | `unwrap` n'était pas consommateur | STD-11.3/OWN-5.8, OWN-5.2 | `E-OWN-DOUBLE-MOVE` |
| 10 | `o.unwrap(); o.is_some()` | 0 | E0382 | idem | idem | `E-OWN-USE-AFTER-MOVE` |
| 11 | `let a: [i32; 2] = [1, true];` | 0 | E0308 | chemin annoté non vérifié élément par élément | TYP-5.1/5.7 | `E-TYP-MISMATCH` |
| 12 | `match x { Some((n,true)) => …, None => … }` | 0 | E0004 | exhaustivité non récursive dans les payloads | G-6.22/TYP-10.4 | `E-TYP-NONEXHAUSTIVE` |
| 13 | `match x { Some(Some(n)) => …, None => … }` | 0 | E0004 | idem | idem | `E-TYP-NONEXHAUSTIVE` |
| 14 | `match e { E::A(Some(n)) => …, E::B => … }` | 0 | E0004 | idem | idem | `E-TYP-NONEXHAUSTIVE` |
| 15 | `takes(owned)` (`String` vers `&str`) | 0 | E0308 | coercition implicite acceptée alors que TYP-8.5 la rend explicite | `String → &str` retiré de `coerce` ; `&String → &str` conservé | `E-TYP-MISMATCH` |
| 16 | `takes(line.trim())` | 0 | E0308 | idem (STD-9.1 : `trim` rend `String`) | idem ; `examples/stats` et `run/stats` migrés vers `.as_str()` | `E-TYP-MISMATCH` |

Défauts supplémentaires corrigés dans la même passe, trouvés par sondage :
références stockées dans un champ/enum/payload de constructeur prélude
(`E-OWN-REF-ESCAPE`, OWN-6.1/6.2), et `&mut self` sur une place immuable
(`E-OWN-IMMUTABLE`, TYP-3.2/OWN-7.4).

### 21.3 Divergences restantes — déclarées ouvertes, hors corpus

Trois formes restent `check-pass`/`build-fail` avec un **E0508 bruyant** (jamais
un miscompilation silencieuse) :

| Forme | Repro | Contrat |
|---|---|---|
| chaîne d'index imbriquée | `fn f(a: [[String; 2]; 2]) -> String { return a[0][1]; }` | OWN-5.3/5.4 autorisent le move |
| tableau derrière un champ/tuple | `struct H { items: [String; 2] }` puis `h.items[0]` ; `t.0[0]` | idem |
| liaison de tableau masquée par un shadowing imbriqué | paramètre `a` + `let a = …` dans un bloc | idem |

Elles exigent un abaissement général des moves par place ; elles sont
documentées dans `CONFORMANCE-BUILD-CONTRACT.md` (§ « Known open divergences »)
et **ne sont pas figées comme échecs attendus** dans les tests. Le gate
`--verify-build` ne prouve donc l'absence de divergence que sur son périmètre.

### 21.4 Seconde métrique : FeatureIds ≠ règles MUST

`tools/must_audit.py` (11 tests unitaires) mesure deux choses distinctes :

| Métrique | Dénominateur | Oracle | Résultat |
|---|---|---|---|
| Couverture FeatureId | **188** FeatureIds | `features_coverage.py --strict` (inchangé) | **188/188** (187 cas + 1 exemption) |
| Traçabilité des énoncés MUST | **807** lignes `Niveau = MUST` (805 ids uniques) | `must_audit.py` | **463/807** atteignables par un FeatureId, **344/807** non |

Détail : liens exacts 110, par plage 72, par groupe 328, par section 40
(recouvrement) ; lien le plus fort : groupe 273, exact 110, plage 43, section 37,
aucun 344. Les documents entièrement non atteints sont ABI (33), COMPAT (39),
CONC (20), MACH (40), DIAG (43), FEAT (11), UNS (8) — soit 194 MUST.

**Conséquence honnête** : T2 était déclaré `PASS` sur la métrique 1, qui ne
couvre pas la métrique 2. Le registre des gates est corrigé en `PARTIAL` pour
T2, sans modifier le seuil : le critère 188/188 reste vrai et vérifié, mais il
ne mesure pas la traçabilité des règles MUST. Aucun test n'a été supprimé ni
assoupli ; aucune règle de spécification n'a été modifiée.

### 21.5 Mesures de la passe

| Mesure | Commande | Résultat |
|---|---|---|
| Tests workspace | `cargo test --workspace --all-targets` | **94 suites, 1525 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance + build | `ravum-conformance run --verify-build` | **424 cas : 423 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build : périmètre **163**, couverts **163**, vérifiés **101**, implicites **62**, échoués **0** ; `build_verified: true` |
| Smoke exemples | `scripts/lang-smoke.ps1 -SkipBuild` | **43 PASS / 0 FAIL / 0 NOT_RUN** (dont 13 assertions `file-store`) |
| Watch fail-closed | `scripts/watch-fail-closed-check.ps1` | **PASS** (ancienne version survit à une édition invalide, activation, 1 seul enfant, 0 orphelin) |
| Registre des codes | `python tools/registry_audit.py` | **108 enregistrés / 126 émis / 0 hors registre / 0 jamais émis** |
| Traçabilité MUST | `python tools/must_audit.py` | **807 MUST, 463 atteints, 344 non atteints** |
| Contrats Python | `python -m unittest discover -s tools/tests` | **33 tests OK** |

Binaires de mesure épinglés : `ravum-lang.exe` sha256
`5816ACDD…C157F661D`, `ravum-conformance.exe` sha256 `7DF6345C…4BE4CCCF`.
Toute re-mesure doit repartir de ces binaires ; l'arbre a été édité par six
agents pendant la passe.

## 22. Passe 7 — fermeture des formes ouvertes, deux familles nouvelles, quatre formes restantes

### 22.1 Les cinq formes précédemment ouvertes sont abaissées

Le §21.3 laissait trois formes ouvertes. Un sondage systématique (round-3,
`scripts/p0a-probe-round3.ps1`, 47 programmes × 11 axes) en a trouvé **deux de
plus**, dont une **régression** introduite par le correctif lui-même en cours
d'édition. Les cinq sont maintenant `check` **et** `build` verts, prouvés par un
test qui compile réellement avec `rustc`, exécute le binaire et vérifie la valeur
(`crates/ravum-lang-codegen/tests/backend_audit.rs`) :

| Forme | Repro | Preuve |
|---|---|---|
| chaîne d'index imbriquée | `fn f(a: [[String; 2]; 2]) -> String { return a[0][1]; }` | `audit_nested_array_element_moves_build_and_run` ; cas `run/array-move-nested` |
| tableau derrière champ/tuple | `h.items[0]`, `t.0[0]`, `h.items[0][1]` | `audit_projected_array_element_moves_build_and_run` ; cas `run/array-move-projected` |
| liaison masquée par shadowing | paramètre `a` + `let a = …` imbriqué | `audit_shadowed_array_bindings_move_build_and_run` ; cas `run/array-move-shadowed` |
| **F1** — récepteur `self` **par valeur** sur élément | `a[0].take()`, `a[i].take()` | `audit_by_value_self_receiver_on_array_element_build_and_run` ; cas `run/array-move-self-receiver` |
| **F2** — payload de tableau dans un bras de `match` | `E::Items(a) => { return a[1]; }` | `audit_match_payload_array_moves_build_and_run` ; cas `run/array-move-match-payload` |

Moyen employé : une *place* est désormais un binding + une chaîne de projections ;
le dédestructuration est récursive (`let [[e00, e01], [e10, e11]] = a;`), les
bindings sont identifiés **par déclaration** (pile de portées dans le walker, sans
changement de `sema`), et un abaissement inline `{ let [..] = place; el }` couvre
les bras de `match` en corps d'expression.

Effet observable déclaré : les emplacements déstructurés deviennent des locales,
donc ils sont libérés à la fin de la portée englobante dans l'ordre inverse de
déclaration, au lieu de partir avec le tableau. **Inobservable en Core 1.0** : le
langage n'a ni `Drop` ni implémentations de traits (`impl` est inhérent), donc
aucun code utilisateur ne s'exécute à l'un ou l'autre instant — seul le moment de
libération de la mémoire diffère.

### 22.2 Quatre formes restent ouvertes — déclarées, bruyantes, hors corpus

Toutes re-mesurées après le correctif : `check` exit 0, `build` exit 1 avec un
**E0508 bruyant** (jamais de miscompilation silencieuse).

| Forme | Repro | Pourquoi elle reste ouverte |
|---|---|---|
| index dynamique dans un tableau imbriqué | `a[0][i]`, `a[i][j]` | il faudrait déstructurer le préfixe constant puis appliquer la forme inline `into_iter().nth(i)` à un **slot**, pas à une place |
| racine de chaîne qui n'est pas une place | `make()[0]`, `a[0].0[1]` | une racine rvalue n'a pas de stockage à déstructurer |
| plusieurs accès dans un bras de `match` en corps d'expression | `E::Items(a) => (a[0], a[1])` | la forme inline est mono-usage par construction ; le corps en bloc fonctionne |
| récepteur *by value* d'une méthode **builtin** | `a[0].unwrap()` sur `[Option<String>; 2]` | les récepteurs des builtins ne sont pas modélisés par le backend, seules les méthodes `self` utilisateur le sont (`facts.methods` + `FnSig.receiver`) |

Conséquence : **`P0-A` reste `PARTIAL`**. Le gate `--verify-build` prouve
l'absence de divergence sur son périmètre (448 cas, 187 places de build), pas sur
le langage entier ; ces quatre formes vivent hors corpus et sont listées ici pour
qu'aucun rapport ne les présente comme fermées.

### 22.3 La CI a trouvé trois défauts d'honnêteté de nos propres tests

Aucun n'était visible en local sous Windows ; tous concernent la passe 6 :

1. **`cargo fmt` jamais exécuté** → job bloquant « Check formatting » rouge sur
   les sept commits. Corrigé (`ab51b17`, formatage seul, 6 fichiers).
2. **Contrat d'`exit code` écrit depuis l'observation Windows seule** : les tests
   exigeaient que `run` nomme `300` et transforme `256` en `1`. Faux sur POSIX :
   `_exit`/`waitpid` tronquent à 8 bits avant que le parent regarde, donc le CLI
   observe `44` et `0`. Les tests sont devenus une paire `cfg(windows)` /
   `cfg(unix)` ; la limitation est documentée (un programme sortant `256` est
   indistinguable d'un succès pour tout parent POSIX). `10459b8`, `c67f17e`.
3. **Course dans le test de build interrompu** : `sleep(120 ms)` + `try_wait` +
   `kill` pouvait laisser le build se terminer entre la sonde et le signal, et le
   test traitait alors une publication **complète** comme une violation. Il attend
   maintenant le début observable de la phase de compilation
   (`.ravum/build/<id>/gen/main.rs`) et accepte deux états : ancien artefact
   intact, ou nouveau programme publié en entier. `f10a12f`.

La règle qui en sort : un test qui observe un comportement dépendant de l'OS doit
être écrit **par plateforme**, avec la même exigence de preuve des deux côtés —
pas une assertion commune assouplie.

### 22.4 Mesures de la passe 7

| Mesure | Commande | Résultat |
|---|---|---|
| Tests workspace | `cargo test --workspace -- --test-threads=2` | **113 suites, 1530 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance + build | `ravum-conformance run --verify-build` | **448 cas : 447 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; build : périmètre **187**, couverts **187**, vérifiés **120**, implicites **67**, échoués **0** ; `build_verified: true` |
| Sondage round-3 | `scripts/p0a-probe-round3.ps1` | **47 programmes × 11 axes : 47 check-pass, 47 build-pass, 0 divergence** (exit 0 ; il sortait 1 avec 4 divergences avant le correctif) |
| Smoke exemples | `scripts/lang-smoke.ps1 -SkipBuild` | **43 PASS / 0 FAIL / 0 NOT_RUN** |
| Watch fail-closed | `scripts/watch-fail-closed-check.ps1` | **PASS** |
| Registre des codes | `python tools/registry_audit.py` | **126 émis / 0 hors registre / 0 jamais émis** |
| Traçabilité MUST | `python tools/must_audit.py` | **807 MUST, 463 atteints, 344 non atteints** (inchangé) |
| Contrats Python | `python -m unittest discover -s tools/tests` | **33 tests OK** |
| CI | GitHub Actions | **12/12 verts** sur `c67f17e`, Linux qualification incluse (run `43026622427`) |

Binaires de mesure épinglés de la passe 7 (Windows 11 x64, `rustc 1.95.0`) :
`ravum-lang.exe` sha256
`5DDF8E5F602D6B657C3644BBE22584B67A655799188E5511491B8202A705C499` ;
`ravum-conformance.exe` sha256
`9F21CFF01629458776E4F66ED0E0587E3A7166F184F197FC5F2014100600DF58`.


## 23. Passe 8 — les six dernières formes connues, deux variantes restantes, couverture re-mesurée (2026-11-11)

> **Base : `7e0c77a`.** Commits de code `5268838` (quatre formes) et `d693162`
> (deux formes). Windows 11 x86-64, `rustc 1.95.0`.

### 23.1 Les quatre formes restantes de la passe 7 sont fermées (`5268838`)

| Forme | Repro | Mécanisme d'abaissement |
|---|---|---|
| index dynamique dans un tableau imbriqué | `a[0][i]`, `a[i][j]` | le préfixe constant est déstructuré une fois, chaque niveau dynamique continue en `into_iter().nth(i)` **sur le slot** (plus sur la place) ; la panique hors bornes garde le texte de rustc |
| racine de chaîne qui n'est pas une place | `make()[i]`, `a[0].0[1]`, `h.items[0].0[1]`, `self.items[0]` | descente *inline* : la racine est matérialisée une fois puis chaque projection/index est appliqué en un bind par étape |
| plusieurs accès dans un bras de `match` en corps d'expression | `E::Items(a) => (a[0], a[1])` | le corps du bras est enveloppé dans un bloc, ce qui crée la frontière d'instruction manquante |
| récepteur *by value* d'une méthode builtin | `a[0].unwrap()`, `unwrap_or`, `ok`, `err` | table STD-11.3 des récepteurs consommants (`Option::{unwrap, unwrap_or, ok_or}`, `Result::{unwrap, unwrap_or, ok, err}`) ; les `is_*` restent des lectures |

### 23.2 Les deux formes signalées à la revue sont fermées (`d693162`)

* `let x = a[0].0[1]; let y = a[1].0[0];` — deux descentes sur des éléments
  **différents** du même tableau.
* `let x = a[0].0[1]; let n = a[1].1;` — descente puis lecture d'un autre
  élément.

Mécanisme : **une seule déstructuration de niveau 0** de la place est hissée
avant le premier usage (`let [e0, e1] = a;`), chaque descente continue depuis son
slot, et un accès constant du même élément est réécrit vers son slot (`e1.1`).
Le groupe n'est ouvert que si le hiss peut dominer tous les usages : descentes en
instructions **directes** du bloc du binder, tout usage à/après le hiss
abaissable depuis un slot, aucun accès consommé avant, aucune autre descente sur
la même place. Sinon le formulaire indexé reste et rustc échoue bruyamment.

Effets de bord observables : les slots de niveau 0 deviennent des locaux
`__rv_el_*` (les non utilisés tombent à la fin de la portée englobante ; sans
`Drop`, inobservable en Core 1.0) ; l'ordre d'évaluation d'origine est conservé ;
le corps d'un bras de `match` en expression est désormais un bloc.

### 23.3 Deux variantes restent ouvertes — mêmes règles d'honnêteté qu'en §22.2

`check` exit 0 puis `build` exit 1, `E0508` bruyant, jamais de valeur fausse.
Elles ne sont **pas** dans le corpus et aucun test ne verrouille le défaut.

**(a) descentes dans deux branches distinctes :**

```rv
fn f(c: bool, a: [([String; 2], i32); 2]) -> String {
    if c { let x = a[0].0[1]; return x; } else { let y = a[1].0[0]; return y; }
}

fn main() {
}
```

Le hiss partagé exige une instruction du bloc du binder qui précède *tous* les
usages ; deux branches n'ont pas d'ancêtre commun, donc le groupe n'est pas
ouvert et le code généré garde `let x: String = a[0usize].0[1usize];`.

**(b) deux descentes dans un même bloc imbriqué :**

```rv
fn f(a: [([String; 2], i32); 2]) -> String {
    if true { let x = a[0].0[1]; let y = a[1].0[0]; return x; }
    return String::from("none");
}

fn main() {
}
```

Un ancêtre valide existerait (première instruction du bloc `if`), mais le plan ne
suit pas l'identité de bloc : il ne peut pas prouver la dominance et n'ouvre donc
pas de groupe. Fermer ces deux variantes demande de suivre une identité de bloc
dans le *walker* puis de valider la dominance (≈15 lignes + tests), hors du budget
de la passe 8. **`P0-A` reste `PARTIAL`** tant qu'elles existent.

### 23.4 Mesures de la passe 8

| Mesure | Résultat |
|---|---|
| Tests workspace (`cargo test --workspace`) | **113 suites, 1536 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance + build (`run --verify-build`) | **453 cas : 452 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; périmètre **192**, couverts **192**, vérifiés **120**, implicites **72**, échoués **0** ; `build_verified: true` |
| Corpus | **448 → 453 cas**, `perimeter` **187 → 192** ; cinq cas `run/array-move-*` ajoutés, tous `check`+`build`+`run` réels |
| Sondage round-3 | **47/47 build-pass, 0 divergence** (inchangé) |
| Sweep `check`/`build` | **118/118 build-pass, 0 divergence** |
| Couverture sema (`llvm-cov`) | **88,94 % lignes / 88,46 % régions** ; `typeck.rs` 92,74 %, `resolve.rs` 93,13 %, `check.rs` 80,26 % (seuil 80) — **tous les seuils tenus, aucun seuil abaissé** |
| CI | conformance Windows **453 cas** `success` (run `43064500916`), et les mêmes chiffres au run PR `43064513146` |

La couverture baisse par rapport au chiffre publié en passe 5 (89,2 % / 88,6 %,
`typeck.rs` 93,8 %) : du code a été ajouté depuis. La baisse est **rapportée, pas
atténuée** ; la marge la plus mince est `check.rs` (80,26 % pour un seuil à 80).

## 24. Passe 9 — les deux dernières variantes sont fermées (2026-11-11)

> **Base : `000708f`.** Commit `21cf7d4` (planificateur + test `rustc` + trois cas
> de corpus). Windows 11 x86-64, `rustc 1.95.0`.

### 24.1 Ce qui était faux

Les deux variantes de la §23.3 n'étaient pas un défaut de sémantique : le
planificateur ne formait un groupe de descentes que si chacune était une
**instruction directe** du bloc du binder. Deux branches d'un `if` et un bloc
imbriqué n'ont pas d'instruction commune dans ce bloc, donc aucun hiss n'était
possible et le formulaire indexé restait (`a[0usize].0[1usize]`, `E0508`).

### 24.2 La correction

Un groupe est désormais formé par **place** et ancré sur la **plus ancienne
instruction du bloc du binder** qui contient une descente. Le déstructurage de
niveau 0 est émis juste avant cette instruction :

* les liaisons sont visibles par tout ce qui est imbriqué dans l'instruction
  d'ancrage (les deux branches de la variante (a), le bloc imbriqué de (b)) ;
* elles le sont aussi par les instructions **suivantes** du même bloc, donc le
  hiss partagé des instructions sœurs (§23.2) reste unique ;
* une utilisation dans une instruction **antérieure** du même bloc voit encore la
  place intacte : elle est laissée telle quelle si elle lit, et elle **refuse** le
  groupe si elle déplace ;
* les corps de boucle restent exclus : un hiss y déstructurerait la même place à
  chaque itération.

### 24.3 Mesure avant / après

| Variante | `000708f` | `21cf7d4` |
|---|---|---|
| (a) deux branches distinctes | `check` 0 / `build` 1 (`E0508`) | `build` 0, exécution `x1` puis `y0` |
| (b) deux descentes dans un bloc imbriqué | `check` 0 / `build` 1 (`E0508`) | `build` 0, exécution `y0` puis `x1` |

Ces deux mesures sont reproduites par
`conformance/cases/run/array-move-branch-descents` et
`conformance/cases/run/array-move-nested-descents`, et par le test
`audit_branch_and_nested_descent_hoists_build_and_run` qui compile réellement le
Rust généré, l'exécute et vérifie que le déstructurage est émis **avant** le
`if`.

Un troisième cas, `run/array-move-read-order`, verrouille la règle d'ordre :
lecture avant l'ancre laissée en place (place intacte), lecture après réécrite
vers le slot, y compris quand le binder est déclaré dans un bloc imbriqué.

### 24.4 Une régression attrapée par la suite, pas par la CI

La première version du correctif groupait par **instruction d'ancrage**, ce qui
séparait deux descentes situées dans des instructions sœurs d'un même bloc et
rouvrait `E0508` sur `array-move-shared-descent` (fermé en passe 8). Le test
`audit_shared_descent_slot_hoist_builds_and_runs` l'a rejetée immédiatement ; le
regroupement se fait désormais par place. C'est l'argument pour garder des tests
qui compilent réellement : le correctif « évident » cassait une forme fermée.

### 24.5 Mesures de la passe 9

| Mesure | Résultat |
|---|---|
| Tests workspace (`cargo test --workspace`) | **113 suites, 1537 passés / 0 échec / 2 ignorés** (exit 0) |
| Conformance + build (`run --verify-build`) | **456 cas : 455 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; périmètre **195**, couverts **195**, vérifiés **120**, implicites **75**, échoués **0** ; `build_verified: true` |
| Corpus | **453 → 456 cas**, `perimeter` **192 → 195** (trois cas `run/array-move-*`) |
| Formes `check`-pass / `build`-fail ouvertes connues | **0** |
| Couverture | **re-mesurée avec une portée élargie** : `cargo llvm-cov -p ravum-lang-sema -p ravum-lang-codegen` → **88,54 % lignes / 87,86 % régions** (totaux), `codegen/emit.rs` **87,02/86,02**, `sema/check.rs` **80,26/72,58** ; trois seuils codegen ajoutés au ratchet, `coverage.yml` étendu aux deux crates ; **aucun seuil abaissé** |
| Reproductibilité, mutation | **non re-mesurées** : aucune n'est affectée par ce changement, aucune n'est revendiquée ici |
| CI | **14/14 verts sur `7665244`** (7 `push` + 7 `pull_request`) ; la CI Windows de qualification rejoue la conformance : `cases=456`, `perimeter=195`, `verified=120`, `implicit=75`, `build_verified=True`, « 9 checks hold » (run `43087652812`) |

### 24.6 Ce que cela ne prouve pas

Zéro variante connue ouverte ne veut pas dire zéro divergence : le corpus ne
teste que ce qu'il contient, et la §10 du contrat de conformance rappelle ce
qu'un run vert signifie exactement. `P0-A` est vert **sur son périmètre mesuré**
et l'invariant général reste une observation outillée. `T2` reste `PARTIAL` pour
la raison indépendante du dénominateur MUST (807 énoncés, 463 atteignables) —
décision **D10**.

## 25. Passe 12 — oracle différentiel génératif `check` ⇒ `build` (2026-11-13)

> Mesure du chantier `goal/production-ready`, phase 2. Générateur
> `crates/ravum-lang-codegen/tests/differential_oracle.rs`, exécuté sur la
> machine de référence (Windows 11 x64, `rustc 1.95.0`).

### 25.1 Ce qui est mesuré

Le générateur est **déterministe** (graine → programme) et produit des programmes
bien typés **par construction** : 34 gabarits d'instructions typés, couvrant
structs, enums à payload (unitaire, tuple, champs), tuples, tableaux `[T; N]`
imbriqués, `Vec<T>`, `String`, `Option<T>`, `Result<T, Error>` (dont `?`), moves
par projection (champ, élément de tuple, élément de tableau), `match` avec
liaisons, `if`/`else` en expression, `while`/`loop`/`for`, shadowing, appels de
fonctions et méthodes (`&self`, `&mut self`, `self`), arguments `&`/`&mut`,
coercition `&[T; N] → &[T]` et arithmétique flottante. Chaque valeur non-`Copy`
est consommée au plus une fois : les programmes ne sont pas des programmes
aléatoires « bien typés par chance ».

Trois propriétés :

1. si `check` accepte, le Rust généré **compile** avec les drapeaux du CLI
   (`--edition 2021`, `overflow-checks=on`, `debuginfo=2`, `opt-level=0`) ;
2. le programme compilé en debug et en release a **même sortie et même code de
   sortie**, sur un sous-échantillon (`RAVUM_DIFF_RELEASE_EVERY=10`) ;
3. même graine ⇒ **mêmes octets** ; deux passes `check`/codegen indépendantes
   donnent le **même résultat**.

### 25.2 Résultats

| Campagne | Commande | Résultat exact |
|---|---|---|
| Rapide (dans `cargo test`) | `cargo test -p ravum-lang-codegen --test differential_oracle differential_oracle_fast -- --nocapture` | **3 graines × 200 = 600 programmes ; 600 acceptés par `check` (100 %), 0 rejet, 0 divergence**, 177 s ; comparaison debug/release sur 60 programmes |
| Longue (exécutée localement) | `RAVUM_DIFF_PROGRAMS=1000 cargo test … differential_oracle_long -- --ignored --nocapture` | **3 graines × 1 000 = 3 000 programmes ; 3 000 acceptés (100 %), 0 rejet, 0 divergence**, 1 104 s ; comparaison debug/release sur 300 programmes |
| Longue (configuration nominale) | idem avec `RAVUM_DIFF_PROGRAMS=10000` | **`NOT_RUN` localement** : coût estimé ≈ 3 h sur cette machine (mesure : 1 000 programmes ≈ 6-7 min). Le job CI `differential-oracle.yml` exécute 10 000 par graine (hebdomadaire + `workflow_dispatch`). |

**Aucune divergence `check`-pass / `build`-fail** n'a été trouvée sur les 3 600
programmes mesurés. Ce n'est **pas** une preuve d'absence : c'est un taux observé
de 0 sur un échantillon déterministe et documenté.

### 25.3 Réducteur et suite

Le réducteur (suppression d'instructions une à une tant que la divergence
persiste) est implémenté et écrit chaque divergence dans
`target/diff-divergences/`. Il n'a **pas été exercé** par une vraie divergence
pendant cette passe : son efficacité reste `UNKNOWN`. Deux tests de
non-vacuité le bordent :

- `oracle_detects_a_deliberately_invalid_generated_file` : le maillon `rustc`
  rejette bien un fichier invalide (sinon l'oracle serait vert par accident) ;
- `generator_produces_distinct_programs_for_distinct_seeds` : deux graines
  produisent deux programmes distincts.

### 25.4 Ce que cette passe ne prouve pas

- Le générateur ne couvre **pas** tout Core 1.0 : pas de `match` imbriqué dans un
  bras, pas de génériques utilisateurs, pas de modules multiples, pas de FFI,
  pas de références stockées (elles sont interdites) ; 8 instructions par
  programme par défaut (`RAVUM_DIFF_STATEMENTS`).
- La propriété 2 est **échantillonnée** (1 programme sur 10), pas exhaustive.
- Une seule machine et une seule toolchain ; le mode long nominal (30 000
  programmes) n'a pas été exécuté localement.
- Aucune divergence trouvée ne prouve pas l'absence de forme inconnue ; le
  corpus de conformance et cet oracle restent deux observations complémentaires.

## 26. Passe 13 — générateur étendu à 42 gabarits, campagnes rejouées (2026-11-13)

> Round 5 du GOAL. Le générateur de la §25 est **étendu** (pas restreint) pour
> couvrir des formes supplémentaires, puis la campagne rapide et la campagne
> longue locale sont rejouées avec ces formes.

### 26.1 Huit gabarits ajoutés (34 → 42)

1. deux moves d'éléments de **même tableau** (`a[0]` puis `a[1]`) — forme qui
   avait produit des E0508 avant la passe 9 ;
2. move du payload `Option<String>` via une liaison `match` ;
3. move d'un champ de struct **imbriquée**, puis lecture d'un autre champ du
   parent (`h.p` déplacé, `h.n` lu) ;
4. move d'un élément d'un **tableau de structs non-`Copy`** ;
5. `if`/`else` produisant une `String` possédée, puis déplacée ;
6. move d'un payload d'enum (`Text::Full(String)`) via `match` ;
7. méthode `&self` appelée sur un **temporaire struct** ;
8. `String` shadowée dans un bloc imbriqué, puis relâchée.

### 26.2 Résultats (générateur v2, 42 gabarits)

| Campagne | Commande | Résultat exact |
|---|---|---|
| Rapide | `cargo test -p ravum-lang-codegen --test differential_oracle differential_oracle_fast -- --nocapture` | **3 × 200 = 600 programmes : 600 acceptés (100 %), 0 rejet, 0 divergence**, 194 s ; comparaison debug/release sur 60 |
| Longue locale | `RAVUM_DIFF_PROGRAMS=1000 … differential_oracle_long -- --ignored` | **3 × 1 000 = 3 000 programmes : 3 000 acceptés, 0 divergence**, 928 s ; debug/release sur 300 |
| Longue nominale | `RAVUM_DIFF_PROGRAMS=10000 …` | **`NOT_RUN` localement** (≈ 3 h) ; job CI `differential-oracle.yml` |

Cumul des deux campagnes déterministes (v1 34 gabarits, v2 42 gabarits) :
**7 200 programmes exécutés, 0 divergence**. Le générateur v2 a produit 100 %
de programmes acceptés par `check`, donc l'oracle n'est pas devenu vide en
s'étendant.

### 26.3 Ce que cela ne prouve pas

Les mêmes limites que §25.4, plus : l'extension ne couvre toujours pas les
modules multiples, la FFI, les génériques utilisateurs ni les références
stockées (interdites). Le mode nominal reste `NOT_RUN` localement.

### 26.4 Campagne nominale 3 × 10 000 (2026-11-14)

La configuration nominale du §25 a été exécutée **localement**, sur la même
machine et le même arbre (`goal/production-ready`, `aa88479`), avec le générateur
v2 :

```
RAVUM_DIFF_PROGRAMS=10000 cargo test -p ravum-lang-codegen \
  --test differential_oracle differential_oracle_long -- --ignored --nocapture
```

| Graine | Programmes | Acceptés | Divergences | Durée |
|---|---|---|---|---|
| `0x00524156554d0001` | 10 000 | 10 000 | **0** | 3 022 s |
| `0x00524156554d0002` | 10 000 | 10 000 | **0** | 2 797 s |
| `0x00524156554d0003` | 10 000 | 10 000 | **0** | 2 783 s |
| **Total** | **30 000** | **30 000** | **0** | **8 602 s** (2 h 23) |

`release_every=10` : 3 000 comparaisons debug == release ont été faites.
**Acceptation 100 %**, donc la campagne nominale n'est pas vide.

Cumul des trois campagnes déterministes (v1, v2, nominale) :
**37 200 programmes, 0 divergence**.

Limites inchangées : une seule machine, une seule toolchain, 8 instructions par
programme, sous-ensemble documenté de Core 1.0. La campagne nominale ne prouve
pas l'absence de divergence sur toutes les formes — elle ferme seulement le
`NOT_RUN` du mode nominal local.

### 26.5 Campagne 3 × 1 000 après l'extension du garde de places (2026-11-14, round 18)

Le générateur v2 exécute aussi, en debug, le garde de cohérence de l'étape B
(`Copy`/`Move` **et** projections de place). Après l'extension de ce garde
(rounds 15-16), la campagne longue locale a été **rejouée** pour vérifier que la
comparaison de places ne produit pas de faux refus à l'échelle :

| Graine | Programmes | Acceptés | Divergences | Durée |
|---|---|---|---|---|
| `0x00524156554d0001` | 1 000 | 1 000 | **0** | 277 s |
| `0x00524156554d0002` | 1 000 | 1 000 | **0** | 277 s |
| `0x00524156554d0003` | 1 000 | 1 000 | **0** | 277 s |
| **Total** | **3 000** | **3 000** | **0** | **830 s** |

`release_every=10` : 300 comparaisons debug == release. Aucun refus de place,
aucune divergence. Cumul des campagnes de l'oracle (v1, v2, nominale, post-gardes+places) :
**40 200 programmes, 0 divergence**.





## 27. Passe 14 — second générateur dirigé par les types, deux divergences `check → build` (2026-11-14)

> Base : branche `goal/consolidation-2`. Commits `40f1737` (générateur),
> `1121308` et `b7e6c7a` (corrections). Windows 11 x64, `rustc 1.95.0`.

### 27.1 Ce qui est mesuré

L'audit du 2026-11-14 (N1/DA) a montré que l'oracle « 40 200 programmes » était
la concaténation de **42 gabarits d'instructions indépendantes** : il ne compose
pas, donc il ne peut pas produire les 13 formes historiques de la §22 à la §24,
toutes nées d'imbrication, de branches ou de shadowing. Un **second générateur**,
additif, est ajouté dans `crates/ravum-lang-codegen/tests/differential_oracle.rs` ;
le générateur à gabarits et ses tests restent **inchangés** et tournent toujours.

Il compose à partir d'un **environnement typé** :

* une variable = `{ nom, type, état }`, état ∈ `Init`, `Moved`,
  `PartMoved(champs)`, `Uninit`, et une pile de portées (shadowing réel) ;
* types produits : `i32`, `bool`, `String`, `(T, U)` (quatre combinaisons),
  `[i32; 2]`, `[i32; 3]`, `[String; 2]`, `Vec<i32>`, `Option<i32>`,
  `Option<String>`, `Pair` et `Holder` (structs de l'en-tête), `Shape` et `Text`
  (enums de l'en-tête) ;
* `gen_expr(ty, profondeur)` tire parmi les règles qui **produisent** `ty` :
  littéral, variable vivante (move si non-`Copy`), projection `x.f`, `x.0`,
  `x[littéral]`, appel d'une fonction de l'en-tête, constructeur, `if/else`,
  `match` avec liaisons, bloc `{ let …; expr }` ; profondeur maximale 4, et à 0
  seulement littéral/variable ;
* `gen_stmt` : `let` (avec shadowing), affectation, `if/else`, `match`,
  `while` borné, `for i in 0..K`, bloc imbriqué ;
* **jonction de branches = intersection** : une valeur déplacée dans une branche
  est considérée déplacée après le `if`/`match` ;
* **10 %** des programmes reçoivent une faute volontaire (usage après move,
  double move, variable non initialisée) ; la propriété exigée est alors que
  `check` **refuse** avec un code `E-OWN-*`.

Correction d'outillage au passage : `check_program` comptait **tout diagnostic,
y compris un warning**, comme un refus. Le CLI sort 0 avec des warnings ; le
générateur typé déclare beaucoup de variables inutilisées, donc son taux
d'acceptation était de 0 % pour cette seule raison. La référence est désormais
l'exit code du CLI : seuls les **erreurs** refusent (même commit `40f1737`).

### 27.2 Taux d'acceptation, histogramme, profondeur

Campagne rapide (toujours exécutée par `cargo test`) : **3 graines × 40
programmes = 120**, profondeur 4, 6 instructions par programme,
`release_every=0` :

| Mesure | Valeur |
|---|---|
| Programmes générés | 120 |
| Sans faute | 110 |
| Acceptés par `check` | **110 / 110 = 100,0 %** |
| Refusés (sans faute) | **0** |
| Fautifs | 10 |
| Fautifs refusés avec `E-OWN-*` | **10 / 10 = 100 %** |
| Divergences `check → build` | **0** |
| Règles distinctes tirées | **53** |
| Instructions générées | 2 964 |
| Profondeur moyenne des expressions | **0,96** (max **4**) |
| Durée | ≈ 30 s (3 graines) |

Le taux de 100 % est **attendu et publié comme tel** : le générateur est
bien typé par construction, donc la discrimination vient des 10 % fautifs et de
la propriété `check accepté ⇒ rustc compile`, pas du taux de refus. Un chiffre
de programmes n'est pas une preuve : les formes distinctes sont les **53 règles**
effectivement tirées, listées ci-dessous.

| Règle (expression) | Tirages | Règle (instruction) | Tirages |
|---|---|---|---|
| `expr:leaf:i32` | 5 586 | `stmt:let` | 2 085 |
| `expr:block` | 1 732 | `stmt:loop` | 321 |
| `expr:if` | 1 699 | `stmt:assign` | 137 |
| `expr:match` | 1 209 | `stmt:match` | 123 |
| `cond:cmp` | 966 | `stmt:if` | 121 |
| `expr:var` | 969 | `stmt:while` | 109 |
| `cond:literal` | 860 | `stmt:for` | 103 |
| `expr:bin_add` | 842 | `stmt:shadow` | 102 |
| `expr:leaf:option` | 749 | `stmt:block` | 98 |
| `expr:leaf:String` | 595 | `stmt:if_no_else` | 86 |
| `expr:leaf:bool` | 503 | `stmt:loop` (corps) | 321 |
| `expr:leaf:tuple` | 537 | `fault` | 10 |
| `expr:leaf:Shape` | 515 | | |
| `expr:leaf:array` | 355 | | |
| `expr:leaf:Text` | 203 | | |
| `expr:leaf:Pair` | 171 | | |
| `expr:leaf:Vec` | 173 | | |
| `expr:leaf:Holder` | 134 | | |
| `expr:tuple:new` | 133 | | |
| `expr:call:make_text` | 109 | | |
| `expr:array:new` | 104 | | |
| `expr:enum:shape.rect` | 100 | | |
| `expr:opt:maybe` | 99 | | |
| `expr:cmp_i32` | 90 | | |
| `expr:enum:shape.line` | 86 | | |
| `expr:enum:text.full` | 58 | | |
| `expr:struct:pair` | 57 | | |
| `expr:proj:holder.n` | 51 | | |
| `expr:call:len_of` | 49 | | |
| `expr:proj:array.i32` | 45 | | |
| `expr:struct:holder` | 36 | | |
| `expr:opt:maybe_text` | 32 | | |
| `expr:proj:pair.a` | 27 | | |
| `expr:call:shape_area` | 25 | | |
| `expr:call:a_value` | 18 | | |
| `expr:call:vec_len` | 16 | | |
| `expr:call:first_of` | 15 | | |
| `expr:opt:some` | 150 | | |
| `expr:proj:string` | 12 | | |
| `expr:proj:array.loop` | 8 | | |
| `expr:call:take_b` | 4 | | |
| `expr:cmp:len` | 2 | | |

(Le tableau complet fait 53 lignes ; les compteurs sont ceux de la campagne
rapide.)

### 27.3 Première divergence trouvée — `a = [a[0], …]`

La campagne nominale **3 × 2 000** a été lancée puis **interrompue** après
**561 programmes de la graine `0x00524156554d1001`**, parce qu'elle a trouvé la
seconde divergence et qu'il fallait corriger avant de continuer. Elle est
`NOT_RUN` en totalité (voir §27.5). La première divergence est à l'index **35** :

```rv
fn main() {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    a = [a[0], String::from("c")];
}
```

Avant correction : `check` exit 0, `build` exit 1,
**`E0508: cannot move out of type [String; 2]`** — le backend émettait
`a[0usize]` littéralement. Cause : la cible d'affectation `a` était enregistrée
comme un *whole use* de la place, ce qui bloquait le hiss dont le RHS a besoin.
Une écriture n'est pas une lecture : elle ré-initialise la place **après**
l'évaluation du RHS. Correction `1121308` : la cible d'affectation d'un binding
entier n'est plus un whole use, et une **région de hiss** se termine à
l'écriture (les accès postérieurs lisent la place ré-initialisée au lieu des
slots périmés — sinon `E0382`). Cas de conformance :
`conformance/cases/run/array-move-assign-back` (run-pass, sortie
`a / c / y / x / 6`). Après correction : `check` 0, `build` 0, exécution
conforme.

### 27.4 Seconde divergence — move d'élément dans une branche à côté d'un whole use

Index **560** de la même graine. Réduit à :

```rv
fn main() {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    if false {
        a = if true { a } else { [String::from("c"), String::from("d")] };
    } else {
        let moved: String = a[1];
        io::println(moved.as_str());
    }
}
```

Avant correction : `check` 0, `build` 1, **`E0508`** sur `v1[1usize]` : le
`whole use` de `a` dans la branche `then` partage l'index d'instruction avec le
move de la branche `else`, donc le fallback inline refusait de s'appliquer. Les
deux sites sont dans des branches **mutuellement exclusives** (ou le whole use
précède le move) : un whole use de la **même instruction** ne doit pas bloquer
le lowering inline, qui est émis *au site de l'accès*. Correction `b7e6c7a` ;
cas `conformance/cases/run/array-move-branch-reassign` (run-pass, sortie `b`).

### 27.5 Test de puissance — NOT_RUN, et le score exact

Le GOAL exige : dans un worktree jetable, annuler un par un les correctifs des
13 formes historiques et vérifier si le générateur retrouve la divergence en
3 × 2 000 programmes, score exact sur 13. **Ce test n'a pas été exécuté.**

Tentative mesurée : `git revert -n 21cf7d4` (le correctif d'ancrage des
descents, §24) dans un worktree détaché à `1121308` :

```
CONFLICT (content): Merge conflict in crates/ravum-lang-codegen/src/emit.rs
CONFLICT (content): Merge conflict in crates/ravum-lang-codegen/tests/backend_audit.rs
error: could not revert 21cf7d4...
```

Les commits de correctif de 2026-11-11 ne s'appliquent plus à l'arbre courant :
`emit.rs` a été réécrit depuis (places, régions, garde OwnFacts). Le worktree a
été supprimé ; aucun revert n'a été forcé, aucune branche de travail n'a été
modifiée.

Substitut **non exécuté** : désactiver le *mécanisme* (par exemple rendre
`plan_slot_groups` inerte) dans le worktree. C'est un autre test (il mesure un
mécanisme désactivé, pas le commit annulé) et il coûte ≈ 25 min par mécanisme
(4 lots de correctifs, 13 formes). Budget de la session insuffisant après les
tâches 1 à 10.

**Score exact : 0 / 13 formes testées — `NOT_RUN`, et non « 0 trouvée ».** Le
critère d'acceptation « score de puissance publié » n'est donc **pas** satisfait ;
T2 est `PARTIAL` pour cette raison, indépendamment des deux divergences
corrigées.

Formes hors de la grammaire actuelle du générateur, par construction :
projections d'un seul niveau, pas de tableau de tuples ni de struct contenant un
tableau, pas de récepteur `self` sur un élément, pas de tableau imbriqué — soit
5 des 13 formes historiques que ce générateur ne peut pas produire. Les deux
divergences trouvées sont, elles, deux classes que le générateur à 42 gabarits
n'avait jamais produites.

### 27.6 Commandes et résultats

| Commande | Résultat |
|---|---|
| `cargo test -p ravum-lang-codegen --test differential_oracle typed_oracle_fast -- --nocapture` | 120 programmes, 110/110 acceptés, 10/10 fautifs refusés, 0 divergence, 53 règles, profondeur 0,96 |
| `cargo test -p ravum-lang-codegen --test differential_oracle` | 6 passés / 2 ignorés (campagnes longues) |
| `cargo run -p ravum-conformance -- run --verify-build` | **478 cas : 477 PASS / 0 FAIL / 1 SKIP** ; `perimeter=206/206`, `verified=123`, `build_verified: true` |
| `git revert -n 21cf7d4` (worktree jetable) | **conflit** — voir §27.5 |

Ce que cette passe ne prouve pas : le générateur n'explore qu'un sous-ensemble
de Core 1.0 (pas de références stockées, pas de traits/closures, une seule
projection) ; 53 règles ne sont pas 53 formes de programme ; et le test de
puissance n'a pas eu lieu.

## 28. Passe 15 — test de puissance T2 (S6) : 9 inverses sur 13, une divergence nouvelle (2026-11-14)

> Base `124e7a4` (branche `goal/wave-3`). Windows 11 x64, `rustc 1.95.0`.
> Worktrees `D:\App\ravum-wt-b` (ajouts durables) et `D:\App\ravum-wt-b-scratch`
> (inverses, jamais commités). Le temps mural n'a pas permis d'aller au bout :
> **4 formes sur 13 restent `NOT_RUN`**, et la campagne nominale 3 × 2 000 n'a
> pas été exécutée. Aucun chiffre de cette section n'est extrapolé.

### 28.1 Méthode

Les 13 formes historiques des §22 à §24 sont recréées **sur l'arbre courant** par
l'inverse écrit à la main du mécanisme du correctif (jamais `git revert` : cf.
§27.5, les commits de 2026-11-11 ne s'appliquent plus). Chaque inverse vit dans
un worktree jetable, est recompilé, puis :

1. le cas de conformance associé doit **échouer** (`check` 0, `build` 1) ;
2. le générateur dirigé par les types tourne avec **3 graines × 200 programmes
   (600), `release_every=0`, 53 règles**, et ses divergences sont inspectées.

L'extension du générateur (étape 5 de la fiche) est un ajout durable : 13 règles
`stmt:power:*` (une par forme historique), plus le test
`power_forms_build_on_the_healthy_tree` qui vérifie que chaque forme est acceptée
par `check`, compilée par `rustc` et exécutée sur l'arbre sain. Le rejeu ciblé
d'une forme se fait par `RAVUM_TYPED_POWER_FORM=k` (générateur étendu, 66 règles).

### 28.2 Tableau des 13 formes — état réel mesuré

`Inverse` = l'inverse a été réalisé et compilé ; `Cas` = le cas de conformance
associé échoue sous cet inverse ; `Campagne` = 3 × 200 programmes exécutés
(600, durée mesurée) ; `Retrouvée` = une divergence de **cette forme** est
apparue dans la campagne initiale (53 règles).

| # | Forme (§) | Cas de conformance | Zone du correctif (`emit.rs`) | Inverse | Cas | Campagne | Retrouvée |
|---|---|---|---|---|---|---|---|
| 1 | `a[0][1]` (§22.1) | `run/array-move-nested` | `build_destructure` : déstructuration récursive des niveaux | oui | **FAIL** | oui (600, 183 s) | non |
| 2 | `h.items[0]` / `t.0[0]` (§22.1) | `run/array-move-projected` | `place_of` : place derrière projection de champ/tuple | oui | **FAIL** | oui (600, 181 s) | non |
| 3 | liaison ombrée (§22.1) | `run/array-move-shadowed` | identité de liaison par déclaration (`declare`/`resolve`) | oui | **FAIL** | oui (600, 177 s) | **oui** (graine `…1003`, index 72) |
| 4 | `a[0].take()` self par valeur (§22.1) | `run/array-move-self-receiver` | `by_value_receiver` : `Receiver::ByValue` des méthodes utilisateur | oui | **FAIL** | oui (600, 302 s) | non |
| 5 | payload de tableau dans un bras de `match` (§22.1) | `run/array-move-match-payload` | `hoist_anchor`/`plan` : liaison `ArmPattern` | oui | **FAIL** | oui (600, 285 s) | non (initiale) ; **oui après extension** |
| 6 | index dynamique imbriqué `a[0][i]`, `a[i][j]` (§23.1) | `run/array-move-nested-dynamic` | `plan_prefix_slots` + `SingleMove::Chain` | oui | **FAIL** | oui (600, 285 s) | non (initiale) ; **oui après extension** |
| 7 | racine qui n'est pas une place `make()[0]`, `a[0].0[1]` (§23.1) | `run/array-move-descent` | boucle finale `descents` : `SingleMove::Chain` | oui | **FAIL** | oui (600, 297 s) | non |
| 8 | plusieurs accès dans un bras en corps d'expression (§23.1) | `run/array-move-arm-multi` | `hoist_anchor` : bloc synthétique du corps de bras | oui | **FAIL** | oui (600, 297 s) | non (initiale) ; **oui après extension** |
| 9 | récepteur *by value* d'un builtin (§23.1) | `run/array-move-builtin-receiver` | `by_value_receiver` : table STD-11.3 | oui | **FAIL** | oui (600, 297 s) | non |
| 10 | deux descentes sur des éléments différents (§23.2) | `run/array-move-shared-descent` (première fonction) | `plan_slot_groups` : groupe par place | **non** | — | **NOT_RUN** | **NOT_RUN** |
| 11 | descente puis lecture d'un autre élément (§23.2) | `run/array-move-shared-descent` (seconde fonction) | `plan_slot_groups` : groupe mono-descente | **non** | — | **NOT_RUN** | **NOT_RUN** |
| 12 | descentes dans deux branches (§24) | `run/array-move-branch-descents` | ancrage sur la plus ancienne instruction du bloc du binder | **non** | — | **NOT_RUN** | **NOT_RUN** |
| 13 | deux descentes dans un bloc imbriqué (§24) | `run/array-move-nested-descents` | idem | **non** | — | **NOT_RUN** | **NOT_RUN** |

**Score brut : 9 formes testées sur 13** (inverse + cas en échec + campagne
exécutée), **4 non testées** (`NOT_RUN`). Sur les 9 testées, la campagne
initiale retrouve **1 forme** (la n° 3) ; après extension du générateur, **4
formes** (3, 5, 6, 8) sont retrouvées. Ce n'est pas un score sur 13.

### 28.3 Campagnes exécutées et divergences

Neuf campagnes de 600 programmes (3 graines × 200) ont été exécutées,
`release_every=0`, 53 règles :

| Inverse | Programmes | Divergences | Dont la forme visée | Durée |
|---|---|---|---|---|
| 1 | 600 | 1 | 0 | 183 s |
| 2 | 600 | 1 | 0 | 181 s |
| 3 | 600 | 2 | **1** (index 72) | 177 s |
| 4 | 600 | 1 | 0 | 302 s |
| 5 | 600 | 1 | 0 | 285 s |
| 6 | 600 | 1 | 0 | 285 s |
| 7 | 600 | 1 | 0 | 297 s |
| 8 | 600 | 1 | 0 | 297 s |
| 9 | 600 | 1 | 0 | 297 s |
| **Total** | **5 400** | **10** | **1** | — |

Neuf des dix divergences sont **la même divergence nouvelle** (graine
`0x00524156554d1003`, index 197), présente sous **chaque** inverse : elle est
donc **antérieure aux inverses**. La dixième (graine `…1003`, index 72) n'existe
que sous l'inverse n° 3 et son programme construit sur l'arbre sain : c'est la
forme ombrée, retrouvée.

Extension (générateur 66 règles), rejeux réellement exécutés :

| Inverse | Règles | Programmes | Divergences | Attribution |
|---|---|---|---|---|
| 5 | 66 | 600 | 60 | **60/60** contiennent `p5_` ou `p8_` (formes 5 et 8) |
| 6 | 66 | 600 | 31 | **31/31** contiennent `p6_` (forme 6) |

Aucun rejeu ciblé `RAVUM_TYPED_POWER_FORM=k` n'a été exécuté pour les autres
formes : elles restent `NOT_RUN` après extension.

### 28.4 Divergence nouvelle sur l'arbre sain — `run/power-array-move-write-region-nested`

Trouvée par le générateur pendant la vague (générateur initial, 53 règles) à
l'index 197 de la graine `0x00524156554d1003`, puis **confirmée sur l'arbre
sain** hors de tout inverse (`check` 0, `build` 1). Elle est réapparue dans
**les neuf campagnes d'inverse** au même index : elle est antérieure aux
inverses. Programme réduit (cas
`conformance/cases/run/power-array-move-write-region-nested`) :

```rv
fn main() {
    let mut a: [String; 2] = [String::from("a"), String::from("b")];
    {
        a = if true { a } else { [String::from("c"), String::from("d")] };
        let s: String = a[0];
        io::println(s.as_str());
        let t: String = a[1];
        io::println(t.as_str());
    }
}
```

Mesure : `check` exit **0** ; `build` exit **1**, rustc
`error[E0508]: cannot move out of type [String; 2], a non-copy array`. Le Rust
généré conserve les deux accès littéraux :

```rust
let s: String = a[0usize];
let t: String = a[1usize];
```

Cause observée : une **écriture de liaison entière** (`a = if c { a } else { … }`)
dans un bloc imbriqué bloque l'abaissement des déplacements d'éléments situés
**après** elle dans ce même bloc ; le `whole use` de `a` dans la branche du RHS
est pris pour un usage bloquant alors que l'écriture ré-initialise la place.
Au niveau de la fonction (sans bloc imbriqué, cas `run/array-move-assign-back`)
la même forme construit correctement : le défaut est propre au bloc imbriqué.
Le programme généré brut (index 197, avant réduction) produit de son côté
`E0382: use of moved value` : deux déstructurations successives de la même place.

Le défaut **n'est pas corrigé** (S6 : signaler, ne pas corriger le compilateur) ;
il est **enregistré** par le cas de conformance ci-dessus, qui échoue. Toute
correction exige une décision de l'orchestrateur.

Campagne nominale réduite sur l'arbre sain, 3 × 200 programmes,
`release_every=0` : générateur **initial** (53 règles), **600 programmes, 1
divergence** (celle-ci, index 197), 417 s ; générateur **étendu** (66 règles),
**600 programmes, 0 divergence**, 274 s — le flux du RNG change avec les règles
ajoutées, donc ce ne sont pas les mêmes programmes. La campagne nominale **3 × 2 000** de la
fiche n'est **pas exécutée** (`NOT_RUN`).

### 28.5 Commandes exactes

| Commande | Résultat |
|---|---|
| `cargo build -p ravum-lang-cli -j 4` (par inverse) | exit 0 |
| `cargo run -p ravum-conformance -- run --filter run/array-move-<forme>` | le cas visé : **FAIL** pour chaque inverse 1…9 |
| `RAVUM_TYPED_PROGRAMS=200 RAVUM_TYPED_RELEASE_EVERY=0 cargo test -p ravum-lang-codegen --test differential_oracle typed_oracle_long -- --ignored --nocapture` | 600 programmes par inverse ; chiffres du tableau |
| `cargo test -p ravum-lang-codegen --test differential_oracle power_forms_build_on_the_healthy_tree -- --nocapture` | 1 test passés : 13 formes acceptées, compilées et exécutées |
| `cargo run -p ravum-conformance -- run --verify-build` (arbre courant) | **485 cas : 483 PASS / 1 FAIL / 1 SKIP / 0 NOT_RUN** ; le seul FAIL est `run/power-array-move-write-region-nested` (E0508) ; `perimeter=208`, `covered=207`, `verified=124`, `implicit=83`, `failed=0`, `not_measured=1` ; **`build_verified: false`** |

### 28.6 Ce que cette passe ne prouve pas

- 4 formes sur 13 ne sont **pas testées** : le score de puissance est `1/9`
  (initiale) et `4/9` (après extension) sur les formes testées, jamais `1/13`.
- La campagne nominale 3 × 2 000 n'a pas été exécutée ; les campagnes d'inverse
  sont des campagnes **réduites** (3 × 200), et le générateur à 66 règles reste
  un sous-ensemble de Core 1.0.
- Les inverses désactivent le mécanisme du correctif ; ils ne rejouent pas le
  commit historique (impossible, cf. §27.5) : ils mesurent un mécanisme désactivé.
- Une inversion peut échouer pour une raison voisine ; les 9 « FAIL » sont
  vérifiés sur le message rustc attendu (`E0508`/`E0382`) et sur la forme réduite.
- La divergence nouvelle est une observation liée à ce SHA, un seul OS, une seule
  toolchain ; elle n'est ni corrigée, ni close.

## 29. Passe 16 — la divergence de la passe 15 était la forme bruyante d'une miscompilation silencieuse (2026-11-15)

> Branche `goal/production-plan-2026-11-15`, Windows 11 x64, `rustc 1.95.0`.
> Mesures locales ; aucun `PROVED`. Décision et conception :
> [`RFC-OWN-ARRAY-ELEMENTS.md`](../rfc/RFC-OWN-ARRAY-ELEMENTS.md).

### 29.1 Constat

`run/power-array-move-write-region-nested` (§28.4) laissait `main` et `dev`
rouges sur les trois OS (runs `43669217867`, `43669218045`, `43669218085`). Avant
de le corriger, deux sondages manuels ont varié la même situation — un tableau
de `String` ré-assigné, des éléments déplacés avant, pendant et après, dans un
bloc, une branche ou une boucle : **32 programmes, dont 30 acceptés par
`check`** (les deux autres étaient déjà refusés).

| Verdict sur l'arbre `581f0f6` | Nombre | Formes |
|---|---|---|
| **sortie fausse, exit 0, aucun diagnostic** | **6** | écriture dans un bloc imbriqué puis move après le bloc ; écriture + move dans une branche puis move après le `if` ; lecture entre l'écriture et les moves ; deux blocs frères ; écriture dans un bloc interne ; écriture puis move dans un `if` imbriqué |
| `check` exit 0, `rustc` `E0382` | 6 | move avant l'écriture imbriquée ; `a = [a[1], a[0]]` dans un bloc ; écriture dans une boucle ; deux écritures ; écriture dans les deux branches ; move, écriture, moves |
| `check` exit 0, `rustc` `E0508` | 5 | la forme de §28.4 ; ré-initialisation de deux éléments puis emprunt du tableau ; ré-initialisation puis emprunt ; écriture par index non littéral dans un tableau partiellement déplacé ; ré-initialisation dans les deux branches |
| correct | 13 | — |
| refusé au `check` | 2 | lecture par index non littéral après un move (`E-OWN-USE-AFTER-MOVE`) ; `a.len()` sur un tableau (`E-RES-UNKNOWN`, sans rapport) |

Soit **17 programmes défaillants sur 30 acceptés**. Les six premières sont plus graves que tout ce que les passes 6 à 15 avaient
mesuré : le programme compile, s'exécute et **affiche les anciennes valeurs**
(`a b` au lieu de `c d`). Cause unique : le déstructurage hissé avant
l'instruction du bloc de la liaison copie les éléments *avant* l'écriture, et les
accès situés *après* l'écriture lisent ces copies.

Le vérificateur avait un défaut indépendant : `a[i] = e` (index non littéral)
effaçait l'enregistrement de déplacement de `a[0]`, comme si l'écriture le
restaurait à coup sûr.

### 29.2 Correctif

Le hissage est supprimé. Chaque accès est abaissé à l'accès par un motif Rust,
et `rustc` suit lui-même les moves par élément (détail dans la RFC). Deux règles
précisent ce que le modèle sait exprimer : `OWN-5.11` (ré-initialisation d'un
élément) et `OWN-5.12` (index non littéral).

### 29.3 Mesure après correctif

| Verdict | Nombre |
|---|---|
| sortie attendue | 28 |
| refusé au `check` (`OWN-5.11`, `OWN-5.12`, `OWN-5.1` ; un `E-RES-UNKNOWN` sans rapport) | 4 |
| sortie fausse | 0 |
| `check` accepte, `rustc` refuse | 0 |

Seize cas rejoignent le corpus : quatorze `run-pass`
(`run/array-move-write-*`, `run/array-move-before-nested-write`,
`run/array-move-swap-in-block`, `run/array-move-two-writes`,
`run/array-move-then-write-in-block`, `run/array-element-reinit-*`) et deux
`compile-fail` (`ownership/array-element-reinit-sibling-moved-fail`,
`ownership/array-dynamic-write-partial-fail`). Le cas de §28.4 passe.

### 29.4 Ce que cette passe ne prouve pas

- Le sondage est manuel et borné à un tableau de deux `String` ; les formes
  voisines (tableaux imbriqués, champs de struct, tuples) passent par le même
  code mais n'ont pas chacune leur campagne.
- Les six sorties fausses n'avaient été trouvées ni par le corpus, ni par
  l'oracle différentiel, ni par le test de puissance : le générateur ne compose
  pas une écriture du tableau entier avec des moves répartis sur plusieurs
  blocs. C'est une lacune de l'oracle, à combler (plan de production, étape E7).
- « Aucune forme ouverte connue » reste une observation sur ce que le corpus et
  ces sondages contiennent.

### 29.5 Vérification E1 (2026-11-15)

Mesures locales, branche `goal/production-plan-2026-11-15`, Windows 11 x64,
`rustc 1.95.0` ; aucune n'est un `PROVED` et la CI des trois OS n'a pas encore
rejoué cette passe.

| Commande | Résultat |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace --no-fail-fast` | **124 groupes, 1 740 passés / 0 échec / 8 ignorés**, exit 0 |
| `cargo run -p ravum-conformance -- run --verify-build` | **503 cas : 502 PASS / 0 FAIL / 1 SKIP / 0 NOT_RUN** ; `perimeter=222 covered=222 verified=124 implicit=98 failed=0` ; `build_verified: true` |
| `cargo test -p ravum-lang-codegen --test differential_oracle differential_oracle_fast` | **3 graines × 200 programmes, 0 divergence** (la campagne nominale 3 × 2 000 reste l'étape E7) |
| `python tools/must_audit.py` | **811 MUST / 467 mappés / 344 non mappés** ; 188/188 FeatureIds |

Tests ajoutés pour verrouiller les formes voisines : sept tests unitaires dans
`own.rs` (ré-initialisation d'un élément avec ou sans voisin déplacé, index non
littéral, champ de struct, tuple, tableau imbriqué) et un test backend qui
compile et exécute des chemins d'éléments sous champ de struct, champ de tuple,
tableau imbriqué, `self` et liaison de `match`. Le corpus gagne les 16 cas
`run/array-*` et `ownership/array-*` listés en §29.3 ; le cas de §28.4 passe.

Reste hors de cette mesure : la conformance et le workspace sur Linux/macOS, la
campagne nominale T2 (E7), la couverture 100 % et la mutation (E6), ainsi que
l'audit externe. La passe ne prouve donc pas l'absence de formes inconnues.

## 30. Passe 17 — les 4 inverses restants exécutés en CI, score sur 13 (2026-11-18)

> Run [43994652442](https://github.com/Rwanbt/Ravum/actions/runs/43994652442)
> (`t2-power.yml`, PR #149), tête `7773a0f`, Linux x86_64, `rustc 1.95.0`,
> conclusion `success`. Chaque forme rejoue
> `bash tools/t2-power/replay-array-descent.sh <forme> 2000` dans un worktree
> jetable : l'inverse n'est jamais commité.

### 30.1 Formes 10 à 13

Chaque job vérifie, dans l'ordre : le test positif passe sur l'arbre sain
(sortie 0) ; sous l'inverse, le test historique et l'oracle sortent à 101 avec
`E0508` ; le générateur produit 3 graines × 2 000 programmes.

| Forme | Job | Générés | Fautes délibérées refusées | Divergences `E0508` |
|---|---|---:|---:|---:|
| 10 | [133743598672](https://github.com/Rwanbt/Ravum/actions/runs/43994652442/job/133743598672) | 6 000 | 561 | 5 439 |
| 11 | [133743598690](https://github.com/Rwanbt/Ravum/actions/runs/43994652442/job/133743598690) | 6 000 | 561 | 5 439 |
| 12 | [133743598666](https://github.com/Rwanbt/Ravum/actions/runs/43994652442/job/133743598666) | 6 000 | 561 | 5 439 |
| 13 | [133743598699](https://github.com/Rwanbt/Ravum/actions/runs/43994652442/job/133743598699) | 6 000 | 561 | 5 439 |

Ligne lue dans chacun des quatre journaux :
`[typed:long] TOTAL generated=6000 faults=561 fault_rejected=561 accepted=0 rejected=0 divergences=5439`.

### 30.2 Score

- **Sensibilité des tests existants : 13/13.** Formes 1 à 9 au §28.2, formes
  10 à 13 ci-dessus : chaque inverse fait échouer son test historique.
- **Détection par le générateur ciblé sur ce run : 4/4** (formes 10 à 13).
- Les campagnes des formes 1 à 9 (§28) ont un autre protocole (3 × 200,
  `release_every=0`) : aucun score de détection homogène sur 13 n'est déduit du
  cumul.
- Campagne nominale (arbre sain) : run
  [43753630956](https://github.com/Rwanbt/Ravum/actions/runs/43753630956),
  6 000 acceptés, 0 divergence.

### 30.3 Ce que cette passe ne prouve pas

Elle ferme l'étape E7 du plan de production (P-9). Elle ne change pas le gate
T2 : T2 reste `PARTIAL` tant que 344 énoncés MUST sur 811 n'ont pas de
FeatureId (D10, `tools/must_audit.py`).

## 31. Lot `diagnostics.md` : 38 MUST sur 45 tracés, une cascade corrigée (2026-11-18)

### 31.1 Mesure

`python3 tools/must_audit.py` : `diagnostics.md` passe de 45 non atteints à
**7** ; dénominateur global 816 MUST, 234 non atteints ; `features_coverage.py
--strict` : 209/209 (1 exemption). Corpus : `cargo run -p ravum-conformance --
run` → `total=602 pass=600 fail=0 skip=2`.

Preuves ajoutées : 13 cas (`cli/diag-*`, `resolve/early-phase-masks-later`),
`crates/ravum-lang-cli/tests/diagnostics_contract.rs` (DIAG-1.8, 4.1, 4.4, 6.5,
6.8, 7.2 : deux exécutions identiques à l'octet, tri par phase, aucun ESC,
aucun chemin absolu, une ligne JSON sans BOM), `tools/diagnostic_codes.snapshot`
et `tools/tests/test_diagnostic_contract.py` (DIAG-9.1/9.2/9.5 : un code publié
n'est jamais retiré ni changé de préfixe ; DIAG-1.2/6.6/9.4 : aucun cas
n'asserte un `message`).

### 31.2 Défaut corrigé

DIAG-4.2 était violé : `let _x: bool = y + 1;` produisait `E-RES-UNKNOWN` **et**
`E-TYP-MISMATCH` (« operator `+` is not defined for `<error>` and `i32` »).
`binary_result_type` rend désormais `Type::Error` sans diagnostic dès qu'un
opérande est déjà en erreur (test `an_unresolved_operand_does_not_cascade_through_operators`,
six opérateurs).

### 31.3 Non tracés, avec la raison

| Règle | Raison |
|---|---|
| DIAG-1.10 | le registre des dépréciations est vide en Core 1.0 : aucun cas de conformance ne peut l'exercer ; couvert par les tests unitaires de `deprecation.rs` uniquement |
| DIAG-5.5 | « span non vide et dans les bornes » : vérifiable seulement sur tous les diagnostics émis ; à ajouter à l'oracle du corpus (vérification générique sur chaque sortie JSON) |
| DIAG-6.4 | aucun diagnostic de Core 1.0 n'émet aujourd'hui de note ni d'aide : la forme `= note:` n'a pas de producteur |
| DIAG-7.3 | règle sur le **consommateur** (ignorer les champs inconnus) ; le runner de conformance la respecte mais aucun test ne le fige |
| DIAG-9.3 | `schema_version` : aucune comparaison automatique entre deux versions du schéma |
| DIAG-9.6 | stabilité de `feature_id`/`rule_id` : FEAT-1.4 le pose, aucun instantané ne le vérifie encore |

DIAG-8.1 était un écart : la règle renvoyait à `run --json`, que `run` refusait. Le
responsable a décidé de l'implémenter (D-18-5) : `run --json` est tracé par F-META-018
(`diagnostics.md` : 39 / 45).

Observation hors MUST : sur un manifeste invalide (`version = "not-a-version"`),
le span de `E-MOD-MANIFEST` couvre `[package]` (octets 0–9) et non la valeur
fautive. Ce n'est pas une violation de DIAG-1.3 (le fichier et les offsets sont
valides), mais le span est imprécis.

## 32. Lot `compatibility.md` : 14 MUST sur 39 tracés, deux textes alignés (2026-11-18)

### 32.1 Mesure

`python3 tools/must_audit.py` : `compatibility.md` passe de 39 non atteints à
**25** ; global 816 MUST, **218** non atteints après fusion de #171 ; `features_coverage.py --strict` :
215/215 (1 exemption). FeatureIds F-META-019 à F-META-023.

Preuves ajoutées : 6 cas (`packages/two-versions-same-name`,
`packages/dependency-other-edition`, `packages/dependency-digest-exposed`,
`cli/compat-json-declares-spec-version`, `cli/compat-no-migrate-command`,
`cli/compat-cache-clean`) ; `crates/ravum-lang-cli/tests/compat_contract.rs`
(COMPAT-4.3 : changer le `path` d'une dépendance change `dependencies_digest`
et rend le lockfile `stale` ; COMPAT-1.4/7.1 : un lockfile invalide est refusé
par `E-MOD-LOCK`, laissé intact, et aucun binaire n'est produit) ;
`tools/rule_ids.snapshot` et `tools/feature_ids.snapshot` (DIAG-9.6,
COMPAT-2.9 : un identifiant publié n'est jamais renommé ni retiré, et tout
nouvel identifiant doit y être ajouté).

### 32.2 Textes alignés

| Règle | Avant | Après | Raison |
|---|---|---|---|
| COMPAT-3.6 | une API dépréciée « n'émet aucun avertissement » | chaque usage émet `W-DEPRECATED` | contredisait DIAG-1.6/1.10, décision D-18-3 |
| COMPAT-4.2 | deux versions du même nom → `E-MOD-DEPENDENCY` | → `E-MOD-DUPLICATE` | le registre canonique (`diagnostics.md` §3) attribue « deux packages de même nom » à `E-MOD-DUPLICATE`, et l'implémentation l'émet |

### 32.3 Non tracés : règles de gouvernance

Les 25 règles restantes décrivent une **politique** (ce qui est une rupture,
quand créer une édition majeure, comment migrer ou revenir en arrière), pas un
comportement de l'outil : COMPAT-1.2, 2.1 à 2.8, 3.1 à 3.6, 4.5, 5.2, 6.1 à 6.3,
6.5, 7.2, 7.3, 7.5, 7.6. Aucun cas de conformance ne peut les exercer sur une
seule édition. Leur contrôle relève de la revue (D2, ratification) ; les seules
parties mécanisables le sont déjà (instantanés d'identifiants et de codes).

## 33. Lot `abi.md` : 14 MUST sur 35 tracés, une contradiction à arbitrer (2026-11-18)

### 33.1 Mesure

`python3 tools/must_audit.py` : `abi.md` passe de 35 non atteints à **21** ;
global 816 MUST, **206** non atteints ; `features_coverage.py --strict` :
219/219 (1 exemption). FeatureIds F-META-024 à F-META-027.

La plupart des preuves existaient déjà sans être citées : `backend_audit.rs`
(absence d'`unsafe` hors wrapper FFI, déterminisme du Rust généré sur tout le
corpus, contrôles de débordement et de bornes) et les cas `types/ffi-*`,
`run/ffi-*`, `run/integer-*`, `run/panic`. Ajouts : deux cas sur le fichier
émis (`cli/abi-emit-rust-forbids-unsafe` : `#![forbid(unsafe_code)]` ;
`cli/abi-emit-rust-includes-prelude` : hook de panic dans le fichier unique) et
un test qui refuse toute crate `rustc_*` dans `Cargo.lock` (ABI-2.7).

Observation : au premier passage, `cli/abi-emit-rust-forbids-unsafe` a échoué
une fois sans détail affiché ; 20 passages suivants ont réussi (0/15 échec en
boucle). La cause n'est pas identifiée ; la CI sur trois OS dira si elle se
reproduit.

### 33.2 Contradiction à arbitrer : ABI-4.7 et le `#![allow]` du prélude

ABI-4.7 dit : « un avertissement `rustc` est un défaut à corriger, pas à
masquer (`#[allow]` interdit) ». Le fichier généré commence par un
`#![allow(...)]` de crate qui couvre 17 lints, dont `arithmetic_overflow` et
`unconditional_panic`. Ces deux-là sont voulus : sans eux, `rustc` refuserait
à la compilation un débordement que la spécification veut voir paniquer à
l'exécution (`backend_audit.rs` : `audit_constant_foldable_panics_are_runtime_panics`).
Les autres (`unused_*`, `non_snake_case`…) masquent des avertissements sur du
code généré.

L'implémentation ne respecte donc pas le texte, et le texte ne décrit pas une
contrainte tenable telle quelle. ABI-4.7 n'est pas tracé. Deux issues :

1. **Recommandé** : amender ABI-4.7 en « une seule liste `allow` fixe, de
   crate, documentée lint par lint et auditée par un test ; aucun `allow` local ;
   aucun avertissement hors de cette liste ». Le test d'audit fige la liste.
2. Retirer l'`allow` et faire émettre au backend du code sans avertissement :
   travail important (noms, variables inutilisées), et `arithmetic_overflow`
   reste nécessaire pour la sémantique des paniques.

### 33.3 Non tracés, avec la raison

| Règles | Raison |
|---|---|
| ABI-1.1 à 1.4, 2.9, 2.10, 5.1 à 5.3, 6.1 à 6.4 | énoncés de **refus** ou de non-garantie (« aucun ABI », « aucune garantie de layout ») : rien d'observable à exercer |
| ABI-2.1, 2.6, 4.4 | sémantique du source et ordre d'évaluation : couverts indirectement par tout le corpus `run/*`, mais aucune ligne de `features.md` ne les cite comme règle |
| ABI-2.8 | tailles des primitifs : pas d'opérateur `size_of` en Core 1.0 ; seules les bornes sont observables |
| ABI-4.1 | un `CheckedProgram` invalide est un bug interne, pas une entrée |
| ABI-4.7 | contradiction, §33.2 |
| ABI-4.8, 4.9 | non-engagements (temps de compilation, format de `--emit-rust`/`--emit-ir`) |

## 34. Lot `modules.md` : 18 MUST de plus tracés, un défaut DIAG-6.8 trouvé (2026-11-18)

### 34.1 Mesure

`python3 tools/must_audit.py` : `modules.md` passe de 21 non atteints à **3** ;
global 816 MUST, **188** non atteints ; `features_coverage.py --strict` :
222/222 (1 exemption). FeatureIds F-MOD-014 à F-MOD-016.

Douze cas ajoutés, chacun mesuré avant d'être écrit : résolution des fichiers
(`mod x;` d'un module fichier non racine → `src/x.rv` ; d'un module inline →
`src/m/x.rv` ; casse exacte ; fichier absent), manifeste et graphe
(auto-dépendance, `git`, plage `^1.0`), usage d'une dépendance (répertoire
libre, item privé, dépendance non déclarée, effets propagés, `io.println`).

### 34.2 Texte aligné

MOD-8.5 annonçait `E-MOD-DEPENDENCY` pour deux packages de même nom ; le
registre et le loader émettent `E-MOD-DUPLICATE` (même correction que
COMPAT-4.2, §32.2).

### 34.3 Défaut trouvé : chemins absolus dans les messages du loader

`mod gone;` produit : « module `b` not found: neither
`/tmp/.../src/b.rv` nor `/tmp/.../src/b/mod.rv` exists ». Le message contient
le chemin absolu de la machine, ce que DIAG-6.8 et DIAG-7.5 interdisent. Le
défaut n'est pas propre à ce message : environ 60 messages du loader
(`crates.rs`, `digest.rs`, `files.rs`, `fsutil.rs`, `graph.rs`, `lockfile.rs`)
écrivent `path.display()`. `tests/diagnostics_contract.rs` ne l'a pas vu parce
que son programme n'a pas d'erreur de module. Correction : PR dédiée qui suit
ce lot (chemins relatifs à la racine du package, et un cas par famille de
message dans le test de contrat).

### 34.4 Défaut trouvé par la CI : la casse sur macOS et Windows

Le cas `modules/module-name-case-sensitive` passait sous Linux et échouait sous
macOS et Windows (CI de #178, commit 33b9d9e : `check --json: exit 0`). Sur un
système de fichiers insensible à la casse, `mod Foo;` trouvait `src/foo.rv` :
le même source était accepté ou refusé selon l'OS, ce que MOD-2.6 (« ne dépend
ni [...] de la casse ») et MOD-1.6 interdisent. Correction : un candidat ne
compte que si le répertoire contient une entrée de nom **exactement** égal
(`has_exact_entry`, comparaison octet à octet de la liste du répertoire).

### 34.4 Non tracés

MOD-6.5 (aucun plugin : rien d'observable), MOD-6.6 (dépendances compilées
depuis la source : invariant de construction), MOD-8.6 (compatibilité
déclarative : non-engagement).
## 35. Lot `concurrency.md` : 8 MUST sur 20 tracés, état d'avant le RFC E11 (2026-11-18)

### 35.1 Mesure

`python3 tools/must_audit.py` : `concurrency.md` passe de 20 non atteints à
**12** ; `features_coverage.py --strict` : 220/220 (1 exemption). FeatureId
F-META-028. Cinq cas `unsupported/*` (`Mutex`, `Arc`, `effects(async)`,
`async { }`, `T: Send`), chacun mesuré avant d'être écrit, et
`crates/ravum-lang-cli/tests/exec_determinism.rs` (CONC-3.4 : quatre exécutions
d'un programme fermé donnent les mêmes octets).

### 35.2 Portée

D-18-2 met la concurrence dans la 1.0 (étape E11) : le RFC de concurrence
réécrira `concurrency.md`. F-META-028 décrit donc l'**état actuel** (« aucune
primitive ») et devra être remplacé, sous un nouvel identifiant (FEAT-1.4), le
jour où le RFC est accepté. Les cas `unsupported/*` changeront alors de verdict
par décision, pas par accident.

### 35.3 Non tracés

CONC-1.4, 2.1, 2.3, 2.4, 2.5, 2.7, 3.1, 3.3, 4.1, 4.2, 4.4 : énoncés de
non-garantie ou de politique d'évolution, rien d'observable. CONC-3.2 (« un
rapport `rt` ne contient jamais `CERTIFIED` ») est vérifiable mais pas par un
cas : il faudrait une assertion d'absence ; à ajouter au test de contrat du
profil `rt` lors du lot `effects.md`.

## 36. Lot `stdlib.md` : 10 MUST de plus tracés, une contradiction à arbitrer (2026-11-19)

### 36.1 Mesure

`python3 tools/must_audit.py` : `stdlib.md` passe de 16 non atteints à **6** ;
dénominateur 817 MUST (STD-12.7 ajouté, §36.2) ; `features_coverage.py
--strict` : 225/225 (1 exemption). FeatureIds F-STD-033 à 035. Quatorze cas
`types/std-*`, chacun mesuré avant d'être écrit (liste fermée, `impl String`,
ajustements implicites, effets de `env::args`, absence d'écriture
d'environnement et de mode d'ouverture, formes arithmétiques explicites).

### 36.2 Texte aligné sur un RFC accepté

STD-12.6 interdisait `wrapping_add`, `checked_add` et `saturating_*`, alors que
[RFC-RT-TRAPS](../rfc/RFC-RT-TRAPS.md) (`ACCEPTED` le 2026-11-14) les a ajoutés
(commit 21533dd), et que les cas `effects/rt-trap-*` s'en servent. Le code avait
suivi le RFC, pas la spécification. STD-12.6 ne les cite plus, et **STD-12.7**
(nouvel identifiant, ajouté à `tools/rule_ids.snapshot`) décrit les 14 formes
réellement implémentées : `wrapping_add/sub/mul/neg`, `saturating_add/sub/mul/neg`,
`checked_add/sub/mul/div/rem/neg`.

### 36.3 Contradiction à arbitrer : STD-1.1 et les modules sans `use`

STD-1.1 (et `modules.md` §8) dit : « aucun module n'est dans le prélude :
`io::println` exige `use std::io;` ». L'implémentation résout `io`, `fs`,
`string`, `math`, `env` et `process` sans `use` (`builtins::PRELUDE_MODULES`),
et quasiment tout le corpus et tous les exemples en dépendent. STD-1.1 n'est
pas tracé.

1. **Recommandé** : amender STD-1.1 et `modules.md` §8 pour dire ce que fait
   l'outil (les modules `std` sont des racines résolues sans `use` ; `use
   std::io;` reste admis), en gardant le refus de `io.println` (MOD-6.7, déjà
   tracé). Aucun programme existant ne change.
2. Appliquer le texte : faire échouer tout `io::println` sans `use`. Cela
   casse presque tous les cas et exemples ; c'est une rupture de langage.

### 36.4 Non tracés

STD-1.1 (§36.3), STD-1.2 (std non déclarable : invariant du loader),
STD-1.4 (signatures normatives : couvert par l'ensemble des cas `run/*`, sans
lien de règle), STD-1.7 (messages non stables : non-engagement), STD-3.1
(aucune normalisation de chemin à l'exécution : nécessite un cas avec E/S
fichier hors du répertoire du cas), STD-6.8 (pas de garantie de précision :
non-engagement).

## 37. Lot `effects.md` : 6 MUST de plus tracés (2026-11-19)

`python3 tools/must_audit.py` : `effects.md` passe de 15 non atteints à **9** ;
`features_coverage.py --strict` : 227/227 (1 exemption). FeatureId F-EFF-010.
Cinq cas `effects/*`, chacun mesuré avant d'être écrit : profil inconnu →
sortie 2 (EFF-4.10) ; `summary.effects` expose l'ensemble inféré (`twice` : `[]`,
`main` : `io, alloc`) (EFF-5.5) ; `#[allow(effects)]` → `E-UNSUPPORTED-ATTR`
(EFF-5.6) ; `String::new` dans `effects()` → `E-EFF-UNDECLARED`, jamais élargi
(EFF-5.3, EFF-6.5) ; le rapport `--profile pure` nomme son profil (EFF-6.7).

Non tracés : EFF-4.8, 5.1, 5.2, 6.1 à 6.3, 6.6 (non-garanties : aucun bac à
sable, aucune borne de temps ou de mémoire, aucun effet d'exécution) ; EFF-4.9
(un profil ne change pas la sémantique) demanderait d'exécuter sous un profil,
or `run` n'a pas d'option `--profile` ; EFF-5.4 (effets du prélude non
redéclarables) n'a pas de forme refusable : un utilisateur ne peut pas déclarer
une fonction du prélude.

## 38. Lot `ownership.md` : 11 MUST de plus tracés (2026-11-19)

`python3 tools/must_audit.py` : `ownership.md` passe de 11 non atteints à **0**
(73 / 73) ; `features_coverage.py --strict` : 230/230 (1 exemption). FeatureIds
F-OWN-017 (déplacements), F-OWN-018 (mutabilité), F-OWN-019 (phases et budget).
Treize cas `ownership/*`, chaque code mesuré avant d'être écrit.

### 38.1 OWN-11.3 nommait le mauvais code

La spécification disait qu'au-delà du budget par fonction le compilateur émet
`E-PARSE-TOO-LARGE`. Le compilateur émet `E-OWN-LIMIT` (enregistré dans
`diagnostics.md`) après 50 diagnostics, et `own.rs` documente pourquoi : la borne
par fonction doit rester sous le budget par fichier (100, DIAG-1.7), sinon elle
ne serait jamais observable. Mesure : 60 usages après déplacement dans `main` →
50 `E-OWN-USE-AFTER-MOVE` puis un `E-OWN-LIMIT` ; la fonction suivante est encore
analysée. La règle est corrigée pour nommer `E-OWN-LIMIT` ; le budget par
fichier reste `E-PARSE-TOO-LARGE`. Même traitement que COMPAT-3.6 et MOD-8.5.

### 38.2 Deux déplacements par argument

`eat(s); eat(s);` donne `E-OWN-USE-AFTER-MOVE`, alors que `let a = s; let b = s;`
donne `E-OWN-DOUBLE-MOVE`. C'est conforme à OWN-5.1, qui range « passage en
argument » parmi les usages d'une place `MOVED` ; OWN-5.2 ne vise que deux
déplacements sans usage entre eux. Signalé pour lecture, aucun changement.

OWN-11.2 (tri par `(fichier, start_byte, code)`) est tenu par le test
`diagnostics_of_one_phase_are_sorted_by_file_byte_and_code` de `diagnostics_contract.rs` (DIAG-4.1).

## 39. Lots `features.md` et `unsupported.md` : 7 MUST de plus tracés (2026-11-19)

`python3 tools/must_audit.py` : `features.md` passe de 11 non atteints à **5**,
`unsupported.md` de 8 à **7** ; `features_coverage.py --strict` : 231/231
(1 exemption). FeatureId F-META-029, tenu par `tools/tests/test_feature_table.py`
(4 tests) : FEAT-1.3 et 12.3 (ligne `unsupported` → code listé dans
`unsupported.md` et cas `compile-fail` qui l'attend), FEAT-12.4 (code cité dans
une ligne FeatureId → enregistré), FEAT-12.5 (lignes `experimental` = liste de la
règle), FEAT-1.6 (aucun code `reserved`, aucun cas n'attend un code hors
registre) ; FEAT-1.4 par `feature_ids.snapshot` ; UNS-0.2 par le cas
`unsupported/unicode-ident`.

### 39.1 Trou trouvé par le test : identifiants Unicode

F-UNS-011 refuse `let café = 1;` avec `E-UNSUPPORTED-IDENT` (LEX-4.2), mais
`unsupported.md` ne listait pas cette forme : FEAT-1.3 n'était pas tenue. La
ligne est ajoutée en §2. Le test échouait avant l'ajout et passe après.

### 39.2 FEAT-1.5 contredit le corpus (au Lead)

FEAT-1.5 exige qu'un cas contienne `pass.rv`, `fail.rv` et `expected.json`. Le
corpus réel (662 cas) utilise `case.json` + `src/main.rv`, un cas par
direction, et le harnais ne lit que cette forme. Aucun cas ne suit FEAT-1.5.
Recommandation : amender FEAT-1.5 pour décrire la forme `case.json` (déjà
documentée dans `conformance/README.md`). Non modifié ici : c'est une décision
de spécification.

### 39.3 Non tracés, et pourquoi

FEAT-1.1, FEAT-1.2, FEAT-12.1 définissent des statuts (aucune forme
observable) ; FEAT-12.2 (« toute forme acceptée a une ligne ») n'est pas
prouvable par un test fini, seulement réfutable par un audit. UNS-0.1 (même
nature que FEAT-12.2), UNS-0.3 (autorité du checker : tenue par le contrat
`check → build` de T2, pas par une ligne), UNS-9.1 à 9.5 (procédure de
proposition d'extension, sans forme exécutable).

## 40. Lots `types.md` et `expressions.md` : 10 MUST de plus tracés (2026-11-19)

`python3 tools/must_audit.py` : `types.md` passe de 6 non atteints à **0**
(94 / 94), `expressions.md` de 4 à **0** (86 / 86) ; `features_coverage.py
--strict` : 233/233 (1 exemption). FeatureIds F-TYP-028 et F-EXPR-019, dix cas,
chaque sortie mesurée avant d'être écrite :

- TYP-13.3 : branches `i32`/`bool` → `E-TYP-MISMATCH` ; `if c { a } else { 2 }`
  avec `a: i64` affiche `5`.
- TYP-13.5 à 13.9 : `String as i64` et `&i64 as i64` → `E-TYP-CAST` ;
  `NaN as i32` = `0`, `1e300 as i32` = `2147483647`, `-1e300 as i32` =
  `-2147483648`, `300 as u8 as i64` = `44`, `7 as i32` = `7`, sans panic.
- EXPR-3.4 : `a << -1` → `E-TYP-SHIFT` ; une quantité `u8` décale un `i64`.
- EXPR-3.6, 3.7 : débordement `i128` → `panic:`, sortie 1.
- EXPR-3.7, 3.8 : sous `test`, un test qui panique est `FAILED` et l'autre
  passe (`failed: 1`, `passed: 1`) ; `catch_unwind` → `E-RES-UNKNOWN`.

### 40.1 Quantité de décalage négative : refusée à la compilation (D-19-1)

Constat : EXPR-3.4 refusait une quantité négative, mais seulement pour un
littéral ; `let s: i64 = -1; a << s` compilait puis paniquait. Décision du
responsable (D-19-1) : la détection doit avoir lieu à la compilation, même en
variable. Le checker (`typeck/helpers.rs`, `shift_amount_refused`) accepte une
quantité non signée ou une constante de littéraux de valeur `>= 0`, et refuse
toute autre quantité signée (`E-TYP-SHIFT`), pour `<<`, `>>`, `<<=`, `>>=`.
Quatre cas `run/shift-*` de plus. Impact mesuré : aucun exemple, application ni
cas du corpus n'utilisait une quantité signée en variable ; trois tests de
contrat du checker qui l'acceptaient sont mis à jour (`typeck_contracts2.rs`,
`typeck_contracts3.rs`).

## 41. `compatibility.md`, 2ᵉ lot : 5 MUST de plus tracés (2026-11-19)

`python3 tools/must_audit.py` : `compatibility.md` passe de 25 non atteints à
**20** (19 / 39) ; `features_coverage.py --strict` : 234/234 (1 exemption).
FeatureId F-META-030 :

- COMPAT-1.2 : `--version` affiche `core-1.0-draft.1` et chaque rapport JSON
  porte `"spec_version":"core-1.0-draft.1"` (cas `cli/version-says-draft`,
  `cli/json-spec-version-says-draft`).
- COMPAT-2.8 : seule `core-1.0` est acceptée, `core-2.0` → `E-UNSUPPORTED-EDITION`.
- COMPAT-3.6 : tests unitaires de `deprecation.rs` (registre vide, un
  `W-DEPRECATED` par usage d'un chemin déprécié, programme accepté).
- COMPAT-4.5 : `recompile_from_source.rs` lance `run`, change la source (même
  longueur, même chemin de binaire), relance : la nouvelle sortie est affichée.
- COMPAT-7.6 : une forme refusée par `core-1.0` reste refusée.

Non tracés, et pourquoi : COMPAT-2.1 à 2.7, 3.1 à 3.5 décrivent ce qu'une
**évolution future** du langage a le droit de faire ; aucune édition ratifiée
n'existe, il n'y a rien à exécuter (le gel des codes publiés est déjà tenu par
`diagnostic_codes.snapshot`, DIAG-9). COMPAT-5.2 oblige les **consommateurs**,
pas l'outil. COMPAT-6.1 à 6.5 et 7.2 à 7.5 décrivent la procédure humaine de
migration et de rollback ; ils relèvent du dossier de release (T11, T12), pas
d'un cas de conformance.

## 42. `abi.md` et `concurrency.md`, 2ᵉ lot : 3 MUST de plus tracés (2026-11-19)

`python3 tools/must_audit.py` : `abi.md` 16 / 35, `concurrency.md` 9 / 20 ;
`features_coverage.py --strict` : 235/235 (1 exemption). FeatureId F-META-031,
chaque sortie mesurée avant d'être écrite :

- ABI-2.8 : `u8` 255, `i8` -128, `u16` 65535, `i32` 2147483647, `u64`
  18446744073709551615, `i128` minimum ; `let a: u8 = 256` → `E-TYP-LITERAL-RANGE`.
- ABI-4.4 : `f("a", 1) + f("b", 2) * f("c", 3)` affiche `a`, `b`, `c` puis `7` ;
  `0.1 * 10.0 - 1.0` affiche `0` (une FMA donnerait `5.55e-17`) et
  `(1e16 + 1) - 1e16` affiche `0` (une réassociation donnerait `1`).
- CONC-3.2 : `rt_profile_claims.rs` lance `check --profile rt`, texte et JSON,
  sur un programme accepté et un programme refusé : aucun ne contient
  `CERTIFIED`.

Non tracés, et pourquoi : ABI-1.x, 2.9, 2.10, 4.8, 4.9, 5.x, 6.x et CONC-2.x,
3.1, 3.3, 4.x sont des **non-garanties** ou des conditions sur une extension
future (rien à exécuter : on ne teste pas l'absence d'une promesse). ABI-2.1 et
2.6 sont tenus en pratique par le contrat `check → build` de T2, sans ligne
propre. ABI-4.1 (un `CheckedProgram` invalide n'atteint jamais le backend)
demanderait d'observer l'absence de binaire, ce que le harnais ne sait pas
affirmer. ABI-4.7 reste en arbitrage (`#![allow]` du fichier généré, §33.2).
CONC-1.4 et 2.7 (un seul cœur, concurrence externe) ne sont pas mesurables par
un cas.
