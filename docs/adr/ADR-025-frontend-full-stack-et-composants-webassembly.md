# ADR-025 — Frontend full-stack et composants WebAssembly

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-023, ADR-005.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Un seul langage doit partager types UI/API sans prétendre supprimer les frontières de sécurité.

## Décision proposée

Interfaces de composants typées, générateur WASM/JS interop, serveur SSR facultatif ; WIT/WASI quand prise en charge cible validée ; authentification/autorisation côté serveur obligatoires.

## Alternatives à comparer / écarter

Tout dans navigateur ; protocole RPC opaque ; une VM propriétaire client obligatoire.

## Conséquences et compromis

+ Contrats de transport et portabilité. - APIs DOM et outils navigateur varient.

## Preuves à produire avant acceptation

E2E Chromium/Firefox/WebKit, hydration/SSR, requêtes interdites client, XSS/CSP/CSRF tests.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le modèle composant existant ne couvre pas des besoins essentiels sans lourds polyfills.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
