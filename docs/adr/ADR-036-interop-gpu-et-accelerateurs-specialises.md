# ADR-036 — Interop GPU et accélérateurs spécialisés

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-007, ADR-024.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

L’universalité suppose GPU/IA/SIMD mais les modèles de mémoire et d’effets diffèrent.

## Décision proposée

Extensions cibles versionnées (WebGPU/SPIR-V/CUDA selon besoin), noyaux purs et frontières de synchronisation vérifiables ; pas de GPU obligatoire pour le cœur.

## Alternatives à comparer / écarter

Backend GPU unique pour tout ; shader compiler universel garanti ; code CPU exécuté sans restriction sur GPU.

## Conséquences et compromis

+ Extensions sans contaminer sûreté du noyau. - Complexité de cibles et preuve flottante.

## Preuves à produire avant acceptation

Bench kernels vectoriels et coût transfert, tests numériques multi-cibles, sécurité mémoire shader et comparaison oracle CPU.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Bénéfice nul vs bibliothèques existantes ou modèles de preuves irréconciliables.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
