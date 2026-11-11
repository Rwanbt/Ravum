//! M4 reference application: HTTP API + SQLite + static UI.
//!
//! This is a demonstration vertical slice, not a production server:
//! no TLS, no keep-alive, no session management, one SQLite connection behind a
//! mutex, and a shared bearer token.
//!
//! The library part lives in `lib.rs`; this binary only parses arguments.

use ravum_todo::{run, ServerConfig};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
        return;
    }
    let db_path = flag(&args, "--db")
        .map(PathBuf::from)
        .or_else(|| std::env::var("RAVUM_TODO_DB").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("target/ravum/todo.db"));
    let port = flag(&args, "--port")
        .and_then(|p| p.parse().ok())
        .or_else(|| {
            std::env::var("RAVUM_TODO_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
        })
        .unwrap_or(8080);
    let token = flag(&args, "--token")
        .or_else(|| std::env::var("RAVUM_TODO_TOKEN").ok())
        .unwrap_or_else(|| {
            eprintln!(
                "warning: no --token/RAVUM_TODO_TOKEN given, using the development token 'dev-token'; writes are NOT protected"
            );
            "dev-token".to_owned()
        });

    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match run(ServerConfig {
        db_path: db_path.clone(),
        token,
        port,
    }) {
        Ok(running) => {
            println!("ravum-todo listening on {}", running.url());
            println!("database: {}", db_path.display());
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        Err(e) => {
            eprintln!("cannot start: {e}");
            std::process::exit(2);
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage() {
    eprintln!(
        "ravum-todo — M4 reference application\n\n\
         USAGE:\n\
         \x20 ravum-todo [--db PATH] [--port N] [--token TOKEN]\n\n\
         ENV: RAVUM_TODO_DB, RAVUM_TODO_PORT, RAVUM_TODO_TOKEN\n\
         NOTE: listens on 127.0.0.1 only; no TLS, no sessions, one shared token."
    );
}
