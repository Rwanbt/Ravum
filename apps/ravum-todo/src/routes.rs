//! Routing and the server loop.

use crate::http::{read_request, write_response, ParseError, Request, Response};
use ravum_store::{Db, StoreError, SCHEMA_VERSION};
use ravum_todo_core::{
    contract, validate_new, validate_patch, NewTask, TaskPatch, ValidationError, CONTRACT_VERSION,
};
use serde_json::json;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub db_path: PathBuf,
    pub token: String,
    pub port: u16,
}

struct Shared {
    db: Mutex<Db>,
    token: String,
}

pub struct Running {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Running {
    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Start the server. Fails closed if the database schema is incompatible.
pub fn run(config: ServerConfig) -> Result<Running, String> {
    let db = Db::open(&config.db_path).map_err(|e| e.to_string())?;
    let listener =
        TcpListener::bind(("127.0.0.1", config.port)).map_err(|e| format!("cannot bind: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("cannot configure the listener: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let shared = Arc::new(Shared {
        db: Mutex::new(db),
        token: config.token,
    });
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let handle = thread::Builder::new()
        .name("ravum-todo-accept".to_owned())
        .spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let shared = Arc::clone(&shared);
                        let _ = thread::Builder::new()
                            .name("ravum-todo-request".to_owned())
                            .spawn(move || {
                                let _ = handle_connection(stream, &shared);
                            });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(Running {
        addr,
        stop,
        handle: Some(handle),
    })
}

fn handle_connection(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let mut stream = stream;
    // On Windows, a socket accepted from a non-blocking listener inherits the
    // non-blocking mode. The request worker must block on the request: otherwise a
    // client that has connected but whose bytes are not in the kernel buffer yet gets
    // an intermittent 400. This was observed with a real Chromium via CDP (the browser
    // sends the request a few hundred microseconds after the handshake) and never with
    // in-process Rust tests, which write immediately after connecting.
    stream.set_nonblocking(false)?;
    let debug = std::env::var_os("RAVUM_TODO_HTTP_DEBUG").is_some();
    if debug {
        eprintln!(
            "ravum-todo http debug [connection] peer={:?}",
            stream.peer_addr()
        );
    }
    let response = match read_request(&stream) {
        Ok(Some(request)) => {
            if debug {
                eprintln!(
                    "ravum-todo http debug [request] {} {} content-length={:?}",
                    request.method,
                    request.path,
                    request.header("content-length")
                );
            }
            route(&request, shared)
        }
        Ok(None) => {
            if debug {
                eprintln!("ravum-todo http debug [connection] clean EOF before any byte");
            }
            return Ok(());
        }
        Err(ParseError::TooLarge) => {
            error(413, "payload_too_large", "the request is too large", vec![])
        }
        Err(ParseError::Incomplete) | Err(ParseError::Malformed(_)) => error(
            400,
            "malformed_request",
            "the HTTP request could not be parsed",
            vec![],
        ),
    };
    if debug {
        eprintln!(
            "ravum-todo http debug [response] status={}",
            response.status
        );
    }
    write_response(&mut stream, &response)?;
    Ok(())
}

fn route(request: &Request, shared: &Shared) -> Response {
    let path = request.route();
    match (request.method.as_str(), path) {
        ("GET", "/health") => Response::json(
            200,
            &json!({
                "status": "ok",
                "schema_version": SCHEMA_VERSION,
                "contract_version": CONTRACT_VERSION,
            }),
        ),
        ("GET", "/api/schema") => Response::json(200, &contract()),
        ("GET", "/api/tasks") => list_tasks(shared),
        ("POST", "/api/tasks") => create_task(request, shared),
        ("GET", "/") | ("GET", "/index.html") => {
            Response::text(200, "text/html; charset=utf-8", INDEX_HTML)
        }
        ("GET", "/app.js") => Response::text(200, "application/javascript; charset=utf-8", APP_JS),
        (method, p) if p.starts_with("/api/tasks/") && method == "PATCH" => {
            with_id(p, |id| patch_task(request, shared, id))
        }
        (method, p) if p.starts_with("/api/tasks/") && method == "DELETE" => {
            with_id(p, |id| delete_task(request, shared, id))
        }
        ("GET", _) | ("POST", _) | ("PATCH", _) | ("DELETE", _) => error(
            404,
            "not_found",
            "no such route",
            vec![json!({"path": path})],
        ),
        _ => error(405, "method_not_allowed", "method not allowed", vec![]),
    }
}

fn with_id(path: &str, handler: impl FnOnce(i64) -> Response) -> Response {
    match path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
    {
        Some(id) => handler(id),
        None => error(400, "invalid_id", "the task id must be an integer", vec![]),
    }
}

fn authorized(request: &Request, shared: &Shared) -> bool {
    let expected = format!("Bearer {}", shared.token);
    request
        .header("authorization")
        .map(|value| value == expected)
        .unwrap_or(false)
}

fn list_tasks(shared: &Shared) -> Response {
    let db = shared.db.lock().unwrap_or_else(|e| e.into_inner());
    match db.tasks() {
        Ok(tasks) => Response::json(200, &json!({ "tasks": tasks })),
        Err(e) => store_failure(e),
    }
}

fn create_task(request: &Request, shared: &Shared) -> Response {
    if !authorized(request, shared) {
        return error(
            401,
            "unauthorized",
            "a valid bearer token is required",
            vec![],
        );
    }
    let new: NewTask = match serde_json::from_slice(&request.body) {
        Ok(value) => value,
        Err(e) => {
            return error(
                400,
                "invalid_json",
                "the body is not a valid task",
                vec![json!({"detail": e.to_string()})],
            )
        }
    };
    if let Err(errors) = validate_new(&new) {
        return validation_failure(errors);
    }
    let db = shared.db.lock().unwrap_or_else(|e| e.into_inner());
    match db.create_task(&new) {
        Ok(task) => Response::json(201, &json!({ "task": task })),
        Err(e) => store_failure(e),
    }
}

fn patch_task(request: &Request, shared: &Shared, id: i64) -> Response {
    if !authorized(request, shared) {
        return error(
            401,
            "unauthorized",
            "a valid bearer token is required",
            vec![],
        );
    }
    let patch: TaskPatch = match serde_json::from_slice(&request.body) {
        Ok(value) => value,
        Err(e) => {
            return error(
                400,
                "invalid_json",
                "the body is not a valid patch",
                vec![json!({"detail": e.to_string()})],
            )
        }
    };
    if let Err(errors) = validate_patch(&patch) {
        return validation_failure(errors);
    }
    let db = shared.db.lock().unwrap_or_else(|e| e.into_inner());
    match db.patch_task(id, &patch) {
        Ok(Some(task)) => Response::json(200, &json!({ "task": task })),
        Ok(None) => error(
            404,
            "not_found",
            "no task with this id",
            vec![json!({"id": id})],
        ),
        Err(e) => store_failure(e),
    }
}

fn delete_task(request: &Request, shared: &Shared, id: i64) -> Response {
    if !authorized(request, shared) {
        return error(
            401,
            "unauthorized",
            "a valid bearer token is required",
            vec![],
        );
    }
    let db = shared.db.lock().unwrap_or_else(|e| e.into_inner());
    match db.delete_task(id) {
        Ok(true) => Response::empty(204),
        Ok(false) => error(
            404,
            "not_found",
            "no task with this id",
            vec![json!({"id": id})],
        ),
        Err(e) => store_failure(e),
    }
}

fn validation_failure(errors: Vec<ValidationError>) -> Response {
    let details: Vec<serde_json::Value> = errors
        .iter()
        .map(|e| json!({ "violation": e, "message": e.message() }))
        .collect();
    error(
        400,
        "validation_failed",
        "the request is not valid",
        details,
    )
}

fn store_failure(error_value: StoreError) -> Response {
    error(
        500,
        "storage_error",
        "the storage layer failed",
        vec![json!({"detail": error_value.to_string()})],
    )
}

fn error(status: u16, code: &str, message: &str, details: Vec<serde_json::Value>) -> Response {
    Response::json(
        status,
        &json!({
            "error": { "code": code, "message": message, "details": details }
        }),
    )
}
