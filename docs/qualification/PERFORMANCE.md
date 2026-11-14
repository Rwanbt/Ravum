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

## 5. Garde de non-régression en CI (2026-11-14)

**Ce qui manquait.** Des p50/p95 publiés ne suivent rien : un document qui dit
« 1,3–8,1 ms » reste vert après une régression d'un facteur 10. R9 demande
« absence de régression **suivie** ».

**Dispositif.**

- `docs/qualification/timings-baseline.json` : référence mesurée sur la machine
  de référence (Windows 11 x64, `rustc 1.95.0`) le 2026-11-14 à `214bf5e`,
  `scripts/lang-timings.ps1 -CheckOnly -Runs 10`, 10 exemples, modes
  `cold` et `edit`.
- `.github/workflows/performance.yml` : sur `windows-latest`, reconstruit le CLI
  en debug, rejoue la même mesure (`-Runs 10`), puis applique
  `tools/timings_gate.py`.
- **Plafond** : `max(200 ms, valeur de référence × 3)` sur les totaux p50/p95 de
  chaque exemple et de chaque mode. C'est une **alarme grossière**, pas un
  budget : le plancher de 200 ms absorbe la différence entre la machine de
  référence et un runner partagé, et le facteur 3 absorbe un CPU plus lent.
- Le gate ne tourne que lorsqu'un crate du langage, le script de mesure, le gate
  ou la baseline changent (filtre `paths`), pas sur une PR documentaire seule.
- L'export complet est uploadé à chaque exécution (`lang-timings-ci`) : une
  dérive peut être inspectée avant de devenir un échec.

**Premier run réel** : [43622599048](https://github.com/Rwanbt/Ravum/actions/runs/43622599048),
commit de fusion `55f11a5e`, 10 exemples, **PASS** (40 comparaisons). Valeurs
CI (p95 totaux `check`, ms) : `hello` 1,766 ; `fib` 2,365 ; `calculator` 3,878 ;
`file-store` 14,325 ; `package-demo` 3,997 ; `tests-demo` 2,794. Elles sont
**meilleures** que la baseline (machine de référence plus chargée), ce qui est
attendu : la comparaison porte sur le plafond, pas sur une égalité.

**Ce que le gate ne prouve pas.** Il ne mesure ni `build` (dominé par `rustc`),
ni `watch`, ni la latence LSP, ni la mémoire, ni le corpus à l'échelle ; il ne
détecte pas une dérive de 20 %. Une dérive légitime (par exemple un test qui
ajoute du travail au `check`) exige de **régénérer la baseline dans le même
commit**, avec la machine et le SHA ; relever le facteur pour faire passer un
rouge est un affaiblissement et doit être refusé.

**Statut R9** : les p50/p95 `check`/`build` restent publiés (§2, et
`docs/reports/LANG-TIMINGS.md` §2–§4) ; l'absence de régression est désormais
**suivie sur le `check`** par une alarme CI. R9 reste `PARTIAL` : `build` et
`watch` ne sont pas sous gate, et une alarme n'est pas un budget.
