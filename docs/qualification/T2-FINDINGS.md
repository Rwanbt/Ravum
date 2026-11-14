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
