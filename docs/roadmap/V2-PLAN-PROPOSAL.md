# Proposition de plan V2 — gates `PARTIAL` restants

| Champ | Valeur |
|---|---|
| Statut | **Validé** par le responsable le 2026-11-20 (V2-P1, V2-D1, V2-E1) — plan d'exécution : `docs/plan/v2/V2-PARALLEL-PLAN.md` |
| Date | 2026-11-20 |
| Source | `docs/roadmap/CORE-1.0-GATE-MATRIX.md` au commit `bd5a732` |

Le plan V1 (T13, T16, T18, T19 → `PASS (V1)`) est clos, sauf l'audit externe
des sites `unsafe` (D4/R13), action humaine. Cette note classe les gates
encore `PARTIAL`/`BLOCKED` et propose un ordre. Elle ne change aucun état.

## A. Bloqués par une décision ou une action humaine (aucun code ne les clôt)

| Gate | Ce qui manque | Qui |
|---|---|---|
| T1 | décision ADR sur les voies A/B | responsable |
| T4 | audit externe du typechecker / borrow checker | auditeur externe |
| T10 | audit externe (qualité) | auditeur externe |
| T11 | D9 : garde de la clé de signature | responsable |
| T12 | Go/No-Go | responsable |
| T15 | audit externe du backend (licence déjà décidée V1-L1) ; dossier prêt : `docs/audit/UNSAFE-AUDIT-DOSSIER.md` | auditeur externe |

## B. Travail d'implémentation possible (proposé)

| Ordre | Gate | Écart mesuré | Travail proposé |
|---|---|---|---|
| 1 | T17 | seule l'étape 1 (replay d'une session hot swap) existe | étape 2 : replay de session `watch` complète (éditions source → diagnostics → activations), digests comparés ; refus identiques |
| 2 | T14 | corpus IR **55/61** ; JIT limité aux structs `Copy` | couvrir les 6 programmes restants (lister chacun et sa construction manquante), structs non-`Copy` dans l'IR/JIT |
| 3 | T2 | **95 MUST sans FeatureId** (`must_audit.py`) | rattacher chaque MUST à un FeatureId et un cas, sans exemption nouvelle |
| 4 | T8 + T9 | colonne « écart » : vertical `examples/dsp-gain-iir` non branché sur l'hôte ; état DSP limité à `{ z1, z2 }` (ABI 2 non ratifiée) ; fondu non mesuré sur un bloc `.rv` ; E2E navigateur absent | brancher `dsp-gain-iir` sur l'hôte audio (`ravum-lang-audio`, WAV en CI), mesurer le fondu ; l'ABI 2 et l'E2E navigateur demandent une décision |
| 5 | T6 | `connect` TCP bloquant, pas d'HTTP asynchrone | `connect` asynchrone puis HTTP asynchrone (RFC d'abord) |

## Questions au responsable

1. Valider (ou réordonner) la liste B.
2. T8 : ratifier l'ABI 2 de l'état DSP (au-delà de `{ z1, z2 }`) ou la laisser hors V2 ?
3. T9 : l'E2E navigateur est-il dans le périmètre V2 ?

Rien ne sera marqué `PASS` sans preuve exécutée et décision du responsable.
