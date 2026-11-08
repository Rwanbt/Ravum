# M1 — Preuves d'exécution (MVP-0a)

> **Ce document rapporte des exécutions réelles.** Chaque chiffre provient d'un
> fichier de `evidence/` ou d'une sortie de test reproductible. Ce qui n'a pas été
> exécuté est marqué « non exécuté » — jamais « devrait fonctionner ».

## 1. Contexte

| Champ | Valeur |
|---|---|
| Date | 2026-11-08 |
| Branche | `feat/3-mvp0a-continuous-loop` |
| Commit de base | `16b1ff2adc11289f1648f42fcf52014542cb30fb` |
| OS / CPU / RAM | Windows 11 26100 / i7-13620H (10C/16T) / 15,71 Go (≈2 Go libres) |
| rustc | 1.95.0 (59807616e 2026-04-14), `x86_64-pc-windows-msvc`, LLVM 22.1.2 |
| Configuration | `CARGO_BUILD_JOBS=2`, tests sérialisés (`--test-threads=1`) |
| Statut global | **M1 démontré sur une unité `f64 -> f64`** ; M2–M4 non commencés |

## 2. Suite de tests

Commande : `cargo test --workspace --offline -- --test-threads=1`

Résultat observé : **51 tests, 51 réussis, 0 échec, 0 ignoré.**

| Groupe | Nombre | Ce qu'il couvre |
|---|---|---|
| `ravum-protocol` (unitaires) | 10 | identités, statuts, sérialisation, absence de `PROVED` synthétique |
| `ravum-snapshot` (unitaires) | 5 | canonicalisation, immuabilité, overlay, staleness |
| `ravum-rust-adapter` (unitaires) | 3 | parsing des diagnostics JSON de rustc |
| `ravum-patch-host` (unitaires) | 3 | reclamation hors thread appelant |
| `ravum-coordinator` (unitaires) | 12 | file coalescente, verdict `Stale`, ordre des étapes |
| `ravum-workspace-coordinator` (unitaires) | 11 | parseur de diff, décisions, journal |
| `collab_flow` (intégration) | 3 | COLLAB-1 → compilation → activation réelle |
| `m1_patch_loop` (intégration) | 10 | activation compatible + 7 refus + appel en vol + STALE |
| `git_writer` (intégration) | 2 | commit des seuls chemins intégrés, refus du vide |
| `ravum-cli` (unitaires) | 2 | percentiles |

Les dix tests d'intégration M1 compilent de **vraies bibliothèques natives** avec
`rustc` et chargent le résultat : ce ne sont pas des mocks.

## 3. Démonstration M1 — hot patch dans un processus vivant

Commande : `powershell -ExecutionPolicy Bypass -File scripts/demo-m1.ps1`
Sortie brute JSON : `evidence/m1-demo.jsonl` (ticks + enregistrement d'activation).

| Observation | Valeur |
|---|---|
| PID du processus hôte | **20964**, inchangé avant et après |
| Calculs avant activation | 34 ticks, version `v1`, dernière valeur `68.0` (= 2 × 34) |
| Activation | `activated` en **520 ms** |
| Détail des étapes | check 107 ms, build 406 ms, verify 3 ms, load 3 ms, activate 1 ms |
| Calculs après activation | 8 ticks, version `v2`, dernière valeur `97.0` (= 2 × 48 + 1) |

Interprétation : la même fonction `ravum_compute` a changé de comportement dans le
même processus. Aucun redémarrage, aucun rechargement d'application.

## 4. Mesures de latence (20 itérations)

Source : `evidence/m1-bench.json`. Commande :
`ravum bench fixtures/calculator/v1.rs fixtures/calculator/v2.rs --iterations 20`

| Étape | p50 | p95 | p99 | max |
|---|---|---|---|---|
| `check` (analyse seule) | 103 ms | 136 ms | 171 ms | 171 ms |
| `build` (codegen + link) | 269 ms | 356 ms | 366 ms | 366 ms |
| `verify` (smoke, chargement inclus) | 3 ms | 4 ms | 4 ms | 4 ms |
| `load` (`LoadLibraryW` + symboles) | 2 ms | 4 ms | 4 ms | 4 ms |
| `activate` (publication atomique) | 0 ms | 0 ms | 0 ms | 0 ms |
| **`total`** | **380 ms** | **481 ms** | **544 ms** | **544 ms** |

- 20/20 itérations activées, 0 refus, code de sortie 0.
- Mémoire du processus CLI : 5 184 KiB → 5 356 KiB (+172 KiB sur 20 itérations,
  reclamation comprise). Aucune fuite observée sur cette fenêtre ; ce n'est pas une
  preuve d'absence de fuite à long terme.

### 4.1 Confrontation aux cibles exploratoires

| Cible (MVP.md) | Mesure | Verdict |
|---|---|---|
| diagnostic local p95 < 50 ms | **136 ms** | **NON ATTEINTE** |
| petit patch local p95 < 500 ms | **481 ms** | atteinte de justesse |

La cible de diagnostic n'est pas atteinte et ne sera pas maquillée. Cause identifiée :
chaque analyse lance un `rustc --emit=metadata` **à froid**, sans daemon ni cache de
requêtes. Le coût est dominé par le démarrage du compilateur, pas par l'unité
analysée. Deux conséquences honnêtes :

1. un daemon avec cache de requêtes est le levier évident (ADR-012/ADR-019), mais il
   n'existe pas encore ;
2. le chiffre de 481 ms pour le patch complet reste sous la cible uniquement parce
   que l'unité est minuscule (un seul fichier, une seule fonction).

## 5. Démonstration COLLAB-1 (scénario scripté, 6 propositions)

Commande : `powershell -ExecutionPolicy Bypass -File scripts/demo-collab.ps1 -Commit`
Journal : `evidence/collab-demo/journal.jsonl`. Propositions : `evidence/collab-demo/proposals/`.

| # | Proposition | Décision | Effet |
|---|---|---|---|
| 1 | `agent-a` modifie `helper` | `AUTO_SAFE` | intégrée |
| 2 | `agent-b` modifie `ravum_compute` (même fichier, base identique) | `AUTO_SAFE` | intégrée |
| 3 | `agent-c` modifie `helper` (même symbole que 1) | `CONFLICT` | refusée, aucune écriture |
| 4 | `agent-d` sur base périmée sans preconditions | `REBASE_REVALIDATE` | non intégrée |
| 5 | `agent-e` touche un contrat | `MANUAL_REQUIRED` | non intégrée |
| 6 | `agent-f` patch malformé | `REJECT` | refusée |

Résultat : snapshot `f6d572a5bf18…` → `b7290265aff8…`, commit unique
`c6b7ca20c829` (parent `56badc76caa1`), `1 path(s)` modifié.
`notes.rs` conserve `WINDOW = 64` : la proposition 4 n'a laissé aucune écriture.

## 6. Expérience multi-agents réelle

Trois subagents sur le **même checkout**, sans worktree :
`evidence/multi-agent-live/README.md`.

| Agent | Tâche | Décision |
|---|---|---|
| A | décalage de `helper` 1.0 → 20.0 | `AUTO_SAFE` |
| B | facteur de `ravum_compute` 2.0 → 5.0 | `AUTO_SAFE` |
| C | décalage de `helper` 1.0 → 30.0 (concurrent de A) | `CONFLICT` |

Le fichier intégré contient `x + 20.0` **et** `helper(x) * 5.0`, compile réellement
(`artifact_id = bd499825f9f5530e12c70a99216fa8ef3c67f9d5001cfb414503f75e9bf2079d`),
et la modification de C n'apparaît nulle part.

**Constat COLLAB** : la première exécution a refusé à tort la proposition B parce que
les hunks partageaient une ligne de **contexte** (règle standard de 3 lignes). Le
test de chevauchement a été corrigé pour ne considérer que les lignes réellement
modifiées. Ce cas est conservé dans `evidence/multi-agent-live/README.md` §4.

## 7. Ce qui n'a pas été exécuté

| Élément | Raison |
|---|---|
| Migration d'état entre versions | non implémentée (ADR-022 ouvert) |
| Reprise après panique dans un patch | `panic=abort` : le processus hôte avorte, pas d'isolation |
| Appels concurrents massifs (>2 threads) | non testé ; le test d'appel en vol est mono-thread |
| Nettoyage des artefacts accumulés | absent : `target/ravum/artifacts/` grossit à chaque build |
| Provider de preuve (Kani, etc.) | M2, non commencé |
| DSP / temps réel | M3, non commencé |
| Full-stack UI+API+SQLite | M4, non commencé |
| Comparaison worktrees vs COLLAB-1 | S8-A/S8-B, non exécuté : aucune mesure de gain de temps total |
| CI GitHub réellement exécutée | workflow ajouté ; le résultat du run est vérifiable dans l'onglet Actions, pas dans ce fichier |

## 8. Reproductibilité

```powershell
cargo build --workspace
powershell -ExecutionPolicy Bypass -File scripts/run-tests.ps1
powershell -ExecutionPolicy Bypass -File scripts/demo-m1.ps1
powershell -ExecutionPolicy Bypass -File scripts/demo-collab.ps1 -Commit
cargo run -q --bin ravum -- bench fixtures/calculator/v1.rs fixtures/calculator/v2.rs --iterations 20 --out evidence/m1-bench.json
```

Les valeurs varient avec la charge machine (observé : 249 ms à 520 ms pour une même
activation). Toute comparaison ultérieure doit citer le commit, la config et la
charge, comme le demande INV-08.
