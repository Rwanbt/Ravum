# RFC-UI-LANG-2 — Handles typés générationnels (`Arena<T>`, `Handle<T>`)

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-UI-LANG-2` |
| Statut | **ACCEPTED** par le responsable le 2026-11-26 (UI-D20, [`DECISIONS-2026-11-24-UI.md`](../decision/DECISIONS-2026-11-24-UI.md)) ; implémentation en cours |
| Date | 2026-11-25 |
| Auteur | agent de développement |
| Cadre | étape U1 de [`docs/ui/PLAN.md`](../ui/PLAN.md) ; jalon fixé par [`RFC-UI-LANG-1`](RFC-UI-LANG-1.md) §4 (« avant U5 ») |
| Portée | `spec/language/stdlib.md` (nouveau §21), `ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-ir`/`ravum-lang-jit`, conformance |

## 1. Problème

Un arbre d'UI retained, un graphe de nœuds ou une liste de composants ont
besoin qu'un élément en **désigne un autre** de façon durable : le parent
d'un nœud, la cible du focus, l'élément sélectionné, l'abonné d'un état
(BENCH-U1B-ROUND2 §1, modèle hybride). Core 1.0 n'a pas de référence
stockable (OWN-6.1, OWN-6.2) et n'en aura pas : les emprunts bornés
(OWN-6.8) ne vivent qu'une instruction ou un bras.

Aujourd'hui, un programme le contourne avec un indice `usize` dans un `Vec`.
C'est fragile :
- un indice **périmé** (élément retiré, place réutilisée) désigne en
  silence un autre élément ;
- un indice d'un `Vec` utilisé sur un autre `Vec` du même type passe le
  typage ;
- rien ne distingue un indice d'une quantité.

## 2. Proposition

Deux types de bibliothèque du prélude, sans `use` :

```rv
impl Arena<T> {
    pub fn new() -> Arena<T> effects(alloc)
    pub fn len(&self) -> usize effects()
    pub fn is_empty(&self) -> bool effects()
    pub fn insert(&mut self, x: T) -> Handle<T> effects(alloc)
    pub fn contains(&self, h: Handle<T>) -> bool effects()
    pub fn get(&self, h: Handle<T>) -> Option<&T> effects()          // vue d'instruction (OWN-6.8)
    pub fn get_mut(&mut self, h: Handle<T>) -> Option<&mut T> effects()  // vue d'instruction
    pub fn remove(&mut self, h: Handle<T>) -> Option<T> effects()
}
// Handle<T> : Copy, ==, aucune autre opération (ni arithmétique, ni accès aux champs).
```

Parcours : `for (h, x) in &a` et `for (h, x) in &mut a` (`h: Handle<T>`
copié, `x` emprunt borné, comme OWN-6.13 et STD-20.6).

### 2.1 Règles normatives (ratifiées, UI-D20)

| ID | Règle |
|---|---|
| HDL-1 | `Handle<T>` est un type **valeur** `Copy`, opaque : un identifiant d'arène, un indice de place et une génération. Il n'est **pas** une référence : il est stockable partout (champ, `Vec`, `Map` comme valeur, message), retournable et transmissible entre fonctions. |
| HDL-2 | `get`, `get_mut`, `contains` et `remove` rendent `None` / `false` pour un handle **périmé** (sa place a été libérée ou réutilisée) ou **étranger** (créé par une autre arène). Jamais un autre élément, jamais de panic. |
| HDL-3 | `remove` libère la place et incrémente sa génération. Une place dont la génération atteindrait sa valeur maximale est **retirée** définitivement (jamais réutilisée) : aucun handle ancien ne peut redevenir valide (pas d'ABA). |
| HDL-4 | Réutilisation **déterministe** : `insert` prend la place libre de **plus petit indice**, sinon en ajoute une à la fin. Les identifiants d'arène sont attribués par un compteur du run, dans l'ordre de création. Un programme donne donc les mêmes handles à chaque exécution, dans les deux voies. |
| HDL-5 | Le parcours suit l'ordre **croissant des indices de place** (pas l'ordre d'insertion). Pendant un parcours ou une vue `get`/`get_mut`, l'arène est empruntée : `insert`/`remove` est `E-OWN-BORROW-CONFLICT` (OWN-6.10). |
| HDL-6 | `get`/`get_mut` sont des vues d'instruction (OWN-6.8) : stocker ou retourner `Option<&T>` est `E-OWN-REF-ESCAPE`, comme `Map::get` (STD-20.3). |
| HDL-7 | Classification de clone (OWN-6.14) : `Handle<T>` est *valeur* (copie) ; `Arena<T>` est *valeur* si `T` l'est, et son clone garde les **mêmes** indices et générations mais reçoit un **nouvel** identifiant d'arène : un handle de l'original est étranger pour la copie (HDL-2). |
| HDL-8 | Effets : `new` et `insert` ont `alloc` ; les autres n'ont aucun effet, mais touchent le tas : refusés dans un module temps réel (INV-05, `Inst::touches_heap_or_io`), comme `Vec` et `Map`. |
| HDL-9 | `==` sur `Handle<T>` compare les trois composants. `Handle<T>` est une **clé de `Map`** (UI-D20, question 3) : l'ordre total de la clé est l'ordre lexicographique sur (identifiant d'arène, indice de place, génération). Il n'y a pas d'opérateur `<` sur `Handle<T>` dans le langage ; seul l'ordre interne de `Map` l'utilise. |

### 2.2 Hot reload

L'état migré (RFC-E11-HOT-RELOAD) contient des handles : ils sont
sérialisés avec l'arène (identifiant, indice, génération, places libres,
places retirées). Une migration conserve donc la validité de chaque handle.
Une arène dont le type `T` change de forme suit les règles de migration de
`T` ; un élément qui ne migre pas rend le patch **refusé** (fail-closed),
jamais une place vidée en silence.

## 3. Implémentation envisagée (non engagée)

- **Voie B** : `Arena<T>` du prélude Rust, un `Vec<Slot<T>>` (génération,
  `Option<T>`), un ensemble ordonné de places libres (`BTreeSet<u32>`, pour
  HDL-4), un compteur d'arène par run. Aucune nouvelle dépendance.
- **Voie C** : objet `Arena` du tas IR (IR 1.8) ; opérations à deux
  résultats servies par `rv_heap`, comme `MapGet` ; écriture des vues
  `get_mut` sur toute sortie, par le mécanisme `LoopWriteBack` déjà utilisé
  par `Map::get_mut` et par L-OWN-020-1.
- **Vérificateur** : `Arena::get`/`get_mut` rejoignent `is_access_view`
  (OWN-6.8) ; parcours comme STD-20.6.

## 4. Tests exigés

- Positifs : insertion, lecture, écriture par `get_mut` dans un `match`,
  retrait, réutilisation de la plus petite place, parcours ordonné, clone,
  handles dans un message et dans un `Vec`.
- Négatifs : vue stockée ou retournée (`E-OWN-REF-ESCAPE`), `insert`
  pendant une vue ou un parcours (`E-OWN-BORROW-CONFLICT`), arithmétique ou
  ordre sur un handle (`E-TYP-*`), `Handle<A>` sur `Arena<B>`
  (`E-TYP-MISMATCH`).
- Adversariaux : handle périmé après `remove` puis `insert` dans la même
  place → `None` ; handle d'une autre arène du même type → `None` ; handle
  d'un clone → `None` ; saturation de génération (place retirée) par un
  test de la bibliothèque à génération réduite ; mêmes handles sur N
  exécutions et dans les deux voies ; migration à chaud qui conserve les
  handles, et migration impossible refusée.

## 5. Alternatives écartées

| Alternative | Pourquoi |
|---|---|
| Références stockables, durées de vie | Change tout OWN-6 (RFC-UI-LANG-1 §5). |
| `usize` dans un `Vec` | Indices périmés ou étrangers silencieux (§1). |
| Compteur de références (`Rc`) | Cycles, destruction non déterministe, coût sur le thread audio. |
| Handles sans identifiant d'arène | Un handle étranger du même type désignerait un autre élément. |
| Réutilisation LIFO de la dernière place libérée | Déterministe aussi, mais dépend de l'histoire des retraits ; la plus petite place est plus simple à spécifier et à tester. |

## 6. Coût

Un handle tient en 16 octets (identifiant 32 bits, indice 32 bits,
génération 32 bits, remplissage) ; `get` est O(1) plus la comparaison de
génération ; `insert` est O(log f), `f` le nombre de places libres. À
mesurer sur le banc U1-B avant toute conclusion ; aucun chiffre ici.

## 7. Ce que cette RFC ne fait pas

- Pas de handle vers un élément d'un `Vec` ou d'une `Map` existants.
- Pas de destruction automatique : un élément vit jusqu'à `remove` ou la
  fin de l'arène.
- Pas de partage entre threads (CONC inchangé) : une arène n'est pas
  `Send` dans cette RFC.

## 8. Décisions du responsable (2026-11-26, UI-D20)

Réponses : 1 **oui** ; 2 **oui** (HDL-2 à HDL-4 ratifiés) ; 3 **maintenant**
(HDL-9 amendé ci-dessus) ; 4 **nouvel identifiant** (HDL-7 tel qu'écrit).

Questions telles que posées :

1. Accepter le **principe** : `Arena<T>` et `Handle<T>` comme seuls moyens
   de désignation durable en Core (pas de références stockables).
2. Ratifier HDL-2 à HDL-4 : `None` pour un handle périmé ou étranger,
   retrait des places à génération saturée, réutilisation de la plus petite
   place.
3. `Handle<T>` comme **clé de `Map`** : non dans cette RFC (HDL-9). À
   ouvrir maintenant (ordre total sur `(arène, indice, génération)`), ou
   plus tard sur besoin démontré ?
4. Clone d'une arène (HDL-7) : nouvel identifiant (les handles de
   l'original y sont étrangers), ou même identifiant (les handles restent
   valides dans les deux copies, au prix d'une ambiguïté) ?
