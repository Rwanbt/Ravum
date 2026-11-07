# ADR-007 — Arithmétique : entiers, flottants et fast-math

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-003, ADR-004.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

L’optimisation change parfois les résultats flottants ; les débordements peuvent provoquer des erreurs.

## Décision proposée

Entiers checked par défaut pour opérations à risque, wrapping/saturating explicites ; IEEE défini en mode strict, tolérance bornée explicite en mode approximation ; fast-math opt-in contractuel.

## Alternatives à comparer / écarter

Overflow implicite ; optimisations flottantes agressives par défaut ; interdiction totale SIMD.

## Conséquences et compromis

+ Reproductibilité et contrats auditables. - Certaines optimisations exigent dérogation.

## Preuves à produire avant acceptation

Suites IEEE NaN/Inf/-0, différentiels multi-arch, contrats d’erreur en ULP/absolu/relatif, tests de profils SIMD.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Des benchmarks démontrent un coût prohibitif avec gains de sûreté marginaux.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
