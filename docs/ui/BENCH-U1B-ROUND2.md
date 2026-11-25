# U1-B — Banc d'essai, tour 2, parties 1 et 2 (2026-11-24, 2026-11-25)

Statut : `TESTED`, local et headless. Ce rapport couvre la réactivité A/B
(UI-D13) et de vrais changements de layout Taffy (UI-D16). Aucune pile n'est
adoptée.

- Machine : conteneur Linux x86_64, 4 CPU, sans GPU, compilation `release`.
- 200 exécutions par mesure (30 pour le layout complet).
- Code : [`bench/ui-candidates/reactivity`](../../bench/ui-candidates/reactivity/src/main.rs), `taffy =0.16.0`.

```bash
cd bench/ui-candidates
cargo run --release -p ui-bench-reactivity
```

## 1. Réactivité : A (reconstruction puis diff) contre B (dépendances granulaires)

Scène : N lignes (un libellé formaté et un curseur). Une mise à jour change
**un** paramètre visible, comme pendant le glissement d'un curseur. A et B
formatent le même texte pour les lignes qu'ils produisent. Le test vérifie
à chaque mise à jour que A et B donnent le même nœud.

| Scène | A, toutes les lignes | A, 50 lignes visibles (virtualisé) | B, granulaire |
|---|---|---|---|
| 1 000 lignes : p50 | 200,7 µs | 29,0 µs | 0,55 µs |
| 1 000 lignes : p99 | 250,4 µs | 49,4 µs | 0,97 µs |
| 10 000 lignes : p50 | 1 842 µs | 15,0 µs | 0,51 µs |
| 10 000 lignes : p99 | 2 774 µs | 33,9 µs | 1,04 µs |

Mémoire de B : 28 octets par ligne pour la table de dépendances
(280 Ko pour 10 000 lignes).

Lecture :
- A sans virtualisation croît avec la taille de l'arbre : environ 1,8 ms
  pour 10 000 lignes, **à chaque** changement de paramètre. Pour un DAW où
  des dizaines de paramètres bougent par frame, c'est hors budget.
- A virtualisé reste dans le budget d'une frame (15 à 30 µs), mais coûte 30 à
  60 fois plus que B. Le coût se multiplie avec la fréquence de mise à jour.
- B est constant (0,5 µs) quelle que soit la taille, pour un coût mémoire faible.
- **Limites de ce modèle** :
  - B ne mesure pas encore l'**enregistrement dynamique** des dépendances
    (quand une vue lit un état différent selon une condition) ;
  - le diff de A est par position, et non par clé ;
  - les deux restent à mesurer avec une structure qui change (ajout ou
    retrait de lignes).

  C'est la partie 2 du tour 2.
- Orientation, non décisionnelle : un **modèle hybride**. L'API reste
  déclarative (`view`), mais le runtime suit les lectures d'état par nœud et
  ne réévalue que les nœuds dépendants (B). Cela suppose que l'accès à
  l'état passe par le runtime : ce sont les « handles » de RFC-UI-LANG-2.

## 2. Taffy : vrais changements de propriétés (10 001 nœuds)

| Changement | Lignes à hauteur auto : p50 | Lignes à hauteur fixe : p50 |
|---|---|---|
| Largeur d'une cellule (`set_style`) | 1,16 ms | 1,13 ms |
| Ajout d'une cellule (structure) | 1,16 ms | 1,12 ms |
| Layout complet | 5,94 ms | 4,40 ms |

Lecture :
- Le tour 1 l'avait suggéré, le tour 2 le confirme avec de vrais
  changements : une modification locale coûte environ **1,1 ms** sur
  10 000 nœuds, soit 20 à 25 % d'un layout complet.
- Une hauteur fixe de ligne, qui ferait une frontière naturelle, **ne réduit
  pas** ce coût : le conteneur racine repasse sur tous ses enfants.
- Conséquence pour U2 : le runtime Ravum UI doit gérer lui-même les
  **frontières de layout**. Un sous-arbre de taille fixée est calculé seul
  et son parent n'est pas recalculé. La virtualisation est l'autre levier.
  Taffy reste utilisable comme algorithme de layout **par sous-arbre**.

## 3. Partie 2 (2026-11-25) : dépendances dynamiques, structure, frontières

Même machine et même mode (`release`, 200 exécutions). Code :
[`reactivity/src/bin/part2.rs`](../../bench/ui-candidates/reactivity/src/bin/part2.rs).

```bash
cd bench/ui-candidates
cargo run --release -p ui-bench-reactivity --bin part2
```

### 3.1 Dépendances dynamiques

Une ligne lit `params[i]` ou `params[i + 1]` selon un drapeau ; un tiers des
mises à jour change ce drapeau, donc **ce que la ligne lit**. B enregistre
les lectures pendant l'évaluation d'une ligne et les remplace à chaque
réévaluation. Chaque mise à jour est vérifiée contre une reconstruction
complète : même texte, ligne par ligne.

| Lignes | A, reconstruction complète : p50 / p99 | B, lectures suivies : p50 / p99 |
|---|---|---|
| 1 000 | 112,9 µs / 706,9 µs | 0,53 µs / 2,9 µs |
| 10 000 | 1 090 µs / 3 980 µs | 1,33 µs / 2,7 µs |

B réévalue une ligne par mise à jour et garde une arête par ligne. Le suivi
dynamique ne change pas l'ordre de grandeur mesuré en partie 1 : le coût de
B reste indépendant de la taille de l'arbre.

### 3.2 Changements de structure

Insertion puis retrait d'une ligne **au milieu** de la liste.

| Lignes | Diff par position : p50 | Diff par clé : p50 | Nœuds réécrits par mise à jour (position / clé) |
|---|---|---|---|
| 1 000 | 8,7 µs | 113,9 µs | 501 / 1 |
| 10 000 | 90,6 µs | 1 567 µs | 5 001 / 1 |

Lecture :
- Le diff par position réécrit **toute la fin de la liste** (la moitié des
  lignes), alors qu'une seule ligne a changé. Chaque nœud réécrit invalide
  ensuite son layout, sa peinture et sa sémantique : c'est ce coût en aval
  qui compte, pas celui du diff seul.
- Le diff par clé ne réécrit qu'un nœud. **Son temps mesuré est celui d'une
  implémentation naïve** (un ensemble reconstruit à chaque mise à jour, en
  O(n log n)), pas une borne : ce chiffre ne doit pas servir à écarter les
  clés. Un diff par clé incrémental est à mesurer avant toute conclusion.
- Conséquence pour U2 : des identifiants **stables** (clés) pour les nœuds
  de liste. Ce sont aussi les handles de RFC-UI-LANG-2.

### 3.3 Frontières de layout au-dessus de Taffy

Chaque ligne de hauteur fixe est un arbre Taffy séparé ; la racine empile
les lignes. Le résultat est vérifié contre le layout global : même largeur
et même `x` pour chaque cellule de la ligne modifiée, et même `y` de ligne.

| 2 000 lignes (10 001 nœuds), une cellule redimensionnée | p50 | p99 |
|---|---|---|
| Arbre global | 1 370 µs | 1 556 µs |
| Un arbre par ligne (frontière) | 8,6 µs | 37,7 µs |

La frontière divise le coût d'un changement local par environ 160 ici. Elle
confirme la conséquence de §2 : le runtime gère lui-même les frontières, et
Taffy calcule chaque sous-arbre. Limite : seules les lignes de **hauteur
fixe** sont des frontières sûres ; une ligne de hauteur automatique dont la
hauteur change doit propager au parent (non mesuré ici).

## 4. Reste du tour 2

- Partie 3 :
  - même scène dans les piles complètes ;
  - rendu CPU (`vello_cpu`, `tiny-skia`) ;
  - mémoire et allocations ;
  - temps de compilation, taille des binaires ;
  - dépendances par catégorie et par plateforme.
- Tour 3 : scripts GPU pour vos machines (UI-D9).
