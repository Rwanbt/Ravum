# T13 étape 2 — vérification incrémentale par fonction (2026-11-19)

> Gate **T13** (ADR-012/020, V1). Décisions V1-S1 (crates partagés) et V1-D1
> (DIAG-1.4). État des preuves : `TESTED` par oracle, **pas** `PROVED`.

## Objectif

Après une édition, ne pas revérifier le corps des fonctions inchangées, et
rendre **exactement** les diagnostics d'une vérification complète.

## Modification

1. **DIAG-1.4 conforme** (commit `9c2e5e2`) : une erreur de résolution ou de
   typage n'arrête plus effets et ownership que dans sa fonction (constat F1
   de l'étape 1 levé). Une erreur hors de toute fonction arrête encore tout.
2. **Checker** (`ravum-lang-sema`) :
   - `check_reusing` saute le corps des fonctions désignées et reprend leurs
     effets directs et leurs appelées (`Facts::reuse_bodies`) ;
   - il rend aussi ce que le pilote met en cache : effets directs, graphe
     d'appels, imports utilisés par corps, fonctions `&'static str`.
3. **Pilote** `ravum-lang-incr::driver::check_incremental` :
   - il garde, par fonction, ses diagnostics (positions relatives), ses
     effets directs, ses appelées (par clé d'item), ses imports utilisés et
     son statut `&'static str` ;
   - à l'édition suivante, le planificateur de l'étape 1 désigne les corps à
     revérifier ; les autres sont réutilisés, et leurs diagnostics remis à la
     nouvelle position de la fonction.
4. **Dépendances entre fonctions trouvées par l'oracle**, puis corrigées :
   - **imports inutilisés** (`W-RES-UNUSED-IMPORT`, global) : chaque import
     compte ses usages, le typeur attribue les usages au corps vérifié, et un
     corps réutilisé remarque les siens ;
   - **noms des variables d'inférence** (`$k` dans un message) : numérotés
     depuis le début de la fonction, et non depuis le début du paquet ;
   - **retour `&'static str`** (point fixe sur tous les corps, lu par
     l'ownership des appelants) : le statut réutilisé est repris ; s'il change
     pour une fonction revérifiée, tout est revérifié.
5. **Fail-closed** : tout est revérifié quand le planificateur ne peut pas
   planifier, quand il y a une erreur de parse, quand une erreur tombe hors de
   toute fonction (arrêt global), ou quand une note de diagnostic pointe hors
   de sa fonction (cette fonction n'est alors pas mise en cache).
6. **Performance** : `fingerprint_source` parcourait tous les tokens pour
   chaque item (quadratique : 1,75 s pour 3 000 fonctions) ; recherche
   binaire désormais (41 ms). Le pilote ne parse le fichier qu'une fois.

## Tests

| Test | Résultat |
|---|---|
| `tests/cold_warm.rs` : 833 fichiers du corpus, 3 348 éditions (corps valide ajouté, corps invalide ajouté, fonction renommée), puis retour à l'original depuis le cache chaud | **diagnostics incrémentaux = vérification complète** (code, sévérité, message, position, notes), **0 divergence** sur 6 696 vérifications ; 6 886 corps réutilisés, 4 733 revérifiés |
| historique | première version : **136** divergences (imports), puis **10** (`$k`, `&'static str`), puis **0** |
| `tests/oracle.rs` (étape 1) | toujours 0 plan non sûr |
| checker : DIAG-1.4 | erreur de typage d'une fonction ne masque plus l'ownership d'une autre ; **négatifs** : même fonction toujours masquée, erreur hors fonction arrête tout |

## Mesure

`cargo test -p ravum-lang-incr --release --test cold_warm measure -- --ignored --nocapture`.
Une fonction modifiée, médiane de 5, conteneur partagé 4 cœurs :

| Fichier | Vérification complète | Incrémentale | Corps revérifiés |
|---|---|---|---|
| 2 400 lignes | 11,8 ms | 6,9 ms | 1 |
| 12 000 lignes | 71 ms | 41 ms | 1 |
| 36 000 lignes | 262 ms | 169 ms | 1 |

Le coût restant est fixe : lexing, parse et empreintes du fichier entier.
Comme l'étape 1 l'avait mesuré, le checker n'est pas le goulot de la boucle
d'édition (T15/T16 le sont) : le gain est de l'ordre de 35–40 %.

## Limites

- **Un seul fichier** : pas de `mod x;`, et le loader n'est pas branché ; le
  LSP et `watch` n'utilisent pas encore ce pilote.
- **Diagnostics seulement** : quand des corps sont réutilisés, les faits sont
  incomplets ; la génération de code passe toujours par une vérification
  complète.
- Le parse et les empreintes restent ceux du fichier entier.
