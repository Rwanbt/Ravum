# Preuves — MVP-0a (M0/M1)

> **Ce dossier contient des sorties brutes, pas des promesses.** Chaque fichier
> indique son origine : commande, commit, machine, toolchain. Les chiffres sont
> locaux et observés ; ils ne sont pas des garanties portables.

## Contenu versionné

| Fichier | Origine | Ce qu'il prouve |
|---|---|---|
| `m1-bench.json` | `ravum bench` | min/p50/p95/p99 par étape, mémoire, **calibration de charge**, artefacts froids |
| `m1-demo.jsonl` | `scripts/demo-m1.ps1` | le processus hôte change de comportement sans redémarrage |
| `m2-proof.json` | `ravum prove` | un `proved` borné et un `tested` avec contre-exemple précis |
| `m2-ledger.jsonl` | `ravum prove` | ledger append-only des deux verdicts (2 lignes, séquencées) |
| `m2-build-cache.json` | `ravum build` ×2 | build froid 2 286 ms vs chaud 3 ms, **même `artifactId`**, aucun relink |
| `collab-demo.txt` | `scripts/demo-collab.ps1` | décisions AUTO_SAFE / CONFLICT / REJECT / MANUAL_REQUIRED / REBASE_REVALIDATE et commit unique |
| `collab-demo/proposals/*.json.tmpl` | auteurs humains/agents | propositions rejouables (base et hashes injectés au moment de l'exécution) |
| `multi-agent-live/` | deux subagents réels | propositions produites indépendamment sur la branche partagée, puis intégrées par le coordinateur |

## Non versionné (`evidence/local/`, ignoré par Git)

Sorties de tests, journaux de `ravum watch`, mesures d'essai. Elles changent à
chaque exécution ; les reproduire est plus utile que les archiver.

## Reproduire

```powershell
cargo build --workspace
powershell -ExecutionPolicy Bypass -File scripts/run-tests.ps1
powershell -ExecutionPolicy Bypass -File scripts/demo-m1.ps1
powershell -ExecutionPolicy Bypass -File scripts/demo-collab.ps1 -Commit
cargo run -q --bin ravum -- bench fixtures/calculator/v1.rs fixtures/calculator/v2.rs --iterations 20 --out evidence/m1-bench.json
cargo run -q --bin ravum -- prove fixtures/proof/positive.rs --claim finite --inputs 0,1,2.5,1000000 --json
cargo run -q --bin ravum -- prove fixtures/proof/negative.rs --claim non-negative --inputs 0,1,2,3 --json
```

## Règles de lecture

1. `TESTS` verts ≠ preuve. Un test réussi est un statut `tested`, jamais `proved`.
2. `build_ms` couvre codegen **et** édition de liens : rustc ne les sépare pas via
   la CLI publique. Aucun chiffre inventé pour combler ce trou.
3. Un p99 calculé sur 20 échantillons est le maximum de ces 20 échantillons.
4. La machine de référence n'est pas un environnement de qualification temps réel.
