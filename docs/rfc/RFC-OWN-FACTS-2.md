# RFC-OWN-FACTS-2 — supprimer la re-dérivation d'ownership dans le backend

- **Statut** : `ACCEPTED` **sous délégation** (plan `PRODUCTION-PLAN-2026-11-15`,
  décision **P-7**, 2026-11-15).
- **Implémentation** : **C1 faite** (`copy_type`, PR #134, merge `b7b534e`) ;
  **C2–C5 `PENDING`**. C1 a immédiatement révélé une divergence réelle :
  `sema::own::is_copy_type` classait `Slice(I32)` comme `Copy` alors qu'OWN-2.1
  ne l'y met pas et qu'OWN-2.2 rend mobile tout le reste ; le backend avait
  raison, le classifieur a été corrigé et un test le verrouille.
- **Motif** : deux implémentations de la même connaissance (le checker et le
  backend) sont la cause de toutes les divergences `check → build` trouvées
  depuis la passe 6. Tant qu'elles existent, une divergence est possible par
  construction.
- **Portée** : `ravum-lang-sema` (émet les faits) et `ravum-lang-codegen`
  (cesse de les recalculer). Aucune règle de spécification n'est modifiée.

## 1. État actuel

`OwnFacts` (RFC-OWN-FACTS-1, étape A) publie trois tables :

| Champ | Contenu | Consommateur |
|---|---|---|
| `use_kind: BTreeMap<NodeId, UseKind>` | move / copy / borrow par expression utilisée | étape B (garde de cohérence) |
| `place: BTreeMap<NodeId, OwnPlace>` | liaison + projections par expression-place | étape B |
| `element_reinit: BTreeSet<NodeId>` | cibles d'affectation qui ré-initialisent un élément de tableau | `emit::paths` |

Le backend **re-dérive** encore trois décisions :

| Site | Ce qu'il décide | Pourquoi c'est un risque |
|---|---|---|
| `Emitter::is_copy_type` (`emitter_items.rs`) | un type est-il `Copy` ? | miroir de `sema::own::is_copy_type` ; un désaccord change un move en copie |
| `Emitter::is_owned_position` | l'expression est-elle en position possédée ? | règle syntaxique dupliquée, indépendante du checker |
| `Emitter::consumed_by_checker` | l'expression a-t-elle été consommée ? | re-déduit ce que le checker a déjà décidé |

L'étape B (`verify_ownership_facts`) compare ces décisions **en build debug**
seulement : c'est un garde-fou de test, pas une source de vérité.

## 2. Extension de schéma proposée

Quatre tables supplémentaires, chacune répondant à un site de re-dérivation :

### 2.1 `copy_type: BTreeSet<NodeId>`

- **Clé** : identifiant du *nœud de type résolu* (`Facts::type_nodes`), pas du
  type structurel.
- **Sens** : le type désigné par ce nœud est `Copy` au sens du checker.
- **Remplace** : `Emitter::is_copy_type`.
- **Invariant** : pour tout nœud de type présent dans `type_nodes`, l'appartenance
  à `copy_type` est décidée une seule fois, par le checker.
- **Pourquoi par nœud** : le backend doit décider sur des types **sans** nœud
  (types de paramètres synthétiques, types de retour inférés). Ces cas sont
  couverts par `type_nodes` quand ils existent ; sinon la table porte une entrée
  par identifiant de type **de syntaxe** effectivement visité, et le backend
  n'invente rien : il consulte ou échoue explicitement (`E-CODEGEN-FACT-MISSING`).

### 2.2 `owned_position: BTreeSet<NodeId>`

- **Clé** : nœud d'expression.
- **Sens** : cette expression est utilisée en position possédée (déplacement
  possible), décision du checker.
- **Remplace** : `Emitter::is_owned_position`.
- **Invariant** : `owned_position` est cohérent avec `use_kind` — une expression
  en `UseKind::Move` est en position possédée ; l'inverse n'est pas vrai (une
  expression possédée peut être copiée).

### 2.3 `consumed: BTreeSet<NodeId>`

- **Clé** : nœud d'expression.
- **Sens** : le checker a consommé cette expression (déplacement effectif ou
  transfert à un appel).
- **Remplace** : `Emitter::consumed_by_checker`.
- **Propagation exigée** (P-7) : la décision est propagée **à travers `match`,
  `if` et bloc**, c'est-à-dire qu'un nœud consommé dans un bras de `match` ou une
  branche de `if` est marqué consommé au même titre qu'un nœud consommé dans le
  flux direct. Sans cette propagation, le backend devrait re-déduire la règle, ce
  qui est exactement ce que cette RFC supprime.

### 2.4 `binding_identity: BTreeMap<NodeId, BindingId>`

- **Clé** : nœud d'expression-path qui référence une liaison.
- **Sens** : identité **unique** de la liaison visée (`BindingId` opaque, stable
  par programme).
- **Remplace** : la re-dérivation du nom de liaison par projection textuelle
  (`emit::names` + comparaison de chemins), qui se trompe sur le shadowing
  imbriqué.
- **Invariant** (P-7) : **une identité de liaison unique par paramètre** ; deux
  paramètres homonymes de fonctions différentes ont des identités différentes,
  et un shadowing crée une nouvelle identité — jamais une réutilisation.

## 3. Ordre de migration

Chaque étape est indépendante, vérifiable, et **ne supprime un site de
re-dérivation qu'après** avoir prouvé l'équivalence sur le corpus :

1. **Étape C1 — `copy_type` (FAITE, 2026-11-15).** Émettre la table, l'utiliser dans `Emitter::is_copy_type`
   en repli (fait d'abord, puis re-dérivation supprimée), étendre l'étape B pour
   exiger l'égalité sur tout le corpus.
2. **Étape C2 — `owned_position`.** Même schéma pour `is_owned_position`.
3. **Étape C3 — `consumed`** avec propagation `match`/`if`/bloc ; supprimer
   `consumed_by_checker`.
4. **Étape C4 — `binding_identity`** ; supprimer la comparaison de noms.
5. **Étape C5 — retrait du miroir** : `sema::own::is_copy_type` reste la seule
   implémentation ; le backend n'a plus de fonction de classification.

## 4. Vérification exigée

Une étape n'est « faite » que si les cinq preuves suivantes existent :

| Preuve | Commande / artefact |
|---|---|
| Tests du crate | `cargo test -p ravum-lang-sema -p ravum-lang-codegen` |
| Garde de cohérence étape B, **non vide** | elle doit échouer si l'on injecte un désaccord (test négatif existant, à étendre par table) |
| Comportement identique | **S7** : Rust généré identique octet pour octet sur les exemples |
| Contrat `check ⇒ build` | `ravum-conformance run --verify-build`, 0 FAIL |
| Oracle différentiel | 0 divergence sur la campagne rapide (la nominale reste E7) |

## 5. Ce que cette RFC ne fait pas

- Elle **ne change aucune règle** de `spec/language/ownership.md` ; elle déplace
  la connaissance, pas la sémantique.
- Elle **ne promeut aucun gate** : `T2` reste `PARTIAL` (dénominateur MUST, D10)
  et `T4` reste `BLOCKED` (audit externe).
- Elle **n'autorise aucune suppression de test** : les gardes de l'étape B
  deviennent plus stricts, jamais plus lâches.
- L'acceptation est **déléguée** par P-7 ; la ratification humaine des ADR
  (D2) reste ouverte et distincte.
