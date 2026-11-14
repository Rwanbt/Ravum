# RFC-RT-TRAPS-1 — Fermer les traps implicites du profil `rt`

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-RT-TRAPS-1` |
| Statut | **PROPOSED** — `PENDING_HUMAN_APPROVAL` (règle de langage) |
| Date | 2026-11-13 |
| Auteur | agent de développement (session GOAL) |
| Portée | `spec/language/effects.md`, `expressions.md`, `stdlib.md`, `effects.rs`, `typeck.rs`/`builtins.rs` |
| Décision attendue | ratifier la sémantique des traps en `rt`, ou choisir une variante |

> **Statut de preuve.** Aucune ligne ci-dessous n'est « prouvée ». Les cas de
> conformance de la phase 5 mesurent le comportement du compilateur sur le
> corpus ; ils ne prouvent pas l'absence de trap dans un programme arbitraire.

## 1. Problème mesuré

Le profil `rt` interdit les effets `io fs net alloc panic unsafe ffi async rt`
dans le corps d'une fonction, vérifiés transitivement (`docs/architecture/RAVUM-CORE-DESIGN.md`
§3.6). Mais les **traps intrinsèques** du langage ne sont pas des effets :

- indexation `a[i]` hors bornes → panic ;
- division et reste par zéro → panic ;
- débordement entier `+ - *` → panic (le design impose `overflow-checks=on`) ;
- décalage ≥ largeur → panic.

Une fonction `rt` peut donc encore s'arrêter dans un callback audio. C'est
contraire à **INV-05** et vide le profil de son sens. Le design le reconnaît
comme « limite assumée » (§3.6) ; cette RFC propose de la fermer **pour `rt`
uniquement**.

## 2. Règle proposée (MUST, profil `rt` seulement)

Soit une opération de l'un des genres suivants. Elle produit l'effet `panic`
— donc `E-EFF-PROFILE` en profil `rt` — **sauf** dans les formes explicites
listées.

| Opération | Forme exemptée de `panic` en `rt` |
|---|---|
| `a[i]` sur `[T; N]` | `i` est un **littéral entier** `k` avec `0 <= k < N` |
| `a[i]` sur `&[T]`, `Vec<T>` | **aucune** (taille inconnue à la compilation) |
| `x / y`, `x % y` entiers | `y` est un littéral **non nul** et, pour un type signé, `y != -1` (le cas `MIN / -1` déborde) |
| `x + y`, `x - y`, `x * y` entiers | **aucune** forme nue : utiliser `wrapping_*`, `saturating_*` ou `checked_*` |
| `-x` entier signé | **aucune** forme nue : utiliser `wrapping_neg`, `saturating_neg` ou `checked_neg` |
| `x << k`, `x >> k` | `k` est un littéral `< largeur du type` |
| arithmétique flottante | **exemptée** (IEEE-754, pas de trap) ; NaN/infini documentés |
| `as` | **exempté** (sémantique définie : troncature, arrondi, saturation) |

Les méthodes explicites ajoutées à la stdlib (toutes enregistrées dans la table
des builtins) :

- `wrapping_add/sub/mul/neg` (résultat de même type) ;
- `saturating_add/sub/mul/neg` (résultat de même type) ;
- `checked_add/sub/mul/div/rem/neg` (résultat `Option<T>`) ;
- **`wrapping_div`/`wrapping_rem` ne sont pas des formes sûres** : ils paniquent
  encore sur un diviseur nul. Seuls `checked_div`/`checked_rem` sont totaux.

Conséquence assumée : en `rt`, une indexation dynamique de `Vec`/slice devient
impossible sans passer par une forme vérifiée ; il n'existe pas encore de motif
de référence (`Some(&x)`) pour lire `get`. C'est une **restriction**, pas un
oubli : elle est écrite et testée comme telle. Les cas positifs montrent les
formes autorisées.

### Pourquoi `core` et `pure` ne changent pas

`core` reste permissif (les traps restent des panics contrôlés, documentés).
`pure` conserve sa limite actuelle : ses traps ne sont pas des effets (design
§3.6). La règle est donc **spécifique au profil `rt`**, implémentée comme un
**fait de trap** (par fonction et par expression) consommé par le contrôle de
profil `rt`, et non comme une modification des ensembles d'effets — sinon `pure`
changerait aussi. Le diagnostic reste `E-EFF-PROFILE`, avec une note nommant
l'opération et la forme explicite attendue.

## 3. Alternatives et coûts

| Option | Coût | Effet |
|---|---|---|
| (a) Statu quo documenté | nul | `rt` ne garantit rien sur les traps ; INV-05 reste violé |
| (b) Cette règle (formes explicites) | moyen : faits de trap, méthodes stdlib, cas de corpus, adaptation d'un exemple | `rt` devient **défendable** sur les traps, sans preuve d'intervalles |
| (c) Preuve d'intervalles / WCET | recherche : analyse de bornes, modèle machine, budgets | hors périmètre Core 1.0, à rouvrir plus tard |

## 4. Validation exigée

Cas de conformance **négatifs** (profil `rt`, attendu `E-EFF-PROFILE`) :

- `a[i]` avec `i` variable sur `[i32; 4]` ;
- `a[i]` avec littéral hors bornes ;
- `x / y` avec `y` variable ;
- `x % y` avec `y` variable ;
- `i + 1` (addition nue) ;
- `-x` (négation nue) ;
- `x << k` avec `k` non littéral.

Cas **positifs** :

- `a[2]` sur `[i32; 4]` ; `a[0][1]` sur tableau imbriqué de taille connue ;
- `x / 3`, `x % 2` (littéral non nul) ;
- `x.wrapping_add(1)`, `x.saturating_mul(2)`, `x.checked_div(y)` + `match`
  `Option<i32>` ;
- arithmétique `f64` (`*`, `/`, `-`) ;
- `as`.
- `examples/dsp-gain-iir` compile en `--profile rt` sans trap implicite.

Contrôles : conformance `--verify-build` 0 FAIL ; oracle différentiel 0
divergence ; aucun changement de verdict pour `core`/`pure` sur le corpus
(vérifié par diff des résultats JSON avant/après).

## 5. Décisions demandées

1. Ratifier la règle du §2 (ou choisir « statu quo » / « preuve d'intervalles »).
2. Trancher le point des **décalages** et de la **négation unaire** : cette RFC
   les inclut par cohérence, alors que l'énoncé initial ne nommait que
   `+ - *`, `/`, `%` et `[]`.
3. Confirmer que `pure` **ne change pas** dans cette itération (asymétrie
   assumée) ou demander une RFC séparée pour l'aligner.

## 6. État d'implémentation (2026-11-13, session GOAL)

| Tâche | État | Preuve / limite |
|---|---|---|
| 5.1 — RFC | fait | ce document, statut `PROPOSED` / `PENDING_HUMAN_APPROVAL` |
| 5.2 — gate `rt` + méthodes explicites | **fait** (profil `rt` uniquement) | `crates/ravum-lang-sema/src/traps.rs` : détection index, division/reste, `+ - *` (binaire **et affectation composée** `x += 1`), négation, décalages ; flottants et `as` exemptés ; `check.rs` ne l'appelle que si `Profile::Rt`. Méthodes `wrapping_*`, `saturating_*`, `checked_*` ajoutées aux builtins et au passage direct du codegen. **10 tests unitaires** dans `traps.rs`. `core`/`pure` inchangés (`effects/intrinsic-panic-not-tracked` reste accepté en core). |
| 5.3 — cas de conformance | **fait** | 5 cas : `rt-trap-bare-arithmetic`, `rt-trap-dynamic-index`, `rt-trap-dynamic-division`, `rt-trap-compound-assignment` (compile-fail, `E-EFF-PROFILE`) et `rt-trap-safe-forms` (compile-pass). Conformance **476 cas / 475 PASS / 0 FAIL / 1 SKIP**, build **203/203**. |
| 5.4 — adapter `examples/dsp-gain-iir` à `rt` | **`NOT_RUN`** | Trois règles indépendantes du profil `rt` s'y opposent : (1) indexation dynamique de slices (`samples[i]`, `out`) sans accesseur sûr ; (2) `Vec::push` = effet `alloc` interdit ; (3) `io::println` = effet `io` interdit. L'exemple calcule et **affiche** son résultat ; le rendre compilable en `rt` exigerait une nouvelle surface stdlib **et** de supprimer sa sortie, donc de changer ce que le smoke teste. Aucune modification de l'exemple ; options (exemple séparé, accesseurs sûrs, sortie conditionnelle) à trancher par décision humaine. |

Statut final : **`PROPOSED` — `PENDING_HUMAN_APPROVAL`**. Implémentation
volontairement limitée à `rt` ; aucune revendication temps réel.

