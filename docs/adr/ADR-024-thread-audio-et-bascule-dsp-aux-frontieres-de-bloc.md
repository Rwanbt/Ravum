# ADR-024 — Thread audio et bascule DSP aux frontières de bloc

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-006, ADR-021, ADR-022.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Un thread audio ne peut compiler, allouer, verrouiller ou attendre une migration.

## Décision proposée

Préparer patch, buffers et état sur thread non RT ; frontière de bloc avec swap non bloquant, anciennes ressources retirées ailleurs ; crossfade optionnel borné.

## Alternatives à comparer / écarter

Compilation dans callback ; mutex bloquant ; migration heap sur thread audio.

## Conséquences et compromis

+ Latence audio prévisible. - Coût de double état et découplage complexe.

## Preuves à produire avant acceptation

Callback instrumenté, compteurs allocation/locks, stress hot swap, xruns sur device réel, analyse worst-case.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Décrochages persistants dus au mécanisme même de swap.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
