# Ravum Core 1.0 — Table de fermeture des fonctionnalités (`FeatureId`)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` (intégral) + décisions du Lead 2026-11-09.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> **But** : aucune fonctionnalité ne doit être absente de la spécification, ni supportée « par accident ». Toute forme acceptée par le checker et le backend **MUST** avoir une ligne ici ; toute ligne `unsupported` **MUST** figurer dans [`unsupported.md`](unsupported.md).

## 1. Conventions

| Colonne | Sens |
|---|---|
| `FeatureId` | identifiant stable `F-<GROUPE>-<NNN>`, référencé par les diagnostics (`feature_id`) |
| `Fonctionnalité` | nom court et formes concernées |
| `Syntaxe` | référence normative (`grammar.md`, `lexical.md`) |
| `Typage` | référence normative (`types.md`) ou règle courte |
| `Runtime` | comportement observable (`expressions.md`, `stdlib.md`, `core-machine.md`) |
| `Pass` / `Fail` | cas attendus : `pass` = programme accepté et résultat attendu ; `fail` = diagnostic attendu |
| `Backend` | `B` = voie B (`rustc` généré, design §1) ; `—` = non supporté |
| `Statut` | `candidate-stable` \| `experimental` \| `unsupported` |
| `Conformance` | présence **réelle** d'un cas dans `conformance/cases/` (le corpus existe : **106 cas**, worker qa). La présence d'un cas n'est **pas** un `PASS` : le statut d'exécution est tenu par `ravum-conformance run`. |

| ID | Règle | Niveau |
|---|---|---|
| FEAT-1.1 | Le statut `candidate-stable` signifie « spécifié, sans décision ouverte bloquante » ; il ne signifie **pas** « implémenté ni testé ». | MUST |
| FEAT-1.2 | `experimental` signifie « décidé pour cette itération mais susceptible de changer » ; l'usage est permis, la stabilité non promise. | MUST |
| FEAT-1.3 | `unsupported` **MUST** produire le code indiqué et **MUST** apparaître dans [`unsupported.md`](unsupported.md). | MUST |
| FEAT-1.4 | Un `FeatureId` est **immuable** une fois publié ; un ajout crée un nouvel identifiant. | MUST |
| FEAT-1.5 | Un cas de conformance **MUST** contenir `pass.rv` (et la sortie attendue) et `fail.rv` (avec `expected.json` listant les codes). | MUST |
| FEAT-1.6 | Un test qui attend un code `reserved` ([`diagnostics.md`](diagnostics.md) §3) **MUST** être marqué `PENDING_IMPL`, jamais `PASS`. | MUST |

Chemin cible conventionnel : `conformance/cases/<FeatureId>/pass.rv`, `.../fail.rv`, `.../expected.json`.

## 2. Lexique (`F-LEX`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-LEX-001 | UTF-8 (BOM de tête **accepté et ignoré**) | lexical §2 | — | offsets absolus, premier token à l'octet 3 | pass : BOM + code ; fail : UTF-8 invalide → `E-UNSUPPORTED-TOKEN` | B | candidate-stable | cases/lexer/bom-accepted, cases/lexer/empty-file |
| F-LEX-002 | Commentaires `//`, `/* */`, doc `///`, `//!` | lexical §6 | — | trivia, aucun effet | pass : bloc non imbriqué ; fail : `/*` non fermé → `E-LEX-COMMENT` | B | candidate-stable | cases/lexer/comments-valid, cases/lexer/unterminated-comment |
| F-LEX-003 | Identifiants ASCII | lexical §4 | — | — | pass : `_x1` ; fail : `café`, `λ` → `E-UNSUPPORTED-IDENT` | B | candidate-stable | cases/lexer/unicode-ident-rejected |
| F-LEX-004 | 33 mots-clés réservés + 2 contextuels (`effects`, `str`) | lexical §5 | — | — | pass : `let effects = 1;` hors signature ; fail : `let fn = 1;` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/lexer/contextual-str-ident, cases/lexer/contextual-effects-ident, cases/lexer/reserved-keyword-ident |
| F-LEX-005 | Entiers déc/hex/oct/bin, `_`, suffixes | lexical §8.1 | TYP-13.2 | valeurs exactes | pass : `0xFF_FFu16` ; fail : `0x_FF` → `E-LEX-NUMBER`, `10z` → `E-LEX-INT-SUFFIX`, `2^128` → `E-LEX-INT-OVERFLOW` | B | candidate-stable | cases/lexer/literals-valid |
| F-LEX-006 | Flottants IEEE (formes) | lexical §8.2 | TYP-13.2 | `f64` défaut | pass : `1.5e-3f32` ; fail : `1.` → `E-LEX-NUMBER`, `1e999` → `E-LEX-NUMBER` | B | candidate-stable | cases/lexer/literals-valid |
| F-LEX-007 | Char + 9 échappements | lexical §8.3, §8.5 | `char` | scalaire Unicode | pass : `'é'`, `'\u{1F600}'` ; fail : `'ab'` → `E-LEX-CHAR`, `'\q'` → `E-LEX-ESCAPE` | B | candidate-stable | cases/lexer/escapes-valid |
| F-LEX-008 | String + échappements ; refus byte/raw/lifetime | lexical §8.4 | `&'static str` | contenu exact, LF brut autorisé | pass : `"a\n"` ; fail : `"\xFF"` → `E-LEX-ESCAPE`, `r"x"` → `E-UNSUPPORTED-TOKEN` | B | candidate-stable | cases/lexer/escapes-valid, cases/lexer/unterminated-string, cases/run/unicode |

## 3. Syntaxe, items, instructions (`F-SYN`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-SYN-001 | `fn` + paramètres + retour | G-3.1, G-3.4 | TYP-11 | appel par valeur | pass : `fn f(a: i32) -> i32` ; fail : `-> ` sans type → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/fn-params-return-pass, cases/parser/fn-params-return-fail |
| F-SYN-002 | Génériques nus `<T, U>` | G-3.5 | TYP-11 | monomorphisation | pass : `fn id<T>(x: T) -> T` ; fail : `where` → `E-UNSUPPORTED-WHERE` | B | candidate-stable | cases/parser/generics-naked-pass, cases/parser/generics-where-fail |
| F-SYN-003 | Clause `effects(...)` | G-3.2 | EFF-3 | vérifiée statiquement | pass : `effects(io)` ; fail : groupe inconnu → `E-EFF-UNKNOWN` | B | candidate-stable | cases/parser/effects-position |
| F-SYN-004 | `test fn` + retour `()`/`Result` | G-3.6 | E-TYP-MAIN | `test` exécute, `Err` = échec | pass : `test fn t() -> Result<(), Error>` ; fail : paramètre → `E-PARSE-EXPECTED` | B | experimental | cases/cli/test-demo, cases/run/tests-demo-run |
| F-SYN-005 | `struct` (+ champs `pub`/privés) | G-4.1–4.2 | TYP-7 | agrégat | pass : `struct P { pub x: f64, y: f64 }` ; fail : champ sans type → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/struct-fields-pass, cases/parser/struct-field-no-type-fail |
| F-SYN-006 | `enum` (unité/tuple/record) | G-4.3 | TYP-15 | discriminant | pass : 3 variantes ; fail : variante mixte → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/enum-variants-pass, cases/parser/enum-variant-mixed-fail |
| F-SYN-007 | `impl` inhérent (`&self`, `&mut self`, `self`, associé) | G-4.5–4.7 | TYP-11.5 | dispatch statique | pass : `impl P { fn new() -> P }` ; fail : `impl i32` → `E-RES-IMPL-SELF-TYPE` | B | candidate-stable | cases/parser/impl-inherent-pass, cases/parser/impl-foreign-type-fail |
| F-SYN-008 | `const` + `const_expr` | G-4.8–4.9, G-5.11 | E-TYP-CONST (R) | valeur compile-time | pass : `const N: i32 = 10;` ; fail : `const N = 1;` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/const-typed-pass, cases/parser/const-no-type-fail |
| F-SYN-009 | `mod` inline | G-4.10 | MOD-1 | arbre de modules | pass : `mod geo { pub fn f() {} }` ; fail : `mod m {} ;` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/modules/inline-module |
| F-SYN-010 | `mod` fichier + résolution | MOD-1.3, §2 | E-MOD-* | chargement source | pass : `src/foo.rv` ; fail : absent → `E-MOD-MISSING`, ambigu → `E-MOD-AMBIGUOUS` | B | candidate-stable | cases/modules/file-module |
| F-SYN-011 | `use` simple (crate/super/self/std/dep) | MOD-3 | E-RES-* | aucun effet | pass : `use crate::geo::P;` ; fail : glob/alias/reexport → `E-UNSUPPORTED-TOKEN` | B | candidate-stable | cases/resolve/import-duplicate, cases/resolve/import-unknown |
| F-SYN-012 | `pub` sur items/champs/modules | MOD-4 | E-RES-PRIVATE | visibilité statique | pass : item `pub` ; fail : privé hors sous-arbre → `E-RES-PRIVATE` | B | candidate-stable | cases/modules/private-field |
| F-SYN-013 | `let` (mut, type, initialisation différée) | G-6.2–6.4 | OWN-4 | liaison | pass : `let mut x: i32; x = 1;` ; fail : pattern de `let` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/let-forms-pass, cases/parser/let-pattern-fail |
| F-SYN-014 | Affectation `=` et **famille complète** des composés (`+= -= *= /= %= &= \|= ^= <<= >>=`) | G-6.8 | EXPR-9 | remplace, détruit l'ancien | pass : `x -= 1;`, `x <<= 2;` ; fail : `x **= 2;` → `E-PARSE-UNEXPECTED` | B | candidate-stable | cases/parser/assign-in-condition |
| F-SYN-015 | `return`/`break`/`continue` | G-6.6–6.7 | E-TYP-RETURN, E-TYP-BREAK-OUTSIDE | abandon de continuation | pass : `return x;` ; fail : `break;` hors boucle → `E-TYP-BREAK-OUTSIDE` | B | candidate-stable | cases/parser/return-break-continue-pass, cases/parser/break-outside-loop-fail |
| F-SYN-016 | Bloc, `tail_expr`, bloc-like sans `;` | G-6.1, G-6.5 | E-TYP-STATEMENT-VALUE | valeur de bloc | pass : `{ if c { return 1; } 2 }` ; fail : `{ 1; }` → `E-TYP-STATEMENT-VALUE` | B | candidate-stable | cases/parser/block-tail-expr-pass, cases/parser/block-statement-value-fail |
| F-SYN-017 | `if`/`else if`/`else` expression | G-6.20 | E-TYP-IF-NO-ELSE | une branche | pass : `let x = if c { 1 } else { 2 };` ; fail : `let x = if c { 1 };` → `E-TYP-IF-NO-ELSE` | B | candidate-stable | cases/parser/if-else-expression-pass, cases/parser/if-no-else-value-fail |
| F-SYN-018 | `match` + bras (expr/bloc, virgules) | G-6.21–6.22 | E-TYP-NONEXHAUSTIVE | premier bras correspondant | pass : `match x { 0 => 1, _ => 2 }` ; fail : non exhaustif → `E-TYP-NONEXHAUSTIVE` | B | candidate-stable | cases/types/nonexhaustive |
| F-SYN-019 | `while` | G-6.19, EXPR-6.1 | type `()` | condition réévaluée | pass : compteur ; fail : `while` en valeur → `E-TYP-STATEMENT-VALUE` si non-`()` | B | candidate-stable | cases/parser/while-loop-pass, cases/parser/while-value-nonunit-fail |
| F-SYN-020 | `loop` + `break` | EXPR-6.2 | type `()` | divergence possible | pass : `loop { break; }` ; fail : `break e;` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/loop-break-pass, cases/parser/loop-break-value-fail |
| F-SYN-021 | `for` sur `a..b` exclusif | G-6.24, EXPR-6.3 | E-TYP-RANGE | bornes évaluées une fois | pass : `for i in 0..4` ; fail : bornes de types différents → `E-TYP-RANGE` | B | candidate-stable | cases/run/loops |
| F-SYN-022 | `for x in Vec<T>` (consommant) | G-6.24, OWN-5.10 | `T` possédé | déplace le Vec | pass : `for line in lines` ; fail : `for x in &v` → `E-TYP-MISMATCH` | B | experimental | cases/run/stats |
| F-SYN-023 | Appels, méthodes, champs, indexation | G-6.30–6.31, EXPR-7 | E-TYP-CALL/FIELD/INDEX | gauche→droite | pass : `p.norme()`, `t.0`, `v[i]` ; fail : `t.0.1` → `E-PARSE-UNEXPECTED` | B | candidate-stable | cases/types/arity |
| F-SYN-024 | Littéral de struct `P { f: e }` | G-6.25–6.26 | E-TYP-FIELD | champs dans l'ordre | pass : `P { x: 1.0, y: 2.0 }` ; fail : `P { x }` → `E-PARSE-EXPECTED`, `..old` → `E-UNSUPPORTED-RANGE` | B | candidate-stable | cases/parser/field-shorthand |
| F-SYN-025 | Littéraux de tableau `[a,b]`, `[e; n]` (ArrayRepeat) | G-6.27 | E-TYP-ARRAY-REPEAT | éléments ordonnés | pass : `[1,2,3]`, `[0; 4]` ; fail : `[s; 2]` sur mobile → `E-TYP-ARRAY-REPEAT` | B | candidate-stable | cases/parser/array-literals-pass, cases/parser/array-repeat-noncopy-fail |
| F-SYN-026 | Tuple, unité, tuple à 1 élément | G-5.2, G-6.28 | TYP-4 | produit ordonné | pass : `(1, 2,)`, `()` ; fail : `(1, 2).0.1` chaîné → `E-PARSE-UNEXPECTED` | B | candidate-stable | cases/run/aggregates |
| F-SYN-027 | Chemins `crate::`, `super::`, `self::` | G-6.45–6.47 | E-RES-* | résolution statique | pass : `super::x` ; fail : `super` à la racine → `E-RES-UNKNOWN` | B | candidate-stable | cases/parser/path-qualifiers-pass, cases/parser/path-super-at-root-fail |
| F-SYN-028 | Variantes du prélude non qualifiées | G-6.29, G-6.41 | TYP-10.2 | identiques aux qualifiées | pass : `Some(x)`, `Ok(v)` ; fail : `Circle(r)` (enum utilisateur) → `E-RES-UNKNOWN` | B | candidate-stable | cases/parser/prelude-variants-pass, cases/parser/prelude-variant-user-enum-fail |
| F-SYN-029 | Pattern littéral, `_`, liaison | G-6.36–6.37 | types du scrutinee | premier correspondant | pass : `match n { 1 => ..., _ => ... }` ; fail : `match n { 1 => ... }` → `E-TYP-NONEXHAUSTIVE` | B | candidate-stable | cases/parser/match-literal-binding-pass, cases/parser/match-nonexhaustive-int-fail |
| F-SYN-030 | Pattern de tuple / parenthèses | G-6.38 | arité exacte | déstructure | pass : `match t { (a, b) => ... }` ; fail : arité → `E-TYP-ARITY` | B | candidate-stable | cases/parser/tuple-pattern-pass, cases/parser/tuple-pattern-arity-fail |
| F-SYN-031 | Pattern de variante + raccourci champ | G-6.40–6.41 | E-TYP-FIELD | lie par valeur | pass : `Shape::Rect { w, h: _ }` ; fail : `ref` pattern → `E-UNSUPPORTED-TOKEN` | B | candidate-stable | cases/parser/variant-pattern-shorthand-pass, cases/parser/variant-pattern-ref-fail |
| F-SYN-032 | Exhaustivité statique (enum, bool, entier via `_`) | G-6.22 | E-TYP-NONEXHAUSTIVE | pas de match dynamique | pass : `bool` complet ; fail : `bool` sans `false` → `E-TYP-NONEXHAUSTIVE` | B | candidate-stable | cases/types/nonexhaustive |
| F-SYN-033 | Boucles comme expressions de contrôle (`While`/`Loop`/`For`) | G-6.19, EXPR-5.11 | type `()` | exécution | pass : `while c { }` en instruction ; fail : `let x = while c { };` → `E-TYP-STATEMENT-VALUE` | B | candidate-stable | cases/parser/loop-control-statement-pass, cases/parser/loop-control-as-value-fail |
| F-SYN-034 | Visibilité de `test fn` | MOD-4.10 | — | non appelable depuis `main` | pass : `test fn t()` ; fail : `pub test fn` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/parser/test-fn-visibility-pass, cases/parser/test-fn-pub-fail |
| F-SYN-035 | Précédence et associativité (comparaisons non associatives) | G-6.2, G-6.12 | — | `(a + b) * c` | pass : `1 + 2 * 3 == 7` ; fail : `a < b < c` → `E-PARSE-UNEXPECTED` | B | candidate-stable | cases/parser/precedence |

## 4. Types (`F-TYP`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-TYP-001 | Primitifs et tailles garanties | grammar §5, TYP-1 | TYP-1.1 | tailles fixes | pass : `i128` (16 o) ; fail : mélange `i32 + i64` → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/mismatch |
| F-TYP-002 | `isize`/`usize` dépendants de la cible | TYP-1.2 | — | 4 ou 8 o | pass : `v.len()` en `usize` ; fail : `usize` → `u32` implicite → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/usize-len, cases/types/usize-to-u32 |
| F-TYP-003 | `bool`, `char` sans conversion implicite | TYP-1.4–1.5 | — | 0/1 ; scalaire | pass : `'é'` ; fail : `x as bool` → `E-TYP-CAST` | B | candidate-stable | cases/types/char-literal, cases/types/int-as-bool |
| F-TYP-004 | `!` (Never) + coercition vers tout type | grammar G-5.11, TYP-1.6 | `!` compatible partout | divergence | pass : `let x: i32 = panic("x");` ; fail : `fn f() -> ! { }` (corps termine) → `E-TYP-RETURN` | B | candidate-stable | cases/types/never-coerces, cases/types/never-return-body |
| F-TYP-005 | Valeurs vs places | TYP-2 | — | copie/déplacement | pass : `p.x = 1.0` ; fail : `(a+b).0 = 1;` → `E-PARSE-EXPECTED` | B | candidate-stable | cases/types/place-field-assign, cases/types/place-non-place-assign |
| F-TYP-006 | Mutabilité des places | TYP-3 | E-OWN-IMMUTABLE | — | pass : `let mut n = 0; n = 1;` ; fail : `let n = 0; n = 1;` → `E-OWN-IMMUTABLE` | B | candidate-stable | cases/types/mut-binding-assign, cases/types/immutable-binding-assign |
| F-TYP-007 | Tuples (aritété fixe, champ `.N`) | TYP-4 | E-TYP-FIELD | ordonné | pass : `t.0` ; fail : `t.2` hors bornes → `E-TYP-FIELD` | B | candidate-stable | cases/run/aggregates |
| F-TYP-008 | Tableaux `[T; N]` + index checké | TYP-5 | E-TYP-ARRAY-REPEAT | panic hors bornes | pass : `a[0]` ; fail : `[u8; 0]` → `E-TYP-ARRAY-REPEAT` | B | candidate-stable | cases/types/array-index, cases/types/array-zero-length |
| F-TYP-009 | `&[T]` + coercition `&[T; N] → &[T]` | TYP-5.5, TYP-13.1 | coercition en argument | lecture seule | pass : `gain(&input, 2.0)` ; fail : `let s: &[f64] = &input;` → `E-OWN-REF-ESCAPE` | B | candidate-stable | cases/run/dsp-gain-iir |
| F-TYP-010 | `&T` / `&mut T` non échappants | TYP-6, OWN-3 | E-OWN-REF-ESCAPE | prêt d'une instruction | pass : `f(&x)` ; fail : `let r = &x;` → `E-OWN-REF-ESCAPE` | B | candidate-stable | cases/ownership/ref-escape |
| F-TYP-011 | `Type::Str` derrière `&` ; littéraux `&str` | TYP-6.3 | `&str` `Copy` | texte UTF-8 | pass : `let s = "x";` ; fail : `fn f(s: str)` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/unicode |
| F-TYP-012 | `String` possédé | TYP-8 | mobile | UTF-8 valide | pass : `String::from("x")` ; fail : `let s: String = "x";` → `E-TYP-MISMATCH` | B | candidate-stable | cases/run/string-vec |
| F-TYP-013 | `Vec<T>` possédé | TYP-9 | mobile | longueur dynamique | pass : `v.push(1)` ; fail : `v[0]` sur `Vec<String>` → `E-OWN-PARTIAL-MOVE` | B | candidate-stable | cases/run/string-vec |
| F-TYP-014 | `Option<T>` | TYP-10 | Copy si `T` Copy | deux variantes | pass : `match o { Some(x) => x, None => 0 }` ; fail : `match` partiel → `E-TYP-NONEXHAUSTIVE` | B | candidate-stable | cases/types/option-match, cases/types/option-partial-match |
| F-TYP-015 | `Result<T, E>` | TYP-10 | Copy si `T`,`E` Copy | deux variantes | pass : `Ok(1)` ; fail : `assert_eq(Ok(1), Ok(1))` → `E-TYP-EQ` | B | candidate-stable | cases/run/result-error |
| F-TYP-016 | Génériques sans bornes | TYP-11 | `T` opaque | monomorphisation | pass : `fn id<T>(x: T) -> T` ; fail : `x + x` sur `T` → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/generic-identity, cases/types/generic-opaque-op |
| F-TYP-017 | Erreurs d'instanciation après substitution | TYP-11.3 | E-TYP-MISMATCH | — | pass : `id(1)` ; fail : instanciation invalide signalée sur l'appel | B | candidate-stable | cases/types/generic-instantiation, cases/types/generic-instantiation-mismatch |
| F-TYP-018 | Layout non observable | TYP-12 | aucune garantie | aucun `size_of`/`transmute` | pass : accès de champ ; fail : `size_of::<i32>()` → `E-RES-UNKNOWN` | B | candidate-stable | cases/types/field-access, cases/types/size-of-unknown |
| F-TYP-019 | Coercitions : exactement 2 | TYP-13.1 | `&[T;N]→&[T]`, `&String→&str` | vues | pass : `parse_f64(&args[1])` ; fail : `a: i32 → b: i64` → `E-TYP-MISMATCH` | B | candidate-stable | cases/run/calculator-args |
| F-TYP-020 | Littéraux typés immédiatement, `i32` défaut, pas d'unification rétroactive | TYP-13.4 | E-TYP-LITERAL-RANGE, E-TYP-FLOAT-LITERAL | — | pass : `let x: i64 = 1;` ; fail : `let mut i = 0; i < v.len()` → `E-TYP-MISMATCH`, `300u8` → `E-TYP-LITERAL-RANGE` | B | experimental | cases/types/literal-range, cases/types/i8-min-accepted, cases/types/ambiguous |
| F-TYP-021 | Égalité scalaires + texte (mélange) | TYP-7.3 | E-TYP-EQ | comparaison de contenu | pass : `s == "x"`, `a == b` (`i32`) ; fail : `v1 == v2` (`Vec`) → `E-TYP-EQ` | B | candidate-stable | cases/run/string-vec |
| F-TYP-022 | Opérateurs et types admis (aucune surcharge) | TYP-7 | E-TYP-MISMATCH/CMP | — | pass : `a + b` ; fail : `&str < &str` → `E-TYP-CMP` | B | candidate-stable | cases/types/int-add, cases/types/str-compare |
| F-TYP-023 | `as` explicite, total, saturant | TYP-13.2 | E-TYP-CAST | jamais d'UB | pass : `x as u8`, `f as i32` ; fail : `n as char` → `E-TYP-CAST` | B | candidate-stable | cases/types/cast-numeric, cases/types/cast-int-char |
| F-TYP-024 | Types récursifs infinis refusés | TYP-10.7 | E-TYP-RECURSIVE | — | pass : `Vec<Node>` ; fail : `struct N { next: Option<N> }` → `E-TYP-RECURSIVE` | B | candidate-stable | cases/types/recursive-vec, cases/types/recursive-option |
| F-TYP-025 | Indexation `Vec`/`&[T]` : lecture, panic hors bornes | TYP-9.3, TYP-5.5 | E-TYP-INDEX | panic | pass : `v[0]` (`i32`) ; fail : `v[i32]` → `E-TYP-INDEX` | B | candidate-stable | cases/run/index-out-of-bounds |
| F-TYP-026 | `Type::Never` non nommable en champ/paramètre | G-5.11 | E-TYP-MISMATCH | — | pass : retour divergent ; fail : `struct S { x: ! }` → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/never-divergent-return, cases/types/never-field |
| F-TYP-027 | Méthode inconnue | G-6.30 | E-RES-UNKNOWN | — | pass : `v.push(1)` ; fail : `v.get(0)` → `E-RES-UNKNOWN` | B | candidate-stable | cases/types/unknown-method |

## 5. Expressions et évaluation (`F-EXPR`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-EXPR-001 | Ordre d'évaluation gauche→droite | EXPR-1 | — | observable par panic | pass : `g(1)+g(2)` ; fail : ordre inversé détecté par trace | B | candidate-stable | cases/types/eval-order |
| F-EXPR-002 | Court-circuit `&&`/`\|\|` | EXPR-2 | `bool` uniquement | opérande droit non évalué | pass : `i < len && v[i] == 0` ; fail : `n && true` (`i32`) → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/short-circuit, cases/types/short-circuit-non-bool |
| F-EXPR-003 | Débordement entier → panic (tous profils) | EXPR-3.1 | — | `panic: overflow` | pass : `255u8 + 0` ; fail (runtime) : `255u8 + 1` → panic | B | candidate-stable | cases/run/integer-overflow |
| F-EXPR-004 | Division/reste par zéro → panic | EXPR-3.2 | — | panic | pass : `4 / 2` ; fail (runtime) : `4 / 0` → panic | B | candidate-stable | cases/run/division-by-zero |
| F-EXPR-005 | Décalage ≥ largeur → panic | EXPR-3.3 | E-TYP-SHIFT | panic | pass : `1u8 << 7` ; fail : `1u8 << 8` → panic, `a << n<0` → `E-TYP-SHIFT` | B | candidate-stable | cases/types/shift-in-range, cases/types/shift-negative |
| F-EXPR-006 | Signes division/reste entiers | EXPR-3.5 | — | troncature vers zéro | pass : `-7 / 2 == -3`, `-7 % 2 == -1` ; fail : arrondi différent | B | candidate-stable | cases/types/division-signs |
| F-EXPR-007 | Flottants IEEE, `NaN`, `-0.0` | EXPR-4 | — | pas de piège | pass : `0.0/0.0 != 0.0/0.0` ; fail : `nan == nan` vrai (non conforme) | B | candidate-stable | cases/types/float-nan-zero |
| F-EXPR-008 | Pas de fast-math / FMA / réassociation | EXPR-4.5 | — | `(a+b)+c` | pass : `a + b + c` ; fail : réassociation observée | B | candidate-stable | cases/types/float-assoc |
| F-EXPR-009 | `as` : sémantique complète | TYP-13.2, EXPR-10 | E-TYP-CAST | saturant, NaN→0 | pass : `NaN as i32 == 0` ; fail : `-1.0 as u8` ≠ 0 (non conforme) | B | candidate-stable | cases/run/fibonacci |
| F-EXPR-010 | `?` sur `Result` | EXPR-8 | E-TYP-TRY | retour immédiat de `Err` | pass : `fs::read_to_string(p)?` ; fail : `-> ()` → `E-TYP-TRY` | B | candidate-stable | cases/run/result-error, cases/run/calculator-args |
| F-EXPR-011 | `?` sur `Option` | EXPR-8.2 | E-TYP-TRY | retour de `None` | pass : `premier(v)?` ; fail : `Option` dans `Result` → `E-TYP-TRY` | B | candidate-stable | cases/types/try-option, cases/types/try-option-in-result |
| F-EXPR-012 | Bloc/`if`/`match` comme expressions | EXPR-5 | types identiques | une branche | pass : `let x = match o { ... };` ; fail : branches incohérentes → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/match-expression, cases/types/if-branch-mismatch |
| F-EXPR-013 | Instruction-expression de type `()` | EXPR-5.6 | E-TYP-STATEMENT-VALUE | valeur ignorée | pass : `if c { io::println("x"); }` ; fail : `1 + 1;` → `E-TYP-STATEMENT-VALUE` | B | candidate-stable | cases/types/statement-unit, cases/types/statement-value |
| F-EXPR-014 | `while`/`loop`/`for` runtime et `break`/`continue` | EXPR-6 | — | boucle la plus interne | pass : `while c { continue; }` ; fail : `continue;` hors boucle → `E-TYP-BREAK-OUTSIDE` | B | candidate-stable | cases/run/loops, cases/run/fibonacci |
| F-EXPR-015 | Affectation : remplacement, mutation, `+=` | EXPR-9 | E-TYP-MISMATCH | détruit l'ancienne valeur | pass : `p.x = 1.0` ; fail : `p.x = "a";` → `E-TYP-MISMATCH` | B | candidate-stable | cases/types/assignment-compound, cases/types/assignment-mismatch |
| F-EXPR-016 | `panic`/`exit` divergents (`!`) | EXPR-11 | `!` | code 1, arrêt | pass : `panic("x")` en position de valeur ; fail : `panic()` sans argument → `E-TYP-ARITY` | B | candidate-stable | cases/run/panic |
| F-EXPR-017 | `for` range : bornes évaluées une fois | EXPR-6.3 | E-TYP-RANGE | zéro itération si `a>=b` | pass : `for i in 0..0 { }` ; fail : bornes réévaluées (trace) | B | candidate-stable | cases/run/loops |
| F-EXPR-018 | `for` Vec : consommation | OWN-5.10 | `T` possédé | déplacement | pass : `for x in v` ; fail : usage de `v` après → `E-OWN-USE-AFTER-MOVE` | B | experimental | cases/run/stats |

## 6. Propriété et destruction (`F-OWN`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-OWN-001 | Classification `Copy` structurelle | OWN-2 | TYP-14 | copie | pass : `let q = p;` (Point) ; fail : `struct D { s: String }` copy → `E-OWN-USE-AFTER-MOVE` | B | candidate-stable | cases/ownership/copy-ok |
| F-OWN-002 | Move et use-after-move | OWN-5.1 | E-OWN-USE-AFTER-MOVE | déplacement | pass : `let t = s; f(t)` ; fail : `let t = s; s.len();` → `E-OWN-USE-AFTER-MOVE` | B | candidate-stable | cases/ownership/use-after-move, cases/ownership/move-in-call |
| F-OWN-003 | Double move | OWN-5.2 | E-OWN-DOUBLE-MOVE | — | pass : `let a = s;` seul ; fail : `let a = s; let b = s;` → `E-OWN-DOUBLE-MOVE` | B | candidate-stable | cases/ownership/double-move |
| F-OWN-004 | Move partiel de struct **autorisé et suivi par champ** | OWN-5.3 | E-OWN-PARTIAL-MOVE | champ `MOVED`, autres champs utilisables | pass : `fn into_titre(self) -> String { self.titre }` ; fail : struct entier réutilisé → `E-OWN-PARTIAL-MOVE` | B | candidate-stable | cases/ownership/partial-move-struct, cases/ownership/partial-move-struct-whole |
| F-OWN-005 | Move d'élément tuple/array si agrégat inutilisé | OWN-5.4 | E-OWN-PARTIAL-MOVE | — | pass : `return t.0;` ; fail : `let s = t.0; t.1` → `E-OWN-PARTIAL-MOVE` | B | candidate-stable | cases/ownership/partial-move-tuple-ok, cases/ownership/partial-move-tuple-whole |
| F-OWN-006 | Move hors `Vec[i]` interdit | OWN-5.5 | E-OWN-PARTIAL-MOVE | — | pass : `v.pop()` ; fail : `let s = v[0];` → `E-OWN-PARTIAL-MOVE` | B | candidate-stable | cases/ownership/vec-pop-ok, cases/ownership/vec-index-move |
| F-OWN-007 | Initialisation définie sur tous les chemins | OWN-4 | E-OWN-UNINIT | — | pass : `if c { x=1 } else { x=2 }` ; fail : `if c { x=1 } use(x)` → `E-OWN-UNINIT` | B | candidate-stable | cases/ownership/conditional-init |
| F-OWN-008 | Ré-initialisation d'une place déplacée | OWN-4.4, EXPR-9.3 | — | remplacement | pass : `let t = s; s = String::from("b");` ; fail : lecture avant réinit → `E-OWN-USE-AFTER-MOVE` | B | candidate-stable | cases/ownership/reinit-after-move, cases/ownership/read-before-reinit |
| F-OWN-009 | Prêts liés à l'instruction ; conflits | OWN-3.1–3.12 | E-OWN-BORROW-CONFLICT | expiration en fin d'instruction | pass : `let n = (&p).x; p.x = 2.0;` ; fail : `f(&mut x, &mut x)` → `E-OWN-BORROW-CONFLICT` | B | candidate-stable | cases/ownership/borrow-conflict-shared-mut, cases/ownership/borrow-conflict-two-mut |
| F-OWN-010 | Paramètres `&T`/`&mut T` utilisables dans tout le corps | OWN-3.13–3.14 | — | prêt lié à l'appel | pass : `v.push(*n)` ; fail : `let w = *v;` → `E-OWN-MOVE-OUT-OF-BORROW` | B | candidate-stable | cases/ownership/borrow-param-body, cases/ownership/move-out-of-mut-borrow |
| F-OWN-011 | Non-échappement des références | OWN-6 | E-OWN-REF-ESCAPE | — | pass : `f(&x)` ; fail : `fn g(v: &Vec<i32>) -> &Vec<i32>` → `E-OWN-REF-ESCAPE` | B | candidate-stable | cases/ownership/ref-escape |
| F-OWN-012 | `&'static str` stockable (seule exception) | OWN-6.2 | — | copie | pass : `let s = "x";` ; fail : `let t = s.as_str();` → `E-OWN-REF-ESCAPE` | B | candidate-stable | cases/ownership/static-str-stored, cases/ownership/as-str-escape |
| F-OWN-013 | `drop(x)` intrinsèque | OWN-9 | — | destruction immédiate | pass : `drop(s)` ; fail : `drop(s); s.len()` → `E-OWN-USE-AFTER-MOVE` | B | candidate-stable | cases/ownership/drop-then-use |
| F-OWN-014 | Ordre de drop inverse, valeurs déplacées non détruites | OWN-8 | — | non observable (pas de `Drop`) | pass : blocs imbriqués ; fail : double destruction (détecté par instrumentation) | B | candidate-stable | cases/ownership/drop-order-scope |
| F-OWN-015 | Pas de `Drop` utilisateur | OWN-9.5 | E-UNSUPPORTED-DROP-IMPL | — | pass : aucune ; fail : `impl Drop for T` → `E-UNSUPPORTED-DROP-IMPL` | B | unsupported | cases/ownership/drop-impl |
| F-OWN-016 | Bloc `unsafe` transparent pour l'ownership | OWN-10 | — | — | pass : move/emprunt dans `unsafe { }` suivent les règles du bloc ; fail : `*const T` → `E-UNSUPPORTED-UNSAFE` | B | candidate-stable | cases/unsupported/raw-pointer |

## 7. Effets (`F-EFF`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-EFF-001 | 9 groupes d'effets | EFF-1 | — | aucun effet runtime | pass : `effects(io, fs)` ; fail : `effects(network)` → `E-EFF-UNKNOWN` | B | candidate-stable | cases/effects/unknown-group |
| F-EFF-002 | Inférence transitive + point fixe | EFF-2 | — | — | pass : `b()` contamine `c()` ; fail : `effects()` avec appel `io` → `E-EFF-UNDECLARED` | B | candidate-stable | cases/effects/io-undeclared, cases/effects/recursive-effects |
| F-EFF-003 | Déclaration majorante | EFF-3 | E-EFF-UNDECLARED | — | pass : `effects(io, alloc)` plus large ; fail : déclaration trop faible → `E-EFF-UNDECLARED` | B | candidate-stable | cases/effects/declaration-insufficient |
| F-EFF-004 | Profil `core` | EFF-4.1 | — | aucune restriction | pass : tout programme | B | candidate-stable | cases/effects/profile-core-unrestricted |
| F-EFF-005 | Profil `pure` | EFF-4.2–4.5 | E-EFF-PROFILE | entrée sans effet | pass : arithmétique pure ; fail : `io::println` sous `pure` → `E-EFF-PROFILE` | B | candidate-stable | cases/effects/pure-main-ok, cases/effects/pure-profile-violated |
| F-EFF-006 | Profil `rt` (expérimental, non certifié) | EFF-4.6–4.7 | E-EFF-PROFILE | — | pass : `effects()` sans `io`/`alloc`/`panic` ; fail : `alloc` sous `rt` → `E-EFF-PROFILE` | B | experimental | cases/effects/profile-rt-ok, cases/effects/profile-rt-alloc |
| F-EFF-007 | Groupes non implémentés refusés | EFF-1, UNS-5 | E-UNSUPPORTED-EFFECT | — | pass : `effects(io)` ; fail : `effects(net)`/`effects(async)` → `E-UNSUPPORTED-EFFECT` | — | unsupported | cases/effects/unsupported-net |
| F-EFF-009 | `unsafe`/`ffi` inférés par bloc `unsafe` et appel `extern` | FFI-3, FFI-4 | E-UNSAFE-CONTEXT | — | pass : `unsafe { abs(-1) }` ; fail : appel `extern` hors `unsafe` → `E-UNSAFE-CONTEXT` ; `pure` → `E-EFF-PROFILE` | B | candidate-stable | cases/run/ffi-abs, cases/types/ffi-outside-unsafe |
| F-FFI-001 | Déclaration `extern "C" { fn f(...); }` (module uniquement) | FFI-1 | primitives FFI-sûres | symbole C lié | pass : `fn abs(x: i32) -> i32;` ; fail : `extern "system"` → `E-UNSUPPORTED-ABI` | B | candidate-stable | cases/run/ffi-abs |
| F-FFI-002 | Appel `extern` uniquement dans `unsafe { }` | FFI-3 | — | — | pass : `unsafe { abs(-7) }` ; fail : `abs(-1)` hors `unsafe` → `E-UNSAFE-CONTEXT` | B | candidate-stable | cases/types/ffi-outside-unsafe |
| F-FFI-003 | Signatures FFI-sûres seulement | FFI-2 | `E-FFI-TYPE` | — | pass : entiers/flottants/`bool`/`()` ; fail : `String`, `&str`, struct → `E-FFI-TYPE` | B | candidate-stable | cases/types/ffi-bad-type |
| F-EFF-008 | Panics intrinsèques non suivis (approximation) | EFF-6.4 | — | panic possible sous `effects()` | pass : `a / b` en `effects()` ; fail : documenter comme absence de panic (faux) | B | candidate-stable | cases/effects/intrinsic-panic-not-tracked |

## 8. Modules et packages (`F-MOD`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-MOD-001 | `ravum.toml` (`name`, `version`, `edition`) | MOD-5.1–5.2 | E-MOD-MANIFEST | — | pass : manifeste des exemples ; fail : clé inconnue → `E-MOD-MANIFEST` | B | candidate-stable | cases/packages/manifest-unknown-key, cases/security/reserved-windows-name |
| F-MOD-002 | `edition = "core-1.0"` uniquement | MOD-5.2 | E-UNSUPPORTED-EDITION | — | pass : `core-1.0` ; fail : `core-2.0` → `E-UNSUPPORTED-EDITION` | B | candidate-stable | cases/modules/edition-core-1-0, cases/modules/edition-unknown |
| F-MOD-003 | Dépendances locales `path` (`..` borné à 8, sans absolu/symlink) | MOD-5.4–5.7 | E-MOD-DEPENDENCY, E-MOD-PATH | source compilée | pass : `util = { path = "../util" }` ; fail : `version` seule → `E-MOD-DEPENDENCY` | B | candidate-stable | cases/packages/dependency-form, cases/packages/local-dependency, cases/packages/dependency-not-lib, cases/run/package-demo, cases/resolve/dependency-undeclared |
| F-MOD-004 | Refus des build scripts | MOD-5.10 | E-MOD-BUILD-SCRIPT | — | pass : aucun `build.rs` ; fail : `[build]` → `E-MOD-BUILD-SCRIPT` | B | unsupported | cases/security/manifest-build-script |
| F-MOD-005 | `ravum.lock` déterministe + digest | MOD-7 | E-MOD-LOCK | `dependencies_digest` | pass : deux builds ⇒ même lockfile ; fail : `lockfile_version=99` → `E-MOD-LOCK` | B | candidate-stable | cases/modules/lockfile-deterministic, cases/modules/lockfile-unknown-version |
| F-MOD-006 | Graphe de dépendances acyclique | MOD-8.3 | E-MOD-CYCLE | — | pass : `app → util` ; fail : cycle → `E-MOD-CYCLE` | B | candidate-stable | cases/packages/dependency-cycle |
| F-MOD-007 | Résolution `foo.rv` puis `foo/mod.rv` | MOD-1.3 | E-MOD-MISSING | — | pass : `src/geo/mod.rv` ; fail : absent → `E-MOD-MISSING` | B | candidate-stable | cases/resolve/module-not-found |
| F-MOD-008 | Ambiguïté `foo.rv` + `foo/mod.rv` | MOD-1.3 | E-MOD-AMBIGUOUS | — | pass : un seul ; fail : les deux → `E-MOD-AMBIGUOUS` | B | candidate-stable | cases/resolve/module-ambiguous |
| F-MOD-009 | Chemins confinés à `src/`, sans absolu ni symlink, noms réservés refusés | MOD-2.3–2.5, MOD-5.13 | E-MOD-PATH-ESCAPE, E-MOD-PATH, E-MOD-RESERVED-NAME | — | pass : `src/x.rv` ; fail : `../x` → `E-MOD-PATH-ESCAPE`, symlink → `E-MOD-PATH`, `CON` → `E-MOD-RESERVED-NAME` | B | candidate-stable | cases/security/dependency-absolute-unix, cases/security/dependency-path-escape |
| F-MOD-010 | Visibilité privé par défaut | MOD-4 | E-RES-PRIVATE | — | pass : accès dans le sous-arbre ; fail : hors sous-arbre → `E-RES-PRIVATE` | B | candidate-stable | cases/modules/private-field, cases/resolve/import-private |
| F-MOD-011 | `main` : signature et unicité | MOD-4.5 | E-TYP-MAIN(-MISSING/-DUPLICATE) | point d'entrée | pass : `fn main()`, `fn main() -> Result<(), Error>` ; fail : `-> i32` → `E-TYP-MAIN` | B | candidate-stable | cases/resolve/main-missing, cases/types/main-invalid |
| F-MOD-012 | Package `std` implicite (7 modules) | MOD-8.3 | E-RES-UNKNOWN | prélude fourni | pass : `use std::io;` ; fail : `use std::convert;` → `E-RES-UNKNOWN` | B | candidate-stable | cases/modules/std-prelude-modules, cases/modules/std-module-unknown |
| F-MOD-013 | Les deux racines `main.rv` + `lib.rv` sont refusées | MOD-1.8 | E-MOD-BOTH-ROOTS | — | pass : une seule racine ; fail : les deux → `E-MOD-BOTH-ROOTS` | B | candidate-stable | cases/modules/single-root, cases/modules/both-roots-rejected |

## 9. Bibliothèque standard (`F-STD`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-STD-001 | `io::print`/`println`/`eprintln` | STD-2 | `&str` | écriture UTF-8 + `\n` | pass : `io::println("x")` ; fail : `io::println("a={}", n)` → `E-TYP-ARITY` | B | candidate-stable | cases/run/hello |
| F-STD-002 | `io::read_line` | STD-2.7–2.9 | `Result<String, Error>` | EOF/UTF-8 invalide → `Err` | pass : ligne lue ; fail : `io::read_line()?` dans `-> ()` → `E-TYP-TRY` | B | candidate-stable | cases/run/calculator-stdin |
| F-STD-003 | `fs::read_to_string` | STD-3.2 | `Result<String, Error>` | UTF-8 strict, non converti | pass : fichier lu ; fail : `?` type incompatible → `E-TYP-TRY` | B | candidate-stable | cases/run/fs-roundtrip |
| F-STD-004 | `fs::write_string` (non atomique) | STD-3.3–3.4 | `Result<(), Error>` | tronque, pas de `rename` | pass : écriture ; fail : `&String` non coercible dans une `let` → `E-OWN-REF-ESCAPE` | B | candidate-stable | cases/run/file-tool |
| F-STD-005 | `fs::exists` (bool, erreurs avalées) | STD-3.5 | `bool` | `false` sur erreur | pass : `if fs::exists(p)` ; fail : usage pour une décision de sécurité | B | candidate-stable | cases/run/fs-roundtrip |
| F-STD-006 | `env::args` | STD-4.1–4.2 | `Vec<String>` | UTF-8 sinon panic | pass : `args()[1]` ; fail : index `i32` → `E-TYP-INDEX` | B | candidate-stable | cases/run/args |
| F-STD-007 | `env::var` | STD-4.3 | `Result<String, Error>` | absent → `Err` | pass : `match env::var("X")` ; fail : `env::set_var` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/env-var-absent, cases/run/env-set-var-unknown |
| F-STD-008 | `process::exit(i32) -> !` | STD-5 | `!` | arrêt, code plateforme | pass : `process::exit(3)` ; fail : `let x: i32 = process::exit(0);` accepté (`!`) mais code 0 | B | candidate-stable | cases/run/process-exit-code, cases/run/process-exit-type |
| F-STD-009 | `math` : `abs`, `sqrt`, `floor`, `ceil`, `round` | STD-6.2–6.4 | `f64` | exact / correctement arrondi | pass : `sqrt(2.0)` ; fail : `math::sqrt(2.0f32)` → `E-TYP-MISMATCH` | B | candidate-stable | cases/run/math-float-ops, cases/run/math-sqrt-f32-rejected |
| F-STD-010 | `math::pow` (précision non garantie) | STD-6.6 | `f64` | libm, non bit-exact | pass : `pow(2.0, 10.0)` ; fail : test bit-exact (doit être exclu) | B | experimental | cases/run/math-pow-exact, cases/run/math-pow-arity |
| F-STD-011 | `math::min`/`max` (NaN, zéro signé) | STD-6.5 | `f64` | autre opérande si NaN | pass : `min(-0.0, 0.0) == -0.0` ; fail : `min(NaN, 1.0) == NaN` (non conforme) | B | candidate-stable | cases/run/math-min-max, cases/run/math-max-signed-zero, cases/run/math-min-type |
| F-STD-012 | `string::from`, `parse_i64`, `parse_f64` | STD-7 | `Result`/`String` | consommation totale, `Err` sur dépassement | pass : `parse_i64("42")` ; fail : `parse_i64("1 ")]` → `Err` | B | candidate-stable | cases/run/calculator-args |
| F-STD-013 | `assert`, `assert_eq` (par valeur), `panic` | STD-8 | `panic` | panic + `file:line:col` | pass : `assert_eq(1+1, 2)` ; fail : `assert_eq(Vec::new(), Vec::new())` → `E-TYP-EQ` | B | candidate-stable | cases/run/panic |
| F-STD-014 | `drop<T>(T)` | STD-8 / OWN-9 | — | destruction | pass : `drop(s)` ; fail : `drop(s); s.len()` → `E-OWN-USE-AFTER-MOVE` | B | candidate-stable | cases/ownership/drop-then-use |
| F-STD-015 | `String` : `len`, `is_empty`, `as_str` | STD-9 | `usize`/`bool`/`&str` | `len` = octets | pass : `s.len()` ; fail : `s.len` sans `()` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/string-vec |
| F-STD-016 | `String` : `push`, `push_str`, `clone` | STD-9.4 | `&mut self` | allocation | pass : `s.push_str("x")` ; fail : `s.push_str` sur `&String` immuable → `E-OWN-IMMUTABLE` | B | candidate-stable | cases/run/string-vec |
| F-STD-017 | `String`/`&str` : `trim`, `to_uppercase`, `to_lowercase` (possédés) | STD-9.1, 9.5–9.6 | `String` | allocation | pass : `line.trim()` ; fail : `let t = s.trim(); t` stocké → OK (possédé), `let t = s.as_str();` → `E-OWN-REF-ESCAPE` | B | candidate-stable | cases/run/stats |
| F-STD-018 | `contains`, `starts_with`, `ends_with` | STD-9.7 | `bool` | comparaison d'octets | pass : `s.starts_with("ra")` ; fail : `s.starts_with('r')` → `E-TYP-MISMATCH` | B | candidate-stable | cases/run/tests-demo-run |
| F-STD-019 | `replace` | STD-9.8 | `String` | toutes occurrences, non chevauchantes | pass : `s.replace("a", "b")` ; fail : `s.replace("a")` → `E-TYP-ARITY` | B | candidate-stable | cases/run/string-replace, cases/run/string-replace-arity |
| F-STD-020 | `lines` (Vec<String> possédé) | STD-9.9 | `Vec<String>` | `\r\n` retiré, pas de ligne finale vide | pass : `text.lines()` ; fail : `for line in text.lines()` sur `&str` → `E-TYP-MISMATCH` | B | candidate-stable | cases/run/stats |
| F-STD-021 | `String::parse_i64`/`parse_f64` | STD-9.10 | `Result` | mêmes règles que `std::string` | pass : `"42".parse_i64()` ; fail : `s.parse_i64` sans `()` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/string-parse-methods, cases/run/string-parse-method-no-call |
| F-STD-022 | `Vec` : `new`, `with_capacity`, `len`, `is_empty` | STD-10 | `Vec<T>` | capacité non observable | pass : `Vec::with_capacity(n)` ; fail : `Vec::new()` sans type → `E-TYP-AMBIGUOUS` | B | candidate-stable | cases/run/dsp-gain-iir |
| F-STD-023 | `Vec` : `push`, `pop`, `clear` | STD-10.3 | `&mut self` | FIFO/LIFO dernier | pass : `v.pop()` ; fail : `v.pop()?` dans `-> ()` → `E-TYP-TRY` | B | candidate-stable | cases/run/string-vec |
| F-STD-024 | `Vec::clone` (si `T: Copy`) | STD-10.4 | condition structurelle | copie | pass : `Vec<i32>::clone` ; fail : `Vec<String>::clone` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/vec-clone-copy, cases/run/vec-clone-non-copy |
| F-STD-025 | `&[T]` : `len`, `is_empty`, `s[i]` | STD-10.6 | — | panic hors bornes | pass : `samples[i]` ; fail : `samples.push(1)` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/dsp-gain-iir |
| F-STD-026 | `Option` : `is_some`, `is_none`, `unwrap`, `unwrap_or`, `ok_or` | STD-11 | `panic` pour `unwrap` | — | pass : `o.unwrap_or(0)` ; fail : `o.expect("x")` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/option-methods, cases/run/option-expect-unknown |
| F-STD-027 | `Result` : `is_ok`, `is_err`, `unwrap`, `unwrap_or`, `ok`, `err` | STD-11 | `panic` pour `unwrap` | — | pass : `r.ok()` ; fail : `r.map(...)` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/result-methods, cases/run/result-map-unknown |
| F-STD-028 | `to_string()` sur primitives (Display Rust) | STD-12.1–12.2 | `String` | round-trip le plus court | pass : `42.to_string()` ; fail : `s.to_string()` sur `String` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/fibonacci |
| F-STD-029 | `f64` méthodes (`abs`, `sqrt`, `powf`, `min`, `max`, …) ; `abs` entiers signés | STD-12.3–12.5 | — | libm pour `powf` | pass : `x.abs().sqrt()` ; fail : `u32::abs()` → `E-RES-UNKNOWN` | B | candidate-stable | cases/run/f64-methods, cases/run/int-abs-signed, cases/run/unsigned-abs-unknown |
| F-STD-030 | `Error` + affichage `error: <message>` | STD-13 | mobile | code 1 sur `Err` de `main` | pass : `main() -> Result` ; fail : `Error` comparé → `E-TYP-EQ` | B | candidate-stable | cases/run/result-error |
| F-STD-031 | `math::sin`, `cos`, `tan`, `exp`, `ln`, `log10` (précision non garantie) | STD-6.6 | `f64` | libm, hors bit-exact | pass : `math::cos(0.0) == 1.0` ; fail : test bit-exact d'un résultat libm (exclu) | B | experimental | cases/run/math-trig-exact, cases/run/math-log2-unknown |
| F-STD-032 | `Vec::as_slice(&self) -> &[T]` | STD-10.7 | `&[T]` | vue non stockable | pass : `f(v.as_slice())` ; fail : `f(&v)` (aucune coercition) → `E-TYP-MISMATCH` | B | candidate-stable | cases/run/vec-as-slice, cases/run/vec-coercion-rejected |

## 10. Refus explicites (`F-UNS`) — voir [`unsupported.md`](unsupported.md)

| FeatureId | Fonctionnalité refusée | Syntaxe/forme | Code | Statut | Conformance |
|---|---|---|---|---|---|
| F-UNS-001 | Traits, bornes, `where`, HRTB, GAT | `trait`, `T: X`, `where` | `E-UNSUPPORTED-TRAIT`, `E-UNSUPPORTED-WHERE` | unsupported | cases/unsupported/trait, cases/unsupported/where-clause |
| F-UNS-002 | `impl Trait` (`dyn` est un identifiant valide) | `-> impl T` ; `dyn T` | `E-UNSUPPORTED-IMPL-TRAIT` ; `E-RES-UNKNOWN` | unsupported | cases/unsupported/dyn-identifier, cases/unsupported/impl-trait |
| F-UNS-003 | Closures et captures | `\|x\| e`, `move \|x\|` | `E-UNSUPPORTED-CLOSURE`, `E-UNSUPPORTED-TOKEN` | unsupported | cases/unsupported/closure |
| F-UNS-004 | `async`/`await` | `async fn`, `.await` | `E-UNSUPPORTED-ASYNC` | unsupported | cases/unsupported/async |
| F-UNS-005 | Threads, atomiques, locks, canaux | `thread::spawn`, `Mutex`, `Atomic*` | `E-RES-UNKNOWN` | unsupported | cases/unsupported/threads-atomics |
| F-UNS-006 | `unsafe fn`, pointeurs bruts, `transmute` | `unsafe fn f()`, `*const T` | `E-UNSUPPORTED-UNSAFE`, `E-TYP-CAST` | unsupported | cases/unsupported/raw-pointer |
| F-UNS-007 | FFI hors cadre Core 1.0 (ABI non-C, types composés, `#[no_mangle]`, `extern fn` avec corps) | `extern "system"`, `fn f(s: String)`, `#[no_mangle]` | `E-UNSUPPORTED-ABI`, `E-FFI-TYPE`, `E-UNSUPPORTED-ATTR`, `E-UNSUPPORTED-EXTERN` | unsupported | cases/types/ffi-bad-type |
| F-UNS-008 | Macros (décl. et proc.) | `m!()`, `macro_rules!` | `E-UNSUPPORTED-MACRO` | unsupported | cases/unsupported/macro |
| F-UNS-009 | Attributs, `derive`, `repr` | `#[...]` | `E-UNSUPPORTED-ATTR` | unsupported | cases/unsupported/attribute |
| F-UNS-010 | `impl Drop`, destructeur utilisateur | `impl Drop for T` | `E-UNSUPPORTED-DROP-IMPL` (R) | unsupported | cases/unsupported/drop-impl |
| F-UNS-011 | Identifiants Unicode | `let café = 1;` | `E-UNSUPPORTED-IDENT` | unsupported | cases/unsupported/unicode-ident |
| F-UNS-012 | Ré-exports, alias, glob, imports groupés | `pub use`, `as`, `*`, `{...}` | `E-UNSUPPORTED-TOKEN` | unsupported | cases/unsupported/glob-import |
| F-UNS-013 | Lifetimes, étiquettes de boucle | `&'a T`, `'outer: loop` | `E-UNSUPPORTED-TOKEN` | unsupported | cases/unsupported/lifetime-token |
| F-UNS-014 | Littéraux byte, raw string | `b"..."`, `r"..."` | `E-UNSUPPORTED-TOKEN` | unsupported | cases/unsupported/raw-string |
| F-UNS-015 | Ranges hors `for`, slicing, `..base` | `0..n` en valeur, `&v[1..2]` | `E-UNSUPPORTED-RANGE` | unsupported | cases/unsupported/range-value |
| F-UNS-017 | `break` avec valeur | `break e;` | `E-PARSE-EXPECTED` | unsupported | cases/unsupported/break-value |
| F-UNS-018 | Patterns `ref`/`\|`/garde/`@`/plage/struct (`ref` est un identifiant) | `ref x`, `1\|2`, `if`, `@`, `1..=5` | `E-UNSUPPORTED-TOKEN` ; `E-PARSE-EXPECTED` (ref) ; `E-UNSUPPORTED-RANGE` (plage) | unsupported | cases/unsupported/ref-identifier, cases/unsupported/or-pattern |
| F-UNS-019 | Turbofish, shorthand de champ, `let`/param patterns | `f::<T>()`, `P { x }`, `let (a,b)` | `E-PARSE-UNEXPECTED`, `E-PARSE-EXPECTED` | unsupported | cases/unsupported/turbofish |
| F-UNS-020 | Appel de module avec point | `io.println(...)` | `E-RES-UNKNOWN` | unsupported | cases/unsupported/module-dot-call |

## 11. Méta : CLI, JSON, machine (`F-META`)

| FeatureId | Fonctionnalité | Syntaxe | Typage | Runtime | Pass / Fail | Backend | Statut | Conformance |
|---|---|---|---|---|---|---|---|---|
| F-META-001 | Diagnostics texte + codes stables | `diagnostics.md` §3, §6 | — | stderr | pass : format §6 ; fail : texte asserté (non contractuel) | B | candidate-stable | cases/parser/multi-error-recovery |
| F-META-002 | JSON `schema_version: 1` | `diagnostics.md` §7 | — | stdout | pass : objet valide ; fail : champ obligatoire manquant | B | candidate-stable | cases/cli/check-json-schema |
| F-META-003 | Codes de sortie `0/1/2/3` | `diagnostics.md` §8 | — | processus | pass : erreur → 1, usage → 2 ; fail : `run` propage (ambiguïté documentée) | B | candidate-stable | cases/cli/unknown-command |
| F-META-004 | Absence d'ABI stable | `abi.md` | — | aucun contrat binaire | pass : `--emit-rust` audité ; fail : revendiquer une ABI stable | B | candidate-stable | cases/parser/meta-abi-emit-rust, cases/parser/meta-abi-native-boundary |
| F-META-005 | Mono-thread, pas de concurrence | `concurrency.md` | — | un seul thread | pass : programme séquentiel ; fail : `thread::spawn` → `E-RES-UNKNOWN` | B | candidate-stable | cases/parser/meta-sequential-ok, cases/parser/meta-thread-spawn-unknown |
| F-META-006 | Machine abstraite du noyau pur (oracle) | `core-machine.md` | — | `NOT_IMPLEMENTED` | pass : aucun (oracle absent) ; fail : citer un résultat comme preuve | — | experimental | NOT_COVERED |
| F-META-007 | Versionnage et rollback | `compatibility.md` | — | fail-closed | pass : `edition` inconnue refusée ; fail : changer une sémantique sans édition majeure | B | candidate-stable | cases/parser/meta-version-flag, cases/parser/meta-edition-unknown |
| F-META-008 | `fmt` idempotent (LF, indentation) | `lexical.md` LEX-3.4, 9.5 | — | réécrit sans changer l'AST | pass : `fmt` deux fois ⇒ identique ; fail : AST changé | B | candidate-stable | cases/fmt/idempotent-comments, cases/fmt/idempotent-realistic |

## 12. Couverture et fermeture

| Indicateur | Valeur |
|---|---|
| Fonctionnalités supportées (`candidate-stable` + `experimental`) | **130** lignes `F-*` hors refus |
| Fonctionnalités refusées (`F-UNS`) | **20** lignes (chacune renvoie à `unsupported.md`) |
| Cas de conformance réellement présents | **0** — `conformance/` absent |
| Cas `NOT_COVERED` | **tous** |
| Cas `PENDING_IMPL` | 0 (aucun corpus) |
| Résultats `PASS` | **aucun** |

| ID | Règle | Niveau |
|---|---|---|
| FEAT-12.1 | Tant que `conformance/` n'existe pas, aucune fonctionnalité ne peut être présentée comme « testée ». Le statut maximal est `candidate-stable` **spécifié**. | MUST |
| FEAT-12.2 | Une fonctionnalité acceptée par le checker sans ligne ici est un **défaut de spécification** : elle doit être ajoutée (avec `FeatureId`) ou refusée. | MUST |
| FEAT-12.3 | Une ligne `unsupported` doit avoir un test négatif dans la conformance (T4) : c'est la garantie qu'elle n'est pas acceptée « par accident ». | MUST |
| FEAT-12.4 | Le mapping `FeatureId → code` doit rester cohérent avec [`diagnostics.md`](diagnostics.md) ; un code cité ici doit exister dans le registre. | MUST |
| FEAT-12.5 | Les lignes `experimental` (F-SYN-004, F-SYN-022, F-EXPR-018, F-TYP-020, F-EFF-006, F-STD-010) sont les seules susceptibles de changer sans édition majeure ; elles doivent être signalées dans tout rapport d'usage. | MUST |


### 12.2 Couverture réelle (corpus `conformance/`, 95 cas)

Présence d'un cas ≠ `PASS` : `ravum-conformance run` décide. Le corpus emploie encore les **anciens noms de codes** (voir §12.3).

| Indicateur | Valeur |
|---|---|
| FeatureIds suivis | 184 |
| FeatureIds avec au moins un cas | 94 |
| FeatureIds `NOT_COVERED` | 90 |
| Cas du corpus | 95 |
| Résultats `PASS` revendiqués ici | **0** |

**FeatureIds sans aucun cas (`NOT_COVERED`)** :

`F-LEX-004`, `F-SYN-001`, `F-SYN-002`, `F-SYN-005`, `F-SYN-006`, `F-SYN-007`, `F-SYN-008`, `F-SYN-013`, `F-SYN-015`, `F-SYN-016`, `F-SYN-017`, `F-SYN-019`, `F-SYN-020`, `F-SYN-025`, `F-SYN-027`, `F-SYN-028`, `F-SYN-029`, `F-SYN-030`, `F-SYN-031`, `F-SYN-033`, `F-SYN-034`, `F-TYP-002`, `F-TYP-003`, `F-TYP-004`, `F-TYP-005`, `F-TYP-006`, `F-TYP-008`, `F-TYP-014`, `F-TYP-016`, `F-TYP-017`, `F-TYP-018`, `F-TYP-022`, `F-TYP-023`, `F-TYP-024`, `F-TYP-026`, `F-EXPR-001`, `F-EXPR-002`, `F-EXPR-005`, `F-EXPR-006`, `F-EXPR-007`, `F-EXPR-008`, `F-EXPR-011`, `F-EXPR-012`, `F-EXPR-013`, `F-EXPR-015`, `F-OWN-004`, `F-OWN-005`, `F-OWN-006`, `F-OWN-008`, `F-OWN-010`, `F-OWN-012`, `F-OWN-014`, `F-OWN-015`, `F-EFF-004`, `F-EFF-006`, `F-EFF-008`, `F-MOD-002`, `F-MOD-005`, `F-MOD-012`, `F-MOD-013`, `F-STD-007`, `F-STD-008`, `F-STD-009`, `F-STD-010`, `F-STD-011`, `F-STD-019`, `F-STD-021`, `F-STD-024`, `F-STD-026`, `F-STD-027`, `F-STD-029`, `F-STD-031`, `F-STD-032`, `F-UNS-002`, `F-UNS-005`, `F-UNS-007`, `F-UNS-010`, `F-UNS-011`, `F-UNS-012`, `F-UNS-013`, `F-UNS-014`, `F-UNS-015`, `F-UNS-017`, `F-UNS-018`, `F-UNS-019`, `F-UNS-020`, `F-META-004`, `F-META-005`, `F-META-006`, `F-META-007`

### 12.3 Écarts de codes entre le corpus et le registre canonique

Le corpus (`conformance/cases/*/case.json`) attend encore des noms antérieurs. À aligner par le worker qa :

| Ancien code (corpus) | Code canonique |
|---|---|
| `E-LEX-BOM` | **(supprimé)** — le BOM est accepté et ignoré |
| `E-LEX-INVALID-CHAR` | `E-UNSUPPORTED-TOKEN` |
| `E-LEX-COMMENT-UNTERMINATED` | `E-LEX-COMMENT` |
| `E-LEX-STRING-UNTERMINATED` | `E-LEX-STRING` |
| `E-PARSE-TOKEN`, `E-PARSE-EXPR` | `E-PARSE-UNEXPECTED` |
| `E-PARSE-CONST`, `E-PARSE-LOOP-CONTROL` | `E-PARSE-EXPECTED` / `E-TYP-BREAK-OUTSIDE` |
| `E-RES-DEPENDENCY-UNDECLARED`, `E-RES-MAIN-MISSING` | `E-RES-UNKNOWN` / `E-TYP-MAIN-MISSING` |
| `E-RES-MODULE-NOT-FOUND`, `E-RES-MODULE-AMBIGUOUS` | `E-MOD-MISSING`, `E-MOD-AMBIGUOUS` |
| `E-TYP-METHOD` | `E-RES-UNKNOWN` |
| `E-MOD-DEP-NOT-LIB`, `E-MOD-MANIFEST-KEY`, `E-MOD-PATH-ESCAPE`, `E-MOD-SYMLINK` | `E-MOD-DEPENDENCY`, `E-MOD-MANIFEST`, `E-MOD-PATH-ESCAPE`, `E-MOD-PATH` |
| `E-EFF-UNKNOWN-GROUP` | `E-EFF-UNKNOWN` |
| `E-UNSUPPORTED-ATTRIBUTE`, `E-UNSUPPORTED-RAW-PTR`, `E-UNSUPPORTED-FIELD-SHORTHAND` | `E-UNSUPPORTED-ATTR`, `E-UNSUPPORTED-UNSAFE`, `E-PARSE-EXPECTED` |

Cas à **modifier** : `lexer/bom-rejected` devient `compile-pass` (BOM ignoré) ; `lexer/invalid-char-control` attend `E-UNSUPPORTED-TOKEN`.

Nouveaux cas à ajouter : `E-MOD-BOTH-ROOTS` (F-MOD-013), `E-MOD-RESERVED-NAME` (F-MOD-009, `security/reserved-windows-name` doit passer de `E-MOD-DEPENDENCY` à ce code), `E-OWN-IMMUTABLE` (F-TYP-006), `E-TYP-CONST` (F-SYN-008), `E-UNSUPPORTED-DROP-IMPL` (F-OWN-015), `E-TYP-RECURSIVE` (F-TYP-024), `E-LEX-INT-SUFFIX`/`E-LEX-INT-OVERFLOW` (F-LEX-005), `E-TYP-FLOAT-LITERAL` (F-TYP-020), `E-OWN-PARTIAL-MOVE` sur struct réutilisée (F-OWN-004), `E-MOD-PATH-ESCAPE` (F-MOD-009, déjà présent sous ce nom dans le corpus : ne pas remapper vers `E-MOD-PATH` !).

## 13. Questions ouvertes

- **Q-FEAT-1 (partiellement résolue)** — `conformance/` existe désormais (95 cas, worker qa) mais ses `expect_codes` utilisent encore les **anciens noms** de codes (voir §12.3). La couverture réelle est indiquée par ligne ; elle ne vaut pas `PASS`. Aucun résultat n'est revendiqué ici.
- **Q-FEAT-2 (résolue par le Lead)** — `for x in Vec` (F-SYN-022), `test fn -> Result` (F-SYN-004), `pow`/trig (F-STD-010/031) et l'absence d'unification rétroactive (F-TYP-020) restent `experimental` : supportés, mais susceptibles de changer.
- **Q-FEAT-3** — Les lignes `F-UNS-011..020` dépendent de la décision sur les mots-clés (`static`, `type`, `ref`, `dyn`, `await`) : si Q-LEX-1 change, plusieurs codes passent de `E-RES-UNKNOWN`/`E-PARSE-EXPECTED` à `E-UNSUPPORTED-TOKEN`.
- **Q-FEAT-4** — `F-STD-010` (`pow`/`powf`) est supporté mais non vérifiable bit-exact ; faut-il le rétrograder en `experimental` avec exclusion explicite de la conformance ? Ce document le fait déjà (`experimental`).
- **Q-FEAT-5** — Le compteur « 130 fonctionnalités supportées » est un compte de **lignes**, pas de règles atomiques ; il ne doit pas être cité comme métrique de complétude du langage.
