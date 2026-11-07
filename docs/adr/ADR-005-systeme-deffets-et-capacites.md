# ADR-005 — Système d’effets et capacités

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-002, ADR-004.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Sécuriser frontend/backend/secrets/audio suppose connaître les accès externes et les opérations dangereuses.

## Décision proposée

Inférer les effets internes, déclarer les capacités externes et le niveau d’autorité ; l’appel ne peut accroître des permissions implicites.

## Alternatives à comparer / écarter

Conventions de nommage ; permissions globales ; sandbox sans analyse de code.

## Conséquences et compromis

+ Principe du moindre privilège et règles realtime réutilisées. - Annotation FFI, polymorphisme d’effets.

## Preuves à produire avant acceptation

Tests compile-fail sur fuite de secret vers frontend, effect escalation, appel bloquant indirect, chemins dynamiques.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Précision d’inférence insuffisante sur code générique ou polymorphe.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
