# RFC OWN-ARRAY-ELEMENTS — moves d'éléments de tableau : un abaissement par accès

> **Statut : ACCEPTED sous délégation — IMPLÉMENTÉE (2026-11-15).** Le
> responsable a demandé en session, le 2026-11-15, de « trancher les décisions »
> du chantier production ; cette RFC est prise sous cette délégation
> ([`PRODUCTION-PLAN-2026-11-15.md`](../roadmap/PRODUCTION-PLAN-2026-11-15.md)
> §2, P-1 et P-7). Elle ajoute deux règles normatives (`OWN-5.11`, `OWN-5.12`) :
> **le responsable peut la révoquer** ; le §5 dit ce que cela coûterait.
>
> Aucun `PROVED`. Les chiffres sont des mesures locales liées à la branche
> `goal/production-plan-2026-11-15`.

## 1. Le défaut mesuré

Le backend abaissait un move d'élément (`let s = a[0];`, interdit tel quel par
Rust : `E0508`) en **hissant** un déstructurage de tout le tableau
(`let [mut e0, mut e1] = a;`) avant l'instruction du bloc de la liaison, puis en
réécrivant chaque accès vers ces copies. Ces copies vivent à côté du tableau :
dès que le tableau est ré-assigné dans un bloc, une branche ou une boucle, elles
sont périmées. Le planificateur devait donc *prouver* localement, sur la
syntaxe, où le hissage restait valide (régions d'écriture, ancres, portée des
liaisons). Onze formes avaient été fermées une par une depuis la passe 6.

Sondage du 2026-11-15, 32 programmes dont 30 acceptés par `check`
(`T2-FINDINGS.md` §29) :

| Résultat avant correctif | Programmes |
|---|---|
| **sortie fausse, aucun diagnostic** (les copies hissées avant l'écriture sont lues après) | 6 |
| `check` accepte, `rustc` refuse (`E0382` / `E0508`) | 11 |
| correct | 13 |

Le cas enregistré par la vague 3 (`run/power-array-move-write-region-nested`)
n'était que la forme bruyante d'un défaut dont six variantes étaient
silencieuses. Le `check` avait aussi un défaut propre : `a[i] = e` (index non
littéral) « restaurait » un élément déplacé qu'il ne désigne pas forcément.

## 2. Décision

**Le tableau reste un tableau Rust, et chaque accès est abaissé à l'endroit de
l'accès.** Rust suit les moves par élément quand ils passent par un motif :

```rust
let s = { let [e, ..] = a; e };        // déplace a[0] seulement
let t = { let [_, e] = a; e };         // a[1] est encore utilisable
a = fresh();                           // restaure tout le tableau
```

Plus rien n'est hissé et aucun élément n'est copié dans une locale séparée : il
n'existe plus de valeur qui puisse devenir périmée. `rustc` re-vérifie chaque
usage, branche par branche ; un désaccord entre le vérificateur Ravum et le
backend devient une **erreur de compilation**, jamais un programme différent.

| Accès | Rust généré |
|---|---|
| move, chemin constant (`a[1]`, `b[0][1]`, `h.items[0]`, `a[1].0`) | `({ let [_, __rv_el, ..] = a; __rv_el })` — un seul motif imbriqué, les autres positions `_`/`..` |
| move après un index non littéral (`b[0][i]`) | préfixe constant par motif, puis `into_iter().nth(i)` (texte de panique de bornes de `rustc` conservé) |
| lecture/emprunt d'un tableau déplacé quelque part dans la fonction | `(*{ let [_, ref __rv_el, ..] = a; __rv_el })` (`ref mut` pour une écriture ou un `&mut`) |
| tableau jamais déplacé | `a[1]`, inchangé |
| ré-initialisation d'un élément déplacé (`a[0] = e`) | `{ let new = e; a = [new, <a[1] déplacé et remis>]; }` |

## 3. Règles ajoutées

Rust n'a pas d'équivalent à « ré-initialiser un seul élément déplacé » : seule
l'affectation du tableau entier le fait. Deux précisions rendent le modèle du
vérificateur exactement exprimable :

- **OWN-5.11** — un élément de tableau `MOVED` n'est ré-initialisé
  individuellement que si tous les index de la cible sont littéraux et
  qu'aucune autre partie du même tableau n'est `MOVED` (le tableau est alors
  reconstruit autour de la nouvelle valeur) ; sinon `E-OWN-PARTIAL-MOVE`.
- **OWN-5.12** — un index non littéral utilise le tableau entier à ce niveau :
  deux chemins qui partagent un tel index ne sont jamais prouvés disjoints.

Elles ne retirent **aucun cas de conformance existant** :
`ownership/array-element-reinit-after-move-pass` (un seul élément déplacé puis
ré-assigné) reste accepté et s'exécute. Les programmes nouvellement refusés au
`check` échouaient déjà au `build` pour ceux qui ont été mesurés (formes `l03`,
`l08` du sondage) ; une forme sans usage ultérieur du tableau entier
(`let x = a[0]; let y = a[1]; a[0] = p; a[1] = q;`) compilait auparavant et est
désormais refusée avec un diagnostic qui indique la ré-assignation totale.

## 4. Implémentation

- `crates/ravum-lang-sema/src/own.rs` : `element_write` (OWN-5.11),
  `diverges_disjoint` (OWN-5.12), fait `OwnFacts::element_reinit`.
- `crates/ravum-lang-codegen/src/emit.rs` : `PathPlanner` (classification
  syntaxique des chemins), `element_move_expr`, `element_borrow_expr`,
  `element_reinit_expr`. `MovePlanner` (hissages, régions d'écriture, descentes,
  groupes de slots) est **supprimé** : `emit.rs` passe de 8 377 à 7 400 lignes.
- Le backend lit le vérificateur pour trois décisions (`use_kind` des arguments,
  des récepteurs et du scrutinee de `match` ; `element_reinit`). La
  classification « valeur consommée » des autres positions reste syntaxique :
  c'est le reste de l'étape C de `RFC-OWN-FACTS-1`.

## 5. Révocation

Revenir en arrière demande de restaurer `MovePlanner` (`git revert` du commit de
code) : les 14 cas `run-pass` ajoutés redeviennent rouges ou faux. Refuser
seulement `OWN-5.11` demanderait un stockage par élément capable de survivre à
un emprunt du tableau entier, ce que le Rust sûr n'offre pas sans reconstruire
le tableau à chaque emprunt.

## 6. Ce que cette RFC ne prouve pas

Les formes mesurées sont celles du sondage et du corpus ; « aucune forme ouverte
connue » reste une observation. La garantie nouvelle est structurelle et plus
étroite : le backend ne produit plus de copie d'élément, donc une erreur de
classification ne peut plus donner une valeur périmée — elle donne une erreur
`rustc`.
