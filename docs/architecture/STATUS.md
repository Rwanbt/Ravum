# Statut architectural — Ravum (8 novembre 2026)

> **Document de situation, non un certificat de réalisation.**

| Élément | État |
|---|---|
| Charte P0-A…P0-I | Documentée, validation humaine en attente |
| ADR-000 à ADR-041 | 42 propositions originales ; **toutes PROPOSED** |
| Consolidation v0.3 | Revue multi-IA ; **REVIEW CANDIDATE**, non ratifiée |
| Compilateur Ravum / frontend / runtime | Non implémentés |
| Outil Rust MVP-0a | **Implémenté (M1), expérimental** : boucle analyse → build → vérification → hot patch, voir [docs/mvp/architecture.md](../mvp/architecture.md) |
| Hot swap natif (une entrée `f64 -> f64`) | **Démontré** par tests et démonstration ; migration d'état **non** implémentée |
| Prototype COLLAB-1 (multi-agents, écrivain Git unique) | **Implémenté et exécuté** ; ni rebase automatique, ni AST, ni CRDT |
| Vérification bornée (M2) | **Implémentée** : `Proved` limité au domaine énuméré, ledger append-only, cache invalidé par mutation ; prouveur externe **absent** (`Unsupported`) |
| DSP de bloc (M3) | **Démonstrateur** : ABI de bloc, état possédé par l'hôte, swap sans verrou, 0 allocation mesurée ; pas d'hôte audio réel, pas de crossfade |
| Verticale full-stack (M4) | **Application témoin** : UI statique, API HTTP, SQLite, contrat v1, migrations additives, E2E réels ; pas de navigateur piloté, pas de TLS |
| Benchmark, test de latence ou certification RT Ravum | Latence de la boucle M1 **mesurée localement** ; aucune qualification temps réel |
| Gate M5 pour un nouveau langage | Proposed, **pas accepté** |
| Code de release | Aucun |
| Politique de licence | À décider avant toute contribution externe de code |

## Séparation des pouvoirs

1. Les exigences utilisateur P0 ont priorité sur les propositions des modèles et les ADR.
2. Les ADR originaux sont historiques. Une modification exige un nouvel amendement traçable, une revue et l'approbation humaine.
3. Les nouvelles propositions multi-IA sont des RFC ; elles ne reçoivent pas de numéro ADR collisionnant sans validation.
4. Un `PROVED` n'existe qu'avec des hypothèses, un périmètre et une base de confiance déclarés. Aujourd'hui aucun PROVED Ravum n'a été émis.
5. Les prétentions des outils tiers ne sont pas des preuves de fonctionnement de Ravum.

## Références

- [42 ADR d'origine](../adr/README.md)
- [Consolidation des retours multi-IA](../reviews/2026-11-08-consolidation.md)
- [Feuille de route MVP](../roadmap/MVP.md)
- [Invariants proposés](PROJECT_INVARIANTS.md)
