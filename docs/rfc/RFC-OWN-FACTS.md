# RFC-OWN-FACTS-1 — Faire d'`own.rs` la source unique des faits d'usage des valeurs

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-OWN-FACTS-1` |
| Statut | **ACCEPTED** — approuvée par le responsable, 2026-11-14 (A1), étape C comprise |
| Date | 2026-11-13 |
| Auteur | agent de développement (session GOAL) |
| Portée | `crates/ravum-lang-sema/src/own.rs`, `facts.rs`, `crates/ravum-lang-codegen/src/emit.rs` |
| Décision attendue | ratifier (ou refuser) l'ajout de `OwnFacts` à `CheckedProgram`, puis l'étape C (suppression du `MovePlanner` local d'`emit.rs`) |

> **Amendement du 2026-11-14 (`goal/consolidation-2`).** Les exemples de §2
> supposent un `NodeId` `u32` 8/24 bits ; le contrat courant est **`u64`
> 32/32**, **ratifié le 2026-11-14** (S2). La RFC est **`ACCEPTED` depuis le
> 2026-11-14** (A1, étape C comprise) ; l'**étape C est `PARTIAL`** : deux
> familles de décision lisent `OwnFacts` en release (`receiver`, `arguments`),
> cinq restent dans le backend pour des causes mesurées, une par une (§9). Le
> `MovePlanner` n'est donc **pas** supprimé.

> **Ce que cette RFC n'est pas.** Elle ne change aucune règle de langage, aucun
> code de diagnostic, aucun schéma JSON public et aucune sémantique d'ownership.
> Elle déplace une **décision interne** (move / copie / emprunt / place) du
> backend vers le vérificateur qui la prend déjà. Elle ne prend effet qu'après
> approbation humaine, parce que `Facts` est une interface gelée du design §4.

## 1. Problème

Le vérificateur d'ownership prend déjà, pour chaque expression, une décision
d'usage : `Access::{Read, Write, Move, BorrowShared, BorrowExclusive}`, sur une
`Place` = racine (`SlotId`) + projections (`Step::{Deref, Field, Tuple, Index}`,
`IndexKey::{Lit, Opaque}`) — `own.rs` lignes 219-243 et 321-328. Ces décisions
**sont jetées** à la fin de la passe : `Facts` ne contient que les types, les
résolutions et les effets.

Le backend, lui, **re-dérive** les mêmes décisions pour produire du Rust qui
compile :

- `emit.rs` a un `MovePlanner` complet (ligne 3574) qui décide quelles
  expressions sont des moves, quelles places doivent être déstructurées et où
  hoister le déstructurage (`plan_array_moves`, `plan_slot_groups`,
  `build_destructure`, `plan_prefix_slots`) ;
- il re-dérive aussi « ce type est-il `Copy` ? » (`is_copy`, ligne 1594) et
  « ce builtin consomme-t-il son récepteur ? »
  (`builtin_method_moves_receiver`, ligne 927) ;
- il doit prouver localement que toute utilisation d'une place après un
  déstructurage est couverte par une liaison — commentaire lignes 3188-3245.

Deux modèles du même fait évoluent donc séparément. C'est la cause structurelle
de la classe **P0-A** (« `check` accepte, `build` refuse ») : quand `own.rs` et
`MovePlanner` divergent, le programme passe le checker puis `rustc` échoue
(E0508, E0382, E0310). Treize formes ont déjà été fermées une par une ; la
classe n'est pas fermée par construction.

## 2. Proposition

`own.rs` écrit ses décisions dans un fait typé, stocké dans `Facts` :

```rust
// facts.rs — ADDITIF, derive Debug/Clone/PartialEq/Eq/Default
pub struct OwnFacts {
    /// Pour chaque expression qui a un usage de valeur : move, copie, emprunt.
    pub use_kind: BTreeMap<NodeId, UseKind>,
    /// Pour les expressions qui sont des places : racine + projections.
    pub place: BTreeMap<NodeId, PlacePath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind { Move, Copy, Borrow, BorrowMut }

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlacePath {
    pub root: PlaceRoot,
    pub steps: Vec<Projection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlaceRoot {
    /// Liaison locale (`let`, paramètre, pattern de `match`) : NodeId de
    /// l'identifiant lié, stable par fichier (voir RFC-NODE-ID-1).
    Binding(NodeId),
    /// Valeur temporaire sans liaison (ex. `make().field`) : pas de racine
    /// nommable ; `steps` part de la valeur.
    Temporary,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Projection {
    Deref,
    Field(String),
    Tuple(u32),
    Index(IndexKey),   // Lit(u128) | Opaque — même clé que own.rs
}
```

Règles de production :

1. **Exhaustivité** : toute expression pour laquelle `own.rs` prend une décision
   d'usage a exactement une entrée dans `use_kind`. Une expression lue par copie
   est `Copy` (le backend doit pouvoir choisir copie vs move sans redériver
   `is_copy`).
2. **Places** : `place` est rempli pour les expressions qui sont des places
   (y compris les récepteurs et les arguments empruntés), pas pour les
   temporaires qui n'en sont pas.
3. **Déterminisme** : `BTreeMap` uniquement, aucune itération dépendante de
   l'ordre d'allocation ; `NodeId` porte déjà le `FileId` (8 bits hauts) et un
   compteur par fichier (24 bits bas), donc deux fichiers ne se décalent pas.
4. **Pas de fuite d'état** : `OwnFacts` ne contient ni `SlotId`, ni `Loan`, ni
   `State` — seulement des identifiants AST et des projections nommables. Le
   vérificateur reste libre de changer son état interne.

## 3. Consommation en trois étapes

| Étape | Contenu | Critère de sortie |
|---|---|---|
| **A** | `own.rs` remplit `OwnFacts` ; personne ne les lit encore. Tests unitaires sur les faits produits (move/ copy / borrow par forme). | `cargo test -p ravum-lang-sema` vert ; conformance 0 FAIL ; aucun changement du Rust émis |
| **B** | En mode debug/`cfg(test)`, `emit.rs` compare sa décision locale à `OwnFacts` et **échoue** en cas de désaccord. Conformance `--verify-build` + oracle différentiel de la phase 2 tournent dans ce mode. | Chaque désaccord est classé (checker ou backend) et corrigé avec un test. **Arrêt si > 20 désaccords** : l'étape B reste comme garde-fou et C est `BLOCKED`. |
| **C** | `emit.rs` lit `OwnFacts` et sa logique locale (`MovePlanner`, `is_copy` local, `builtin_method_moves_receiver`) est supprimée. | Rust généré **identique octet pour octet** sur `examples/` et tout le corpus de conformance ; toute différence est listée et justifiée. |

## 4. Alternatives et coûts

| Option | Coût | Verdict |
|---|---|---|
| (a) Statu quo + sondage manuel | Nul à court terme ; la classe P0-A reste ouverte à chaque extension | Refusé : c'est l'état actuel |
| (b) IR de type MIR partagée | Élevé : réécriture du backend et du checker, hors périmètre Core 1.0 | Différé : c'est la cible théorique, mais pas maintenant |
| (c) `OwnFacts` (cette RFC) | Moyen : extension additive de `Facts`, migration en trois commits, comparaison byte-à-byte exigée | **Retenu** |

`OwnFacts` n'est pas une IR : c'est un **journal de décisions** déjà prises. Il
ne duplique ni les types, ni les effets, ni les résolutions.

## 5. Validation exigée

- `cargo run -p ravum-conformance -- run --verify-build` : **0 FAIL**, périmètre
  build inchangé ou en hausse.
- Oracle différentiel (phase 2) : mode rapide 3 × 200, mode long aussi large que
  la session le permet, **0 divergence**.
- Comparaison byte-à-byte du Rust généré avant/après l'étape C sur `examples/`
  et tout le corpus ; un diff non expliqué annule le commit.
- `cargo test --workspace --all-targets -- --test-threads=2` : aucun test
  supprimé, aucun seuil de couverture abaissé.

## 6. Risques et limites

- **Couplage** : `own.rs` cesse d'être un pur validateur et devient producteur
  pour le backend. C'est explicitement le compromis accepté par la décision D3
  de l'audit ; l'interface est versionnée par cette RFC.
- **Non-échappement** : les décisions sur les temporaires (place `Temporary`)
  restent partielles ; le backend garde la responsabilité de la forme Rust, pas
  de la sémantique de move.
- **Portée** : la RFC ne traite pas les traps, les effets ni les lifetimes.
- **Réversibilité** : l'étape A est purement additive ; un `git revert` de
  l'étape C restaure le `MovePlanner` sans changer les faits.

## 7. Décision demandée

1. Approuver l'ajout de `OwnFacts` à `Facts` (interface gelée, design §4) — ou
   demander une variante de structure.
2. Approuver l'étape C **seulement si** l'étape B ne révèle pas plus de
   20 désaccords et si le Rust généré reste identique octet pour octet.
3. En cas de désaccord sur une règle MUST pendant l'étape B, la RFC s'arrête :
   la règle se tranche dans une RFC de langage séparée, pas ici.

## 8. État d'implémentation (2026-11-13, session GOAL)

| Étape | État | Preuve / limite |
|---|---|---|
| **A** — production des faits | **`TESTED`** | `own.rs` remplit `Facts.own` (`use_kind`, `place` avec binding + projections) ; `crates/ravum-lang-sema/tests/own_facts.rs` : 2 tests (move, copy, borrow, move projeté `p.b`, déterminisme) |
| **B** — comparaison en debug | **`PARTIAL`** | `emit.rs::verify_ownership_facts` compare la classification `Copy`/`Move` de chaque usage en build debug et échoue en cas de désaccord ; il compare aussi, depuis le round 15, les **projections enregistrées** (`x.f`, `.0`, `[lit]`, `[*]`, `*p`) à une place recalculée **depuis la syntaxe** (et non depuis `MovePlanner`). Deux tests négatifs prouvent la non-vacuité : décision `Copy` injectée sur un `String`, et projection corrompue injectée sur `a[1]` ⇒ `CodegenError`. Mesuré : **600 programmes générés + 476 cas de conformance, 0 désaccord**. Exclusions documentées : l'identité de la liaison (le span de déclaration n'est pas dans l'AST), les **récepteurs de méthode** (auto-déréférence par l'appel) et les **arguments de type référence** (reborrow implicite) ne sont pas comparés. |
| **C** — suppression de la logique locale | **`PARTIAL`** | Deux familles (`receiver`, `arguments`) lisent `OwnFacts` **en release** et leur code local est supprimé (§9.1). Cinq familles sont `BLOCKED` avec une cause mesurée chacune (§9.2) : le `MovePlanner` reste nécessaire pour elles, et n'est donc pas supprimé. L'étape B reste le garde-fou de debug des familles non migrées. |

Statut de la RFC : **`ACCEPTED` (2026-11-14, A1)**, étape C comprise. L'étape A
est additive et sans effet sur le Rust émis ; l'étape B est un garde-fou de
debug, désactivé en release ; l'étape C est **`PARTIAL`** : deux familles sont
migrées et lisent les faits en release, cinq sont `BLOCKED` (§9). Le
`MovePlanner` n'est pas supprimé.

## 9. Étape C — familles de décisions (vague 2, 2026-11-14)

### 9.1 Méthode et périmètre de preuve

Une **famille** est un endroit où `emit.rs` prend seul une décision d'usage
déjà prise par `own.rs`. Chaque famille est migrée par **un commit** qui
remplace la décision locale par la lecture d'`OwnFacts`, puis vérifiée :

- **S7** : le Rust généré doit être identique à une référence produite sur
  `goal/wave-3` (`3ca4d0e`, intégration 1) pour **tout `examples/`** et **tout
  le corpus de conformance**. La normalisation est **limitée** aux deux motifs
  `__rv_root_<id>` et `rvm_ffi_<id>` (les chiffres qui suivent ces préfixes) ;
  toute autre différence — y compris `__rv_sel_<id>`, l'ordre, les blancs et
  les noms — est un échec qui annule le commit de la famille.
- **Corpus de comparaison** : 497 paquets (11 `examples/` + 486 cas de
  conformance), dont **239 fichiers Rust** émis (11 + 228) ; les 258 autres
  cas n'émettent pas par construction (`check-fail`, parse-fail, `unsupported`
  et méta-cas CLI). La liste des cas émis fait partie de la comparaison : un
  cas qui cesse d'émettre est un échec.
- Les tests `backend_audit` et l'oracle typé rapide sont exécutés **à chaque
  famille**, et ils ne sont pas redondants : `descents` est détectée par le
  corpus S7 **et** par l'audit backend, tandis que `pattern-consumption` donne
  un corpus **identique** (239/239) et n'est détectée que par l'oracle typé.
  Aucune des trois preuves n'est suffisante seule.
- Les outils de mesure (générateur de référence, comparaison S7) sont **hors
  dépôt** ; aucun fichier de preuve n'entre dans le dépôt.

### 9.2 Résultat, famille par famille

| Famille | Décision locale d'`emit.rs` | État | Preuve |
|---|---|---|---|
| `receiver` | `MovePlanner::by_value_receiver` + `builtin_method_moves_receiver` : récepteur `self` par valeur (signature utilisateur, ou liste STD-11.3 `Option`/`Result` recopiée dans le backend) | **`TESTED`** (migrée) | Rare de `UseKind` de l'expression récepteur : `Move`/`Copy` = consommé, `Borrow`/`BorrowMut` = emprunté. Les deux dérivations locales sont supprimées. S7 : 239/239 fichiers identiques, 497 cas de manifeste inchangés. Conformance 486 cas : 485 pass, 0 fail, 1 skip ; build perimeter 207/207, 0 échec. |
| `arguments` | `call_argument_positions` + `method_argument_positions` : type du paramètre déclaré (`is_copy_type`) pour les fonctions/méthodes utilisateur, `true` forcé pour les builtins | **`TESTED`** (migrée) | Rare de `UseKind` de chaque expression d'argument. Les deux dérivations locales sont supprimées. S7 : 239/239 identiques, 497 cas inchangés. Conformance 486 cas : 485 pass, 0 fail, 1 skip. |
| `move-simple` | `is_owned_position` (type non `Copy`) dans `Access::consumed` et dans le drapeau `mobile` d'une projection de champ sur une chaîne d'index | **`BLOCKED`** | **2** fichiers sur 239 changent : `ownership/array-element-projection-move-pass` et `ownership/array-projection-two-moves-pass`. Le déstructurage `let [mut __rv_el_0_0, mut __rv_el_0_1] = a;` … `return __rv_el_0_1.0;` disparaît au profit de `return a[1usize].0;` — la classe **P0-A** (`E0508`) se rouvre. **Cause** : `own.rs` enregistre la décision sur l'expression de place **la plus externe** (`a[1].0`) ; le nœud d'index interne `a[1]` n'a **aucune** entrée `use_kind`. |
| `descents` | `owned && is_owned_position(expr)` à l'entrée des `descents` (chaîne d'index dont la racine n'est pas une place : `make()[0]`, `a[0].0[1]`, `h.items[0].0[1]`) | **`BLOCKED`** | **1** fichier sur 239 change : `run/array-move-descent` — la descente matérialisée (`{ let __rv_root_# = make(); … into_iter().nth(…) … }`) devient `crate::rvm_root::make()[i]`, et `backend_audit::audit_non_place_array_roots_build_and_run` échoue : `error[E0508]: cannot move out of type [String; 2], a non-copy array`. **Cause** : aucune décision n'est enregistrée pour un index sur une racine rvalue, alors que le type (donc la décision locale) est disponible. |
| `pattern-consumption` | `pattern_consumes_value` : consommation de l'initialiseur d'un `let` déduite de la forme du motif | **`BLOCKED`** | Le corpus S7 est **identique** (239/239) : il n'exerce pas la forme. L'**oracle typé rapide** diverge : `seed=0x00524156554d1001 index=35 kind=build_failed`, `error[E0508]` sur `v18[1usize]` dans `let v15: String = match (…) { … => { let v18: [String; 2] = …; v18[1] }, … }`. **Cause** : les faits enregistrent des décisions sur des **places** ; un `match`/`if`/bloc n'a pas d'entrée `use_kind`, donc le drapeau « consommé » qui doit traverser ces constructions jusqu'à leur queue est perdu. |
| `field-projection` | `place_of` : reconstruction syntaxique de la place (liaison résolue par nom + projections `Field`/`Tuple`) | **`BLOCKED`** | `OwnFacts::place` porte `binding: Span`, et le span de déclaration d'un **paramètre** est celui de l'**item** englobant : deux paramètres d'une même fonction enregistrent le **même** span (mesuré : `fn f(a: [String; 2], b: [String; 2])` ⇒ les deux places de `a[0]` et `b[0]` portent `binding=Span { file: 1, start: 0, end: 100 }`). L'identité de liaison que `place_of` rend (`BinderId`, sensible au masquage) n'est donc pas récupérable, et grouper des accès par ce span fusionnerait deux paramètres distincts. |
| `literal-index` | `constant_index` : valeur d'un index littéral, et bornes via `array_layers` | **`BLOCKED`** | Même cause mesurée que `descents` : les pas `Index(Lit(v))` n'existent que sur l'expression de place **la plus externe** et seulement pour une racine-place ; les deux sites qui baissent les chaînes à racine rvalue (`chain_move_expr`, `first_index` d'un `descent`) n'ont aucun fait. Un remplacement fact-only y produirait exactement l'`E0508` mesuré sur `descents` ; la famille n'a pas fait l'objet d'un run S7 séparé (aucune sortie différente à prouver : elle est impossible à couvrir sans les faits manquants). |

**Ce qui reste de `MovePlanner`, et pourquoi.** Le planificateur reste
**entier** : il porte les cinq familles `BLOCKED` ci-dessus, c'est-à-dire le
dégradé de déstructurage des places d'index (`plan`, `build_destructure`,
`plan_prefix_slots`, `plan_slot_groups`), la dérivation de la place
(`place_of`), la valeur des index littéraux (`constant_index`) et la
propagation syntaxique de « consommé » à travers motifs et constructions de
valeur. Ces cinq décisions reposent sur des informations que `OwnFacts` **ne
contient pas** aujourd'hui (identité de liaison unique par paramètre, décision
sur chaque nœud d'une chaîne de place, propagation à travers `match`/`if`/bloc
et racines rvalue). Les réduire demande une **extension de schéma** (nouvelle
RFC), pas un simple déplacement de lecture ; c'est la raison pour laquelle la
RFC ne supprime pas le planificateur.

**Deux limites structurelles du schéma actuel, mesurées** (à l'origine de
quatre des cinq blocages) :

1. **La décision est enregistrée par expression évaluée, pas par place
   physique** : une place chaînée `a[1].0` n'a qu'une entrée, sur l'expression
   la plus externe ; ses sous-places n'en ont aucune, et une racine rvalue n'a
   aucune entrée du tout.
2. **L'identité de liaison n'est pas unique** : `OwnPlace::binding` est un
   `Span` de déclaration, et pour un paramètre (ou `self`) c'est le span de
   l'item englobant — deux paramètres sont indiscernables. Le garde-fou de
   l'étape B compare donc ces spans sans pouvoir distinguer deux paramètres
   d'une même fonction.

Ces deux limites n'invalident pas l'étape A/B : elles bornent ce que l'étape C
peut déplacer sans nouveau fait.

### 9.3 État des preuves de l'étape C

| Élément | État |
|---|---|
| Familles migrées | 2 (`receiver`, `arguments`) — `TESTED` sur le corpus S7, la conformance et les tests du crate |
| Familles `BLOCKED` | 5 (`move-simple`, `descents`, `pattern-consumption`, `field-projection`, `literal-index`) — cause mesurée pour chacune |
| `MovePlanner` | conservé (aucune famille ne permet sa suppression) |
| Code local supprimé | `by_value_receiver`, `builtin_method_moves_receiver`, `call_argument_positions`, `method_argument_positions` |
| Lecture en release | `Emitter::consumed_by_checker` et `Emitter::moved_by_checker` ne sont **pas** derrière `debug_assertions` : pour les familles migrées il n'y a plus de seconde décision à comparer |
| Garde-fou de debug | `verify_ownership_facts` reste actif (comparaison `use_kind` ⇔ type, et places ⇔ syntaxe) : il couvre les familles **non** migrées |

Aucun de ces constats n'est un `PROVED` : ce sont des observations liées au
commit, à la commande et à la machine cités dans le rapport d'exécution. La
décision A1 approuve l'étape C ; son résultat est **`PARTIAL`** et ne
revendique ni exhaustivité ni maturité.

