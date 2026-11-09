# Intégration MVP-0a / 0b / 0c dans `dev` — chaîne propre et provenance

> **Document d'historique d'intégration, pas un certificat de maturité.** Aucun ADR n'est
> accepté, aucune licence générale n'est choisie, `dev` n'est pas `main`, et rien ici ne
> prétend que Ravum est un langage autonome fonctionnel.

**Date :** 9 novembre 2026
**Décision de référence :** autorisation explicite du responsable d'intégrer les trois MVP
dans `dev`, sous gate de provenance.

## 1. Constat de blocage

Les trois branches MVP publiées portent, dans leur **ascendance**, un commit d'outillage
d'agents qui n'appartient pas au produit :

| Élément | Valeur |
|---|---|
| Commit contaminant | `16b1ff2adc11289f1648f42fcf52014542cb30fb` — `chore: install ai-native dev-stack (verified profile)` |
| Fichiers ajoutés par ce commit | 51 |
| Présent sur `origin/dev` | **non** |
| Ancêtre de `feat/3-mvp0a-continuous-loop` | oui |
| Ancêtre de `feat/mvp0b-hardening` | oui |
| Ancêtre de `feat/mvp0c-hardening` | oui |

Une fusion de branche classique aurait donc importé dans `dev`, par simple ascendance,
l'état d'installation local de l'outillage, y compris :

- des doublons stricts entre `.agents/skills/**` et `.claude/skills/**` ;
- `.claude/settings.json`, qui contient un chemin absolu propre à une machine
  (`D:/App/Ravum/tools/ai_docs/run_hook.ps1`) ;
- `.ai-native/lifecycle/**` et `.ai-native/trust/**`, qui sont de l'état d'installation ;
- `conventions.json`, qui référence `scripts/validate_conventions.py`, absent du dépôt.

## 2. Audit de provenance (résultat)

| Question | Résultat vérifié |
|---|---|
| Dépôt d'origine | `https://github.com/Rwanbt/ai-native-dev-stack` |
| Licence du dépôt d'origine | **MIT** (métadonnée GitHub `repos/Rwanbt/ai-native-dev-stack`) |
| Fork d'un tiers ? | non (`fork: false`) ; dépôt original du même titulaire que Ravum |
| Ravum a-t-il une licence ? | **non** — aucun `LICENSE`/`COPYING` ; décision réservée (#34) |
| Redistribution du contenu tiers autorisée ? | **oui**, sous MIT, avec conservation de l'avis de copyright et du texte de permission |
| Code d'un autre tiers copié ? | aucun identifié : `graphify` n'est cité que comme outil externe à installer, pas comme code embarqué |

Conclusion : le contenu n'est **pas** non-redistribuable. L'exclusion décidée ci-dessous
est donc un choix d'**hygiène et de duplication**, pas une contrainte de licence.

## 3. Décision d'intégration

Reconstruction d'une chaîne propre à partir de `dev`, sans réécrire ni forcer les branches
publiées, et sans reprendre le commit contaminant dans l'historique fusionné :

```text
origin/dev (045b285)
   └── integration/mvp0a-clean   21 commits  (rejoue 16b1ff2..550847e sans 16b1ff2)
         └── integration/mvp0b-clean   54 commits  (rejoue 550847e..45d84343)
               └── integration/mvp0c-clean   39 commits  (rejoue 45d84343..2f92c240)
```

| Branche d'origine | HEAD d'origine | Chain propre | Commits rejoués |
|---|---|---|---|
| `feat/3-mvp0a-continuous-loop` (#78) | `550847e00e6c932729a1e14e08fd7581a8105c95` | `integration/mvp0a-clean` | 21 |
| `feat/mvp0b-hardening` (#80) | `45d84343d3fbf13ed4bbb9837fef356b938cf151` | `integration/mvp0b-clean` | 54 |
| `feat/mvp0c-hardening` (#82) | `2f92c2410ef2c7bb33f67db6737c92fddf2cefa6` | `integration/mvp0c-clean` | 39 |

Auteurs et messages des commits sont préservés. Le commit `docs(branding): show Ravum
banner in README` est omis de la chaîne 0a : son patch est identique à `045b285` déjà
présent sur `dev` (détection `git rebase` par `patch-id`).

Seul conflit de rejeu : `.gitignore`, où le bloc du stack a été retiré et l'entrée
Ravum `impact.json` conservée.

## 4. Preuve d'équivalence des arbres

Comparaison de `origin/feat/mvp0c-hardening` et de `integration/mvp0c-clean`, en excluant
les 51 chemins d'outillage :

- **660 fichiers** de chaque côté, **ensembles de noms identiques** ;
- une seule différence de contenu : `.gitignore`, où 6 lignes du bloc
  `ai-native-dev-stack` ont disparu ;
- aucun fichier `.agents/`, `.claude/`, `.ai-native/`, `tools/ai_docs/`, `conventions.json`
  ni `.github/ISSUE_TEMPLATE/` dans le diff contre `dev`.

Aucune modification produit n'est perdue : ce qui est exclu est exactement le contenu
introduit par `16b1ff2`.

## 5. Validation

| Étape | Résultat |
|---|---|
| Baseline avant intégration (`cargo test --workspace --offline -- --test-threads=1` sur l'ancien HEAD MVP-0c `2f92c24`) | **495 réussis / 0 échec / 78 suites** |
| CI produit et impact sur les PR d'origine #78/#80/#82 | vertes (runs `42552560527`, `42552554186`, `42557619322`) |
| CI des PR d'intégration #83, #84, #85 | **vertes** : `mvp`, `impact`, `docs`, soak auto-GC borné |
| Suite complète sur `dev` (`6b0ac7b`, `cargo test --workspace --offline -- --test-threads=1`) | **496 réussis / 0 échec / 79 suites** |
| `cargo fmt --all -- --check` sur `dev` | **0** |
| `cargo build --workspace --offline` sur `dev` | **0** |
| Démonstrations M1, COLLAB-1 (`-Commit`), preuve bornée M2, DSP M3 (IIR + gain, 200 blocs) | **0** |
| Démo M4 (UI + API + SQLite) et **E2E navigateur réel** (`demo-m4-browser.ps1 -RequireBrowser -Screenshots`) | **0** |

Le compteur `dev` est supérieur d'une unité à la baseline : un test parcourt les
documents du dépôt et prend en compte le présent fichier. Les 63 binaires de test sont
identiques entre les deux exécutions.

La validation des branches d'origine **ne remplace pas** la validation du code réellement
fusionné : la suite ci-dessus a été exécutée sur `dev` après les trois fusions, et la CI
de chaque PR d'intégration a été verte avant fusion.


## 6. Ce qui reste décidé par le responsable

- La licence générale de Ravum (issue #34) — inchangée, non tranchée ici.
- Le choix de versionner ou non un jour cet outillage d'agents dans le dépôt produit, et
  sous quel encadrement d'attribution MIT.
- L'acceptation d'un ADR quelconque — les 42 ADR restent `PROPOSED`.

## 7. Point de reprise

Si l'outillage local doit être restauré sur un poste (il reste sur le disque, simplement
non versionné ici) :

```powershell
git checkout 16b1ff2adc11289f1648f42fcf52014542cb30fb -- .agents .claude
git reset -- .agents .claude   # conserve les fichiers sur disque, ne les indexe pas
```
