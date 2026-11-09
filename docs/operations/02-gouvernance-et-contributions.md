# Gouvernance, responsabilités, RFC/ADR et règles de contribution

## Autorité et statut

- Un ADR n'est **accepté** qu'après discussion des alternatives, expérience reproductible et décision explicite du mainteneur ; l'état actuel est `PROPOSED`.
- Une RFC est requise pour modifier sémantique, ABI, règles d'ownership, statuts de preuve, modèle de capacités, migration, versions majeures des schémas d'agents.
- Une PR d'implémentation doit référencer au moins une exigence et un ADR ; une décision technique non approuvée reste expérimentation isolée.
- Maintenir historique `SUPERSEDED` et migration, jamais écraser un ADR ancien en lui substituant discrètement une autre décision.

## Rôles (personnes à désigner ; une personne peut assurer plusieurs rôles, mais pas auto-auditer un P0)

- **Mainteneur / owner** : priorité du produit et acceptation d'ADR, changement des obligations métier.
- **Architecte compilateur** : frontend/MIR/cache/codegen.
- **Responsable formal methods** : soundness, SMT, modèles, TCB.
- **Responsable runtime/temps réel** : patcher, allocations, deadlines.
- **Responsable sécurité** : menaces, secrets, FFI/unsafe, supply chain.
- **Responsable CI/qualif** : tests exact SHA, performance, matrice de cibles.
- **Agent IA** : contributeur sans autorité normative ni contrôle de release.

## Règles Git et CI

- `main` protégée, PR requises, branches limitées à un objectif ; préférer PR ≤400 LOC de code hors tests/docs, sauf dérogation motivée.
- Pas de merge auto sans CI de gate applicable, review indépendante pour `unsafe`, parser, typechecker, solver encodeur, cache de preuve et hot patch.
- Conserver l'implémentation de tests dans les PR ; état RED avant GREEN documenté pour corrections et nouvelles garanties.
- Jobs attachés au commit SHA exact et à la version de politique ; pas de « previous green » accepté pour nouveau SHA.
- Dépendances versionnées, license scan, SBOM sur release ; pas de secrets en CI.

## Politique d'essais et de rejet

- Un spike à résultat défavorable peut être « terminé » si reproductible, documenté, et si la décision de repli est claire.
- La métrique qualité n'est jamais le nombre de lignes de code ou de PR, mais les exigences démontrées et la réduction des risques.
- Les obligations non démontrées ne sont pas supprimées du tableau de bord.

## Dossier minimum d'une PR critique

`Intent`, `Requirement IDs`, `ADR IDs`, `Threat model delta`, `Tests red`, `Tests green`, `Toolchain SHA`, `Artifacts hashes`, `Bench baseline`, `Unproven obligations`, `Reviewer signoff`, `Rollback plan`.

## Gestion des releases

Versionnement du langage et de la stdlib séparé si besoin ; matrice cible x fonctionnalités et niveaux de garantie ; notes de breaking changes; contract de compatibilité du format de preuves ; procédure de révocation de preuves/artefacts en cas de bug du compilateur ou solver. Aucun badge marketing ne vaut preuve de qualification.

## Protection effective des branches (9 novembre 2026)

Configuration vérifiée par lecture de l'API GitHub et test réel de refus.

| Règle | `main` | `dev` |
|---|---|---|
| PR obligatoire | oui | oui |
| Approbations requises | 0 (mainteneur solo) | 0 (mainteneur solo) |
| Check requis | `mvp` (produit) | `mvp` (produit), ajouté le 2026-11-09 |
| Force push | interdit | interdit |
| Suppression de branche | interdite | interdite |
| Résolution des conversations | — | obligatoire |
| Administration exemptée | non (`enforce_admins = false`) | non |

Preuve de refus : sur une branche canari protégée par la même configuration, un
force push et une suppression ont été refusés par GitHub (`GH006: Cannot
force-push to this branch`, `GH006: Cannot delete this branch`). La branche canari
a ensuite été supprimée après retrait de sa protection.

### Promotion `dev` → `main`

Aucune promotion n'est automatique et aucune n'a eu lieu. La procédure requise est :

1. une PR `dev` → `main` explicite, jamais un push direct ;
2. le check produit `mvp` vert sur le SHA exact de la tête de PR ;
3. la suite complète, les démonstrations M1/M3/M4 et l'E2E navigateur exécutés
   sur ce SHA, avec preuves publiées ;
4. aucune conversation non résolue ;
5. un **go/no-go humain** explicite : la fusion dans `main` reste une décision du
   responsable, hors du mandat d'un agent.

Une CI verte sur une branche de travail ne vaut pas validation de `main` : c'est le
SHA promu qui doit avoir été validé.
