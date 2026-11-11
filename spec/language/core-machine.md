# Ravum Core 1.0 — Machine abstraite du noyau pur (oracle indépendant)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : noyau pur de Ravum Core 1.0 (profil `pure`)** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.3, §3.4, §3.6 ; `docs/architecture/PROJECT_INVARIANTS.md` INV-12.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. **L'interpréteur de référence n'existe pas** : son état est `NOT_IMPLEMENTED` (§9). Ce document spécifie ce qu'il devra faire, pas ce qui a été fait. Aucune preuve, aucun résultat de test n'est revendiqué.

## 1. Objet et statut

La machine abstraite fournit une **sémantique opérationnelle** indépendante du backend `rustc` pour le **noyau pur** : le sous-ensemble des programmes qui n'ont **aucun effet** (`effects()`), donc sans E/S, sans allocation, sans panic explicite. Elle sert d'**oracle** (INV-12 : « Rust ne suffit pas pour démontrer une sémantique Ravum »).

| ID | Règle | Niveau |
|---|---|---|
| MACH-1.1 | Le domaine de la machine est exactement le profil `pure` ([`effects.md`](effects.md) EFF-4.2) : valeurs `Copy` uniquement, aucune allocation, aucune E/S, aucun appel à `std` hors `math::*` et méthodes pures. | MUST |
| MACH-1.2 | La machine est **déterministe** : aucune entrée externe, aucun temps, aucune adresse, aucune allocation ⇒ une configuration donnée a au plus une transition. | MUST |
| MACH-1.3 | La machine est **indépendante de l'implémentation** : elle ne réutilise ni le parser, ni le checker, ni le codegen. Une divergence entre les deux est un défaut à instruire, jamais à contourner. | MUST |
| MACH-1.4 | Son état est `NOT_IMPLEMENTED`. Aucun résultat de la machine ne peut être cité comme preuve. | MUST |

## 2. Valeurs

```text
v ::= n                    n ∈ ℤ, type entier t (i8..u128, isize, usize)
    | f                    f ∈ flottants IEEE-754 (f32, f64)
    | b                    b ∈ {true, false}
    | c                    c ∈ scalaires Unicode
    | ()                   unité
    | (v1, ..., vn)        tuple (n ≥ 1)
    | [v1, ..., vn]        tableau (n ≥ 1)
    | S { f1: v1, ... }    struct (valeurs Copy uniquement)
    | E::V(...) | E::V{...} | E::V   variante d'enum (valeurs Copy)
    | Some(v) | None | Ok(v) | Err(v)
```

| ID | Règle | Niveau |
|---|---|---|
| MACH-2.1 | Aucune valeur ne contient de `String`, `Vec`, de référence non-`&str`, ni d'adresse : ces valeurs sont exclues du noyau pur (elles exigent `alloc` ou une région). | MUST |
| MACH-2.2 | `&str` est admis **uniquement** pour les littéraux (`&'static str`) et les comparaisons de texte ; aucune autre référence n'entre dans le domaine. | MUST |
| MACH-2.3 | Les valeurs sont **immuables** ; une « mutation » est un remplacement de liaison dans l'environnement. | MUST |

## 3. Configurations et environnements

```text
Configuration  C ::= ⟨e, ρ⟩          évaluation d'une expression
                   | ⟨s ; κ, ρ⟩      exécution d'une instruction, continuation κ
                   | ⟨v⟩            valeur finale
                   | ⟨panic: msg⟩   état terminal (arrêt contrôlé)
                   | ⟨diverge: n⟩   budget d'étapes épuisé (n = budget)
ρ : Ident ⇀ v                        environnement (liaisons immuables)
```

| ID | Règle | Niveau |
|---|---|---|
| MACH-3.1 | Un identifiant non lié est une **erreur de la machine** (ne devrait pas arriver : le programme est supposé accepté). Elle produit `⟨fault: unbound x⟩`, jamais une valeur par défaut. | MUST |
| MACH-3.2 | Le shadowing crée une nouvelle liaison ; l'ancienne reste dans l'environnement jusqu'à la fin du bloc (elle est simplement masquée) — sans effet observable en pur. | MUST |
| MACH-3.3 | Le budget d'étapes est un entier **fini** fourni par l'oracle ; il ne fait pas partie de la sémantique du langage. | MUST |

## 4. Règles d'évaluation (extrait normatif)

L'évaluation est **de gauche à droite** ; les règles ci-dessous sont des schémas.

```text
⟨a, ρ⟩ →* ⟨va⟩     ⟨b, ρ⟩ →* ⟨vb⟩
──────────────────────────────────  (op binaire, op ∈ {+,-,*,/,%,<<,>>,&,|,^})
⟨a op b, ρ⟩ → ⟨apply(op, va, vb)⟩

⟨a, ρ⟩ →* ⟨false⟩                        ⟨a, ρ⟩ →* ⟨true⟩
──────────────────  (court-circuit)      ──────────────────
⟨a && b, ρ⟩ → ⟨false⟩                    ⟨a || b, ρ⟩ → ⟨true⟩

⟨e, ρ⟩ →* ⟨v⟩        v = apply(f, v⃗)     (arguments évalués de gauche à droite)
─────────────────────────────────────────
⟨f(e1, ..., en), ρ⟩ → ⟨v⟩

⟨c, ρ⟩ →* ⟨true⟩    ⟨then, ρ⟩ →* ⟨v⟩
──────────────────────────────────────
⟨if c { then } else { other }, ρ⟩ → ⟨v⟩

⟨c, ρ⟩ →* ⟨false⟩   ⟨other, ρ⟩ →* ⟨v⟩
──────────────────────────────────────
⟨if c { then } else { other }, ρ⟩ → ⟨v⟩

⟨if c { then }, ρ⟩ → ⟨()⟩     (branche else absente ; then doit être de type ())
```

| ID | Règle | Niveau |
|---|---|---|
| MACH-4.1 | Chaque opérande est réduit **entièrement** avant l'opération ; l'ordre gauche→droite est observable via `panic` (un panic dans `a` empêche l'évaluation de `b`). | MUST |
| MACH-4.2 | `&&`/`||` court-circuitent : l'opérande droit n'est **pas** réduit si le résultat est déterminé. | MUST |
| MACH-4.3 | Les arguments d'appel sont réduits de gauche à droite, **avant** l'appel. | MUST |
| MACH-4.4 | Un `if`/`match` ne réduit **qu'une** branche. | MUST |
| MACH-4.5 | Un `match` évalue les bras dans l'ordre et sélectionne le premier motif correspondant ; l'échec de tous les motifs est impossible sur un programme accepté (exhaustivité statique). S'il survient, `⟨fault: no-match⟩`. | MUST |
| MACH-4.6 | Les boucles `while`/`loop`/`for` sont dépliées une itération à la fois ; `break`/`continue` agissent sur la boucle la plus interne. | MUST |
| MACH-4.7 | Un appel de fonction est une **substitution** de paramètres (monomorphe) et l'exécution de son corps ; la récursion est autorisée et peut diverger. | MUST |

```rv
// POSITIF — ordre observable
fn f() -> i32 { g(1) + g(2) }                  // g(1) puis g(2)
fn g(n: i32) -> i32 { if n == 2 { panic("x"); } n }

// NÉGATIF (runtime) — ⟨panic: x⟩, g(1) évalué mais pas de résultat final
// NÉGATIF (runtime) — ⟨panic: division by zero⟩ pour `1 / 0`
```

## 5. Arithmétique et traps

| Opération | Règle | Résultat en cas d'invalidité |
|---|---|---|
| `+ - *` entiers | arithmétique mathématique du type ; résultat hors bornes ⇒ trap | `⟨panic: attempt to ... with overflow⟩` |
| `/` entiers | troncature vers zéro ; `MIN / -1` ⇒ trap ; `x / 0` ⇒ trap | `⟨panic: division by zero⟩` |
| `%` entiers | reste de la division tronquée, signe du dividende ; `x % 0` ⇒ trap ; `MIN % -1` = `0` | `⟨panic: remainder by zero⟩` |
| `<< >>` | `0 ≤ n < largeur` ; sinon trap | `⟨panic: shift out of range⟩` |
| `& \| ^` entiers | bit à bit, complément à deux | — |
| `!` entier / `bool` | complément / négation | — |
| unaire `-` | entiers signés, flottants | — |
| `+ - * / %` flottants | IEEE-754, aucun trap (`±inf`, `NaN`) | — |
| comparaisons flottantes | IEEE ; `NaN` compare faux sauf `!=` | — |
| `as` | sémantique de [`types.md`](types.md) TYP-13.2 (saturant, troncature, arrondi) | — |

| ID | Règle | Niveau |
|---|---|---|
| MACH-5.1 | Toute opération invalide produit un **état terminal** `⟨panic: msg⟩` ; il n'y a **pas** de valeur de repli, pas d'enroulement, pas de saturation silencieuse (sauf `as` flottant→entier, explicitement saturant). | MUST |
| MACH-5.2 | `panic` est **terminal** : aucune continuation n'est exécutée après. | MUST |
| MACH-5.3 | Les traps sont identiques dans tous les profils et toutes les optimisations (pas de mode release différent). | MUST |
| MACH-5.4 | `math::sqrt` est IEEE correctement arrondi ; `floor`/`ceil`/`round` sont exacts (`round` : égalité loin de zéro) ; `min`/`max` suivent [`stdlib.md`](stdlib.md) STD-6.5. `sin`, `cos`, `tan`, `exp`, `ln`, `log10`, `pow`/`powf` sont **hors** du noyau vérifiable (libm, non correctement arrondis). | MUST |
| MACH-5.5 | La machine **MUST** refuser (`⟨fault: unsupported-libm⟩`) plutôt que produire un résultat non reproductible pour `sin`, `cos`, `tan`, `exp`, `ln`, `log10`, `pow`, `powf`. | MUST |

```rv
// POSITIF
fn f(a: u8) -> u8 { a + 1 }        // ⟨27⟩ si a = 26 ; ⟨panic: overflow⟩ si a = 255
fn g(a: i32) -> i32 { a / 2 }      // -7 / 2 = -3 ; -7 % 2 = -1
fn h(x: f64) -> bool { x != x }    // vrai ssi x est NaN (0.0/0.0)
```

## 6. Instructions et contrôle

```text
⟨let x = e; κ, ρ⟩     →* ⟨κ, ρ[x ↦ v]⟩           si ⟨e,ρ⟩ →* ⟨v⟩
⟨x = e; κ, ρ⟩          →* ⟨κ, ρ[x ↦ v]⟩
⟨return e; κ, ρ⟩       →  Retour(v)              (κ abandonnée)
⟨break; κ, ρ⟩          →  Break                  (boucle la plus interne)
⟨continue; κ, ρ⟩       →  Continue
⟨{ s1; ...; sn }, ρ⟩   →  exécution séquentielle, valeur = dernière expression
```

| ID | Règle | Niveau |
|---|---|---|
| MACH-6.1 | `let x: T;` sans initialiseur n'entre pas dans le domaine de la machine (programme non accepté) : la machine suppose l'initialisation définie vérifiée (OWN-4). | MUST |
| MACH-6.2 | `return` abandonne la continuation courante et rend la valeur à l'appelant. | MUST |
| MACH-6.3 | La valeur d'un bloc est celle de sa *tail_expr* ; un bloc sans *tail_expr* vaut `()`. | MUST |
| MACH-6.4 | Une instruction-expression doit valoir `()` ; la machine ne rencontre donc jamais de valeur non-`()` ignorée. | MUST |
| MACH-6.5 | `for i in a..b` évalue `a` et `b` **une fois**, puis itère `i = a, a+1, ..., b-1` ; si `a >= b`, zéro itération. | MUST |
| MACH-6.6 | La machine ne modélise **pas** `break`/`continue` avec valeur (inexistants) ni les étiquettes (inexistantes). | MUST |

## 7. Divergence et budget

| ID | Règle | Niveau |
|---|---|---|
| MACH-7.1 | L'oracle exécute au plus `N` étapes (paramètre, ex. 1 000 000). Épuisement ⇒ `⟨diverge: N⟩`. | MUST |
| MACH-7.2 | `⟨diverge: N⟩` signifie « non concluant », **jamais** « ne termine pas ». Aucun résultat ne doit être publié comme preuve de terminaison ou de non-terminaison. | MUST |
| MACH-7.3 | La récursion profonde peut dépasser une limite de pile de l'oracle ; l'oracle doit échouer explicitement (`⟨fault: stack-limit⟩`) et non produire un résultat partiel. | MUST |
| MACH-7.4 | Aucune garantie de terminaison n'est offerte par le langage (pas de profil `total`). | MUST |

## 8. Ce que l'oracle doit vérifier (obligations)

| # | Obligation | Méthode |
|---|---|---|
| O1 | Ordre d'évaluation gauche→droite (observable par panic) | corpus de programmes à panic placé |
| O2 | Court-circuit `&&`/`||` | opérande droit paniquant |
| O3 | Débordement, division/reste par zéro, décalage, index | valeurs limites par type |
| O4 | Signes de division/reste entiers | valeurs négatives |
| O5 | `NaN`, `±inf`, `-0.0`, comparaisons | prédicats IEEE, sans bits |
| O6 | `as` : troncature, saturation, `NaN → 0`, `±inf → MIN/MAX` | table de cas |
| O7 | `if`/`match` : une seule branche, exhaustivité statique | programmes valides |
| O8 | `for` range/collection : bornes évaluées une fois, zéro itération | compteurs via panic |
| O9 | `return`/`break`/`continue` : continuation correcte | traces |
| O10 | Récursion bornée : résultat identique au backend | comparaison |

| ID | Règle | Niveau |
|---|---|---|
| MACH-8.1 | L'oracle **MUST** être écrit indépendamment (pas de réutilisation du checker ni du codegen) ; un désaccord est un **défaut** à instruire, jamais une tolérance. | MUST |
| MACH-8.2 | L'oracle **MUST** produire un résultat **comparable** : `valeur` (avec type), `panic: msg`, `diverge: N`, ou `fault`. | MUST |
| MACH-8.3 | La comparaison des valeurs est **structurelle** ; les flottants sont comparés par prédicats IEEE, jamais par bits (pas de `to_bits`, EXPR-4.4). | MUST |
| MACH-8.4 | L'oracle **MUST NOT** être utilisé pour prouver la sûreté mémoire ou l'ownership : c'est un oracle **de résultats**, pas de typage. | MUST |
| MACH-8.5 | Aucun résultat d'oracle n'est `PROVED` : c'est `TESTED` sur un corpus fini, avec modèle et limites déclarés. | MUST |

## 9. État d'implémentation

| Élément | État |
|---|---|
| Spécification de la machine (§2–§6) | `SPECIFIED` (ce document, candidate) |
| Interpréteur de référence | **`NOT_IMPLEMENTED`** |
| Corpus de conformance pur | **`NOT_COVERED`** (`conformance/` absent) |
| Comparaison différentielle backend ↔ oracle | `NOT_RUN` |
| Résultats / chiffres | **aucun** |

| ID | Règle | Niveau |
|---|---|---|
| MACH-9.1 | Tant que l'interpréteur n'existe pas, aucun document ne doit affirmer qu'une sémantique Ravum est « vérifiée » au-delà de la relecture. | MUST |
| MACH-9.2 | Le futur interpréteur **MUST** être versionné avec la spécification (`core-1.0-draft.N`) et refuser une version inconnue. | MUST |
| MACH-9.3 | Le futur interpréteur **MUST** être déterministe et sans dépendance réseau. | MUST |

## 10. Exemple de réduction complète

```rv
fn somme(n: i32) -> i32 effects() {
    let mut s = 0;
    for i in 0..n { s += i; }
    return s;
}
```

```text
⟨somme(4)⟩
→ ρ = {n ↦ 4}
→ s = 0 ; i = 0 ; s = 0+0 = 0
→ i = 1 ; s = 0+1 = 1
→ i = 2 ; s = 1+2 = 3
→ i = 3 ; s = 3+3 = 6
→ i = 4 ⇒ 4 < 4 faux ⇒ sortie
→ return 6 ⇒ ⟨6⟩
```

```rv
// NÉGATIF (runtime) — budget épuisé
fn boucle() -> i32 { let mut x = 0; loop { x += 1; } }
// ⟨diverge: N⟩, jamais ⟨valeur⟩
```

## 11. Questions ouvertes

- **Q-MACH-1** — Le noyau pur exclut `String`/`Vec` (`alloc` interdit par `pure`). Faut-il un **second** noyau (avec allocation, mais sans E/S) pour vérifier `String`/`Vec` ? Recommandation : oui, plus tard, avec un modèle de tas simplifié ; hors périmètre actuel.
- **Q-MACH-2 (résolue par le Lead)** — Les fonctions `libm` (`sin/cos/tan/exp/ln/log10/pow/powf`) sont exclues de l'oracle (MACH-5.5) : elles restent supportées par la toolchain, mais non vérifiables bit-à-bit.
- **Q-MACH-3** — Le budget `N` n'est pas normatif (MACH-3.3) ; deux oracles avec des budgets différents peuvent différer sur un programme divergent. La conformance doit donc **exclure** les programmes non terminants plutôt que comparer `diverge`.
- **Q-MACH-4** — Les littéraux `&str` entrent dans le domaine (MACH-2.2) mais leur comparaison exige une règle de texte ; faut-il les exclure pour un noyau strictement numérique ? Recommandation : les garder pour tester l'égalité texte (TYP-7.3).
- **Q-MACH-5** — Aucun interpréteur n'existe : toute la section §8 est une **obligation future**. Elle ne doit pas être citée comme réalisée. Point de blocage pour T4 (« aucun programme safe du corpus de réfutation accepté ») tant que l'oracle n'existe pas.
