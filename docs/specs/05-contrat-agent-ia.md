# Spécification exploratoire — intégration native des agents IA

## Objectif

Réduire le nombre de cycles `générer→compiler→échouer→réessayer`, sans donner à l'IA autorité pour affaiblir les exigences ou certifier ses propres changements.

## Séparation des responsabilités

- **Humain / mainteneur** : définit et signe les contrats normatifs, la politique de sécurité, les niveaux d'acceptation et les changements de périmètre.
- **Agent** : propose implémentations, optimisations, invariants supplémentaires, tests, expériences et corrections ; justifie les changements.
- **Compilateur/vérificateur indépendant** : retourne résultat typé, preuves conditionnelles, contre-exemples et statut de confiance.
- **Runner isolé** : exécute tests et workloads sous quotas de ressources et capacités autorisées.
- **Système de versionnage/CI** : constate le SHA effectivement testé ; rejette résultats venant d'un autre SHA.

## Protocole candidat (exemples fictifs)

Actions publiques `analyze_changes`, `check`, `verify`, `get_diagnostic`, `get_counterexample`, `compare_versions`, `benchmark`, `prepare_patch`, `activate_patch`, `get_evidence`. Chaque requête contient `workspace_id`, `revision`, `target`, `profile`, `budgets` et porte une `trace_id` corrélable.

```json
{
  "protocol_version": "0.1-draft",
  "workspace_revision": "sha256:SOURCE_REVISION",
  "target": "x86_64-linux-gnu",
  "profile": "verified",
  "property_id": "module::transfer::no_underflow",
  "property": "result >= 0",
  "status": "DISPROVED",
  "trust": {"method":"symbolic","tool":"solver-adapter","version":"PINNED"},
  "counterexample": {"balance": 0, "amount": 1},
  "source_span": {"file":"src/payments.ul","start_byte":120,"end_byte":164},
  "assumptions": [],
  "replay_id": "sha256:REPLAY_FIXTURE"
}
```

Ce JSON est **une cible de conception** et non un résultat réel. Le contrat effectif sera décrit par JSON Schema versionné ; chaque statut valide comporte champs obligatoires propres, source URI sécurisée et limites de taille.

## Contrat d'intention de modification

L'agent soumet : `goal`, `allowed_paths`, `forbidden_changes`, `required_properties`, `perf_targets`, `evidence_plan`, `expected_risks`. Toute demande de changement du contrat normatif est déplacée dans une proposition de RFC/ADR et bloquée jusqu'à approbation ; aucun `git apply` ne peut contourner le contrôle de la PR.

## Optimisation sous contraintes

L'agent génère candidats A/B/C, chacun évalué sur EXACTEMENT la même suite, même profil, même machine/charge/OS. Les garanties normatives prouvées doivent rester satisfaites ; les métriques p95/p99 ne sont pas des preuves de bornes absolues. Le meilleur candidat n'est choisi qu'après test statistique/variabilité et vérification de correctness.

## Sécurité agent

- Modifications soumises à revue indépendamment de son propre score.
- Contrats verrouillés hors droits agent ; permission d'écriture limitée aux répertoires ciblés.
- Scripts de dépendances et outils tournent sous sandbox ; aucun secret d'exécution transmis par défaut.
- L'agent ne peut ni forcer merge, ni marquer CI green manuellement, ni supprimer tests de sécurité.
- Spécifier le comportement sur `UNKNOWN`, `TIMEOUT`, `UNSUPPORTED`, `ASSUMED` : **ne pas présenter la tâche comme validée**.

## Mesures de l'expérience IA

- Taux de correctifs réellement corrects sur corpus de bugs connus.
- % de contrats inchangés/renforcés, 0 affaiblissement non autorisé.
- Coût tokens + coût CPU/RAM/temps pour tâche vérifiée.
- Nombre de boucles, taux de diagnostics utilisables et répétitions inutiles.
- Régressions redécouvertes après plusieurs modifications.
