# S8 MVP-0c — harnais du protocole réel + pilote scripté

Harnais Node 26 **zéro dépendance** pour comparer trois stratégies de
collaboration multi-agents sur une même fixture Rust réelle :

- **A** — une copie privée par agent, puis intégration par fusion 3-way en
  mémoire (aucune commande Git) ;
- **B** — un répertoire partagé, agents concurrents, garde de version optimiste
  et détection de conflits ;
- **C** — séquentiel sur un répertoire unique (contrôle).

> **Le pilote utilise des éditeurs SCRIPTÉS DÉTERMINISTES, pas des LLM.**
> Aucun token IA n'y est mesuré (aucune session DSH produite) et le temps de
> supervision humaine n'y est pas mesuré. La campagne réelle avec subagents DSH
> réutilise le même harnais en `--mode live` et mesure les tokens avec
> `tools/agent-usage/usage.mjs` (voir `docs/mvp/MVP-0c-s8.md`, section 8).

## Contenu

```
benchmarks/s8-mvp0c/
  fixture/                 crate Rust réel (3 modules, 12 tests) : tests ROUGES avant éditions
    Cargo.toml             [workspace] vide -> jamais membre du workspace Ravum
    src/{lib,stats,text,dates}.rs
    tests/behavior.rs      tests protégés (aucun agent ne les modifie)
  lib/
    tasks.mjs              corpus pré-enregistré : 4 tâches, 3 agents, extraits exacts
    merge.mjs              fusion 3-way ligne à ligne (sans Git)
    verify.mjs             contenu exact, marqueurs, fichiers protégés, écrasements
    agent-scripted.mjs     agent-command du pilote (éditeur scripté, garde de version en B)
    write-receipt.mjs      reçu d'agent (utilisé par les subagents réels)
    receipt.mjs            contrat de reçu (chemins + sha256 avant/après)
    sampler.ps1/.mjs       RAM/CPU (WorkingSet, temps CPU), anti-réutilisation de PID
    usage-attribution.mjs  tokens DSH par agent (marqueurs de session, jamais inventés)
    exec.mjs, fsutil.mjs, env.mjs, report.mjs
  run.mjs                  harnais : pilot | live prepare/collect/aggregate
  results/latest.md        dernier rapport brut généré
```

## Pilote scripté

```powershell
pwsh -File scripts/s8-mvp0c.ps1                 # 3 répétitions, ordre alterné
pwsh -File scripts/s8-mvp0c.ps1 -Keep           # conserve les répertoires temporaires
node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 3
```

Le harnais :

1. vérifie que la fixture est **rouge avant éditions** (sinon code 1) ;
2. exécute une **sonde de contrôle** : un écrasement d'écriture simulé doit être
   détecté par le détecteur de conflits ;
3. exécute A/B/C en ordre **alterné** sur des copies éphémères (cache `target/`
   froid identique) avec `CARGO_BUILD_JOBS=2`, sans réseau ;
4. vérifie contenu exact, marqueurs, fichiers protégés, reçus et `cargo test` ;
5. mesure temps, attentes, conflits, itérations, builds, RAM/CPU/disque ;
6. écrit `evidence/mvp0c-s8-pilot-2026-11-09.json` + `results/latest.md`.

En stratégie B, une **fenêtre de collision déterministe** est injectée (agent-2
lit `src/stats.rs`, attend 200 ms, relit) pour exercer réellement la détection de
conflit ; l'éditeur scripté utilise une garde de version optimiste pour ne perdre
aucune édition.

### Agent-command pluggable

Le pilote peut exécuter n'importe quelle commande d'agent à la place de
l'éditeur scripté, avec les substitutions `{agent}`, `{workspace}`, `{tasks}`,
`{strategy}`, `{base}`, `{receipt}` :

```powershell
node benchmarks/s8-mvp0c/run.mjs --mode pilot --reps 1 `
  --agent-command "node C:\chemin\mon-agent.mjs --agent {agent} --workspace {workspace} --tasks {tasks} --base {base} --receipt {receipt}"
```

Pour les subagents DSH, on n'utilise pas cette option : le mode `live`
(section suivante) prépare les prompts/arborescences et collecte les sessions
réelles — c'est le chemin recommandé pour la campagne IA.

## Campagne réelle (subagents IA)

Procédure complète : `docs/mvp/MVP-0c-s8.md` section 8. Résumé :

```powershell
# 1. préparer une campagne (une stratégie x une répétition)
node benchmarks/s8-mvp0c/run.mjs --mode live --phase prepare --scenario A --rep 1 --campaign C:\tmp\s8-A1
# 2. le Lead spawne 3 subagents avec les prompts générés (description = marqueur S8MVP0C-AGENT-n)
# 3. collecter (vérifie, intègre, attribue les tokens DSH)
node benchmarks/s8-mvp0c/run.mjs --mode live --phase collect --campaign C:\tmp\s8-A1 --usage auto --out evidence\mvp0c-s8-live-A1-2026-11-09.json
# 4. agréger
node benchmarks/s8-mvp0c/run.mjs --mode live --phase aggregate --campaigns C:\tmp\s8-A1,C:\tmp\s8-B1 --out evidence\mvp0c-s8-live-summary-2026-11-09.json
```

## Limites déclarées

- pilote : agents scriptés, pas de LLM, tokens et supervision humaine non mesurés ;
- A = copies + fusion en mémoire, pas worktrees Git (contrainte « aucune commande git mutante ») ;
- collision de B injectée au pilote (déterministe) ;
- échantillon restreint (3 répétitions), fixture locale, machine partagée ;
- `cargo test` froid par répétition : le coût fixe de compilation amortit les écarts.

Détails et critères pré-enregistrés : [`docs/mvp/MVP-0c-s8.md`](../../docs/mvp/MVP-0c-s8.md).
