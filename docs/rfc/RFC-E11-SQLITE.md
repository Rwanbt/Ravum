# RFC-E11-SQLITE — SQLite dans Core 1.0 par un runtime natif livré

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-E11-SQLITE` |
| Statut | **`PROPOSED`** — aucune implémentation avant décision du responsable sur §7 |
| Date | 2026-11-19 |
| Auteur | agent de développement |
| Origine | [D-18-2](../decision/DECISIONS-2026-11-18.md) ; ferme une partie de T6 et de T9 |
| S'appuie sur | [`RFC-P2-VERTICALS-BLOCKERS`](RFC-P2-VERTICALS-BLOCKERS.md) §3 et §6 (BL-6, BL-7), [`RFC-D7-FFI-EFFECTS`](RFC-D7-FFI-EFFECTS.md) |
| Portée | `crates/ravum-lang-cli/src/pipeline.rs` (édition de liens), nouveau crate `crates/ravum-rt-sqlite`, `spec/language/{stdlib,effects,unsupported}.md`, release (SBOM) |

> **Ce que cette RFC ne promet pas.** Ni un système de paquets natifs général,
> ni la liaison de bibliothèques choisies par l'utilisateur, ni la concurrence
> d'écriture multi-processus. Elle ouvre **une** dépendance native, choisie et
> livrée par la toolchain.

## 1. Problème, mesuré

`RFC-P2-VERTICALS-BLOCKERS` §3 l'a prouvé : le pipeline appelle `rustc` sur un
seul fichier, sans `-L`, `-l` ni `--extern` ; `sqlite3_open_v2` donne `LNK2019`.
Les quatre éléments L1–L4 (manifeste, propagation, résolution, confiance)
manquent. Le dépôt sait déjà compiler SQLite côté Rust : `ravum-store` utilise
`rusqlite` avec `bundled` (6/6 tests, sonde #13).

## 2. Proposition : un runtime natif fermé, livré avec la toolchain

Au lieu d'un système de dépendances natives général (L1–L4), la toolchain
livre un **rlib précompilé** `ravum_rt_sqlite` (crate Rust `crates/ravum-rt-sqlite`,
`rusqlite` + `bundled`), et le pipeline ajoute, **seulement si le programme
utilise `std::sqlite`** :

```
--extern ravum_rt_sqlite=<toolchain>/lib/libravum_rt_sqlite.rlib -L <toolchain>/lib/deps
```

| Élément de §3.3 | Réponse de cette RFC |
|---|---|
| L1 manifeste | **aucun** : l'usage de `std::sqlite` suffit, rien n'est déclaré par l'utilisateur |
| L2 propagation | `pipeline.rs` ajoute `--extern`/`-L` quand le graphe d'effets contient `db` |
| L3 résolution | une seule bibliothèque, au chemin fixe de la toolchain, son digest dans le manifeste de release |
| L4 confiance | le code natif est celui de SQLite, version épinglée, construit par la CI du projet, listé dans le SBOM ; l'utilisateur n'ajoute aucun code natif |

Conséquence : `#[link]`, build scripts et FFI vers des bibliothèques tierces
**restent refusés**. D7 n'est pas élargi.

**Coût d'édition de liens** : le rlib doit être construit avec **le même
`rustc`** que celui qui compile le programme (rlibs non stables entre versions).
La toolchain épingle déjà `rustc` 1.95.0 ; `ravum-lang doctor` doit vérifier
l'accord et refuser sinon (`E-TOOLCHAIN-MISMATCH`, exit 2).

## 3. Surface `std::sqlite`, effet `db`

```rust
fn open(path: &str) -> Result<Db, DbError> effects(db)
fn open_memory() -> Result<Db, DbError> effects(db)
fn execute(db: &mut Db, sql: &str, params: &Vec<Value>) -> Result<u64, DbError> effects(db)
fn query(db: &Db, sql: &str, params: &Vec<Value>) -> Result<Vec<Row>, DbError> effects(db)
fn begin(db: &mut Db) -> Result<(), DbError> effects(db)
fn commit(db: &mut Db) -> Result<(), DbError> effects(db)
fn rollback(db: &mut Db) -> Result<(), DbError> effects(db)

enum Value { Null, Int(i64), Real(f64), Text(String) }
struct Row { values: Vec<Value> }
```

| ID | Règle proposée | Niveau |
|---|---|---|
| DB-1.1 | Toute fonction de `std::sqlite` porte l'effet `db` (nouveau groupe) ; `db` est refusé sous `core` et `rt`. | MUST |
| DB-1.2 | Les paramètres passent **uniquement** par `params` (requêtes préparées) ; aucune API ne concatène de valeur dans le SQL. | MUST |
| DB-1.3 | Une erreur SQLite devient `Err(DbError { code, message })`, jamais une panique. | MUST |
| DB-1.4 | Un `Db` détruit avec une transaction ouverte fait un `rollback`. | MUST |
| DB-1.5 | `Db` n'est **pas** transférable entre threads en 1.0 (`E-CONC-NOT-TRANSFERABLE`) : une connexion par thread. | MUST |
| DB-1.6 | Pas de BLOB en 1.0 (pas de type octets, BL-8). | MUST |
| DB-1.7 | La version de SQLite est épinglée et affichée par `ravum-lang version --json`. | MUST |

## 4. Qualification

- Cas `run-pass` sur `open_memory()` (hermétique, sans fichier) : création,
  insertion paramétrée, requête, transaction annulée, transaction validée.
- Cas sur fichier sous un répertoire temporaire effacé après le test.
- Cas d'erreur : SQL invalide → `Err` avec code ; contrainte `UNIQUE` → `Err`.
- Sécurité : un cas montre qu'une valeur `"'; DROP TABLE t; --"` passée en
  paramètre est stockée telle quelle.
- Trois OS en CI ; le rlib est un artefact de release, couvert par le SBOM et
  la provenance (T11).
- `ravum-core-machine` refuse `db` (fault).

## 5. Alternatives écartées

| Alternative | Pourquoi écartée |
|---|---|
| Dépendances natives générales (L1–L4 complets) | ouvre D7 entier, registre et build scripts refusés ; hors de proportion pour une bibliothèque |
| SQLite en `.rv` | hors de portée (≈ 250 000 lignes de C, pas d'E/S binaire) |
| Service hôte hors processus | exige un transport ; possible après RFC-E11-NET-HTTP, mais ajoute un processus et un protocole à qualifier |
| Magasin texte (`examples/file-store`) | existe ; ce n'est **pas** SQL et ne ferme pas T9 |

## 6. Coût estimé (surfaces comptées, pas des délais)

Nouveau crate (~300 lignes), `pipeline.rs` (+30), prélude/builtins (~150),
spec (~80 lignes), 8–12 cas de conformance, job de release du rlib.

## 7. Décisions attendues du responsable

1. Accepter le **runtime natif fermé** au lieu d'un système de dépendances natives.
2. Accepter un nouveau groupe d'effet `db`.
3. Accepter la contrainte « même `rustc` que le rlib » et son refus explicite.
4. Pas de BLOB ni de connexion partagée entre threads en 1.0.
