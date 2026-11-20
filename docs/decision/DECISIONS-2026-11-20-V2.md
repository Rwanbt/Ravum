# Décisions du responsable — 2026-11-20 (plan V2)

- **V2-P1** : ordre du plan V2 validé (T17 → T14 → T2 → T8/T9 → T6), à
  exécuter en **sessions parallèles** d'outils différents ; plan dans le
  dépôt : `docs/plan/v2/V2-PARALLEL-PLAN.md`, prompt de lancement :
  `docs/plan/v2/LAUNCH-PROMPT.md`.
- **V2-D1** : une RFC « ABI DSP 2 » (état DSP au-delà de `{ z1, z2 }`) est
  rédigée (`PROPOSED`) ; aucune implémentation avant ratification.
- **V2-E1** : l'E2E navigateur (T9) fait partie de la V2 (chantier L6).

## Ratifications (2026-11-20)

- **V2-R1** : `RFC-DETERMINISTIC-PROFILE-1` **ACCEPTED**, avec l'enregistrement
  et le rejeu des E/S (§2.3, DET-9 à DET-15) demandés par le responsable.
- **V2-R2** : `RFC-DSP-ABI-2` **ACCEPTED** (ABI 3, état opaque déclaré par le module).
- **V2-R3** : `RFC-ASYNC-NET-1` **ACCEPTED** ; la dépendance `mio`/`polling`
  (§5 point 4) n'est **pas** décidée : aucune dépendance nouvelle.
