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
