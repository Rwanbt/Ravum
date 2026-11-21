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
  **Correction (agent, 2026-11-20)** : la question reposait sur une erreur.
  Le Rust généré est compilé par `rustc` en **un seul fichier autonome**
  (`crates/ravum-lang-cli/src/pipeline.rs`, aucun `--extern`, F-META-024) :
  un programme Ravum ne peut lier **aucune** crate, `socket2` compris, sans
  une nouvelle infrastructure de liaison. V2-N1 est donc **inapplicable**
  en l'état ; nouvelle question posée au responsable.
- **V2-S1** : STD-1.1 est appliqué : le checker refuse `io::…` (et tout
  module `std`) sans `use std::…;`. Les cas et exemples qui s'en passaient
  sont corrigés (rupture admise en phase draft, COMPAT-2.1).
  **État (2026-11-20)** : appliqué **à moitié**. Fait : `use std::…;`
  ajouté à 84 fichiers `.rv` du corpus, des exemples et des paquets (correct
  dans les deux régimes) ; `std::task` ajouté aux modules importables (il
  ne l'était pas : défaut masqué par l'accès sans `use`). **Pas fait** : le
  refus dans le checker. Essayé : conformance 748/0 après correction du
  corpus, mais **109 tests Rust** de 9 crates embarquent des sources sans
  `use`, dont `ravum-lang-jit`/`ravum-lang-ir` (chantier L2, en cours dans
  une autre session) : refus retiré pour ne pas créer de conflit ; à faire
  après la fusion de L2.
- **V2-M1** : les MUST **déclaratifs** (non testables) sont comptés à part
  par `must_audit.py`, sur une liste relue par le responsable.
- **V2-K1** : `ravum-lang run` arrête le programme lancé quand il est
  lui-même interrompu.
- **V2-W1** : le workflow E2E navigateur devient bloquant (sur les PR vers
  `dev`) après un premier passage vert en CI. « Bloquant » signifie qu'un
  échec empêche la fusion, pas que Ravum serait limité au navigateur ; ce
  test fait tourner un serveur `.rv` natif servi à un navigateur, pas Ravum
  dans le navigateur (WebAssembly navigateur : non couvert).

- **V2-N2 (responsable, 2026-11-21)** : `connect_async` utilise un thread
  auxiliaire de runtime (amendement de RFC-ASYNC-NET-1 §4) ; ouvre
  `http::get_async` / `post_async`.
- **V2-M1, question (2026-11-21)** : le responsable a demandé pourquoi les
  80 règles ne sont pas testables ; réponse donnée (négations de garantie,
  procédures humaines, définitions), avec la réserve que certaines (par ex.
  MACH-2.2) sont limites. La liste reste **PROPOSED**.
- **V2-M1 (responsable, 2026-11-21)** : tri confirmé ; la liste
  déclarative passe à 63 règles **ACCEPTED** ; ABI-4.7 amendée (liste
  fermée de lints, « Amender la règle »). Réécritures de MACH-9.1, CONC-3.1,
  ABI-5.1 proposées dans `docs/reports/V2-M1-DECLARATIVE-REVIEW.md` §D.
- **V2-M1, réécritures (responsable, 2026-11-21)** : MACH-9.1, CONC-3.1 et ABI-5.1 remplacées par les textes du §D de la revue (état actuel : machine de référence, blocs DSP `rt`, ABI DSP versionné).
- **STD-1.4 (responsable, 2026-11-21)** : `fs::read_to_string` et `fs::write_string` n'ont pas l'effet `io` ; `stdlib.md` et `effects.md` alignés sur le checker.
- **FEAT-1.5 (responsable, 2026-11-21)** : règle réécrite pour décrire le format réel des cas (`case.json` + programme), imposé par `tools/conformance`.
