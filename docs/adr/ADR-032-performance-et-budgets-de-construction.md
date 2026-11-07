# ADR-032 — Performance et budgets de construction

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-012, ADR-018.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

« Compilation instantanée » et « très légère » doivent être mesurables.

## Décision proposée

SLO exploratoires et reproductibles sur hardware de référence : diagnostics locaux p95 <50 ms ; petite UI patch p95 <150 ms ; petite fonction chaude p95 <500 ms ; budgets RAM/CPU profilés ; aucun engagement sans baseline.

## Alternatives à comparer / écarter

SLA marketing sans périmètre ; optimiser compilateur sans mesurer app ; ignorer coût énergétique.

## Conséquences et compromis

+ Décisions fondées sur mesures. - Variance matériel et test harness complexe.

## Preuves à produire avant acceptation

Bench froid/chaud et p50/p95/p99, RSS/peak CPU/IO, taille, latence de preview et énergie si mesurable ; comparer Rust/Cranelift/rust-analyzer.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Targets impossible à atteindre sur matériel ciblé après enquête et budgets décidés.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
