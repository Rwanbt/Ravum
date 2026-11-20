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

## Décisions du 2026-11-20 (suite)

- **V2-N1** : `socket2` (MIT/Apache-2.0) est autorisé pour `connect_async`
  non bloquant (RFC-ASYNC-NET-1 ANET-1) ; seule exception à « aucune
  dépendance nouvelle » de V2-R3.
- **V2-S1** : STD-1.1 est appliqué : le checker refuse `io::…` (et tout
  module `std`) sans `use std::…;`. Les cas et exemples qui s'en passaient
  sont corrigés (rupture admise en phase draft, COMPAT-2.1).
- **V2-M1** : les MUST **déclaratifs** (non testables) sont comptés à part
  par `must_audit.py`, sur une liste relue par le responsable.
- **V2-K1** : `ravum-lang run` arrête le programme lancé quand il est
  lui-même interrompu.
- **V2-W1** : le workflow E2E navigateur devient bloquant (sur les PR vers
  `dev`) après un premier passage vert en CI. « Bloquant » signifie qu'un
  échec empêche la fusion, pas que Ravum serait limité au navigateur ; ce
  test fait tourner un serveur `.rv` natif servi à un navigateur, pas Ravum
  dans le navigateur (WebAssembly navigateur : non couvert).
