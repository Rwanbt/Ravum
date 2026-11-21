# Intégration V2 — L2, L4, L7b et L5b

| Champ | Valeur |
|---|---|
| Destination | `claude/trusting-cerf-4hhv5a` |
| SHA de départ | `c3a96ef0d2fd4bf42aab74015a3897d4802726fb` |
| SHA du code validé | `d9fbc57a46b71010fd718f98594ae8cda878278b` |
| Environnement | Linux x86_64, rustc 1.95.0, Cargo 1.95.0, Node 24.19.0, Playwright 1.58.2 |

## Fusions

| Lot | Branche | Commit de fusion |
|---|---|---|
| L2 | `v2/L2-ir-corpus` | `be2964e` |
| L4 | `v2/L4-dsp-audio` | `7a95188` |
| L7b | `v2/L7b-async-net` | `bafe92d` |
| L5b | `v2/L5b-dsp-abi3` | `d9fbc57` |

L2 et L4 ont fusionné directement. L7b conserve les cas de conformance
déjà présents et une seule implémentation HTTP asynchrone. Le cas `det`
qui exigeait le refus en l’absence d’horloge virtuelle vérifie désormais
l’acceptation du délai virtualisé. La déclaration de l’effet `nondet`
reste contrôlée et les entrées non rejouables restent interdites.

L5b partage le chargeur ABI 3 et le tampon d’état entre l’hôte avec fondu et
l’éditeur/runner à publication concurrente. La signature ABI 3 suit
l’ordre du RFC, avec le pointeur d’état avant les buffers audio. L’ABI 2
garde son ordre historique. Le wrapper intercepte un trap de migration
dans sa bibliothèque. L’extension de statut permet au chargeur de
refuser l’état candidat avant adoption.

## Vérifications

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 tools/unsafe_audit.py
python3 tools/features_coverage.py --strict
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo --config 'profile.test.package.sha2.opt-level=3' \
  test --workspace --no-fail-fast -- --test-threads=1
cargo run -p ravum-conformance --release -- run
python3 tools/build_sqlite_native.py
NODE_PATH="$playwright_modules" node scripts/e2e-browser/notes.mjs target/debug/ravum-lang
```

- Conformance locale `TESTED` : 756 réussites, 0 échec, 2 `SKIP`.
  Dimension `--verify-build` `NOT_RUN`.
- Workspace local `TESTED` : 2 351 réussites, 0 échec, 14 ignorés.
- Navigateur `TESTED` : 11 contrôles réussis, incluant l’état SQLite et
  l’échec observé après arrêt du serveur.
- Traçabilité stricte : 248 règles sur 248. Le contrôle de confinement
  `unsafe_audit.py` a réussi. Audit indépendant `NOT_RUN`.

## Limites d’intégration

La campagne workspace utilise un checkout sur système de fichiers Linux
natif, avec l’optimisation de la seule dépendance `sha2`. Aucun seuil
n’est ajusté. Le pipeline ne lie toujours pas de crate aux programmes
Ravum générés. La connexion TCP non bloquante, les clients HTTP
asynchrones et leur campagne TSan restent hors des résultats intégrés.
La matrice des gates relève du suivi d’intégration.
