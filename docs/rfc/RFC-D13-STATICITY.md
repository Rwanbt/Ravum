# RFC D13 — Staticité de `&str` : une preuve déterministe, pas une hypothèse

> **Statut : PROPOSED** (décision du responsable, 2026-11-11 : « il n'y a pas une
> solution pour permettre au vérificateur de prouver tout ça de manière
> déterministe ? »). Ce document répond oui, et décrit la preuve.
>
> **Le problème, en une phrase** : le type `&str` mélange aujourd'hui deux
> choses très différentes — une chaîne **statique** (littéral, stockable et
> retournable) et une **vue empruntée** sur un `String` local (non stockable hors
> de la portée). Le vérificateur traite tout `&str` comme potentiellement
> emprunté, donc il refuse `a[1]` extrait d'un agrégat, alors que la valeur y est
> forcément statique.

## 1. La preuve en deux règles

**Invariant I-STR (la propriété à prouver).** Toute valeur de type `&str` qui
*quitte une instruction* (stockage dans un agrégat, champ, `Vec`, payload,
retour de fonction) est **statique**.

**Règle 1 — origine (analyse ascendante, déterministe).** La staticité d'une
expression est calculée par une fonction totale sur l'AST :

| Expression | Staticité |
|---|---|
| littéral de chaîne `"…"` | **statique** |
| copie d'une valeur statique (`let t = "x"`, argument, `match` sur statique) | **statique** (induction) |
| `a[i]`, `s.champ`, `t.0`, payload de pattern, `Some(x).unwrap()` sur une valeur statique | **statique** (projection conserve la provenance) |
| agrégat construit de valeurs statiques (`["a", "b"]`, `S { text: "x" }`, `Label::Text("x")`) | **statique** (chaque élément l'est) |
| `&s`, `&*s`, `s.as_str()` sur un `String` local | **emprunté** (non statique) |
| appel de fonction `fn f(…) -> &str` | **statique** — parce que le corps de `f` est vérifié par la règle 2 ; c'est un contrat vérifié, pas une supposition |
| `trim()`, `to_uppercase()`, … | **possédé** (`String`), donc jamais concerné |

**Règle 2 — stockage (vérification descendante).** Un `&str` **emprunté** ne peut
pas : être stocké dans un agrégat, un champ, un `Vec`, un payload d'enum ; être
retourné par une fonction ; être envoyé à une fonction qui le stocke. Le
vérificateur émet `E-OWN-REF-ESCAPE` à ces sites — c'est déjà ce qu'il fait
aujourd'hui, et c'est ce qui **rend la règle 1 vraie** : si aucun `&str`
emprunté ne peut entrer dans un agrégat, alors tout `&str` lu dans un agrégat est
statique, par construction.

**Conséquence directe** : `fn f(a: [&str; 2]) -> &str { return a[1]; }` et
`let s = a[1];` deviennent **acceptés**, avec une preuve locale : les éléments du
tableau sont statiques (règle 2), la projection préserve la provenance (règle 1).
Aucune hypothèse, aucune durée de vie, aucun solveur.

## 2. Pourquoi ce n'est ni A ni B

- **Option A (« accepter, en supposant que tout `&str` stocké est statique »)**
  était une hypothèse non vérifiée. La règle 2 la transforme en **invariant
  outillé** : elle est vérifiée à chaque site de stockage.
- **Option B (« garder le refus »)** était conservateur mais laissait une
  limitation non nécessaire. Elle devient inutile : le refus n'est plus le seul
  moyen de rester sûr.

## 3. Ce qu'il faut implémenter (et ce que ça coûte)

1. **Un fait de staticité** sur les expressions `&str` : `static_str: BTreeSet<NodeId>`
   (ou un booléen dans le type résolu). Calculé dans `own.rs` là où les faits
   d'ownership sont déjà produits, avec la table de la règle 1 — c'est une
   analyse locale, sans unification ni itération.
2. **Le site de refus** actuel (`E-OWN-REF-ESCAPE` sur les extractions
   d'agrégat) est remplacé par : refus si l'expression n'est pas statique,
   acceptation sinon. Le refus sur `&s` / `s.as_str()` **reste** (correct).
3. **Un contrat inter-fonctions** : `fn f(…) -> &str` doit prouver que tous ses
   `return` sont statiques (règle 2 appliquée au retour) ; le résultat d'un appel
   est alors statique par contrat. C'est vérifié dans le corps, donc déterministe.
4. **Tests** : la matrice ci-dessous, dont les cas aujourd'hui refusés.

Estimation : quelques centaines de lignes dans `sema` plus la matrice de tests.
Ce n'est pas un changement de modèle (pas de lifetimes, pas de région), c'est
l'exploitation d'un invariant déjà présent.

## 4. Matrice de tests (acceptation / refus)

| Programme | Aujourd'hui | Cible | Pourquoi |
|---|---|---|---|
| `return a[1]` sur `[&str; 2]` | refusé | **accepté** | élément statique par règle 2 |
| `let s = a[1];` puis `io::println(s)` | refusé | **accepté** | idem |
| `let s = a[1]; return s;` | refusé | **accepté** | copie d'une valeur statique |
| `S { text: "x" }` / `Label::Text("x")` | accepté | accepté | littéral |
| `let t = "x"; return t;` | accepté | accepté | copie de littéral |
| `let s = String::from("x"); return s.as_str();` | refusé | **refusé** | emprunt d'un local (règle 2) |
| `let s = String::from("x"); let a = [s.as_str()];` | refusé | **refusé** | stockage d'un emprunt |
| `fn f(a: [&str; 2]) -> &str { let s = a[1]; return s; }` | refusé | **accepté** | statique |
| `match a[1] { "x" => …, _ => … }` | accepté | accepté | comparaison, pas d'échappement |
| `a[1] == "x"` | accepté | accepté | inchangé |

## 5. Ce que cela ne résout pas

- **`trim() -> &str`** : un `&str` emprunté au `String` receveur devrait porter
  une durée de vie (« aussi longtemps que le receveur »), ce que le langage
  n'exprime pas. `trim()` reste donc `-> String` : c'est une **conséquence du
  sous-ensemble sans lifetimes**, pas un oubli. Le jour où les lifetimes
  arrivent, `trim -> &str` devient exprimable et la règle 1 gagne une colonne.
- **FFI** : une chaîne C n'est pas statique ; elle ne pourra jamais emprunter la
  règle 1. Le contrat FFI (D7) devra l'interdire ou la copier.
- **La preuve reste locale** : elle prouve I-STR, pas la sûreté générale du
  langage. C'est un théorème de plus, pas une certification.
