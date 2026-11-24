# Décisions du responsable — 2026-11-24 (Ravum UI)

Décisions données en conversation le 2026-11-24, enregistrées par l'agent
sans interprétation ajoutée.

- **UI-D1 — Objectif** : réaliser **Ravum UI abouti**, « pas de MVP et zéro
  dette technique », validé par des tests réels sur de petites applications.
  **Unifia est reporté** : il ne sera envisagé qu'une fois que Ravum UI aura
  fait ses preuves.
- **UI-D2 — Événements** : d'abord un modèle **à messages** (enum
  d'événements traité par une fonction `update`, type Elm/Iced). Les
  closures viendront ensuite, dans une RFC séparée.
- **UI-D3 — Voie Web** (vérifier et optimiser des sites, contributions aux
  navigateurs) : **reportée**, hors du chantier Ravum UI natif.
- **UI-D4 — Dépendances de rendu** : **banc d'essai mesuré puis décision du
  responsable**. Aucune dépendance n'est adoptée sans cette décision.
- **UI-D5 — Ordre** : langage (RFC et extensions), puis moteur natif avec
  hot reload, testé sur de petites applications, puis Verify, responsive et
  motion construits sur de vraies interfaces.

Source de l'analyse discutée : un échange externe (plan « GOAL Ravum UI »),
dont seules les parties compatibles avec ces décisions sont reprises dans
[`docs/ui/PLAN.md`](../ui/PLAN.md).
