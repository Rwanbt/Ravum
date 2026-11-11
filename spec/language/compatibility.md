# Ravum Core 1.0 — Versionnage, compatibilité et retour arrière

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §2, §3.5, §5 ; [`modules.md`](modules.md) §7–§8 ; [`diagnostics.md`](diagnostics.md) §9.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).

## 1. Objets versionnés

| Objet | Identifiant | Portée | Stabilité en `core-1.0-draft.1` |
|---|---|---|---|
| Langage | édition `core-1.0` | syntaxe, typage, ownership, effets | **draft** : peut changer sans préavis |
| Spécification | `core-1.0-draft.N` | l'ensemble de `spec/language/` | draft, N incrémente à chaque révision |
| Bibliothèque standard | incluse dans l'édition | API de [`stdlib.md`](stdlib.md) | draft |
| Packages | `MAJOR.MINOR.PATCH` + `path` | [`modules.md`](modules.md) §8 | résolution exacte, pas de plage |
| Lockfile | `lockfile_version = 1` | [`modules.md`](modules.md) §7 | fail-closed sur version inconnue |
| Diagnostics JSON | `schema_version: 1` | [`diagnostics.md`](diagnostics.md) §7 | additif seulement |
| Codes de diagnostic | `E-*`, `W-*` | [`diagnostics.md`](diagnostics.md) §3 | immuables, jamais réutilisés |
| CLI | options de `ravum-lang` | design §2 | non stable (draft) |
| ABI | **aucun** | [`abi.md`](abi.md) | néant |

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-1.1 | Un artefact (rapport, binaire, cache) **MUST** déclarer la version de spécification (`core-1.0-draft.1`) et la version de l'outil. | MUST |
| COMPAT-1.2 | Aucun document ni rapport ne doit présenter `draft` comme `stable`. | MUST |
| COMPAT-1.3 | Une édition inconnue de `ravum.toml` est refusée `fail-closed` (`E-UNSUPPORTED-EDITION`). | MUST |
| COMPAT-1.4 | Un `lockfile_version` inconnu est refusé (`E-MOD-LOCK`), jamais interprété partiellement. | MUST |

## 2. Politique de rupture du langage

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-2.1 | Pendant la phase **draft**, toute règle peut changer. Un changement est journalisé dans `spec/language/` (version `draft.N+1`) avec la liste des programmes affectés. | MUST |
| COMPAT-2.2 | Après ratification d'une édition (`core-1.x`) : **aucune** rupture de sémantique, de syntaxe acceptée, ni de diagnostic attendu n'est permise dans la même édition majeure. | MUST |
| COMPAT-2.3 | Une rupture **MUST** créer une nouvelle **édition majeure** (`core-2.0`) et laisser `core-1.x` supportée le temps d'une migration documentée. | MUST |
| COMPAT-2.4 | Est **rupture** : retirer/renommer une fonctionnalité ; changer le sens d'un programme accepté ; changer la sémantique d'un opérateur ; changer un code de diagnostic ou sa sévérité ; rendre obligatoire une annotation auparavant optionnelle ; accepter un programme auparavant refusé **si** cela change le comportement d'un programme existant. | MUST |
| COMPAT-2.5 | N'est **pas** une rupture : ajouter une fonctionnalité qui était `UNSUPPORTED` ; ajouter une fonction à `std` ; ajouter un champ **optionnel** au JSON ; améliorer un message ; ajouter un code de diagnostic (les consommateurs doivent tolérer les codes inconnus). | MUST |
| COMPAT-2.6 | Une fonctionnalité `UNSUPPORTED` peut devenir supportée dans une édition mineure **si et seulement si** aucun programme accepté n'utilisait la forme refusée (elle était refusée, donc c'est sûr par construction). | MUST |
| COMPAT-2.7 | Le statut `candidate-stable`, `experimental`, `unsupported` de [`features.md`](features.md) est **contractuel** : passer de `experimental` à `candidate-stable` est une décision, pas une formalité. | MUST |
| COMPAT-2.8 | Aucun mécanisme de « compatibilité silencieuse » : pas de mode qui accepte l'ancienne et la nouvelle sémantique. Une édition = une sémantique. | MUST |
| COMPAT-2.9 | Renommer un `rule_id` ([`diagnostics.md`](diagnostics.md) DIAG-9.6) ou un `feature_id` est une **rupture** : ces identifiants sont contractuels. | MUST |

## 3. Compatibilité de la bibliothèque standard

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-3.1 | L'ajout d'une fonction/méthode/variante est **additif** et compatible. | MUST |
| COMPAT-3.2 | Le retrait, le renommage, ou un changement de signature/effet d'une API `std` est une **rupture majeure** (`core-2.0`). | MUST |
| COMPAT-3.3 | Un changement de **comportement** (ex. `write_string` devenant atomique, `exists` rendant `Result`) est une rupture même si la signature est compatible. | MUST |
| COMPAT-3.4 | Les messages d'erreur (`Error::message`) ne sont **pas** contractuels : les changer n'est pas une rupture (STD-1.7). | MUST |
| COMPAT-3.5 | Les effets d'une fonction `std` sont **contractuels** : ajouter `alloc` ou `io` à une fonction `effects()` est une rupture (elle casse des programmes `pure`/`rt`). | MUST |
| COMPAT-3.6 | Aucun mécanisme de dépréciation implicite : une API dépréciée est documentée, reste fonctionnelle jusqu'à l'édition majeure suivante, et n'émet **aucun** avertissement (liste `W-*` fermée). | MUST |

## 4. Compatibilité des packages

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-4.1 | En Core 1.0, une dépendance est un **chemin** : la compatibilité est celle du source effectivement présent. Aucune plage de versions, aucun solveur ([`modules.md`](modules.md) MOD-8.4). | MUST |
| COMPAT-4.2 | Le champ `version` d'un package sert à l'**identification** et au lockfile, pas à la résolution. Deux versions différentes du même nom sont un conflit (`E-MOD-DEPENDENCY`). | MUST |
| COMPAT-4.3 | Changer le `path` d'une dépendance sans changer son `name`/`version` est légal mais **change le digest** : `check --json` expose `dependencies_digest` et `lockfile_stale`. | MUST |
| COMPAT-4.4 | Un package **MUST NOT** dépendre d'une édition différente de la sienne : `edition` est unique par package et doit valoir `core-1.0` ([`modules.md`](modules.md) MOD-5.2). | MUST |
| COMPAT-4.5 | Le contenu compilé est **toujours** recompilé depuis la source ; aucun cache binaire partagé entre versions n'est un contrat ([`abi.md`](abi.md)). | MUST |

## 5. Compatibilité du JSON et du CLI

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-5.1 | `schema_version` reste `1` tant qu'aucun champ obligatoire n'est retiré/renommé/retypé. Ajouter un champ optionnel ne l'incrémente pas. | MUST |
| COMPAT-5.2 | Un consommateur **MUST** ignorer les champs inconnus et tolérer des codes de diagnostic inconnus. | MUST |
| COMPAT-5.3 | Un producteur **MUST NOT** retirer un champ, changer son type, ni changer le sens d'un champ existant sans incrémenter `schema_version`. | MUST |
| COMPAT-5.4 | Les codes de sortie `0/1/2/3` sont **contractuels** pour les commandes de compilation ; `run` propage le code du programme ([`diagnostics.md`](diagnostics.md) §8). | MUST |
| COMPAT-5.5 | Les options CLI peuvent changer pendant la phase draft ; un script de CI doit vérifier `ravum-lang --version` et échouer si la version majeure change. | SHOULD |

## 6. Migration

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-6.1 | Toute migration **MUST** être un **patch de source** proposé, revu et testé, jamais une réécriture silencieuse. Une IA peut proposer un correctif, pas modifier un contrat (INV-02, SEC-006). | MUST |
| COMPAT-6.2 | La procédure est : (1) `ravum-lang check --json` sur l'ancienne édition, (2) liste des diagnostics de rupture, (3) patch proposé, (4) `check`/`test` sur la nouvelle édition, (5) comparaison des résultats observables, (6) revue humaine. | MUST |
| COMPAT-6.3 | Une migration ne **MUST NOT** modifier les contrats normatifs (`effects`, signatures publiques, invariants) sans approbation explicite. | MUST |
| COMPAT-6.4 | Aucun outil de migration automatique n'existe en Core 1.0 (`ravum-lang migrate` n'est pas une commande). Une migration est manuelle/assistée. | MUST |
| COMPAT-6.5 | La migration d'un package multi-modules **MUST** conserver l'arbre de modules et les digests documentés, ou expliquer chaque changement. | MUST |

## 7. Retour arrière (rollback)

| ID | Règle | Niveau |
|---|---|---|
| COMPAT-7.1 | Le retour arrière est **fail-closed** : si une source, un manifeste ou un lockfile est invalide, la version saine précédente est conservée ; aucun état partiel n'est activé. | MUST |
| COMPAT-7.2 | Le rollback d'édition consiste à repointer `edition = "core-1.0"` (ou l'édition précédente) et à restaurer les sources ; le lockfile doit être cohérent. | MUST |
| COMPAT-7.3 | Un rollback ne **MUST NOT** prétendre annuler des effets externes irréversibles (fichiers écrits, sorties consommées) : il restaure du code, pas le monde (design, INV-05 de `docs/specs/03`). | MUST |
| COMPAT-7.4 | Le cache de compilation peut être vidé (`ravum-lang cache clean`) ; aucun résultat de cache n'est une preuve et un cache périmé ne doit jamais être réutilisé. | MUST |
| COMPAT-7.5 | Un rollback **MUST** être enregistré avec : version quittée, version restaurée, raison, SHA des sources, commandes exécutées, résultat de `check`/`test`. | MUST |
| COMPAT-7.6 | Aucun rollback ne peut réintroduire une construction refusée par l'édition cible (`UNSUPPORTED`) : le refus reste. Les formes supportées dans l'édition restaurée (`unsafe { }`, FFI scalaire) restent régies par ses règles, jamais par celles d'une édition plus permissive. | MUST |

```text
// POSITIF — rollback traçable
1. editer ravum.toml : edition = "core-1.0"   (retour depuis une edition refusee)
2. restaurer les sources (git, hors de ce perimetre)
3. ravum-lang check --json   -> status success
4. ravum-lang test --json    -> 0 echec
5. consigner SHA + versions + sorties brutes

// NÉGATIF — interdit
- activer un binaire dont `check` a echoue
- reutiliser un cache dont le digest ne correspond plus
- annoncer « rollback reussi » sans avoir rejoue check/test
```

## 8. Matrice de compatibilité (résumé)

| Changement | Langage | `std` | Packages | JSON | Verdict |
|---|---|---|---|---|---|
| Nouvelle fonction `std` | — | additif | — | — | compatible |
| Nouveau code `E-*` | additif | — | — | additif | compatible |
| Nouveau champ JSON optionnel | — | — | — | additif | compatible |
| Effet ajouté à une fonction `std` | — | rupture | — | — | **majeur** |
| Signature `std` modifiée | — | rupture | — | — | **majeur** |
| Syntaxe `UNSUPPORTED` → supportée | mineur | — | — | — | compatible |
| Sémantique d'un opérateur modifiée | rupture | — | — | — | **majeur** |
| Code de diagnostic renommé | rupture | — | — | rupture | **majeur** |
| `lockfile_version` inconnu | — | — | refus | — | fail-closed |
| `edition` inconnue | refus | — | refus | — | fail-closed |

## 9. Questions ouvertes

- **Q-COMPAT-1** — La phase `draft` n'a pas de politique de changement formalisée (qui décide, quel préavis). Recommandation : un journal `spec/language/CHANGELOG` (absent) et une règle « toute rupture de draft incrémente `draft.N` ». À créer.
- **Q-COMPAT-2** — Faut-il supporter **deux** éditions simultanément (compilation d'un package `core-1.0` par un outil `core-2.0`) ? Le design ne le prévoit pas ; une réponse positive exige une matrice de compatibilité et des tests croisés. Recommandation : non en Core 1.0.
- **Q-COMPAT-3 (résolue par le Lead)** — Tous les codes anciennement `reserved` sont devenus canoniques ; il n'y a plus de code réservé. Leur émission est un ajout compatible au sens de DIAG-9.5.
- **Q-COMPAT-4** — Le versionnage de `spec_version` (`core-1.0-draft.1`) n'est pas relié à un hash de commit. Pour la traçabilité (INV-04), un champ `spec_commit` serait utile dans le JSON. Non retenu (schéma `1` figé) ; à trancher avant ratification.
- **Q-COMPAT-5** — Aucun mécanisme de dépréciation visible (COMPAT-3.6) : une API dépréciée est seulement documentée. Un champ JSON `"deprecated": true` sur les faits serait plus utile que des warnings. À considérer.
