# Preuves — MVP-0a (M0/M1) et MVP-0b (L2 incrémental)

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
| `mvp0b-incremental-bench.json` | `ravum-daemon-bench --suite campaign` | 7 catégories d'édition × 3 modes (rustc froid, `-C incremental` persistant, daemon) × 10 échantillons : min/p50/p95, diagnostics comparés à rustc froid, charge par catégorie |
| `mvp0b-incremental-scale.json` | `ravum-daemon-bench --suite scale` | seuil de rentabilité de `-C incremental` : cold vs incrémental à 1 089 / 3 209 / 7 629 lignes |
| `mvp0b-incremental-invariants.json` | `ravum-daemon-bench --suite invariants` | 14 contrôles négatifs/limites exécutés : révision périmée, annulation réelle d'un rustc en vol, buffer incomplet, corruption de cache/snapshot, redémarrage, budgets, timeout, JSON, fichiers utilisateur intacts |

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

# MVP-0b — daemon persistant d'analyse incrémentale (voir docs/mvp/MVP-0b-incremental.md)
cargo build -p ravum-daemon --offline
cargo test  -p ravum-daemon --offline -- --test-threads=1
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite campaign --samples 10
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite scale    --scale-reps 5
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite invariants
```

## Règles de lecture

1. `TESTS` verts ≠ preuve. Un test réussi est un statut `tested`, jamais `proved`.
2. `build_ms` couvre codegen **et** édition de liens : rustc ne les sépare pas via
   la CLI publique. Aucun chiffre inventé pour combler ce trou.
3. Un p99 calculé sur 20 échantillons est le maximum de ces 20 échantillons.
4. La machine de référence n'est pas un environnement de qualification temps réel.
5. Les campagnes MVP-0b ont été faites sur une machine partagée avec d'autres
   agents : chaque fichier porte sa charge (`load_start`/`load_end`, CPU, mémoire
   libre, processus, calibration `rustc -vV`). Ne pas comparer deux fichiers sans
   citer la charge de chacun.
