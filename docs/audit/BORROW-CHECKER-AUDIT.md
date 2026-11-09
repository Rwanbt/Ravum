# Audit adversarial du borrow checker Ravum Core 1.0

> **Statut : audit interne indépendant — PAS un audit externe certifié.**
> Réalisé par le worker `qa-conformance`, qui n'a **pas** écrit `own.rs`. Le but
> était de **casser** le vérificateur, pas de le valider. Aucun fichier `src/`
> n'a été modifié ; les seuls ajouts sont ce document et
> `crates/ravum-lang-sema/tests/own_audit.rs`.
>
> Branche : `feat/lang-core-1.0-rc` · commit `8fd1547` · date 2026-11-09.
> **État testé** : arbre de travail avec les modifications en cours des autres
> workers (`own.rs`, `typeck.rs`, etc. marqués `M`) ; les findings ont été
> reproduits sur cet état, pas seulement sur le commit. Re-exécuter
> `--ignored` après chaque correction.
> Toolchain : `rustc 1.95.0` / `cargo 1.95.0` (Windows 11).
> Périmètre de preuve : **lecture du code + exécution du checker réel** sur
> 85 programmes, pas de preuve formelle, pas de fuzzing, pas de runtime.

## 1. Périmètre et méthode

Sources lues : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.7,
`spec/language/ownership.md`, `spec/language/diagnostics.md` §3.5,
`crates/ravum-lang-sema/src/own.rs` (~90 Ko), `types.rs`, `check.rs`,
`facts.rs`, les tests privés de `own.rs` et les cas
`conformance/cases/ownership/`.

Démarche :

1. cartographie règle → fonction de `own.rs` → test existant ;
2. identification des zones non couvertes (CFG, projections imbriquées,
   réassignation, portées, coercitions, provenance `&'static str`) ;
3. écriture de **85 cas** dans `tests/own_audit.rs` : 55 négatifs (le programme
   doit être refusé, avec un code précis) et 30 positifs légitimes
   (anti-faux-positifs), dont 6 reproductions de findings marquées `#[ignore]` ;
4. exécution du vrai parseur + `check()` public, sans mocker le checker ;
5. tout écart = finding avec repro minimale.

Les helpers de test de `own.rs` sont privés (`#[cfg(test)]`) ; `own_audit.rs`
les reproduit à l'identique via l'API publique
(`parser::parse_file` + `ravum_lang_sema::check` + `CheckOptions`).

Commandes exactes :

```sh
cargo test -p ravum-lang-sema --test own_audit -- --test-threads=1
cargo test -p ravum-lang-sema --test own_audit -- --test-threads=1 --ignored
cargo test -p ravum-lang-sema
```

## 2. Modèle supposé (et ce qui n'est pas vérifiable ici)

Règles visées : initialisation définie (OWN-1/4), Copy vs mobile (OWN-2), move
et use-after-move / double move (OWN-5), move partiel par projection
(OWN-5.3/5.4/5.5), prêts partagés/exclusifs et conflits (OWN-3.1–3.12),
non-échappement des références (OWN-3.13–3.17, OWN-6), `drop` (OWN-8/9),
réassignation (OWN-4.4), et leur interaction avec `if`/`match`/`while`/`loop`/
`for`/`?`/early return, les portées et le shadowing.

Hypothèses :

- le prêt dure au plus l'instruction englobante, sans two-phase borrows
  (OWN-3.6) ; les prêts d'un appel expirent à la fin de l'appel ;
- une référence non `&'static str` n'est ni stockable ni retournable ;
- un agrégat partiellement déplacé n'est plus utilisable comme un tout, mais
  ses champs non déplacés le restent ;
- `E-OWN-*` est la seule famille attendue dans ce périmètre ; les messages ne
  sont pas contractuels (DIAG-9.4).

Hors périmètre (non audités) : lifetimes explicites (inexistantes), validité
d'aliasing à l'exécution, `unsafe`/FFI, threads, codegen et runtime, résolution
de noms, typage, effets, fuzzing, preuve formelle de sûreté.

## 3. Cartographie règle → implémentation → test

| Règle | Implémentation (`own.rs`) | Tests d'audit (extrait) |
|---|---|---|
| Init définie, join de branches | `check_availability` :1403 (`uninitialised` :1465), `write_place` :1347, `join`/`join_slot` :1853/1883 | `audit_uninit_after_zero_iteration_loop`, `audit_double_deferred_init_is_allowed`, `audit_init_in_all_match_arms_is_allowed`, `audit_init_in_one_match_arm_is_rejected` |
| Copy / mobile | `consume_place` :1330, `is_copy_type` :2076 | `audit_move_then_reassign_on_*`, `audit_shadowing_after_move_is_allowed` |
| Move, use-after-move, double move | `perform_move` :1495, `report_use_after_move` :1671, `join_slot` :1882 | ancres move/double move, `audit_move_then_reassign_on_one_path_is_rejected`, `audit_move_in_inner_block_persists_after_block` |
| Move partiel par projection | `perform_move` :1524–1542, `record_partial_move` :1547, `classify_partial` :1956 | `audit_move_field_*`, `audit_read_moved_field`, `audit_move_parent_projection_then_child_is_rejected`, `audit_write_ancestor_restores_nested_field` |
| Move d'élément `Vec` | `perform_move` :1531, `report_move_indexed` :1741 | `audit_move_of_vec_element_is_rejected` |
| Move dans un appel / récepteur | `eval_call` :1158, `eval_method_call` :1217, `eval_call_arg` :1277 | `audit_move_in_call_then_use_anchor`, `audit_by_value_receiver_move_is_tracked` |
| Move et `match` | `eval_match` :880 | `audit_match_moves_scrutinee_then_use_is_rejected`, `audit_match_binding_moved_then_used_is_rejected` |
| Prêts : conflits, recouvrements | `borrow_place` :1376, `check_availability` :1472–1490, `places_overlap` :1976 | `audit_two_exclusive_args_conflict`, `audit_parent_and_child_mut_borrows_conflict`, `audit_opaque_array_indices_are_conservatively_conflicting`, `audit_receiver_loan_conflicts_with_argument_read` |
| Pas de two-phase borrow | `borrow_receiver` :1268 puis évaluation des arguments | `audit_receiver_loan_conflicts_with_argument_read`, `audit_receiver_loan_conflicts_with_index_argument`, `audit_setter_receiver_conflicts_with_field_argument` |
| Non-échappement des références | `check_let` :469 (refus :485–488), `analyze_block` :446, `Return` :731, agrégats :666/696, patterns :901, `for` :1071 | `audit_store_reference_in_let_is_rejected`, `audit_store_as_str_is_rejected`, `audit_return_reference_parameter_is_rejected`, `audit_reference_in_struct_literal_is_rejected`, `audit_return_local_as_str_is_rejected` |
| Provenance `&'static str` | `is_static_ref` :1602, `is_nonstatic_ref` :1625 | `audit_static_text_*`, **F1**, **F2** |
| `drop` | `eval_call` branche `drop` :1187 | `audit_drop_then_use_is_rejected`, `audit_drop_one_field_keeps_sibling_usable` |
| `if` / early return / divergence | `eval_if` :839 | `audit_early_return_after_move_on_other_path_is_allowed`, `audit_move_in_if_without_else_then_use_is_rejected` |
| Boucles et point fixe | `eval_while` :932, `eval_loop` :1005, `eval_for` :1061 | `audit_loop_move_then_break_then_use_is_rejected`, `audit_loop_move_then_break_then_use_is_rejected`, `audit_move_in_while_condition_then_use_after_is_rejected` |
| Court-circuit `&&`/`||` | `eval_value` :619–634 | `audit_short_circuit_move_then_use_is_rejected` |
| `?` | `eval_value(Try)` :713 | `audit_try_moves_result_twice_is_rejected` |
| Portées et shadowing | `push_scope`/`pop_scope` :345–356 | `audit_inner_block_shadow_does_not_move_outer`, `audit_shadowing_after_move_is_allowed` |
| Coercitions en argument | `eval_call_arg` :1277 | `audit_string_to_str_argument_is_allowed`, `audit_array_to_slice_argument_is_allowed`, `audit_mut_reborrow_parameter_is_allowed` |

## 4. Findings

### F2 — CRITICAL : une référence à une place/temporaire `&str` est acceptée comme « statique »

**Statut : acceptation non sûre.** Le checker accepte des programmes qui
stockent une référence vers un emplacement local/temporaire, contrairement à
OWN-3.3/OWN-6.1 (« seule `&'static str` est stockable »). Le troisième repro
lit un temporaire mort : c'est un **use-after-free** au sens du modèle
(pas de *temporary lifetime extension*, OWN-3.6).

Repro minimale 1 (test `audit_use_of_reference_to_temporary_is_rejected`,
ignoré) :

```rv
fn g(s: &str) {}
fn f() {
    let r = &"a";   // r : &&str, pointe un temporaire mort à la fin de l'instruction
    g(*r);          // lecture d'un emplacement mort
}
```

Observé : **programme accepté** (`checked.is_some()`, aucun `E-OWN-*`).
Attendu : `E-OWN-REF-ESCAPE` sur le `let r`.

Repro minimale 2 (test `audit_reference_to_static_local_is_rejected`, ignoré) :

```rv
fn f() {
    let text = "a";   // text : &'static str
    let r = &text;    // r : &&str, référence à la place de pile `text`
}
```

Observé : accepté. Attendu : `E-OWN-REF-ESCAPE`.

Repro minimale 3 (test `audit_reference_to_string_literal_is_rejected`, ignoré) :
`let r = &"a";` seul → accepté.

Cause racine : `own.rs::is_static_ref` :1602, branche :1605–1610 :

```rust
ExprKind::Unary { op: UnOp::Ref, expr: inner }
    => matches!(inner.kind, ExprKind::Str(_)) || self.is_static_ref(inner),
```

`is_static_ref` confond la **valeur** `&'static str` avec une **référence à une
place** qui contient une telle valeur. `&"a"` a le type `&&str` (référence à un
temporaire), et `&text` référence la pile ; les deux sont marquées « statiques »
puis `check_let` :485–488 saute le `E-OWN-REF-ESCAPE`. La provenance statique
est purement syntaxique et récursive, sans contrôle du **niveau** de référence.

Impact : viole le contrat de non-échappement et permet un accès à un
temporaire/libération de pile. Le programme ci-dessus n'est pas seulement
« accepté par erreur » : il déréférence un emplacement dont la durée de vie est
terminée. Nonobstant l'absence de codegen dans ce test, c'est un défaut de
sûreté du vérificateur, pas un simple faux positif. Cela suffit à invalider un
verdict T4 « aucun programme non sûr accepté » pour l'état actuel.

Piste de correction (non appliquée — `src/` interdit dans ce périmètre) :
la provenance statique doit être une propriété du **type** de l'expression
(`&text` avec un seul niveau), pas une récursion syntaxique :
`Unary Ref` produit toujours une référence à une place/temporaire et doit
retourner `false` ; `Str` seul, une variable dont le type est exactement `&str`
et initialisée statiquement, le `tail` d'un bloc, et (après correction de F1)
`if`/`match` dont **toutes** les branches sont statiques sont les cas
légitimes. Idéalement, exposer un fait `is_static_text(expr_id)` calculé par le
type-checker plutôt que de refaire une analyse syntaxique.

### F1 — MEDIUM : faux positifs sur la provenance `&'static str` (règles valides refusées)

**Statut : faux positif (pas de trou de sûreté).** Trois programmes légitimes
sont refusés avec `E-OWN-REF-ESCAPE` :

1. `audit_static_text_through_if_is_storable` :

```rv
fn f(c: bool) -> &str {
    let text: &str = if c { "a" } else { "b" };
    return text;
}
```

2. `audit_static_text_through_match_is_storable` : même chose avec
   `match c { true => "a", false => "b" }`.
3. `audit_static_text_from_function_is_storable` :

```rv
fn lit() -> &str { return "a"; }
fn f() -> &str {
    let text = lit();
    return text;
}
```

Observé : deux `E-OWN-REF-ESCAPE` (stockage + retour). Attendu : acceptation.
Cause : `is_static_ref` ne traite ni `If`, ni `Match`, ni `Call` (branche `_ =>
false`). `own.rs` documente une restriction volontaire (« seules les variables
initialisées par un littéral `&str` sont stockables ») ; F1 est donc une
limitation conservatrice **documentée**, mais elle refuse des programmes dont
la staticité est évidente et devrait être levée avec F2 (même fonction, même
correctif). Un `&str` retourné par une fonction ne peut, en Core 1.0, être
qu'une référence statique (les retours non statiques sont refusés par le même
checker) : le refus de `let text = lit();` est donc particulièrement coûteux.

### LOW-1 — Messages non contractuels, mais grammaticalement faux

`report_ref_escape` :1798 produit « une référence ne peut pas être **stocker** »
et « **retourner** » (infinitifs). Les messages étant hors contrat (DIAG-9.4),
c'est LOW, mais cela nuit à la lecture des diagnostics. Attendu : « stockée » /
« retournée ».

### LOW-2 — Codes émis absents du registre canonique `diagnostics.md`

`own.rs` émet `E-OWN-NOT-COPY` (:679), `E-OWN-MOVE-INDEXED` (:1743),
`E-OWN-MOVE-BORROWED` (:1755) et `E-OWN-LIMIT` (:1647). Le registre canonique
`diagnostics.md` §3.5 ne liste que `E-OWN-UNINIT`, `E-OWN-USE-AFTER-MOVE`,
`E-OWN-DOUBLE-MOVE`, `E-OWN-PARTIAL-MOVE`, `E-OWN-BORROW-CONFLICT`,
`E-OWN-REF-ESCAPE`, `E-OWN-MOVE-OUT-OF-BORROW`, `E-OWN-IMMUTABLE` : les quatre
codes ci-dessus (dont `MOVE-BORROWED`, explicitement choisi par `own.rs` contre
`MOVE-OUT-OF-BORROW`) sont donc hors registre. Ce n'est pas un défaut du
checker mais un **écart spec↔implémentation** à trancher avant la RC, car un
consommateur qui applique le registre ou un test d'ensemble exact les rejettera.

## 5. Défenses vérifiées (aucun contournement trouvé)

Les 79 cas actifs passent. Points particulièrement sensibles testés sans trou :

- move/use-after-move/double move, y compris dans un appel, via `?`, dans un
  bloc interne, après shadowing, et réassignation sur tous les chemins ;
- move partiel de champ/tuple/tableau, champ déplacé relu/emprunté, agrégat
  entier après move partiel, réassignation exacte ou de l'ancêtre ;
- move à travers `&`/`&mut` (`E-OWN-MOVE-BORROWED`), index `Vec`
  (`E-OWN-MOVE-INDEXED`) ;
- conflits de prêts : deux `&mut`, `&mut`+lecture, `&mut`+move, récepteur
  `&mut self` + argument (absence de two-phase borrow), parent/enfant, index
  littéraux disjoints vs index opaque conservateur, deux reborrows du même
  `&mut` ;
- non-échappement : `let`, `as_str`, retour de paramètre, champ de struct
  littéral, tableau de références, affectation à un champ `&str`, retour d'un
  `as_str` local, branche non statique d'un `if` ;
- init définie : deux branches, un bras de `match` manquant, zéro itération de
  boucle, double initialisation différée ;
- `drop` d'un champ puis usage du champ frère ; `drop` d'un Copy inoffensif ;
- récepteurs `self` par valeur (move de champ autorisé) vs `&self`/`&mut self`
  (move interdit) ;
- coercitions `&String → &str`, `&[T; N] → &[T]`, reborrow `&mut *p` ;
- CFG : early return après move sur l'autre chemin, `if` sans `else`,
  `match` sur le scrutinee, court-circuit, boucles avec break/move, condition
  de `while` qui déplace.

## 6. Résultats exacts

```text
$ cargo test -p ravum-lang-sema --test own_audit -- --test-threads=1
test result: ok. 79 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out

$ cargo test -p ravum-lang-sema --test own_audit -- --test-threads=1 --ignored
test result: FAILED. 0 passed; 6 failed; 0 ignored; 79 filtered out
# 6 échecs = 3 repros F1 (faux positifs) + 3 repros F2 (acceptations non sûres)

$ cargo test -p ravum-lang-sema
# lib : 127 passed, 0 failed
# autres cibles : 5 passed, 1 passed + 1 ignored (cible existante)
# own_audit : 79 passed, 6 ignored
```

Les 6 tests de finding sont `#[ignore = "AUDIT-FINDING-F1/F2 …"]` pour que la
suite normale reste verte **sans masquer** les défauts : `--ignored` est la
reproduction. Chaque test porte la règle visée en commentaire.

## 7. Verdict

**Le borrow checker n'est pas sûr en l'état (F2 CRITICAL).** Un programme qui
stocke `&"a"` (type `&&str`, référence à un temporaire) puis le déréférence dans
une instruction ultérieure est accepté, alors que le temporaire est mort
(OWN-3.6/OWN-6.1). Les chemins move/emprunt/partiel/CFG testés (79 cas actifs)
n'ont révélé aucun autre contournement, et le modèle conservateur produit des
refus attendus ; mais un seul trou d'acceptation non sûre suffit à refuser un
verdict T4 PASS.

Recommandation : **bloquer la RC** jusqu'à correction de F2 (et idéalement F1,
même fonction) + ajout de tests de régression dans `own.rs` et d'un cas
`conformance/cases/ownership/` (hors périmètre de cet audit). Après correction,
relancer ce fichier : les 6 tests ignorés doivent passer (ou être convertis en
tests actifs).

## 8. Limites de l'audit

- Audit **interne indépendant**, pas un audit externe certifié, pas une preuve
  formelle ; aucun résultat n'est `PROVED`.
- 85 programmes écrits à la main ; pas de fuzzing, pas de génération
  aléatoire, pas d'exploration exhaustive des chemins.
- Pas de test d'exécution/codegen : les acceptations non sûres sont établies au
  niveau du vérificateur, pas par un binaire exécuté.
- Les messages et les codes ne sont vérifiés que lorsqu'ils sont contractuels ;
  les messages sont hors contrat (DIAG-9.4).
- Les limitations connues du frontend (paramètres `mut` ignorés, `&&str`
  non nommable, patterns exigeant un bras `_`) ont été contournées dans les
  cas, pas auditées.
- Le périmètre de l'audit exclut lifetimes, références stockées en général
  (sauf non-échappement), `unsafe`/FFI, threads, runtime et sûreté mémoire du
  code émis.

## 9. Correctifs appliqués après l'audit (Lead, 2026-11-09)

| Finding | Correctif | Preuve |
|---|---|---|
| **F2 (CRITICAL)** | `own.rs::is_static_ref` : la branche `Unary Ref` retourne désormais **toujours** `false` (une référence à une place/temporaire n'est jamais `'static`). `&"a"` (`&&str`) et `&text` sont donc refusés comme tout échappement. | `own_audit` : `audit_reference_to_string_literal_is_rejected`, `audit_reference_to_static_local_is_rejected`, `audit_use_of_reference_to_temporary_is_rejected` passent ; cas de conformance `ownership/ref-to-temporary` (E-OWN-REF-ESCAPE). |
| **F1 (MEDIUM)** | `is_static_ref` gère `if`/`match` (toutes branches statiques) ; `Facts::static_str_fns` calcule par point fixe les fonctions dont **tous** les `return`/la queue sont statiques, et `Call` n'est statique que s'il cible une de ces fonctions utilisateur (jamais un builtin). | `audit_static_text_through_if_is_storable`, `..._through_match_is_storable`, `..._from_function_is_storable` passent ; tests unitaires own.rs (dont récursion non classée). |
| F2 tests | Les 6 tests de findings ne sont plus `#[ignore]` : ils sont devenus des tests de régression actifs. | `cargo test -p ravum-lang-sema --test own_audit` → **85 passés, 0 échec**. |

Limites conservatrices restantes (faux positifs uniquement, aucune acceptation
non sûre connue) : provenance via variable locale non propagée entre fonctions,
branches divergentes (`if c { return "a"; } else { "b" }`) non classées,
méthodes utilisateur/constantes non propagées.

Verdict après correctifs : **SOUND sur le sous-ensemble audité** (F2 fermé,
aucun programme non sûr accepté connu sur les 85 cas), **PARTIAL globalement** —
audit interne, pas externe, pas de preuve formelle, pas de fuzzing du checker.
