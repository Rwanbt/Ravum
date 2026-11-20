# Décisions du responsable — 2026-11-19

> Enregistrement d'une décision **énoncée par le responsable** en session
> (2026-11-19). L'agent l'enregistre ; il ne l'étend pas. Les gates proposés en
> §2 sont des **propositions** (`PROPOSED`) tant que le responsable ne les a
> pas validés.

## V1 — Définition de « production ready » (D2)

**Énoncé du responsable** : « le production ready doit avoir tous les points
d'architecture prévus implémentés et validés ».

**Conséquence directe** : D2 ne peut pas être prononcé tant que les décisions
d'architecture ci-dessous ne sont pas implémentées **et** validées par
mesure/tests négatifs, au SHA, dans le langage `ravum-lang-*` (pas seulement
dans la plateforme Rust).

### Conflit avec des décisions antérieures — à trancher

| Décision antérieure | Contenu | Conflit avec V1 |
|---|---|---|
| **A2** (2026-11-14) | Bloc DSP / hot swap Ravum reporté à la 1.1 | ADR-021/024 sont des points d'architecture prévus |
| **A3** (2026-11-14) | Backend natif voie C hors 1.0 | ADR-018 (backend rapide) est un point prévu |

**V1-Q1 — TRANCHÉE (2026-11-19)** : oui, **A2 et A3 sont annulées** ; bloc DSP/hot swap Ravum et backend direct (voie C) reviennent dans le périmètre 1.0.

## 2. Gates V1 — **VALIDÉS par le responsable (2026-11-19)**, inscrits au registre

| Gate | ADR | Critère de validation proposé | État mesuré au 2026-11-19 |
|---|---|---|---|
| T13 — Incrémental par requêtes | ADR-012, ADR-020 | Typage/ownership/effets recalculés **par fonction** ; modifier le corps d'une fonction sans changer sa signature ne revérifie qu'elle (compteur testé) ; oracle froid = chaud | `FAIL` : seul le parsing est mis en cache (`ravum-lang-lsp/src/analysis.rs`) |
| T14 — IR stable | ADR-010 | IR versionnée, sérialisable, consommée par ≥ 2 backends | `FAIL` : `--emit-ir` = dump de débogage |
| T15 — Backend de développement rapide | ADR-018 | Backend direct (Cranelift ou équivalent) ; oracle différentiel 0 divergence vs voie B ; gain mesuré | `NOT_STARTED` |
| T16 — Hot swap Ravum | ADR-021, ADR-022, ADR-024 | Swap atomique d'une fonction `.rv` dans un processus vivant aux safepoints ; migration d'état déclarée ou refus ; 0 alloc callback RT ; p95 édition→activation mesuré | `FAIL` : `watch` = redémarrage |
| T17 — Déterminisme | ADR-016, ADR-007 | Profil/effet déterministe vérifié ; replay par seed bit-à-bit | `NOT_STARTED` |
| T18 — Contrats | ADR-004, ADR-028 | `requires`/`ensures`/invariants vérifiés (test/fuzz/preuve bornée) avec statut honnête | `NOT_STARTED` |
| T19 — API agent | ADR-027, ADR-040 | Liens diagnostic→contrat→preuve→test en JSON versionné | `PARTIAL` : diagnostics JSON à codes stables |

Les ADR restants (000–041) doivent être passés en revue texte intégral pour
compléter cette table ; état actuel de cette revue : `NOT_REVIEWED`.

## V1-L1 — Licence de Cranelift (2026-11-19)

**Énoncé du responsable** : « oui autorise la licence ». `deny.toml`
`[licenses].allow` reçoit `Apache-2.0 WITH LLVM-exception` (Cranelift, backend
JIT de T15). **Non couvert par cette décision** : les 3 doublons de versions
(`hashbrown`, `bitflags`, `windows-sys`) refusés par `[bans]
multiple-versions = "deny"` ; `cargo deny check` reste rouge sur ce seul point
tant que le responsable ne l'a pas tranché.

## V1-L2 — Doublons de versions tirés par Cranelift (2026-11-19)

**Énoncé du responsable** : « oui autorise les doublons ». `deny.toml`
`[bans].skip` liste les **versions exactes** `hashbrown@0.14.5`,
`hashbrown@0.15.5`, `bitflags@1.3.2`, `windows-sys@0.52.0` (tirées par
Cranelift 0.116 / regalloc2 / region). `multiple-versions = "deny"` reste en
vigueur pour tout autre doublon.


## V1-M1 — `migrate` (HR-1.7) sur la voie JIT : migration côté éditeur

Décision du responsable (2026-11-19), choisie parmi trois options (côté
éditeur ; sur le thread audio ; garder le refus) : **côté éditeur**.

- Le thread temps réel ne fait qu'une copie de ses mots d'état dans une zone
  d'atomiques préallouée, puis échange des `Box` à l'adoption : ni
  allocation, ni libération, ni exécution de `migrate` (AGENTS.md, INV-05).
- Écart assumé vis-à-vis de HR-1.2 : les échantillons traités entre
  l'instantané et l'adoption ne font pas partie de l'état migré (mesuré :
  un bloc, 64 échantillons ≈ 1,3 ms à 48 kHz). Le journal de session (T17)
  enregistre les deux instants, donc le replay reste exact.
- Implémentation et preuves : [rapport T16 §9](../reports/T16-NATIVE-HOT-SWAP.md).

## V1-D1, V1-S1, V1-C1 — crates partagés, DIAG-1.4, contrats (2026-11-19)

Décisions du responsable, choisies parmi les options présentées :

- **V1-S1** : l'autre ligne de travail est terminée ; les crates partagés
  (`ravum-lang-sema`, `ravum-lang-syntax`) peuvent être modifiés sur cette
  branche, `dev` étant fusionnée avant chaque étape.
- **V1-D1 (DIAG-1.4)** : *conformer le checker* à la spec — une erreur d'une
  phase précoce n'empêche les phases suivantes que sur le nœud fautif, pas
  sur tout le paquet (constat F1 de T13). Les sorties de diagnostics
  changent en conséquence.
- **V1-C1 (T18)** : contrats `requires`/`ensures` **vérifiés à
  l'exécution** (trap si violé), statut honnête `TESTED`, aucune preuve
  statique ; une RFC d'abord, jamais auto-acceptée.

## V1-C2, V1-T2, V1-P1 (2026-11-20)

- **V1-C2** : [RFC-CONTRACTS-1](../rfc/RFC-CONTRACTS-1.md) est **ratifiée**
  (`ACCEPTED`). Les preuves restent `TESTED`.
- **V1-T2** : T13 doit être complet malgré un gain mesuré modeste :
  identifiants de nœuds stables, fusion des faits, plusieurs fichiers, et
  branchement sur le LSP et `watch`.
- **V1-P1** : prochain chantier prioritaire, la couverture du JIT (`String`,
  `Vec`, `Result`, génériques).

## V1-C3, V1-C4, V1-A1 (2026-11-20)

- **V1-C3 (T19)** : amender RFC-CONTRACTS-1 (CON-5/CON-6) : le message de
  violation montre les valeurs des arguments scalaires et chaînes, ex.
  `contract violated: requires b != 0 (a = 1, b = 0)` ; préfixe inchangé ;
  identique sur rustc et le JIT.
- **V1-C4 (T18)** : invariants de type sous la forme
  `struct S { .. } invariant <expr>`, vérifiés à chaque construction
  (littéral) et à la sortie de chaque méthode `&mut self` ; RFC d'abord,
  puis rustc et JIT.
- **V1-A1** : hôte audio = une abstraction, une implémentation fichier WAV
  testable dans le conteneur, et une implémentation `cpal` derrière une
  feature désactivée par défaut (à tester sur la machine du responsable).
