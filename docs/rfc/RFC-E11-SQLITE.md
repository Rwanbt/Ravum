# RFC-E11-SQLITE — Dépendances natives et SQLite dans Core 1.0

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-E11-SQLITE` |
| Statut | **`PROPOSED`** — choix de conception fixé par [D-19-4](../decision/DECISIONS-2026-11-19.md) (dépendances natives générales) ; ratification attendue sur §8 |
| Date | 2026-11-19 |
| Auteur | agent de développement |
| Origine | [D-18-2](../decision/DECISIONS-2026-11-18.md) ; ferme une partie de T6 et de T9 |
| S'appuie sur | [`RFC-P2-VERTICALS-BLOCKERS`](RFC-P2-VERTICALS-BLOCKERS.md) §3 (L1–L4) et §6, [`RFC-D7-FFI-EFFECTS`](RFC-D7-FFI-EFFECTS.md) (frontière C-FFI-1 à C-FFI-10, élargie ici) |
| Portée | `ravum.toml`/`ravum.lock` (`crates/ravum-lang-sema/src/loader.rs`), `crates/ravum-lang-cli/src/pipeline.rs` (édition de liens), types FFI (`ravum-lang-sema`), `spec/language/{modules,ffi,stdlib,effects,unsupported}.md`, `SAFETY.md`, nouvelle bibliothèque `.rv` `std::sqlite` |

> **Ce que cette RFC ne promet pas.** Ni registre de paquets, ni build scripts,
> ni compilation de code C par la toolchain, ni sûreté du code natif lié. Le
> code natif est **non vérifié** : la RFC le confine et le rend visible, elle ne
> le prouve pas.

## 1. Problème, mesuré

Le pipeline appelle `rustc` sur un seul fichier, sans `-L`, `-l` ni `--extern` ;
`sqlite3_open_v2` donne `LNK2019` (sonde #8). Il manque les quatre éléments
L1–L4. D-19-4 choisit de les construire **en général**, pas pour SQLite seul.

## 2. L1 — Déclaration dans le manifeste

```toml
[native.sqlite3]
kind = "static"              # "static" | "dylib"
path = "native/sqlite3"      # répertoire relatif au paquet
lib = "sqlite3"              # nom passé à -l
sha256 = "…"                 # digest du fichier .a/.lib/.so/.dll/.dylib résolu
```

| ID | Règle proposée | Niveau |
|---|---|---|
| NAT-1.1 | Une bibliothèque native n'est liée que si elle est déclarée dans `[native.<nom>]` du paquet racine. | MUST |
| NAT-1.2 | `path` est relatif au paquet et ne sort pas de son répertoire (`..` refusé, `E-MOD-NATIVE-PATH`). | MUST |
| NAT-1.3 | Le digest du fichier résolu doit égaler `sha256` ; sinon `E-MOD-NATIVE-DIGEST`, rien n'est lié. | MUST |
| NAT-1.4 | Le digest est recopié dans `ravum.lock` ; un changement de bibliothèque est un changement de lock visible en revue. | MUST |
| NAT-1.5 | Build scripts, compilation C et téléchargement restent refusés (`E-MOD-BUILD-SCRIPT`) : la bibliothèque est fournie **déjà compilée**. | MUST |

## 3. L2 — Propagation vers `rustc`

`pipeline.rs` ajoute `-L native=<path>` et `-l <kind>=<lib>` pour chaque
bibliothèque déclarée **et** utilisée par un bloc `extern "C" "<nom>" { ... }`.
Une déclaration non utilisée donne `W-UNUSED` (pas d'édition de liens inutile).

| ID | Règle proposée | Niveau |
|---|---|---|
| NAT-2.1 | Un bloc `extern "C"` peut nommer sa bibliothèque : `extern "C" from "sqlite3" { ... }` ; le nom doit exister dans `[native]` (`E-FFI-UNKNOWN-LIB`). | MUST |
| NAT-2.2 | Les arguments ajoutés à `rustc` font partie de l'identité de build (déterminisme, T11). | MUST |

## 4. L3 — Résolution par plateforme

`path` peut contenir `{os}` et `{arch}` (`native/{os}-{arch}`) ; une plateforme
sans fichier donne `E-MOD-NATIVE-MISSING` avec la plateforme nommée, jamais un
repli. Le digest est alors déclaré par plateforme (`sha256.linux-x86_64 = …`).

## 5. L4 — Confiance et frontière FFI élargie

| ID | Règle proposée | Niveau |
|---|---|---|
| NAT-3.1 | Un programme qui lie une bibliothèque native porte l'effet `ffi` sur toute fonction qui l'appelle (règle D7 existante) ; `--profile core` le refuse. | MUST |
| NAT-3.2 | `ravum-lang build` affiche chaque bibliothèque liée et son digest ; `--json` les liste. | MUST |
| NAT-3.3 | Types FFI ajoutés : **handle opaque** `extern type Db;` utilisable seulement comme `*mut Db` dans `extern "C"`, et tampon texte `&CStr`/`CString` ; aucun pointeur brut hors des blocs `extern`. | MUST |
| NAT-3.4 | Toute nouvelle construction FFI est ajoutée à `SAFETY.md` avec sa justification ; D7 est mis à jour (C-FFI-11 et suivants). | MUST |

## 6. SQLite au-dessus du mécanisme

`std::sqlite` est une **bibliothèque `.rv`** livrée avec la toolchain, qui
déclare `[native.sqlite3]` et appelle l'API C par `extern "C" from "sqlite3"`.
Les binaires de SQLite (amalgame compilé par la CI du projet, version épinglée)
sont publiés comme artefacts de release, couverts par le SBOM et la provenance.

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
| DB-1.1 | L'API publique porte l'effet `db` (nouveau groupe) qui **masque** `ffi` : l'utilisateur de `std::sqlite` n'écrit pas `unsafe`. `db` est refusé sous `core` et `rt`. | MUST |
| DB-1.2 | Paramètres uniquement par `params` (requêtes préparées). | MUST |
| DB-1.3 | Une erreur SQLite devient `Err(DbError { code, message })`, jamais une panique. | MUST |
| DB-1.4 | Un `Db` détruit avec une transaction ouverte fait un `rollback`. | MUST |
| DB-1.5 | `Db` n'est pas transférable entre threads en 1.0. | MUST |
| DB-1.6 | Pas de BLOB en 1.0. | MUST |
| DB-1.7 | Version de SQLite épinglée, affichée par `ravum-lang version --json`. | MUST |

## 7. Qualification

- Mécanisme natif : cas `compile-fail` pour `E-MOD-NATIVE-PATH`, `-DIGEST`,
  `-MISSING`, `E-FFI-UNKNOWN-LIB` ; un cas `run-pass` qui lie une petite
  bibliothèque de test (compilée dans le test Rust, pas par la toolchain).
- SQLite : `open_memory()` hermétique ; insertion paramétrée, requête,
  transaction annulée et validée ; erreurs SQL et `UNIQUE` ; injection stockée
  telle quelle. Trois OS.
- Déterminisme : deux builds avec les mêmes bibliothèques donnent le même
  binaire (job existant).
- `ravum-core-machine` refuse `ffi` et `db` (fault).

## 8. Décisions encore attendues

1. Syntaxe `extern "C" from "<lib>"` (NAT-2.1) et `extern type` (NAT-3.3).
2. Effet `db` qui masque `ffi` dans une bibliothèque de la toolchain (DB-1.1).
3. Publication des binaires SQLite comme artefacts de release.
