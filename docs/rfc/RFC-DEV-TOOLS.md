# RFC-DEV-TOOLS-1 — Outils de vérification locaux (Miri, cargo-mutants)

> **Statut : `PROPOSED` — `PENDING_HUMAN_APPROVAL`.**
> Cette RFC ne modifie ni le langage, ni la sémantique, ni les gates. Elle
> demande l'autorisation d'installer **localement** deux outils déjà utilisés par
> la CI, pour rejouer localement les deux dernières qualifications `NOT_RUN`.

## 1. Problème

La règle R5 du GOAL interdit d'ajouter une dépendance ou un outil sans RFC et
approbation. Deux vérifications restent donc `NOT_RUN` **localement** :

| Vérification | Pourquoi elle manque localement | Ce qui existe déjà |
|---|---|---|
| **Miri** (GOAL 9.5) | `cargo-miri` n'est pas installé pour la toolchain nightly locale (`error: 'cargo-miri.exe' is not installed…`) | Le workflow `linux-qual.yml` installe le composant `miri` et exécute syntax (65 tests), sema (134 tests) et le fuzz court sous Miri ; run vert sur `878e212` |
| **Mutation** (GOAL 9.3) | `cargo-mutants` n'est pas installé localement | Le workflow `mutation.yml` installe `cargo-mutants 27.1.0` et rejoue la campagne ciblée ; run vert sur `b5298db` (183 mutants / 150 tués / 24 survivants / 9 non viables) |

Ces deux preuves CI existent, mais elles sont **Linux** et ne remplacent pas un
rejeu local sur la machine de référence (Windows 11, `rustc 1.95.0`) : les
chiffres de Miri et de mutation sont liés à l'environnement, et le GOAL demande
des preuves reproductibles avec commande, SHA et machine.

## 2. Proposition

Autoriser, **pour la vérification uniquement**, l'installation locale de :

1. le composant officiel `miri` pour la toolchain nightly déjà présente
   (`rustup component add miri --toolchain nightly-x86_64-pc-windows-msvc`),
   plus `rust-src` si absent ;
2. `cargo-mutants` **version 27.1.0 exactement** (la version de la CI),
   installée avec une méthode épinglée et vérifiable (`cargo install
   cargo-mutants --version 27.1.0 --locked`).

Aucun de ces outils n'entre dans `Cargo.toml`, dans l'artefact livré ou dans les
dépendances du produit. Ce sont des outils de développement, comme `cargo
llvm-cov` déjà utilisé pour la couverture.

### Commandes prévues après approbation

```powershell
# Miri (mêmes exclusions que la CI, documentées dans FUZZ-MIRI-SEMA.md §4.2)
$env:MIRIFLAGS='-Zmiri-disable-isolation'
cargo +nightly miri test -p ravum-lang-syntax --lib -- `
  --skip hostile_100k_tokens --skip hostile_deep_nesting `
  --skip too_large_token_stream --skip examples_are_idempotent_and_meaning_preserving
cargo +nightly miri test -p ravum-lang-sema --lib -- --skip loader::
$env:RAVUM_FUZZ_ITERS='3'
cargo +nightly miri test -p ravum-lang-sema --test fuzz_check

# Mutation (même portée ciblée que §9/§10 de MUTATION-EQUIVALENCE.md)
cargo mutants --no-shuffle -j 8 --file crates/ravum-lang-sema/src/own.rs -F …
cargo mutants --no-shuffle -j 8 --file crates/ravum-lang-sema/src/resolve.rs -F …
cargo mutants --no-shuffle -j 8 --file crates/ravum-lang-sema/src/typeck.rs -F …
```

Les filtres `-F` sont recopiés du workflow `mutation.yml` pour que les totaux
restent comparables.

## 3. Alternatives

| Option | Coût | Conséquence |
|---|---|---|
| **Statu quo** (CI seulement) | nul | Miri et mutation restent `NOT_RUN` sur la machine de référence ; les seules preuves sont Linux/CI |
| **Installer sans RFC** | — | Interdit par R5 ; refusé |
| **Installer via cette RFC** | revue de cette RFC | Deux `NOT_RUN` locaux peuvent être fermés par des commandes reproductibles ; aucun impact produit |

## 4. Risques et limites

- **Chaîne d'approvisionnement** : les deux outils viennent de sources
  officielles (composant rustup, crate `cargo-mutants` 27.1.0 déjà utilisée par
  la CI). La version est épinglée et la CI utilise la même.
- **Reproductibilité** : les chiffres Miri/mutation resteront liés à la machine,
  à la toolchain et au SHA ; ils ne seront jamais présentés comme `PROVED` ni
  comme un score de crate (mutation ciblée).
- **Coût machine** : Miri ≈ 6 min, mutation ciblée ≈ 15-30 min ; aucune
  installation dans l'artefact livré.
- **Réversibilité** : `rustup component remove miri` et `cargo uninstall
  cargo-mutants` suffisent ; aucun fichier du dépôt n'est requis.

## 5. Décision demandée

1. **Accorder** ou **refuser** l'installation locale de Miri et de
   `cargo-mutants 27.1.0` dans le périmètre ci-dessus.
2. En cas d'accord, préciser si la version de `cargo-mutants` doit être
   exactement 27.1.0 (recommandé, aligné CI) ou une autre.
3. Confirmer que ces outils restent **hors** du produit et des gates de release.

En attendant la décision : aucune installation, aucun changement de code, les
statuts restent `NOT_RUN` localement et les preuves CI sont citées avec leur run,
leur SHA et leur portée.
