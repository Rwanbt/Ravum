# ADR-017 — Codegen initial : Rust sûr comme backend de confiance limité

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-010, ADR-012.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

La création immédiate d’un backend machine et d’un borrow checker complet multiplie les risques de sémantique.

## Décision proposée

Produire du Rust sûr dans la première chaîne exécutable ; `rustc` est un validateur secondaire ; effectuer vérifications sémantiques propres ; backend direct différé.

## Alternatives à comparer / écarter

Direct LLVM dès le premier jour ; seulement transpilateur sans IR ; compiler via C non typé.

## Conséquences et compromis

+ Livraison expérimentale rapide, outillage mature. - Overhead de transpilation et double-compilation possible.

## Preuves à produire avant acceptation

Différentiels oracle entre évaluateur de référence, Rust généré et règles du langage ; mesurer builds froid/chaud.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

Le pont Rust impose un coût structurel contraire aux SLA malgré cache, ou bloque des caractéristiques nécessaires.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
