# S8 — Benchmark worktrees Git vs workspace partagé Ravum

Harnais reproductible (Node 26, **aucune dépendance npm**) comparant trois
organisations de travail multi-agents sur un **même projet**, avec les **mêmes
demandes d'édition**, le **même nombre d'agents (3)**, les **mêmes tests** et la
**même machine** :

| Scénario | Organisation |
|---|---|
| **A** | un worktree Git par agent, branches séparées, intégration séquentielle par `git merge` réels dans une branche d'intégration |
| **B** | un seul workspace partagé : les agents produisent des `ChangeProposal` JSON, le coordinateur Ravum décide (`AUTO_SAFE`/`CONFLICT`/`MANUAL_REQUIRED`/`REBASE_REVALIDATE`/`REJECT`) et l'écrivain Git unique `GitWriter` publie transactionnellement (`prepare`/`publish`) |
| **C** | contrôle : agents séquentiels sur une branche unique, mêmes éditions appliquées l'une après l'autre |

> **Les « agents » sont des éditeurs scriptés déterministes, PAS des LLM.**
> Aucun token IA n'est consommé ni mesuré ; le temps de supervision humaine
> n'est pas mesuré non plus. Le benchmark ne mesure que des coûts mécaniques
> reproductibles (wall-clock, conflits, écritures, RAM/CPU, cache, disque).

## Contenu

```
benchmarks/s8/
  run-s8.mjs            # entrée CLI : construit, programme, exécute, agrège, écrit les preuves
  fixture/              # projet Rust identique pour A/B/C (4 modules, 8 fonctions, 10 tests)
    Cargo.toml          # [workspace] vide -> jamais membre du workspace Ravum
    src/{lib,alpha,beta,gamma,delta}.rs
    tests/behavior.rs
  lib/
    tasks.mjs           # corpus des 6 tâches + application des éditions + génération des diffs
    campaign.mjs        # scénarios A/B/C, setup Git, intégration, vérifications
    exec.mjs            # exécution instrumentée (wall-clock, journal de commandes, tracking PID)
    sampler.ps1/.mjs    # échantillonneur RAM/CPU des processus (WorkingSet + temps CPU)
    env.mjs             # environnement machine + sondes de charge
    report.mjs          # agrégation (médiane, min/max, IQR, MAD) + tableaux
  results/latest.md     # dernier rapport généré (résultats bruts)
```

## Usage

Depuis la racine du dépôt :

```powershell
# Campagne officielle : 5 répétitions par scénario, ordre alterné A,B,C / C,B,A...
pwsh -File scripts/s8.ps1

# Variantes utiles
pwsh -File scripts/s8.ps1 -Reps 5 -Keep          # conserve les répliques temporelles pour inspection
pwsh -File scripts/s8.ps1 -Scenarios A,B -Reps 3 # sous-ensemble
pwsh -File scripts/s8.ps1 -SkipBuild             # ne recompile pas ravum-collab
```

Équivalent direct Node :

```powershell
node benchmarks/s8/run-s8.mjs --reps 5 --scenarios A,B,C
node benchmarks/s8/run-s8.mjs --reps 1 --scenarios A,B,C --keep
```

Le script propage le code de sortie de Node : `0` = campagnes exploitables et
conformes au protocole ; `1` = échec de protocole (tests rouges, édition
indépendante perdue, publication non transactionnelle, etc.).

## Ce que fait une répétition

Pour chaque `(scénario, répétition)` :

1. copie éphémère de `fixture/` dans `%TEMP%/ravum-s8-<stamp>/<scénario><rep>/`
   et initialisation d'un dépôt Git (`autocrlf=false`, hooks locaux neutres) ;
2. exécution des 3 agents scriptés (6 tâches : `t1`..`t6`) selon le scénario ;
3. intégration/publication (merges réels en A, coordinateur + GitWriter en B,
   commits séquentiels en C) ;
4. vérification de contenu par rapport à une référence calculée hors ligne,
   puis `cargo test` final ;
5. collecte des métriques (timings, conflits, écritures, RAM/CPU, cache, disque)
   et des preuves Git (`git log`, `git status`, `git worktree list`, journaux) ;
6. suppression de la réplique (sauf `--keep`).

Rien n'est écrit dans le dépôt Ravum : les commits appartiennent aux répliques
temporaires et la racine ne reçoit que les fichiers de preuve.

## Preuves et rapport

- `evidence/mvp0b-s8-<A|B|C>-<date>.json` : échantillons bruts par répétition,
  environnement, corpus, charge machine, commandes exécutées.
- `evidence/mvp0b-s8-summary-<date>.json` : tableaux agrégés + validation.
- `benchmarks/s8/results/latest.md` : rapport Markdown regénéré à chaque run.
- `docs/mvp/MVP-0b-s8.md` : protocole pré-enregistré, résultats, verdict honnête,
  limites.

## Limites déclarées

- agents = éditeurs déterministes, pas de LLM ; tokens IA et supervision humaine
  non mesurés ;
- échantillon restreint (5 répétitions par scénario) et fixture locale petite ;
- machine partagée avec d'autres agents : la charge est mesurée et documentée,
  pas contrôlée ;
- chaque répétition a un cache `target/` froid (même coût de compilation pour
  A/B/C), ce qui amortit les différences d'organisation dans le temps total ;
- l'échantillonneur RAM/CPU a une granularité de ~200–300 ms : les pics très
  courts (rustc) peuvent être sous-échantillonnés ; les valeurs sont comparatives.
