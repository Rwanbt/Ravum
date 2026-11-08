# M0 — Baseline, corpus et protocole de mesure

> **Statut : partiellement satisfait.** Cette page enregistre ce qui a été
> réellement observé et ce qui manque encore pour déclarer le gate M0 atteint.
> Aucun chiffre n'est extrapolé d'une autre machine.

## 1. Contexte d'exécution

| Champ | Valeur observée |
|---|---|
| Date | 2026-11-08 |
| Branche de travail | `feat/3-mvp0a-continuous-loop` (depuis `dev` via `main`, voir §7) |
| Commit de base | `16b1ff2adc11289f1648f42fcf52014542cb30fb` |
| OS | Windows 11 Professionnel, 10.0.26100, build 26100 |
| Modèle | Pulse 15 B13VGK (machine de référence 16 Go) |
| CPU | 13th Gen Intel Core i7-13620H, 10 cœurs physiques / 16 logiques |
| RAM totale | 15,71 Go |
| RAM libre pendant la session | 2,0 – 2,3 Go (charge utilisateur non contrôlée) |
| Disque de travail | `D:` — 55 Go libres |
| Rustc | `rustc 1.95.0 (59807616e 2026-04-14)` |
| Cible | `x86_64-pc-windows-msvc` |
| LLVM | 22.1.2 |
| Git | 2.51.1.windows.1 |
| Édition Rust | 2021 (`rust-version = 1.74` dans le workspace) |

**Conséquence directe de la RAM libre :** aucun build parallèle n'est lancé.
`CARGO_BUILD_JOBS=2` et `cargo test --test-threads=1` sont la configuration de
référence. Ce n'est pas une optimisation, c'est une contrainte de mesure : lancer
dix `rustc` simultanés avec 2 Go libres mesurerait la pagination, pas le code.

## 2. Dépendances externes

Toutes déclarées dans `Cargo.toml` du workspace et verrouillées par `Cargo.lock`
(versionné). Aucune dépendance à une API privée de `rustc` (INV-11).

| Crate | Rôle | Frontière |
|---|---|---|
| `serde`, `serde_json` | protocoles JSON versionnés | sérialisation uniquement |
| `sha2` | identités de contenu SHA-256 | aucune décision sémantique |
| `libloading` | chargement natif multiplateforme | isolé dans `ravum-patch-host` |

Le reste est la bibliothèque standard. Aucun solveur, aucun runtime, aucun cadre
d'interface : il n'y a pas encore de consommateur réel pour eux (INV-10).

## 3. Les cinq identités (RAV-005)

Elles sont **cinq types distincts**, pas cinq usages d'une même chaîne. Voir
`docs/mvp/architecture.md` §2.1 pour le calcul exact, et
`crates/ravum-protocol/src/ids.rs` pour l'implémentation. Le compilateur Rust
refuse de passer l'une à la place de l'autre.

## 4. Corpus de référence

### 4.1 Fixtures positives

| Fichier | Contenu | Usage |
|---|---|---|
| `fixtures/calculator/v1.rs` | `ravum_compute(x) = x * 2` | version initiale du démonstrateur M1 |
| `fixtures/calculator/v2.rs` | `ravum_compute(x) = x * 2 + 1` | patch compatible, comportement observable différent |
| `fixtures/latency/slow_v1.rs` | dort 400 ms pour l'entrée sentinelle 99.0 | test des appels en vol |
| `fixtures/latency/slow_v2.rs` | idem + décalage de 1000 | test des appels en vol |
| `fixtures/collab/two_functions.rs` | deux fonctions indépendantes | COLLAB-1 / COLLAB-2 puis compilation du résultat fusionné |
| `fixtures/collab/notes.rs` | constantes indépendantes | proposition sur base périmée |

### 4.2 Corpus négatif (résultat attendu explicite)

| Fichier | Attaque simulée | Résultat attendu | Test |
|---|---|---|---|
| `fixtures/negative/broken_type.rs` | erreur de type | refus à l'analyse, aucune compilation | `invalid_source_is_refused_before_any_build` |
| `fixtures/negative/abi_mismatch.rs` | ABI native 999 | `AbiMismatch` avant résolution du symbole | `incompatible_native_abi_is_refused` |
| `fixtures/negative/signature_mismatch.rs` | signature `i64->i64` | `SignatureMismatch` | `incompatible_logical_signature_is_refused` |
| `fixtures/negative/missing_export.rs` | `ravum_compute` absent | `MissingExport` | `missing_export_is_refused` |
| artefact périmé | build depuis un ancien snapshot | `StaleBuild` | `stale_build_is_refused` |
| artefact non vérifié | pas de preuve | `NotVerified` | `activation_without_evidence_is_refused` |
| même artefact | patch déjà actif | `AlreadyActive` | `building_the_active_version_twice_is_refused_as_redundant` |

Chaque refus est vérifié **deux fois** : le statut renvoyé, et le fait que la
valeur observée par l'appelant n'a pas changé.

## 5. Protocole de mesure

1. **Étapes mesurées séparément** : `check_ms`, `build_ms`, `verify_ms`, `load_ms`,
   `activate_ms`, `total_ms`.
2. `check` = `rustc --emit=metadata` (aucun code machine).
3. `build` = `rustc --crate-type=cdylib` : **codegen et édition de liens sont dans
   le même processus**. Aucun découpage n'est inventé pour faire joli.
4. `verify` = chargement réel de l'artefact + 5 entrées × 2 appels (finitude et
   déterminisme).
5. `load` = `LoadLibraryW` + résolution des symboles + validation du manifeste.
6. `activate` = publication atomique du pointeur de version.
7. **Percentiles** : rang le plus proche (`ceil(p/100 × n)`), pas d'interpolation.
   Avec 20 échantillons, `p99` est le maximum de l'échantillon : c'est écrit dans
   `evidence/m1-bench.json`, pas caché.
8. **Aucun échantillon écarté** : si une itération échoue, elle est comptée comme
   refus et le code de sortie n'est pas zéro.
9. Source alternée (`v1` / `v2`) pour éviter un `AlreadyActive` artificiel.

## 6. Ce que le gate M0 exige encore

| Élément | État |
|---|---|
| Corpus et corpus négatif versionnés | **fait** |
| Baseline machine + toolchain + SHA enregistrés | **fait** |
| Identités distinctes documentées et implémentées | **fait** |
| Protocole de mesure reproductible | **fait** |
| CI produit (build + tests + démonstrations) | **fait** (`.github/workflows/mvp.yml`) |
| Matrice de traçabilité P0/ADR/tests complète (#24) | **partielle** |
| Audit formel des 42 ADR (#25) | **non fait** |
| Décision de licence (#34) | **non faite — décision humaine requise** |
| Pilote de productivité IA S8-A/S8-B (#33) | **non exécuté** |
| Protection de branche (#77) | **non configurée — action administrateur** |
| Critères du gate M5 pré-enregistrés (#36) | **partiels** (voir `docs/mvp/M5-criteria.md`) |

Le gate M0 n'est donc **pas** déclaré atteint. Ce qui est reproductible l'est ;
le reste est listé sans être maquillé.

## 7. Écart de branche assumé

La convention du dépôt demande une branche créée depuis `dev`. Au moment de
l'ouverture, `dev` était l'**ancêtre direct** de `main` (`7841102`) et `main`
contenait un commit d'outillage supplémentaire (`16b1ff2`). Brancher depuis `dev`
aurait supprimé ces fichiers d'outillage de l'arbre de travail.

La branche `feat/3-mvp0a-continuous-loop` part donc de `main`, ce qui **contient
intégralement `dev`** (`git merge-base --is-ancestor 7841102 16b1ff2`). Aucun
travail de `dev` n'est absent. Cet écart est documenté ici pour être vérifiable.

## 8. Reproduire

```powershell
cargo build --workspace
powershell -ExecutionPolicy Bypass -File scripts/run-tests.ps1
powershell -ExecutionPolicy Bypass -File scripts/demo-m1.ps1
powershell -ExecutionPolicy Bypass -File scripts/demo-collab.ps1 -Commit
cargo run -q --bin ravum -- bench fixtures/calculator/v1.rs fixtures/calculator/v2.rs --iterations 20 --out evidence/m1-bench.json
```

Les résultats observés sont consignés dans `docs/mvp/M1-evidence.md` et
`evidence/m1-bench.json`.
