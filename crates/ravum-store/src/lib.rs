//! SQLite boundary and versioned migrations for the M4 reference application.
//!
//! Rules of this crate:
//! * SQL lives here and nowhere else;
//! * migrations are numbered, applied in order, each in its own transaction;
//! * an unknown **newer** schema is refused at startup instead of being used with
//!   missing columns (fail-closed);
//! * no ORM, no query builder: prepared statements only.

use ravum_todo_core::{NewTask, Task, TaskPatch};
use rusqlite::{params_from_iter, Connection, OptionalExtension};
use std::fmt;
use std::path::Path;

/// Schema this build understands. Bump it **and** add a migration.
pub const SCHEMA_VERSION: i32 = 2;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(String),
    /// The database was written by a newer build: refuse rather than guess.
    SchemaTooNew { found: i32, supported: i32 },
    NegativeSchema { found: i32 },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Sqlite(d) => write!(f, "sqlite error: {d}"),
            StoreError::SchemaTooNew { found, supported } => write!(
                f,
                "database schema v{found} is newer than this build (v{supported}); refusing to start"
            ),
            StoreError::NegativeSchema { found } => {
                write!(f, "database reports an impossible schema version {found}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e.to_string())
    }
}

#[derive(Debug)]
pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        let db = Self::prepare(conn)?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn schema_version(&self) -> Result<i32, StoreError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Apply every migration the database is missing.
    fn migrate(&mut self) -> Result<(), StoreError> {
        let mut version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 0 {
            return Err(StoreError::NegativeSchema { found: version });
        }
        if version > SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }

        if version < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE tasks (
                     id         INTEGER PRIMARY KEY AUTOINCREMENT,
                     title      TEXT    NOT NULL,
                     done       INTEGER NOT NULL DEFAULT 0,
                     created_at TEXT    NOT NULL
                 );",
            )?;
            tx.execute_batch("PRAGMA user_version = 1")?;
            tx.commit()?;
            version = 1;
        }

        if version < 2 {
            // Additive migration: existing rows keep their data, `due_date` is NULL.
            let tx = self.conn.transaction()?;
            tx.execute_batch("ALTER TABLE tasks ADD COLUMN due_date TEXT;")?;
            tx.execute_batch("PRAGMA user_version = 2")?;
            tx.commit()?;
        }

        Ok(())
    }

    pub fn tasks(&self) -> Result<Vec<Task>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, title, done, due_date, created_at FROM tasks ORDER BY id ASC",
        )?;
        let rows = statement.query_map([], row_to_task)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn task(&self, id: i64) -> Result<Option<Task>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, title, done, due_date, created_at FROM tasks WHERE id = ?1",
        )?;
        Ok(statement.query_row([id], row_to_task).optional()?)
    }

    pub fn create_task(&self, new: &NewTask) -> Result<Task, StoreError> {
        let created_at = now_iso8601();
        self.conn.execute(
            "INSERT INTO tasks (title, done, created_at, due_date) VALUES (?1, 0, ?2, ?3)",
            rusqlite::params![new.title.trim(), created_at, new.due_date],
        )?;
        let id = self.conn.last_insert_rowid();
        self.task(id)?.ok_or_else(|| StoreError::Sqlite("inserted task vanished".to_owned()))
    }

    pub fn patch_task(&self, id: i64, patch: &TaskPatch) -> Result<Option<Task>, StoreError> {
        let mut sets: Vec<&str> = Vec::new();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(title) = &patch.title {
            sets.push("title = ?");
            values.push(rusqlite::types::Value::Text(title.trim().to_owned()));
        }
        if let Some(done) = patch.done {
            sets.push("done = ?");
            values.push(rusqlite::types::Value::Integer(i64::from(done)));
        }
        if patch.clear_due_date {
            sets.push("due_date = NULL");
        } else if let Some(due) = &patch.due_date {
            sets.push("due_date = ?");
            values.push(rusqlite::types::Value::Text(due.clone()));
        }

        if sets.is_empty() {
            return self.task(id);
        }

        values.push(rusqlite::types::Value::Integer(id));
        let sql = format!("UPDATE tasks SET {} WHERE id = ?", sets.join(", "));
        let changed = self.conn.execute(&sql, params_from_iter(values))?;
        if changed == 0 {
            return Ok(None);
        }
        self.task(id)
    }

    pub fn delete_task(&self, id: i64) -> Result<bool, StoreError> {
        let changed = self.conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        done: row.get::<_, i64>(2)? != 0,
        due_date: row.get(3)?,
        created_at: row.get(4)?,
    })
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso8601() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso8601_from_unix(seconds)
}

/// Civil date from a Unix timestamp, without a date library.
pub fn iso8601_from_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`, valid for the whole range we care about.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates_are_formatted_correctly() {
        assert_eq!(iso8601_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_from_unix(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(iso8601_from_unix(1_800_000_000), "2027-01-15T08:00:00Z");
    }

    #[test]
    fn a_fresh_database_is_at_the_current_version() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn crud_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let created = db
            .create_task(&NewTask {
                title: "  écrire un test  ".to_owned(),
                due_date: Some("2026-11-09".to_owned()),
            })
            .unwrap();
        assert_eq!(created.title, "écrire un test", "the title is trimmed");
        assert!(!created.done);
        assert_eq!(created.due_date.as_deref(), Some("2026-11-09"));

        let patched = db
            .patch_task(
                created.id,
                &TaskPatch {
                    done: Some(true),
                    ..TaskPatch::default()
                },
            )
            .unwrap()
            .expect("task");
        assert!(patched.done);

        let cleared = db
            .patch_task(
                created.id,
                &TaskPatch {
                    clear_due_date: true,
                    ..TaskPatch::default()
                },
            )
            .unwrap()
            .expect("task");
        assert_eq!(cleared.due_date, None);
        assert!(cleared.done, "other fields are untouched");

        assert!(db.delete_task(created.id).unwrap());
        assert!(!db.delete_task(created.id).unwrap());
        assert!(db.tasks().unwrap().is_empty());
    }

    #[test]
    fn patching_a_missing_task_returns_none() {
        let db = Db::open_in_memory().unwrap();
        assert!(db
            .patch_task(42, &TaskPatch { done: Some(true), ..TaskPatch::default() })
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        let err = Db::prepare(conn).unwrap_err();
        assert!(matches!(err, StoreError::SchemaTooNew { found: 99, supported: 2 }));
    }

    #[test]
    fn migrating_from_v1_preserves_rows_and_adds_the_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NOT NULL,
                 done INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL
             );
             INSERT INTO tasks (title, done, created_at) VALUES ('ancienne tache', 0, '2026-11-01T10:00:00Z');
             PRAGMA user_version = 1;",
        )
        .unwrap();

        let db = Db::prepare(conn).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        let tasks = db.tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "ancienne tache");
        assert_eq!(tasks[0].due_date, None, "the new column defaults to NULL");

        // The migrated database accepts writes to the new column.
        let updated = db
            .patch_task(
                tasks[0].id,
                &TaskPatch { due_date: Some("2026-12-01".to_owned()), ..TaskPatch::default() },
            )
            .unwrap()
            .expect("task");
        assert_eq!(updated.due_date.as_deref(), Some("2026-12-01"));
    }
}
