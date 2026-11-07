# Spécification exploratoire — compilation permanente, preview et hot swap

## Invariants obligatoires

**INV-01** : source invalide ou version obsolète n'est jamais activée. **INV-02** : preview n'active que build vérifié pour son profil. **INV-03** : toute activation identifie l'ABI, codehash, contrats, fonctionnalités et dépendances. **INV-04** : un appel déjà engagé dans ancienne version ne saute pas sur code désalloué. **INV-05** : rollback ne revendique pas d'annuler les effets externes irréversibles. **INV-06** : mises à jour realtime préparées hors callback.

## Chemin d'édition

1. Client IDE ou agent émet `workspace_revision`, `buffer_version`, `edit`, `intent` ; le serveur vérifie version monotone et droit d'écriture.
2. Reparse incremental des nœuds affectés ; erreurs tolérées seulement dans l'analyse/diagnostics.
3. Les unités closes et valides alimentent HIR/MIR ; invalidation des requêtes exactement concernées.
4. Analyses obligatoires (types/ownership/effets), puis `verify(required_policy)` sur obligations bloquantes.
5. Codegen des unités réévaluées, liaison avec API de module stable et gestionnaire de versions.
6. `prepare` : charger patch en staging, exécuter migrations & tests spéciaux, valider signatures, capacités et état.
7. `commit`: changement atomique des points d'entrée à safepoint si préparé/autorisé ; `abort` sinon et garder la dernière version sûre.
8. Garbage collection des **versions de code** hors thread critique après disparition des appels in-flight ; ne pas confondre avec un GC mémoire applicatif.

## Machine à états du patch

`EDITING -> PARSED -> TYPECHECKED -> REQUIRED_VERIFIED -> BUILT -> STAGED -> READY -> ACTIVE`, avec branches d'erreur vers `REJECTED`; un `ACTIVE` précédent reste valide. Si preuve requise `UNKNOWN/TIMEOUT`, patch en `PENDING_PROOF` mais non activé en mode strict.

## Conditions de compatibilité

- Même signature, ABI, alignement, disposition et conventions d'appel, sauf adaptateur/migration vérifié.
- Pointeurs et références vers types modifiés ne sont jamais réinterprétés aveuglément.
- Les ressources externes persistent avec version de schéma/contrat distincte du snapshot mémoire.
- Exigences de contrats réévaluées si code ou hypothèses changent, jamais sur seule similitude d'empreinte visuelle.

## Fonctionnalités de preview

- UI/style/assets : fast path sans relier tout le binaire lorsque sémantiquement sûr ; éventuellement mécanisme JS/CSS standard, sans prétendre que ce code relève de la même garantie.
- Backend : process/version isolée, router de requêtes ou hot patch strict si ABI compatible.
- Audio : double version prête, swap RT safe à frontière bloc, migration préparée hors callback, crossfade si besoin.
- DB : un hot swap de code ne déclenche **jamais** automatiquement une migration destructive en prod.

## Cache & débit

Le daemon maintient graphe de tâches annulables, priorise diagnostics de frappe, interdit les jobs périmés, applique TTL/LRU quotas. Tests contre oracle full cold sur modifications de source et contexte. Les benchmarks mesurent séparément parse, type, proof, codegen, link, activation, rendu et p95/p99.

## Risques de hot reload

- Changement d'état non migrable → refuser patch / redémarrage contrôlé.
- Activation interrompue → old version encore active ; shadow state rejeté.
- Deux agents éditent même module → CAS sur révision + merge explicite ; pas de last-write-wins silencieux.
- Optimisation ou inlining traverse frontière patchable → imposer barrière/point d'indirection, versionner et mesurer coût.
- Anciennes versions de code s'accumulent → reclaim epoch/RCU hors RT, quotas, stratégie de stop-the-world contrôlé hors domaines stricts.

## Contrats testables

- 10 000 séquences d'édition fuzzées ne doivent jamais activer AST invalide.
- Tests d'invalidation cold vs warm sur changement de signature publique et axiome de preuve.
- Concurrence : remplacement pendant appel long, rollback après échec migration et perte de liaison.
- Mesure mode idle, caps RAM/CPU, kill daemon/restart, versions stale avec événements network out-of-order.
- Report exact du cas où une preview exige un redémarrage.
