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

Conséquence assumée : en `rt`, une indexation dynamique de `Vec` — et de slice
hors des deux formes bornées ci-dessous — reste impossible sans passer par une
forme vérifiée ; il n'existe pas encore de motif de référence (`Some(&x)`) pour
lire `get`. C'est une **restriction**, pas un oubli : elle est écrite et testée
comme telle. Les cas positifs montrent les formes autorisées.

### Indexation bornée (amendement T6, 2026-11-14)

La règle « aucune indexation dynamique » est restreinte par **deux formes
reconnues syntaxiquement**, sans analyse d'intervalles :

- `for i in 0..K { a[i] }` où `K` est un **littéral** (écrit `Kusize`, car
  l'index d'un tableau doit être `usize`) et `K <= N` pour un tableau `[T; N]`,
  y compris à travers une référence `&[T; N]` ;
- `for i in 0..a.len() { a[i] }` où `a` est un tableau ou une tranche (y compris
  `&[T; N]` / `&[T]`), où l'index est **la variable de boucle elle-même**, et où
  le corps ne réaffecte ni `a` ni la variable de boucle (et ne la masque pas par
  un `let` ou une boucle imbriquée).

Rien d'autre n'est reconnu : un index **calculé** (`a[i + 1]`,
`a[i.wrapping_add(0)]`), une borne littérale **trop grande**, une variable
masquée ou réaffectée, et `Vec<T>` restent des traps. Deux précisions mesurées :

1. Les tableaux n'ont pas de méthode `.len()` en Core 1.0 : la forme
   `0..a.len()` ne s'applique donc qu'aux tranches (`a: &[T]`), et la forme
   littérale est la voie des tableaux.
2. `a = b` sur une tranche est déjà refusé par la propriété
   (`E-OWN-REF-ESCAPE` : une référence ne peut pas être stockée) : la clause
   « non réaffecté » est **défensive**. Elle est implémentée et couverte par un
   cas de conformance qui attend *les deux* diagnostics.

La reconnaissance est **locale à la boucle** (`traps.rs` empile les boucles
reconnues et refuse la forme dès qu'un `let`/`for` masque la variable ou qu'une
affectation la vise, balayage conservateur du corps) : aucune preuve d'intervalle
n'est revendiquée.

### Profil `rt` par point d'entrée (T6)

Les règles `rt` (effets interdits **et** traps) peuvent s'appliquer non plus au
paquet entier mais aux **fonctions atteignables** depuis une fonction désignée :

- `ravum-lang check . --profile rt --rt-entry <chemin::de::fonction>` ;
- `ravum-lang build . --rt-entry <chemin::de::fonction>` (le `build` n'a pas
  d'option `--profile` : `--rt-entry` y implique l'analyse `rt`).

`--profile rt` **sans** `--rt-entry` garde le comportement historique (paquet
entier, `main` ou tests comme entrées). L'entrée doit être une **fonction libre**
adressée par son chemin de modules (`process`, `dsp::process`); les méthodes
d'`impl` ne sont pas adressables et une entrée inconnue est refusée
(**`E-TOOL`**, code d'outillage hors registre du langage, exit 1) — jamais un
ensemble atteignable vide accepté silencieusement. L'atteignabilité est
syntaxique, sur le graphe d'appels déjà construit par l'inférence d'effets
(`Facts::calls` / `Facts::methods`) : elle ne modélise ni pointeurs de fonction
(inexistants), ni récursion indirecte par données.

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
- `examples/dsp-gain-iir` compile en `--profile rt --rt-entry process` sans trap
  implicite, avec une sortie identique octet pour octet (le smoke reste 43/0/0).

**T6 (amendement 2026-11-14)** — cas de conformance `effects/rt-entry-*` :

- `rt-entry-bounded-index` (**compile-pass, build vérifié**) : `process` indexe
  `[f64; 4]` / `&[f64; 4]` dans `for i in 0..4usize`, et `main` (io, `+` nu) est
  hors du point d'entrée ;
- `rt-entry-computed-index` : `a[i.wrapping_add(0)]` ⇒ `E-EFF-PROFILE` ;
- `rt-entry-bound-too-large` : `0..9usize` sur `[i32; 4]` ⇒ `E-EFF-PROFILE` ;
- `rt-entry-reassigned-slice` : tranche réaffectée ⇒ `E-EFF-PROFILE` +
  `E-OWN-REF-ESCAPE` (la réaffectation est déjà refusée par la propriété) ;
- `rt-entry-missing` : entrée inexistante ⇒ `E-TOOL`, exit 1.

Le corpus passe à **483 cas : 482 PASS / 0 FAIL / 1 SKIP**, périmètre build
**207/207**, `verified=124` (voir §7).

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
4. Ratifier l'**amendement T6** (point d'entrée `--rt-entry`, indexation bornée,
   obligations et limites du §2) ou demander une variante.

## 6. État d'implémentation (2026-11-13, session GOAL)

| Tâche | État | Preuve / limite |
|---|---|---|
| 5.1 — RFC | fait | ce document, statut `PROPOSED` / `PENDING_HUMAN_APPROVAL` |
| 5.2 — gate `rt` + méthodes explicites | **fait** (profil `rt` uniquement) | `crates/ravum-lang-sema/src/traps.rs` : détection index, division/reste, `+ - *` (binaire **et affectation composée** `x += 1`), négation, décalages ; flottants et `as` exemptés ; `check.rs` ne l'appelle que si `Profile::Rt`. Méthodes `wrapping_*`, `saturating_*`, `checked_*` ajoutées aux builtins et au passage direct du codegen. **10 tests unitaires** dans `traps.rs`. `core`/`pure` inchangés (`effects/intrinsic-panic-not-tracked` reste accepté en core). |
| 5.3 — cas de conformance | **fait** | 5 cas : `rt-trap-bare-arithmetic`, `rt-trap-dynamic-index`, `rt-trap-dynamic-division`, `rt-trap-compound-assignment` (compile-fail, `E-EFF-PROFILE`) et `rt-trap-safe-forms` (compile-pass). Conformance **476 cas / 475 PASS / 0 FAIL / 1 SKIP**, build **203/203**. |
| 5.4 — adapter `examples/dsp-gain-iir` à `rt` | **fait (T6)** | L'exemple utilise des buffers fixes et une entrée `process` (`--rt-entry process`) : `check` en `rt` passe, la sortie reste identique octet pour octet (smoke **43/0/0**). Les trois obstacles initiaux sont traités : indexation bornée (`for i in 0..4usize`), plus de `Vec::push`, et l'`io` reste dans `main`, hors du point d'entrée. |
| 5.5 — point d'entrée et indexation bornée (T6) | **fait**, règle `PENDING_HUMAN_APPROVAL` | `--rt-entry` sur `check` (avec `--profile rt`) et `build` ; atteignabilité sur le graphe d'appels ; deux formes d'indexation bornée ; entrée inconnue refusée (`E-TOOL`). 5 tests unitaires, 1 test CLI et 5 cas de conformance (§7). |

Statut final : **`PROPOSED` — `PENDING_HUMAN_APPROVAL`**. Implémentation
volontairement limitée à `rt` ; aucune revendication temps réel. L'amendement T6
(§2 et §7) est implémenté et mesuré, mais la règle elle-même n'est pas ratifiée.

## 7. État T6 (2026-11-14)

**Commit** : `986f542` `feat(rt): scope the rt gate to a named entry and bound
loop indexes`. **Fichiers** : `crates/ravum-lang-sema/src/{effects,check,traps}.rs`,
`crates/ravum-lang-cli/src/{args,pipeline}.rs`,
`crates/ravum-lang-cli/src/commands/{check,build,mod}.rs`,
`examples/dsp-gain-iir/src/main.rv` (+ `ravum.lock`), cinq cas
`conformance/cases/effects/rt-entry-*`.

**Ce qui est implémenté.**

- `effects::infer_and_check` prend l'entrée `rt` (`Option<NodeId>`) et renvoie le
  graphe d'appels ; `effects::reachable_from` le parcourt en largeur.
- `check_with_entry` / `check_entry_with_timings` transmettent l'entrée ; les
  traps ne visitent que les fonctions atteignables (`traps::check_rt_traps`,
  `reachable: Option<&BTreeSet<NodeId>>`), les effets interdisent la même
  atteignabilité.
- L'entrée est résolue dans la CLI par chemin de modules (`find_function`) ; une
  entrée inconnue produit `E-TOOL` et **aucune** vérification n'est exécutée
  (fail-closed).
- Indexation bornée dans `traps.rs` : pile de boucles reconnues, refus dès que le
  corps masque/réaffecte la variable ou réaffecte la base, comparaison
  `K <= len(tableau)` en regardant à travers les références.

**Preuves exécutées.**

```
cargo test -p ravum-lang-sema -- --test-threads=2
  17 groupes : 711 passés / 0 échec / 1 ignoré
cargo test -p ravum-lang-cli -- --test-threads=2
  8 groupes : 54 + 7 + 44 + 6 + 11 + 5 + 3 + 3 passés / 0 échec
cargo run -p ravum-conformance -- run --verify-build
  summary: total=483 pass=482 fail=0 skip=1 not_run=0
  build: perimeter=207 covered=207 verified=124 implicit=83 failed=0
pwsh -File scripts/lang-smoke.ps1
  RESUME: 43 PASS / 0 FAIL / 0 NOT_RUN
cargo run -p ravum-lang-cli -- check examples/dsp-gain-iir --profile rt --rt-entry process
  0 error (5 items, 4 functions) ; sortie identique à l'ancienne version
pwsh -File scripts/evidence.ps1
  7 contrôles PASS, evidence/current.json écrit
```

**Limites.** L'atteignabilité est syntaxique (aucune analyse d'alias) ; l'entrée
est *déclarée* par l'utilisateur — le compilateur ne prouve pas qu'elle est
appelée depuis un callback audio ; la règle d'indexation reste locale à deux
formes et ne prouve pas les bornes en général ; `Vec<T>` reste refusé ; les
méthodes d'`impl` ne sont pas adressables ; `build` sans `--profile` ne peut pas
exprimer `pure` ; la clause « base non réaffectée » est défensive (§2). Aucune
preuve d'absence de trap dans un programme arbitraire.

