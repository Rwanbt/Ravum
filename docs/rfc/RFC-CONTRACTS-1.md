# RFC-CONTRACTS-1 — Contrats `requires` / `ensures` vérifiés à l'exécution

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-CONTRACTS-1` |
| Statut | **ACCEPTED** — ratifiée par le responsable le 2026-11-20 (décision V1-C2) |
| Date | 2026-11-19 |
| Auteur | agent de développement (session GOAL) |
| Gate | T18 (ADR-004/028), décision V1-C1 : vérification à l'exécution |
| Portée | `ravum-lang-syntax`, `ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-ir`/`ravum-lang-jit`, `spec/language/` |

> **Ratification du 2026-11-20.** Le responsable a accepté cette RFC telle
> qu'implémentée (voie `rustc` et JIT, 6 cas de conformance). Les preuves
> restent `TESTED` ; rien ne devient `PROVED`.

> **Statut de preuve.** Un contrat de cette RFC est **vérifié à l'exécution**,
> au moment de l'appel. Un programme qui passe ses tests n'a donc **pas**
> prouvé ses contrats : l'état honnête est `TESTED`, jamais `PROVED`.
> Aucune preuve statique, aucun solveur.

## 1. Problème

Une IA qui écrit une application doit pouvoir dire *ce qu'une fonction
promet* et être arrêtée **au point exact** où une promesse est rompue, avec
un message qui la nomme. `assert`/`panic` existent, mais ils sont mêlés au
corps, ne distinguent pas la faute de l'appelant (`requires`) de celle de la
fonction (`ensures`), et ne sont pas visibles dans la signature, donc pas
dans les faits exportés aux outils (T19).

## 2. Syntaxe

Après le type de retour et `effects(...)`, avant le corps, zéro ou plusieurs
clauses, dans l'ordre :

```
fn div(a: i64, b: i64) -> i64 effects()
    requires b != 0
    ensures result * b + a % b == a
{
    return a / b;
}
```

- `requires` et `ensures` sont des mots **contextuels** (comme `effects`,
  LEX-5.5) : ils ne sont reconnus qu'à cette place ; `let requires = 1;`
  reste valide (COMPAT-2.4 : aucun programme existant ne change).
- L'expression est lue sans littéral de struct, comme une condition de `if`
  (G-6.8 : pas d'affectation).

## 3. Règles (MUST)

| Règle | Énoncé |
|---|---|
| CON-1 | Une clause est de type `bool` (`E-TYP-MISMATCH` sinon). |
| CON-2 | Une clause est **pure** : effet vide, hors `panic` implicite des opérations (`E-CON-EFFECT` sinon). Elle ne modifie rien. |
| CON-3 | Une clause voit les paramètres de type `Copy` (pour `ensures`, leur valeur **à l'entrée**) ; `ensures` voit aussi `result`, la valeur rendue, qui doit être `Copy`. Un paramètre non `Copy` dans une clause est refusé (`E-CON-NONCOPY`) : une clause ne déplace ni n'emprunte rien que le corps utilise (V1 ; l'emprunt partagé est une extension future). |
| CON-4 | `result` n'est lié que dans `ensures` ; ailleurs c'est un identifiant ordinaire. |
| CON-5 | À l'entrée, après l'évaluation des arguments, chaque `requires` est évalué dans l'ordre ; le premier faux arrête le programme : `panic: contract violated: requires <texte>` (texte source normalisé de la clause). |
| CON-6 | À chaque retour (`return`, valeur de queue), chaque `ensures` est évalué dans l'ordre, `result` lié ; le premier faux arrête : `panic: contract violated: ensures <texte>`. Une panique du corps n'évalue aucun `ensures`. |
| CON-7 | Profil `rt` : une clause est refusée (`E-EFF-PROFILE`) : sa violation est un trap implicite (RFC-RT-TRAPS-1). |
| CON-8 | Les clauses font partie de la signature : elles entrent dans l'empreinte d'interface de la fonction (T13 : un appelant est revérifié si elles changent). |

## 4. Implémentation

1. **Syntaxe** : `FnItem::contracts: Vec<Contract { kind, expr, text, span }>`.
2. **Sema** : résolution et typage dans la portée des paramètres ; `result`
   lié au type de retour pour `ensures` ; effets vérifiés purs ; règle
   `rt`.
3. **Codegen `rustc`** : la fonction devient une enveloppe qui vérifie
   `requires`, copie les paramètres `Copy` lus par `ensures`, appelle le
   corps (fonction interne), vérifie `ensures`, rend `result`.
4. **IR/JIT** : mêmes vérifications, `requires` en tête de fonction,
   `ensures` avant chaque `Return`.

## 5. Preuves attendues avant `ACCEPTED`

Tests positifs et **négatifs** par règle ; même message de violation sur la
voie `rustc` et sur le JIT (sortie comparée octet par octet) ; cas de
conformance ; programmes existants inchangés (`requires`/`ensures` comme
identifiants).

## 6. Alternatives écartées

- **Preuve statique (SMT)** : écartée pour la V1 par la décision V1-C1
  (coût, statuts `UNKNOWN`/`TIMEOUT`) ; compatible plus tard avec cette
  syntaxe.
- **Contrats désactivables en release** : écarté : un contrat qui disparaît
  change le comportement entre deux builds (déterminisme, T17).
- **Invariants de type** : reportés (RFC séparée) ; ils demandent de définir
  les points d'observation d'un objet.
