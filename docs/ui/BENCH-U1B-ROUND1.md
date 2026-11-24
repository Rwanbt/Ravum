# U1-B — Banc d'essai graphique, tour 1 (2026-11-24)

Statut des preuves : `TESTED`, local. Ce tour est **partiel** : il couvre
ce qui se mesure sans GPU ni écran. Aucune pile n'est adoptée (UI-D4).

- Machine : conteneur Linux x86_64, 4 CPU, sans GPU, compilation `release`.
- Code : [`bench/ui-candidates/`](../../bench/ui-candidates/). C'est un
  workspace **séparé** de celui de Ravum : les candidats n'entrent dans aucun
  crate Ravum.
- Versions épinglées exactement :
  - `taffy 0.16.0`, `parley 0.13.0`, `egui` et `eframe 0.37.0` ;
  - `slint 1.19.1`, `winit 0.31.0-beta.4`, `wgpu 30.0.2`, `vello 0.12.0` ;
  - `accesskit 0.25.2`, `blitz-dom 0.3.0-beta.4`.

Commandes :

```bash
cd bench/ui-candidates
cargo run --release -p ui-bench-headless
```

```bash
cargo tree -p fp-modular -e normal --prefix none --target x86_64-unknown-linux-gnu
```

## 1. Licences et chaîne d'approvisionnement

Le nombre de crates compte les dépendances normales, cible Linux, versions
distinctes.

| Pile | Crates | Licences hors MIT/Apache/BSD/Zlib | Verdict |
|---|---|---|---|
| Modulaire : winit, wgpu, Vello, Taffy, Parley, AccessKit | 177 | aucune | compatible avec la licence de Ravum |
| egui et eframe (wgpu, AccessKit) | 214 | `self_cell` : `Apache-2.0 OR GPL-2.0-only`, l'option Apache suffit | compatible |
| Blitz (`blitz-dom`) | 198 | aucune | compatible ; **version bêta** |
| **Slint** | 335 | le cœur `i-slint-*` est sous `GPL-3.0-only OR` licence commerciale Slint | **bloquant** sans décision du responsable : Ravum est `MIT OR Apache-2.0`, et un framework construit sur Slint imposerait la GPL ou une licence Slint à ses utilisateurs |

## 2. Coûts CPU headless (N = 30 exécutions après un échauffement)

| Mesure | p50 | p95 | max |
|---|---|---|---|
| Taffy, 1 001 nœuds, layout complet | 0,60 ms | 0,68 ms | 0,94 ms |
| Taffy, 1 001 nœuds, une feuille modifiée | 0,096 ms | 0,12 ms | 0,18 ms |
| Taffy, 10 001 nœuds, layout complet | 6,4 ms | 7,1 ms | 7,9 ms |
| Taffy, 10 001 nœuds, une feuille modifiée | 1,2 ms | 1,8 ms | 2,9 ms |
| Parley, 650 caractères, largeur 300 | 0,18 ms | 0,32 ms | 0,63 ms |
| Parley, 13 000 caractères, largeur 300 | 3,5 ms | 3,9 ms | 4,2 ms |
| egui, 200 widgets, une frame (exécution et tessellation) | 0,33 ms | 0,43 ms | 0,46 ms |
| egui, 2 000 widgets, une frame | 3,0 ms | 3,5 ms | 4,0 ms |

Lecture :
- **L'invalidation de Taffy n'est pas locale.** Une seule feuille modifiée
  coûte encore environ 1/6 du layout complet, parce que `mark_dirty`
  remonte jusqu'à la racine, qui recalcule ses enfants. Sur 10 000 nœuds, il
  reste 1,2 ms à chaque modification. Le runtime Ravum UI devra borner le
  layout aux sous-arbres atteints (frontières de layout, virtualisation) ;
  Taffy seul ne le fait pas.
- **Le texte domine** : 13 000 caractères coûtent 3,5 ms. Un cache de
  shaping par paragraphe est indispensable (U2).
- **egui** reconstruit tout à chaque frame : son coût croît avec le nombre
  de widgets visibles ou parcourus (ici aucune virtualisation, 2 000 widgets
  coûtent 3 ms). C'est la **référence** de réactivité à égaler ou battre,
  avec un arbre persistant et une invalidation fine.
- Ces chiffres ne comparent pas encore des scénarios équivalents. egui
  inclut la mise en page de son texte et la tessellation ; Taffy n'inclut
  aucun texte. Le tour 2 construira la même scène dans chaque pile.

## 3. Ce qui reste à mesurer (tours suivants)

| Tour | Contenu | Où |
|---|---|---|
| 2 | La même scène dans chaque pile : liste de 10 000 éléments virtualisée, formulaire, texte long ; rendu CPU (`vello_cpu`, `tiny-skia`) ; mémoire, allocations, temps de compilation et taille des binaires | ce conteneur (headless) |
| 3 | Rendu GPU : temps de frame CPU et GPU (p50, p95, p99), latence d'entrée, repos, VRAM, démarrage, animations | vos machines (UI-D9) : scripts fournis avec le tour 2 |
| 4 | Accessibilité (AccessKit sur chaque OS), IME, texte bidirectionnel, Android et iOS | vos machines et vos appareils |

## 4. Recommandation provisoire, non décisionnelle

D'après ce tour, la **pile modulaire** (winit, wgpu, Vello, Taffy, Parley,
AccessKit) est la meilleure candidate :
- licences toutes permissives ;
- le moins de dépendances ;
- chaque brique est remplaçable, ce qui sert l'indépendance du renderer.

**egui** reste la référence de mesure. **Blitz** (bêta) est à suivre pour les
styles de type CSS. **Slint** est écarté sauf décision contraire de votre
part sur la licence.

Le choix définitif attend les tours 2 et 3 et votre décision.
