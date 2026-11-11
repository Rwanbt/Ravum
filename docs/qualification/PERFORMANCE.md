# Performance mesurée — `check` et `build` sur programmes générés

> **Statut : mesures réelles, portée explicite.** Ces chiffres décrivent **une**
> machine, **une** toolchain et des programmes **synthétiques** déterministes.
> Ils ne sont pas un benchmark concurrentiel et ne prétendent pas à une
> signification statistique.

| Élément | Valeur |
|---|---|
| Hôte | Windows 11 x86-64, 16 cœurs, 16 Go |
| Toolchain | `rustc 1.95.0`, `cargo 1.95.0`, profil `dev` (`-C opt-level=0`) |
| Binaire mesuré | `target/debug/ravum-lang.exe` |
| Script | [`scripts/lang-bench.ps1`](../../scripts/lang-bench.ps1) |
| Date | 2026-11-10 |
| Répétitions | 3 par point, **médiane** rapportée |

## 1. Programme généré

Pour `N` fonctions, le générateur produit :

- `main` avec `N` appels `f_i(i)`, une accumulation `total = total + v_i` et un
  `println` final ;
- `N` fonctions `f_i(x: i32) -> i32` avec deux liaisons locales, une condition et
  deux `return`.

Aucune optimisation de forme : c'est du code “plat” qui mesure le passage à
l'échelle du frontend, pas la qualité du codegen.

## 2. Résultats

| N fonctions | lignes | `check` médiane | `check` min–max | pic mémoire `check` | `build` médiane | pic mémoire `build` |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 1 106 | **38,7 ms** | 26,8 – 54,6 ms | **9,1 Mo** | **284,9 ms** | 10,0 Mo |
| 500 | 5 506 | **86,4 ms** | 76,8 – 89,5 ms | **20,1 Mo** | **543,5 ms** | 21,7 Mo |
| 2 000 | 22 006 | **296,6 ms** | 293,7 – 303,2 ms | **60,4 Mo** | **1 789,6 ms** | 63,8 Mo |

Ordres de grandeur :

- `check` est **sous-linéaire à linéaire** : ×5,1 de lignes (100 → 2 000
  fonctions) donnent ×7,7 de temps ; ×4 de lignes (500 → 2 000) donnent ×3,4.
- mémoire de `check` : ≈ **2,7 Ko par ligne** sur cette forme.
- `build` est dominé par `rustc` : à 2 000 fonctions, 1,79 s au total dont
  ~1,49 s hors analyse Ravum. **Ce n'est pas** un temps de compilation Ravum.

## 3. Méthode exacte

```powershell
cargo build -p ravum-lang-cli
pwsh -File scripts/lang-bench.ps1 -Runs 3 -Sizes 100,500,2000
```

Le script : génère les paquets sous `target/bench/`, lance un **processus neuf**
par mesure, chronomètre `check`/`build`, échantillonne la working set toutes les
5 ms pendant l'exécution, et supprime tout par défaut (`-Keep` conserve).

## 4. Limites honnêtes

- **Pic mémoire échantillonné** : `PeakWorkingSet64` vaut zéro une fois le
  processus terminé, donc la mémoire est échantillonnée en cours d'exécution.
  Les valeurs sont des **bornes inférieures**.
- **Profil `dev`** : un binaire `--release` serait plus rapide ; ces chiffres
  décrivent la toolchain telle qu'un utilisateur la construit par défaut, pas son
  potentiel.
- **Une seule machine, un seul OS** : aucune mesure Linux, aucune mesure ARM.
- **`build` inclut `rustc`** : la colonne ne mesure pas le compilateur Ravum seul
  mais la chaîne complète voie B.
- **Programmes synthétiques** : pas de génériques imbriqués, pas de modules
  multiples, pas de code de test. La mise à l'échelle réelle peut différer.
- Ces mesures ne sont **pas** une preuve de performance temps réel, ni une
  comparaison avec un autre compilateur.
