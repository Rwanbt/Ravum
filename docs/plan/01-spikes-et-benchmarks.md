# Plan expérimental — spikes, critères falsifiables et benchmarks

**Règle** : les chiffres ci-dessous sont **hypothèses de recherche** et ne sont pas présentés comme des mesures réelles. Toutes les expériences exigent baseline, SHA, toolchains, machine, OS, commandes et logs. La répétabilité compte autant que la performance.

## Matériel de référence et protocoles

Choisir deux classes de machines : **léger** (PC portable / 16 Go RAM, 4–8 threads utiles) et **standard** (32 Go / 8–16 threads). Renseigner CPU, gouverneur énergétique, stockage, température, OS, load/background ; exécuter 30 itérations chaudes + 10 à froid, reporter p50/p95/p99 et variabilité. Rien ne dit que ces cibles sont déjà atteintes. Pour RT, ajouter plateforme hôte audio et mesure de XRUN.

**Métriques obligatoires** : délai frappe→diagnostic / patch prêt / UI visible, pic RSS, RSS idle, CPU idle/burst, octets réécrits, taille binaire, débit/latence runtime, allocations et locks RT, énergie si mesurable, durée solver, hit ratio et exactitude d'invalidation.

| Spike | Question falsifiable | Expérience | Critères de sortie |
|---|---|---|---|
| S1 — parser/queries | Peut-on analyser un petit edit sans reparser toute l'unité ? | parser CST + Salsa / parser entier baseline ; edits artificiels et corpus réel | oracle identique cold/warm, baisse calculs mesurée, pas de stale diagnostic |
| S2 — preuve incrémentale | Le cache de preuves reste-t-il correct sous mutations hypothèses ? | 100 mutations de types, axiomes, dépendances, flags, solveurs ; solver recalcul froid comme oracle | 0 faux `PROVED` ; ROI cache quantifié |
| S3 — backend | Rust transpile vs Cranelift direct améliorent-ils délai et coût total ? | même IR jouet, kernels DSP, data et API ; vérifier sémantique | choisir par correctness et p95/RSS, non par hypothèse |
| S4 — hot swap | Appels in-flight et état muté restent-ils sûrs ? | machine patch versionnée avec 10k séquences, aborts, threads concurrents | 0 UAF / invalid ABI détectés ; fallback documenté |
| S5 — audio | Patch précompilé évite-t-il allocation/blockage audio ? | DSP réel + RT instrumentation, hot-swap en charge | zéro appel interdit détecté, xruns/latences mesurés ; pas de promesse hard-RT |
| S6 — solver | Quel mix Z3/cvc5/Kani/Verus est pertinent ? | corpus contrat math, mémoire, concurrence, FFI | portée des preuves, faux succès=0, coût et unsupported |
| S7 — fullstack | Interfaces client/server/SQL restent-elles cohérentes ? | petit service WASM + Postgres/SQLite + migration N/N-1 | tests contractuels + E2E, drift bloqué |
| S8 — agent IA | Diagnostic structuré réduit-il cycles sans abaisser contrat ? | même corpus bugs avec interface texte vs API structurée | correction réelle, nombre d'itérations, coût, altération contrat=0 |
| S9 — déterminisme | Peut-on rejouer des races/pannes systématiquement ? | réseau, timer, queue, RNG et simulateur ; 100 seeds | replays bit-identiques sur environnement verrouillé, limites annoncées |
| S10 — optimizer | Une transformation qui change la sémantique est-elle refusée ? | mutants NaN/overflow/aliasing, differential+SMT | mutants détectés, rejets `UNKNOWN`, coût mesuré |

## Seuils exploratoires de réactivité

- `analyze` locale p95 < 50 ms pour edit 1 ligne dans projet préchargé.
- Patch simple UI p95 < 150 ms jusqu'à affichage (sans migration ou gros link).
- Petite fonction chaude p95 < 500 ms jusqu'à codegen ; separate métrique activation visible.
- CPU idle cible : quasi nul après stabilisation ; RAM idle et pic **quantifiés et ajustables** selon profil.

Les budgets exacts de RAM et le PC de référence doivent être approuvés dans la revue F0. Les seuils sont des **cibles** : aucun dépassement n'est masqué.

## Falsification et protocole d'échec

Si un spike échoue : attacher logs, reproducer minimal, profil, analyse du goulot, ADR affectés, alternative moins risquée, coût pour pivoter. Aucun biais de benchmark (cache volontairement préchauffé seulement pour un candidat, tests différents, hardware non comparable). Rapports même quand la conclusion est No-Go.

## Résultats attendus sous dossier `evidence/`

`experiment-id/revision/target/{environment.json,commands.txt,stdout.log,stderr.log,metrics.json,verdict.json,artifacts.sha256,coverage.json}`. Les preuves de performance se mesurent ; les propriétés formelles exigent un rapport solver + hypothèses.
