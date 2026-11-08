# Statut architectural — Ravum (8 novembre 2026)

> **Document de situation, non un certificat de réalisation.**

| Élément | État |
|---|---|
| Charte P0-A…P0-I | Documentée, validation humaine en attente |
| ADR-000 à ADR-041 | 42 propositions originales ; **toutes PROPOSED** |
| Consolidation v0.3 | Revue multi-IA ; **REVIEW CANDIDATE**, non ratifiée |
| Compilateur Ravum / frontend / runtime | Non implémentés |
| Outil Rust MVP-0a | Non implémenté |
| Benchmark, test de latence ou certification RT Ravum | Aucun réalisé |
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
