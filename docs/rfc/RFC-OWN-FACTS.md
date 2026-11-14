# RFC-OWN-FACTS-1 — Faire d'`own.rs` la source unique des faits d'usage des valeurs

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-OWN-FACTS-1` |
| Statut | **PROPOSED** — `PENDING_HUMAN_APPROVAL` (extension de l'interface gelée `Facts`) |
| Date | 2026-11-13 |
| Auteur | agent de développement (session GOAL) |
| Portée | `crates/ravum-lang-sema/src/own.rs`, `facts.rs`, `crates/ravum-lang-codegen/src/emit.rs` |
| Décision attendue | ratifier (ou refuser) l'ajout de `OwnFacts` à `CheckedProgram`, puis l'étape C (suppression du `MovePlanner` local d'`emit.rs`) |

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
| **C** — suppression de la logique locale | `NOT_RUN` | Le `MovePlanner` est toujours la source du dégradé de déstructurage ; la suppression exigera l'étape B complète et le diff byte-à-byte. |

Statut de la RFC : **`PROPOSED`** — `PENDING_HUMAN_APPROVAL`. L'étape A est
additive et sans effet sur le Rust émis ; l'étape B est un garde-fou de debug,
désactivé en release.

