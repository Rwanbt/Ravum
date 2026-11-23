# Décisions du responsable — 2026-11-23 (suite du plan V2)

Décisions données en conversation le 2026-11-23, enregistrées par l'agent
sans interprétation ajoutée.

- **V2-G1 — Passage en `PASS`** : « tu peux passer en pass ce qui est
  implémenté et testé de manière déterministe ». Appliqué dans
  `docs/roadmap/CORE-1.0-GATE-MATRIX.md` à **T2, T6, T8, T9, T17**
  (`PASS (V2)`), chacun avec les tests qui le portent et ses limites
  restantes dans la colonne « écart ». Un gate n'est passé qu'après
  vérification de la preuve et ajout des tests qui manquaient :
  - T6 : ANET-T2, T3, T4 et T6 n'existaient pas ; ils ont été écrits ;
  - T8 : le test ABI 3 jouait une copie du bloc ; il joue désormais
    `examples/dsp-gain-iir` lui-même ;
  - T9 : l'E2E navigateur n'était lancé qu'à la main ; il tourne désormais
    sur chaque PR.
  Restent non `PASS` : T1 (choix du backend, en attente), T4, T10, T15
  (audits externes), T11 (clé de signature, D9), T12 (Go/No-Go), T14
  (améliorations IR/JIT en cours).
- **V2-G2 — Dette T2** : « il faut corriger la dette technique, je veux un
  langage propre et sans aucune dette technique ». Appliqué :
  - deux lignes de tableau de la spécification (MACH-4.2, EXPR-9.4)
    tronquaient leur texte normatif à cause d'un `|` non échappé ;
  - un faux refus du checker a été corrigé (boucle dans un bloc de bras de
    `match`) ;
  - un `ravum.lock` versionné était périmé.
  Les 63 MUST déclaratifs ne sont pas de la dette : aucun programme ne peut
  les faire échouer, et leur tri a été accepté le 2026-11-21 (V2-M1).
- **V2-G3 — Audits externes** (T4, T10, T15) : ils auront lieu **en
  dernier**, après la mise en production. La mise en production elle-même
  suivra des essais de réalisation d'applications avec Ravum.
- **V2-G4 — Améliorations** : « toute amélioration est bonne à prendre ».
  Les améliorations IR/JIT de T14 sont en cours (voir le rapport L2).
- **V2-G5 — PR #203** : revue et fusion autorisées en l'absence de
  bloquant. La revue n'a trouvé ni test supprimé, ni seuil affaibli, ni
  `unsafe` hors inventaire, ni secret, et la fusion vers `dev` était sans
  conflit, CI verte. La PR a été fusionnée (`578af68`).
- **T1 et T11** : explications données en conversation. Aucune décision n'a
  encore été prise ; D9 reste **différée** jusqu'à la mise en production.
