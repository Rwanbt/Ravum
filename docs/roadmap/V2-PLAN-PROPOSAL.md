# Proposition de plan V2 — gates `PARTIAL` restants

| Champ | Valeur |
|---|---|
| Statut | **PROPOSED** — à valider par le responsable avant tout code |
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
| 4 | T6, T8, T9 | les critères écrits semblent atteints ; la raison du `PARTIAL` n'est pas écrite dans la matrice : **NOT_REVIEWED** | relire les rapports, écrire le critère de sortie manquant, puis proposer `PASS` ou le travail restant |

## Questions au responsable

1. Valider (ou réordonner) la liste B.
2. Pour T6/T8/T9 : quel était le critère de sortie attendu ?

Rien ne sera marqué `PASS` sans preuve exécutée et décision du responsable.
