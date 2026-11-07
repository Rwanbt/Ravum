# ADR-000 — Périmètre universel et garanties conditionnelles

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : Aucune.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Universel ne peut signifier simultanément prouvable sans hypothèse, sans runtime et optimisé pour tout matériel.

## Décision proposée

Un cœur sémantique commun et des profils cibles optionnels ; toute garantie est liée à un profil, un modèle, une version et un périmètre de confiance.

## Alternatives à comparer / écarter

Langage/runtimes distincts par domaine ; VM universelle obligatoire ; promesse générale sans exclusions.

## Conséquences et compromis

+ Cohérence et sincérité des garanties. - Documentation des hypothèses et compatibilités plus exigeante.

## Preuves à produire avant acceptation

Matrice de cibles et contrats de domaine ; deux programmes (audio no_std et serveur+web) passent les critères de leur profil.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Un profil cible démontre qu’il ne peut respecter les propriétés du noyau commun.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
