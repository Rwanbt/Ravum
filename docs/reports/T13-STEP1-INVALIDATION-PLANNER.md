# T13 — étape 1 : planificateur d'invalidation par item (2026-11-19)

> Gate **T13** (V1, [DECISIONS-2026-11-19-V1](../decision/DECISIONS-2026-11-19-V1.md)),
> ADR-012 / ADR-020 (`PROPOSED`). Rapport d'étape, **pas une clôture** :
> T13 reste `FAIL` tant que le checker ne réutilise pas réellement les
> résultats par item.

## Objectif et périmètre

Décider, pour deux versions d'un fichier `.rv`, **quels items doivent être
revérifiés** (types, ownership, effets) et lesquels peuvent garder leurs
résultats, de façon **fail-closed**. Nouveau crate isolé
`crates/ravum-lang-incr` ; **aucun crate existant n'est modifié** (seule la
liste `members` du `Cargo.toml` racine gagne une ligne), pour ne pas entrer en
conflit avec la session parallèle sur `dev`.

Hors périmètre de l'étape 1 : faire tourner `ravum-lang-sema` par item
(étape 2), paquets multi-fichiers (`mod x;`), branchement LSP/daemon.

## Modification

- Empreintes SHA-256 **par jetons** (type + texte, sans position) : espaces et
  commentaires n'invalident rien.
- Interface d'un item : signature (`fn` à `effects(...)` déclaré) ; signature
  **et** corps (`fn` à effets inférés, EFF-1.5) ; item entier (`struct`,
  `enum`, `const`, `extern`). Point fixe : une interface qui mentionne une
  interface changée change aussi.
- Dépendances par **nom** (sur-approximation : homonymes et masquages ne
  font qu'élargir le plan).
- Plan `Full` si : diagnostic de parsing, changement de `use`/`mod`, clé
  dupliquée, ou **règle de barrière de phase** (ci-dessous).

## Constat F1 (trouvé par l'oracle)

`ravum-lang-sema` (`check.rs`) arrête les phases effets et ownership **pour
tout le paquet** dès qu'une erreur de résolution ou de typage existe. Une
erreur dans `f` masque donc les diagnostics ownership/effets de `g`, même sans
lien entre elles. La première version du planificateur était **non sûre** sur
ces cas (l'oracle a relevé ~150 réutilisations fausses). Correction
fail-closed : `Plan::confirm_gate(old_typed, new_typed)` ne garde un plan
partiel que si les deux versions passent résolution + typage.

**Conséquence** : toute édition qui introduit une erreur de typage force
aujourd'hui une revérification complète. Lever cette limite exige que le
checker poursuive les phases par item malgré une erreur ailleurs : c'est une
modification de `ravum-lang-sema` (étape 2), à coordonner avec la session
parallèle.

## Tests

| Test | Type | Résultat |
|---|---|---|
| 10 tests unitaires (`src/lib.rs`) : identique, espaces/commentaires, corps à effets déclarés, effets inférés → appelants, signature → appelants, `struct` → propagation par signatures, suppression, parse error → Full, `use` → Full, clés de méthodes | positifs et négatifs | **10 / 0 échec** |
| `oracle::planner_is_sound_on_the_corpus` : 779 fichiers mono-fichier (`examples/`, `conformance/cases/`), 3 048 mutations (`body-ok`, `body-error`, `rename` sur chaque `fn`), vérification complète à froid des deux versions, diagnostics identiques exigés pour chaque item réutilisé | oracle de sûreté | **0 plan non sûr** |
| `oracle::negative_a_planner_that_reuses_everything_is_caught` | test de puissance (négatif) | l'oracle réfute un planificateur « tout réutiliser » |

Répartition mesurée :

| Mutation | Cas | `Full` | Partiel |
|---|---|---|---|
| `body-ok` | 1 016 | 386 | **630** |
| `body-error` | 1 016 | 1 016 | 0 (F1) |
| `rename` | 1 016 | 942 | 74 (F1) |
| **Total** | 3 048 | 2 344 | 704 — 2 602 / 12 660 items réutilisés |

Beaucoup de fichiers du corpus sont des cas d'échec volontaires (donc déjà
hors barrière) : la proportion de `Full` n'est pas représentative d'une
session d'édition réelle. Aucun chiffre de temps n'est revendiqué à cette
étape.

## Commandes

```
cargo test -p ravum-lang-incr -- --nocapture
cargo clippy -p ravum-lang-incr --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test --workspace --no-fail-fast
```

## Non-régression workspace

`cargo test --workspace --no-fail-fast` : **155 groupes, 2 207 passés / 5 échecs
/ 10 ignorés**. Les 5 échecs sont **hors du crate ajouté et préexistants** :

- 4 × `ravum-workspace-coordinator/tests/git_publication_crash.rs` (refus de
  permissions) : échouent **aussi sur `origin/dev` `9b1b6d8` non modifié**
  (5 échecs dans ce rejeu, dont `crash_before_reference_swap…`, qui ne
  reproduit pas d'une exécution à l'autre). Cause probable : le conteneur tourne en
  `root`, qui ignore les permissions refusées que ces tests simulent.
  `NOT_VERIFIED` sur une machine non-root.
- 1 × `ravum-dsp-harness/tests/dsp_stress.rs::concurrent_publications_render_whole_versions_only`
  (472 s, conteneur partagé chargé) : non rejoué sur `dev`, `NOT_VERIFIED`.

Aucun test n'a été désactivé ni modifié.

## Contexte et état des preuves

Linux x86_64 (conteneur cloud), `rustc 1.95.0 (59807616e 2026-04-14)`, branche
`claude/trusting-cerf-4hhv5a` fusionnée avec `origin/dev` `9b1b6d8`. État :
`TESTED` (oracle différentiel sur corpus), **pas** `PROVED`. Limites : mono-fichier,
dépendances par nom, barrière F1, pas encore de réutilisation effective.

## Mesure de coût par phase (2026-11-19, après fusion de `dev` `bb8d5a2`)

`cargo test -p ravum-lang-incr --release --test phase_timings -- --ignored --nocapture`
(fichier synthétique, fonctions indépendantes, conteneur Linux partagé, une
seule exécution — ordre de grandeur, pas une référence) :

| Lignes | parse | resolve | typeck | effects | own | total check |
|---|---|---|---|---|---|---|
| 603 | 1,5 ms | 0,1 ms | 0,7 ms | 0,1 ms | 0,8 ms | ~3 ms |
| 6 003 | 13,9 ms | 0,5 ms | 6,4 ms | 1,0 ms | 5,8 ms | ~28 ms |
| 30 003 | 50,2 ms | 3,7 ms | 40,0 ms | 9,6 ms | 36,9 ms | ~140 ms |

Le coût est **linéaire** (~4–5 ms / 1 000 lignes, parse compris).
`LANG-TIMINGS.md` §3 mesure déjà que `rustc` représente **97 à 99 %** du
temps de `build`. **Conclusion** : l'incrémentalité du checker (T13) ramène
quelques dizaines de ms ; le gain qui change l'expérience (secondes → ms) est
dans T15 (backend direct) et T16 (hot swap). T13 reste requis par V1, mais
l'ordre de travail proposé devient T14 → T15 → T16, T13 étape 2 en parallèle.

## Étape suivante proposée (étape 2)

1. `ravum-lang-sema` : continuer effets/ownership par item hors des items en
   erreur (lève F1) — modifie un crate partagé, **à coordonner**.
2. Checker par item avec cache de résultats indexé par empreinte ; oracle
   froid = chaud (diagnostics complets identiques).
3. Multi-fichiers via le loader ; branchement LSP ; mesure p50/p95.
