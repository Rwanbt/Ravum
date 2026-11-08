# AGENTS.md — Mandat permanent des agents de développement Ravum

> Lisez ce fichier **avant de modifier une ligne**. Il n'autorise aucune acceptation d'ADR, suppression de tests, publication ou déploiement sans instructions explicites du responsable.

## Contexte et autorité

Ravum est un **projet d'exploration**, pas encore un compilateur fonctionnel. Le langage universel reste l'objectif à long terme, et l'architecture F (plateforme Rust outillée) est une **stratégie expérimentale** conditionnelle, non une substitution permanente à l'objectif. Les décisions ADR-000…041 sont `PROPOSED`.

Sources à lire, dans cet ordre :
1. `README.md` et `docs/architecture/PROJECT_INVARIANTS.md` (P0-A à P0-I).
2. `docs/architecture/STATUS.md` (réalité actuelle).
3. `docs/roadmap/MVP.md`, `docs/roadmap/BACKLOG.md`, issue assignée.
4. ADR originaux touchés dans `docs/adr/` **texte intégral**.
5. `docs/reviews/2026-11-08-consolidation.md` pour les alternatives et désaccords, **pas** comme source d'ADR déjà acceptés.

Ne pas deviner à partir des titres. Quand un texte manque, indiquer `NOT_REVIEWED`.

## Obligations de mission

- Priorité n°1 : **temps total d'édition/correction jusqu'à livrable validé**, incluant humains et agents ; ne pas optimiser seulement un codegen.
- Compiler/analyser seulement ce qui a changé, mais ne **jamais désactiver ni supprimer** les tests d'intégration/E2E pertinents en échec.
- Consommer peu de RAM et CPU (référence primaire 16 Go), scheduler annulable et résultats de snapshots périmés rejetés.
- Sécurité mémoire et profils d'effets explicites ; `unsafe` / FFI explicitement audités.
- Toute preuve doit déclarer modèle, assumptions, toolchain, version, contexte ; `UNKNOWN`, `TIMEOUT`, `ASSUMED`, `TESTED`, `PENDING`, `STALE` **ne sont pas** `PROVED`.
- Ne jamais affaiblir contrats, validations ou seuils pour obtenir du vert ; proposer le changement et demander une approbation.
- Source incomplète ou patch incompatible : conserver ancienne version saine ; mécanisme fail-closed.
- Thread audio : aucun travail de compilation, allocation, blocage, migration ou destruction coûteuse au callback.
- Architecture indépendante des API privées de rustc ; chaque abstraction a un consommateur réel et une stratégie de retrait.

## Git et autorité

- `main` : branche par défaut et référence documentaire, **pas une preuve de maturité produit**.
- `dev` : branche d'intégration ; nouvelles branches depuis `dev`, PR vers `dev`.
- Ne pas forcer une branche, ne pas supprimer de travail d'autres agents, ne pas fusionner/déployer sans autorisation.
- Les nouveaux ADR ont d'abord des identifiants RFC pour éviter les collisions ; jamais d'auto-acceptation.

## Définition de terminé / rapport

Chaque ticket comporte : objectif, périmètre, P0/ADR, modification, tests positifs et **négatifs**, source SHA, versions, OS/matériel si benchmark, commandes, limites et état des preuves. Faire passer les tests disponibles, puis rapporter honnêtement les échecs.

Le MVP-0a n'a pas à créer d'emblée un frontend Ravum, une IR complète, un linker propriétaire, un solveur maison, un IDE ou un framework UI. Il doit démontrer la boucle source→analyse→patch avec refus sûr, mesurer la valeur et garder la possibilité d'une future sémantique autonome.

**Aucun résultat de benchmark ni de vérification Ravum n'est encore établi.**
