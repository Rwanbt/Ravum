# RFC-INVARIANTS-1 — Invariants de struct vérifiés à l'exécution

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-INVARIANTS-1` |
| Statut | **ACCEPTED** — ratifiée par le responsable le 2026-11-20 (décision V1-C5), avec INV-9 (décision V1-C6) |
| Date | 2026-11-20 |
| Auteur | agent de développement (session GOAL) |
| Gate | T18 (ADR-004/028), décision V1-C4 (forme `invariant` sur struct) |
| Portée | `ravum-lang-syntax` (parseur, formateur), `ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-ir`/`ravum-lang-jit`, `ravum-lang-cli` (faits T19) |

> **Statut de preuve.** Comme les contrats de RFC-CONTRACTS-1, un invariant
> est **vérifié à l'exécution**, aux points d'observation définis ci-dessous.
> Un programme qui passe ses tests n'a **pas** prouvé ses invariants : état
> honnête `TESTED`, jamais `PROVED`. Aucune preuve statique, aucun solveur.

## 1. Problème

`requires`/`ensures` décrivent une fonction ; ils ne disent rien d'une
propriété qu'un **objet** doit garder pendant toute sa vie (`lo <= hi`,
`len <= cap`). Écrire cette propriété dans chaque `ensures` de chaque
méthode est répétitif et s'oublie. RFC-CONTRACTS-1 §6 l'avait reportée : il
faut d'abord définir **où** l'objet est observé.

## 2. Syntaxe

Après l'accolade fermante d'une struct à champs nommés, zéro ou plusieurs
clauses :

```
struct Range {
    lo: i64,
    hi: i64,
}
    invariant self.lo <= self.hi
    invariant self.hi - self.lo < 1000
```

- `invariant` est un mot **contextuel** : il n'est reconnu qu'à cette place.
  À cet endroit seul un item pouvait suivre, et `invariant` ne commence aucun
  item : aucun programme existant ne change (COMPAT-2.4) ; `let invariant = 1;`
  reste valide.
- L'expression est lue sans littéral de struct (comme une condition de `if`).
  Elle voit `self` (`&S`).
- `ravum-lang fmt` met chaque clause sur sa ligne, indentée d'un niveau.

## 3. Règles (MUST)

| Règle | Énoncé |
|---|---|
| INV-1 | Une clause est de type `bool` (`E-TYP-MISMATCH`) et **pure** : elle est le corps d'une méthode synthétique `pub fn __rv_invariant_<n>(&self) -> bool effects()`, donc soumise aux règles ordinaires (résolution, typage, propriété, effets : un effet donne `E-EFF-UNDECLARED`). Les diagnostics nomment « the `invariant` clause », jamais la méthode générée. |
| INV-2 | Chaque **littéral de struct** `S { .. }` est vérifié après l'évaluation de ses champs : chaque clause, dans l'ordre ; la première fausse arrête : `panic: contract violated: invariant <texte>`. |
| INV-3 | À chaque **sortie normale** d'une méthode `&mut self` de `S` (`return`, valeur de queue, `?`), après ses `ensures`, chaque clause est vérifiée de même. Une panique du corps ne vérifie rien. |
| INV-4 | Le message suit l'amendement 1 de RFC-CONTRACTS-1 : ` (self.f = v, …)` pour les champs dont le type est montré (scalaires, `String` entre `"`), dans l'ordre de déclaration. Voies `rustc`, JIT et interpréteur de l'IR : même texte. |
| INV-5 | Profil `rt` : une clause est refusée (`E-EFF-PROFILE`). |
| INV-6 | V1 : pas d'invariant sur une struct générique (`E-CON-UNSUPPORTED`). |
| INV-7 | Les noms synthétiques ne peuvent pas être écrits : `_rv_` est réservé par le lexer (`E-LEX-RESERVED-IDENT`). |
| INV-9 | Hors des méthodes de `S` (son `impl`), un champ de `S` n'est **jamais écrit ni emprunté `&mut`** : affectation, affectation composée, `&mut s.f`, méthode `&mut self` appelée sur le champ (`s.v.push(..)`) donnent `E-CON-FIELD-WRITE`. La lecture reste libre. Toute modification passe donc par une méthode `&mut self`, vérifiée à sa sortie (INV-3). Dans une méthode de `S`, un champ d'une **autre** struct à invariant reste protégé. |
| INV-8 | Faits T19 : chaque clause est un contrat `S#invariant.<n>` (`kind: invariant`, `verification: runtime-checked`), `exercised_by` = tests qui atteignent une construction de `S` ou une méthode `&mut self` de `S` (arêtes du graphe d'appels vers la méthode synthétique). `test --json` le nomme dans `violated_contracts`. |

### Limites (déclarées, non cachées)

- **Points d'observation** : INV-2 et INV-3 ; INV-9 ferme les écritures directes. Restent hors de la règle : une méthode `self` par valeur qui modifie sa propre copie (elle ne modifie pas l'objet observé) ; la déstructuration d'un `&mut S` est impossible : Core 1.0 n'a pas de motif de struct (`let`/`match` le refusent, vérifié le 2026-11-20).
- Pas de vérification à l'entrée des méthodes (l'invariant est supposé tenu,
  il l'était à la dernière observation).
- Le JIT ne gère que les structs `Copy` (limite existante de l'IR) ; la voie
  `rustc` gère toutes les structs non génériques.

## 4. Implémentation

1. **Syntaxe** : `StructItem::invariants: Vec<Invariant { text, method, span }>` ;
   le parseur place juste après la struct un `impl S` synthétique dont chaque
   méthode porte l'expression d'une clause (l'expression n'existe qu'une
   fois dans l'arbre : pas de `NodeId` dupliqué). Le texte de clause est
   normalisé par jetons (espacement canonique) : `fmt` ne change jamais un
   message — vrai aussi pour `requires`/`ensures`.
2. **Sema** : `StructInfo::invariants` ; `effects` ajoute les arêtes
   « littéral → méthodes d'invariant » et « méthode `&mut self` → méthodes
   d'invariant » (recalculées à chaque vérification) ; refus `rt`.
3. **Codegen `rustc`** : un littéral devient
   `{ let v = S { .. }; if !v.__rv_invariant_0() { panic!(..) } v }` ; une
   méthode `&mut self` devient une méthode interne privée plus une enveloppe
   qui l'appelle avec `&mut *self` et vérifie au retour.
4. **IR/JIT** : appel des méthodes synthétiques après un littéral et à chaque
   `Return` d'une méthode `&mut self` ; note de trap (IR v1.4) pour les
   valeurs.

## 5. Preuves (état `TESTED`)

- `struct_invariants_behave_the_same_on_rustc_and_jit` : même sortie et même
  message sur `rustc` et le JIT (littéral, sortie anticipée de méthode, pas de
  vérification dans une méthode `&self` ni sur une écriture directe).
- `invariants_are_contract_facts_and_named_by_failed_tests` (T19).
- Tests du parseur (désucrage, texte normalisé, cas négatifs) et du
  formateur (disposition, idempotence, sens préservé).
- `field_writes_outside_the_methods_of_an_invariant_struct_are_refused` (INV-9 : 4 chemins d'écriture refusés, acceptés sans invariant, permis dans les méthodes).
- Conformance : `types/invariant-field-write`, `run/invariant-holds`, `run/invariant-violated-literal`,
  `run/invariant-violated-method`, `types/invariant-not-bool`,
  `effects/invariant-impure`, `effects/invariant-rt-profile`,
  `parser/invariant-generic-struct`, `lexer/invariant-reserved-name`.

## 6. Alternatives écartées

- **Vérifier après toute écriture de champ** : coût à chaque affectation et
  sémantique floue au milieu d'une mise à jour en plusieurs étapes
  (`lo` puis `hi`) ; écarté pour la V1.
- **Vérifier aussi à l'entrée des méthodes** : double coût sans gain tant que
  les points INV-2/INV-3 sont les seuls à modifier l'objet.
- **Attribut `#[invariant(..)]`** : les attributs ne sont pas sémantiques en
  Core 1.0.
