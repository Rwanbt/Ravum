# `sqlite` — SQLite for Ravum

A Ravum package (`src/lib.rv`) over SQLite **3.54.1**, decision D-19-7 and
[`RFC-E11-SQLITE`](../../docs/rfc/RFC-E11-SQLITE.md) §6.

## Build the native library first

The Ravum toolchain never compiles C (`modules.md` MOD-5.10). Run, once per
machine:

```bash
python3 tools/build_sqlite_native.py
```

It takes the SQLite amalgamation from the `libsqlite3-sys` crate that
`Cargo.lock` pins, refuses it unless `sqlite3.c` and `sqlite3.h` match the
SHA-256 digests pinned in the script, compiles it with
`native-src/ravum_sqlite.c` into `native/<os>-<arch>/libravum_sqlite.a`, and
writes `ravum.toml` with that library's digest for this platform. Both are
build outputs and are not committed. Linux and macOS only; Windows is not
supported yet.

## Use it

```toml
[dependencies]
sqlite = { path = "../sqlite" }
```

```rv
let db: sqlite::Db = sqlite::open_memory()?;
let mut params: Vec<sqlite::Value> = Vec::new();
params.push(sqlite::Value::Text(String::from("ada")));
let changed: i64 = sqlite::execute(&db, "INSERT INTO t (name) VALUES (?1)", params)?;
let rows: Vec<sqlite::Row> = sqlite::query(&db, "SELECT id, name FROM t", Vec::new())?;
sqlite::close(db);
```

`packages/sqlite-notes` is a complete program, run by
`crates/ravum-lang-cli/tests/sqlite_package.rs`.

## What it guarantees, and what it does not

- Values reach SQL only as statement parameters (`?1`, `?2`, …); no function
  of this package splices a value into SQL text.
- Every SQLite error is an `Err`, never a panic.
- No BLOB, no connection shared between threads (`Db` holds a `CHandle`).
- **Differences from the RFC**: callers declare `unsafe` and `ffi` (there is
  no `db` effect hiding them: a package cannot mask effects); a `Db` must be
  closed with `sqlite::close`, because Ravum has no destructor, so an open
  transaction is not rolled back automatically; one statement is active per
  connection at a time.
