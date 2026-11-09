//! End-to-end tests: a real server, a real socket, a real SQLite file.
//!
//! What these tests do **not** execute: the JavaScript client (no browser in this
//! environment) and TLS (not implemented). The client-side contract check is
//! exercised at the level of the shared rule (`ravum_todo_core::client_supports`).

use ravum_todo::{run, Running, ServerConfig};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

const TOKEN: &str = "test-token";

struct TestServer {
    running: Running,
    dir: PathBuf,
}

struct Reply {
    status: u16,
    body: String,
}

impl TestServer {
    fn start(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/ravum-tests")
            .join(format!("todo-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("work dir");
        Self::start_in(dir)
    }

    fn start_in(dir: PathBuf) -> Self {
        Self::start_with_db(dir, "todo.db")
    }

    fn start_with_db(dir: PathBuf, db_name: &str) -> Self {
        let running = run(ServerConfig {
            db_path: dir.join(db_name),
            token: TOKEN.to_owned(),
            port: 0,
        })
        .expect("server starts");
        Self { running, dir }
    }

    fn url(&self) -> String {
        self.running.url()
    }

    fn request(&self, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> Reply {
        let addr = self.running.addr;
        let mut stream = TcpStream::connect(addr).expect("connect");
        let mut request =
            format!("{method} {path} HTTP/1.1\r\nHost: ravum.test\r\nConnection: close\r\n");
        if let Some(token) = token {
            request.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        if let Some(body) = body {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("\r\n");
        if let Some(body) = body {
            request.push_str(body);
        }
        stream.write_all(request.as_bytes()).expect("write request");

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        let text = String::from_utf8(raw).expect("utf-8 response");
        let (head, body) = text
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("malformed response: {text:?}"));
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        Reply {
            status,
            body: body.to_owned(),
        }
    }
}

fn json(reply: &Reply) -> serde_json::Value {
    serde_json::from_str(&reply.body)
        .unwrap_or_else(|e| panic!("invalid JSON ({e}): {}", reply.body))
}

#[test]
fn full_crud_round_trip() {
    let server = TestServer::start("crud");

    let created = server.request(
        "POST",
        "/api/tasks",
        Some(TOKEN),
        Some(r#"{"title":"  acheter du cafe  ","due_date":"2026-11-09"}"#),
    );
    assert_eq!(created.status, 201, "{}", created.body);
    let task = &json(&created)["task"];
    let id = task["id"].as_i64().expect("id");
    assert_eq!(
        task["title"], "acheter du cafe",
        "the server trims the title"
    );
    assert_eq!(task["done"], false);
    assert_eq!(task["due_date"], "2026-11-09");
    assert!(task["created_at"].as_str().unwrap_or("").ends_with('Z'));

    let listed = server.request("GET", "/api/tasks", None, None);
    assert_eq!(listed.status, 200);
    assert_eq!(json(&listed)["tasks"].as_array().unwrap().len(), 1);

    let patched = server.request(
        "PATCH",
        &format!("/api/tasks/{id}"),
        Some(TOKEN),
        Some(r#"{"done":true}"#),
    );
    assert_eq!(patched.status, 200, "{}", patched.body);
    assert_eq!(json(&patched)["task"]["done"], true);

    let deleted = server.request("DELETE", &format!("/api/tasks/{id}"), Some(TOKEN), None);
    assert_eq!(deleted.status, 204);
    assert!(deleted.body.is_empty());

    let after = server.request("GET", "/api/tasks", None, None);
    assert_eq!(json(&after)["tasks"].as_array().unwrap().len(), 0);

    let missing = server.request("DELETE", &format!("/api/tasks/{id}"), Some(TOKEN), None);
    assert_eq!(missing.status, 404);
    assert_eq!(json(&missing)["error"]["code"], "not_found");
}

#[test]
fn validation_is_enforced_on_the_server() {
    let server = TestServer::start("validation");

    let empty = server.request(
        "POST",
        "/api/tasks",
        Some(TOKEN),
        Some(r#"{"title":"   "}"#),
    );
    assert_eq!(empty.status, 400);
    assert_eq!(json(&empty)["error"]["code"], "validation_failed");
    assert_eq!(
        json(&empty)["error"]["details"][0]["violation"]["code"],
        "title_empty"
    );

    let long_title = format!(r#"{{"title":"{}"}}"#, "a".repeat(200));
    let too_long = server.request("POST", "/api/tasks", Some(TOKEN), Some(&long_title));
    assert_eq!(too_long.status, 400);
    assert_eq!(
        json(&too_long)["error"]["details"][0]["violation"]["code"],
        "title_too_long"
    );

    let bad_date = server.request(
        "POST",
        "/api/tasks",
        Some(TOKEN),
        Some(r#"{"title":"x","due_date":"2026-13-40"}"#),
    );
    assert_eq!(bad_date.status, 400);
    assert_eq!(
        json(&bad_date)["error"]["details"][0]["violation"]["code"],
        "due_date_invalid"
    );

    let malformed = server.request("POST", "/api/tasks", Some(TOKEN), Some("{not json"));
    assert_eq!(malformed.status, 400);
    assert_eq!(json(&malformed)["error"]["code"], "invalid_json");

    // Nothing was created by any of the rejected requests.
    let listed = server.request("GET", "/api/tasks", None, None);
    assert_eq!(json(&listed)["tasks"].as_array().unwrap().len(), 0);
}

#[test]
fn mutations_require_a_valid_bearer_token() {
    let server = TestServer::start("auth");

    let anonymous = server.request("POST", "/api/tasks", None, Some(r#"{"title":"x"}"#));
    assert_eq!(anonymous.status, 401);
    assert_eq!(json(&anonymous)["error"]["code"], "unauthorized");

    let wrong = server.request("POST", "/api/tasks", Some("nope"), Some(r#"{"title":"x"}"#));
    assert_eq!(wrong.status, 401);

    // Reads are public; that is a deliberate choice, stated in the report.
    let read = server.request("GET", "/api/tasks", None, None);
    assert_eq!(read.status, 200);

    let created = server.request("POST", "/api/tasks", Some(TOKEN), Some(r#"{"title":"ok"}"#));
    let id = json(&created)["task"]["id"].as_i64().unwrap();
    let delete_without_token = server.request("DELETE", &format!("/api/tasks/{id}"), None, None);
    assert_eq!(delete_without_token.status, 401);
    let still_there = server.request("GET", "/api/tasks", None, None);
    assert_eq!(json(&still_there)["tasks"].as_array().unwrap().len(), 1);
}

#[test]
fn the_contract_is_served_and_versioned() {
    let server = TestServer::start("contract");

    let health = server.request("GET", "/health", None, None);
    assert_eq!(health.status, 200);
    assert_eq!(json(&health)["schema_version"], 2);

    let schema = server.request("GET", "/api/schema", None, None);
    assert_eq!(schema.status, 200);
    let contract = json(&schema);
    assert_eq!(
        contract["contract_version"],
        ravum_todo_core::CONTRACT_VERSION
    );
    let fields = contract["resources"]["task"]["fields"].as_array().unwrap();
    assert!(fields.iter().any(|f| f["name"] == "due_date"));
    assert!(fields.iter().any(|f| f["name"] == "title"));
    assert!(
        contract["compatibility"]
            .as_str()
            .unwrap_or("")
            .contains("exact"),
        "the contract must state that compatibility is exact"
    );
}

#[test]
fn a_client_with_another_contract_version_is_not_compatible() {
    // The browser client refuses to mutate when the version differs; the rule it
    // uses is the shared one, exercised here.
    assert!(ravum_todo_core::client_supports(1, 1));
    assert!(!ravum_todo_core::client_supports(1, 2));
    assert!(!ravum_todo_core::client_supports(2, 1));
}

#[test]
fn data_survives_a_restart() {
    let server = TestServer::start("restart");
    let created = server.request(
        "POST",
        "/api/tasks",
        Some(TOKEN),
        Some(r#"{"title":"persistant"}"#),
    );
    assert_eq!(created.status, 201);
    let TestServer { running, dir } = server;
    running.shutdown();

    let restarted = TestServer::start_in(dir);
    let listed = restarted.request("GET", "/api/tasks", None, None);
    let tasks = json(&listed)["tasks"].as_array().unwrap().clone();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["title"], "persistant");
}

#[test]
fn an_unknown_schema_version_refuses_to_start() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join("todo-future-schema");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("todo.db");
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
    }

    let error = run(ServerConfig {
        db_path,
        token: TOKEN.to_owned(),
        port: 0,
    })
    .map(|_| ())
    .expect_err("a newer schema must be refused");
    assert!(
        error.contains("newer than this build"),
        "the refusal must be explicit: {error}"
    );
}

#[test]
fn e2e_migration_from_a_legacy_database() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ravum-tests")
        .join("todo-migration");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("legacy.db");

    // A database written by the previous schema version: no `due_date` column.
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 title TEXT NOT NULL,
                 done INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL
             );
             INSERT INTO tasks (title, done, created_at)
                 VALUES ('tache v1', 0, '2026-11-01T10:00:00Z');
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }

    let server = TestServer::start_with_db(dir.clone(), "legacy.db");

    let health = server.request("GET", "/health", None, None);
    assert_eq!(json(&health)["schema_version"], 2, "the migration ran");

    let listed = server.request("GET", "/api/tasks", None, None);
    let tasks = json(&listed)["tasks"].as_array().unwrap().clone();
    assert_eq!(tasks.len(), 1, "the existing row must be preserved");
    assert_eq!(tasks[0]["title"], "tache v1");
    assert!(
        tasks[0]["due_date"].is_null(),
        "the new column defaults to NULL"
    );

    // The migrated database accepts writes to the new column.
    let id = tasks[0]["id"].as_i64().unwrap();
    let patched = server.request(
        "PATCH",
        &format!("/api/tasks/{id}"),
        Some(TOKEN),
        Some(r#"{"due_date":"2026-12-01"}"#),
    );
    assert_eq!(patched.status, 200, "{}", patched.body);
    assert_eq!(json(&patched)["task"]["due_date"], "2026-12-01");
}

#[test]
fn the_static_ui_is_served() {
    let server = TestServer::start("static");
    let index = server.request("GET", "/", None, None);
    assert_eq!(index.status, 200);
    assert!(index.body.contains("Ravum TODO"));
    assert!(index.body.contains("/app.js"));

    let script = server.request("GET", "/app.js", None, None);
    assert_eq!(script.status, 200);
    assert!(script.body.contains("contract_version"));

    let unknown = server.request("GET", "/nope", None, None);
    assert_eq!(unknown.status, 404);
    assert_eq!(json(&unknown)["error"]["code"], "not_found");
}

#[test]
fn the_served_url_points_at_the_bound_socket() {
    let server = TestServer::start("url");
    assert!(server.url().starts_with("http://127.0.0.1:"));
}
