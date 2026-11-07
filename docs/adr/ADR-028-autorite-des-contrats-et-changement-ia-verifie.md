# ADR-028 — Autorité des contrats et changement IA vérifié

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-027.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Un agent IA peut faire passer ses tests en affaiblissant une spécification.

## Décision proposée

Contrats séparés par statut verrouillé : obligations normatives signées/contrôlées par humain ; agent peut proposer mais pas accepter suppressions, affaiblissement ou changement d’hypothèse.

## Alternatives à comparer / écarter

Agent propriétaire de toutes les preuves ; signatures de contrats non tracées ; autorisation au cas par cas ad hoc.

## Conséquences et compromis

+ Régression contractuelle bloquée. - Plus de workflow pour changer légitimement un contrat.

## Preuves à produire avant acceptation

Tests injection d’agents modifiant préconditions/skip/CI, fail closed, diff sémantique et audits.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

La gouvernance devient trop lourde pour de petits projets : profils d’autorité à définir.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
