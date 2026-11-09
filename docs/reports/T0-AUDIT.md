# T0 — Audit et durcissement du socle (rapport honnête)

> Statut : **PARTIAL en cours**. Ce rapport documente ce qui a été *exécuté* et
> observé, avec les commandes. Il ne remplace ni une revue indépendante ni un
> audit humain. Aucun ADR n'est accepté. Aucune release n'existe.

## 1. Sources de vérité relues

- `AGENTS.md`, `README.md`, `docs/architecture/PROJECT_INVARIANTS.md`,
  `docs/architecture/STATUS.md`, `docs/roadmap/MVP.md`, `docs/roadmap/BACKLOG.md`.
- `docs/reviews/2026-11-08-consolidation.md`, `docs/mvp/INTEGRATION-0abc.md`,
  `docs/mvp/MVP-0d-REPORT.md`, `MVP-0d-{incremental,overlays,batch-window}.md`.
- ADR `docs/adr/` : 43 fichiers, **tous PROPOSED** (aucune ratification par ce rapport).
- `docs/architecture/RAVUM-CORE-DESIGN.md` (décision technique candidate de cette itération).

## 2. Environnement de mesure

| Élément | Valeur |
|---|---|
| OS | Windows 11 x86-64 (machine de référence locale) |
| CPU | Intel Core i7-13620H, 10 cœurs / 16 threads |
| RAM | 15,7 Go (référence 16 Go) |
| Rust | `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`, `rustc 1.95.0 (59807616e 2026-04-14)` |
| Dépôt | `D:\App\Ravum`, branche `dev`, HEAD `5408d9b3a48dab2884e2b3eb1aa89c3c16ce3d96` |
| Arbre de travail | propre au début de l'audit |

## 3. Baseline reproduite (T0.2)

Commande :

```
cargo test --workspace -- --test-threads=4
```

Résultat brut (fichier `target/t0-baseline-tests.log`, 53 093 octets) :

- **80 suites**, **511 tests passés**, **0 échec**, **0 ignoré** ; code de sortie 0.
- Ce compteur vaut **uniquement** pour le SHA `5408d9b` ci-dessus. Il ne doit pas être
  recopié pour un autre SHA.

> Ces 511 tests sont ceux de l'outillage d'orchestration Rust existant. **Ils ne
> testent pas un compilateur Ravum** : il n'en existait pas avant cette itération.
> Les confondre serait un faux succès.

## 4. Défauts P0 de la revue précédente

| # | Défaut | État initial (reproduit dans le code) | Action et résultat |
|---|---|---|---|
| 1 | `ChangeCoordinator::publish()` avance la base sans remettre `applied` à zéro | **Confirmé** : `publish()` remplaçait `self.base` mais `self.applied` persistait → fausse conflit `ConflictPath` au cycle suivant | **CORRIGÉ** : `applied` vidé, compteurs de stockage réinitialisés, `history` conservé ; test de régression à 2 cycles et test CAS (ancienne base → `RejectedStale`, nouvelle → `Applied`) |
| 2 | `BaseSnapshot::from_files()` puis `materialize()` sans validation de chemin | **Confirmé** : clés stockées telles quelles, `write_relative` sans revalidation, `collect_files` suivait les liens | **CORRIGÉ** : validation stricte (absolu, `..`, UNC, verbatim, ADS, noms réservés Windows, longueurs), `from_files -> Result`, `materialize` revalide + canonicalise + refuse les symlinks + écriture atomique, limites 100k fichiers/1 Gio ; corpus adversarial à 3 entrées, preuve d'absence d'écriture hors racine, symlink SKIP explicite si privilège absent |
| 3 | `snapshot_after()` non branché à `GitWriter` + journal + reprise | **Confirmé** : aucun appel writer/journal dans ce chemin | **CORRIGÉ (API additive)** : `commit_cycle` (CAS base↔HEAD, oracle sur le candidat exact, `prepare/publish_request` CAS + journal durable, `publish()` seulement après succès) ; 13 tests : crash avant/après swap, rejeu idempotent, base≠HEAD, writer concurrent (1 seul publie, `git fsck` sain), hook refusé, oracle refusé sans état fantôme. Limite : crashes simulés par états durables, pas de kill OS au niveau `commit_cycle` |
| 4 | Inconnues Cargo (OUT_DIR, build scripts, optionnelles, cfg, targets, macros) | Partiellement traité par MVP-0d | **CORRIGÉ** : replis conservateurs `MultipleTargets` (hors tests/examples/bench), `NonLibCrateType`, `ProcMacroTarget`, `BuildConfig` (`.cargo/config*`, `CARGO_HOME`), `#[path]`, `include_str!/include_bytes!`, `env!`, `SourceUnreadable`, `OptionalDependency` ; 13 tests dont vrai workspace lib+bin, aucune artefact périmé au 2e run |

Preuve reproduite par le Lead : `cargo test -p ravum-snapshot -p ravum-workspace-coordinator
-p ravum-unit-graph` → **24 suites, 208 tests passés, 0 échec** (log
`target/p0-tests-lead.log`). `rustfmt --check` et `cargo check` des crates
touchés verts (rapport du worker).

### Limites résiduelles du correctif P0

- TOCTOU filesystem atténué (validation + canonicalisation + écriture atomique),
  **non éliminé** ; documenté dans le module.
- `commit_cycle` prouve la cohérence des chemins modifiés, pas que la base décrit
  tout `HEAD`.
- `RUSTFLAGS`/overrides user-level non modélisés (pré-existant).
- Campagne A/B/C/D de collaboration non exécutée ; pas de mesure de latence de bout
  en bout dans ce cycle.
- Aucune revue indépendante de ces correctifs P0 à ce stade.

## 5. CI et protections (constat)

- Workflows présents : `.github/workflows/{docs-check,mvp-impact,mvp}.yml`.
- Le SHA `5408d9b` est la tête observée de `dev` ; aucune PR ouverte au moment du
  cadrage (à revérifier si l'accès réseau est disponible).
- `main` n'est **pas** modifiée par cette itération.

## 6. Inventaire des composants réutilisables (pour le langage)

| Composant | Réutilisation prévue |
|---|---|
| `ravum-protocol` (ids content-adressés) | identités de snapshots d'analyse `.rv` |
| `ravum-snapshot` | snapshots des sources `.rv` non sauvegardées (T8) |
| `ravum-workspace-coordinator` (overlays, GitWriter, journal) | collaboration multi-agents sur un projet Ravum (T8) |
| `ravum-unit-graph` / `ravum-batch` | compilation ciblée des modules `.rv` (T8) |
| `ravum-daemon` | analyses continues LSP/IDE (T7/T8) |
| `ravum-proof` | cache de vérification (T10), **sans** revendication PROVED |
| `ravum-cli` | outil d'orchestration Rust historique ; reste séparé du compilateur `.rv` |

## 7. Risques ouverts à ce stade

- R-T0-1 : TOCTOU résiduel sur les chemins (atténué, non éliminé) — à documenter
  dans le module après correctif.
- R-T0-2 : aucune revue indépendante des correctifs P0 dans cette itération.
- R-T0-3 : la CI GitHub n'a pas été relancée sur le SHA de travail ; seules les
  exécutions locales sont des preuves.
- R-T0-4 : l'audit de licence/supply-chain n'est pas refait ici (décision humaine #34).

## 8. Ce que ce rapport ne prouve pas

Aucune garantie de sécurité générale, aucune absence de bug, aucun `PROVED`,
aucune maturité « production ». Il établit uniquement une baseline reproductible et
des défauts identifiés avec leur chemin de correction.
