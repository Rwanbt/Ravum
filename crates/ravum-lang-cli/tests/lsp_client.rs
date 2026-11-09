//! End-to-end language-server test: a hand-written LSP client drives the real
//! `ravum-lang lsp` binary over pipes.
//!
//! The framing is implemented *here*, independently of `ravum-lang-lsp::rpc`,
//! so the test proves wire interoperability rather than internal consistency.
//! Every wait is bounded: a hung server fails the test instead of blocking the
//! suite.

use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

static COUNTER: AtomicU64 = AtomicU64::new(0);

const TIMEOUT: Duration = Duration::from_secs(30);

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ravum-lang")
}

// ---------------------------------------------------------------------------
// Minimal temp package
// ---------------------------------------------------------------------------

struct TempPackage {
    dir: PathBuf,
}

impl TempPackage {
    fn new(tag: &str, source: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "ravum-lsp-test-{tag}-{}-{}-{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("src")).expect("create package");
        fs::write(
            dir.join("ravum.toml"),
            "[package]\nname = \"lspdemo\"\nversion = \"0.1.0\"\nedition = \"core-1.0\"\n",
        )
        .expect("write manifest");
        fs::write(dir.join("src").join("main.rv"), source).expect("write source");
        Self { dir }
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    fn main_path(&self) -> PathBuf {
        self.dir.join("src").join("main.rv")
    }

    fn main_uri(&self) -> String {
        uri_for(&self.main_path())
    }
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn uri_for(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

// ---------------------------------------------------------------------------
// Framing (independent implementation)
// ---------------------------------------------------------------------------

fn read_framed<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            length = Some(
                rest.trim()
                    .parse()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad length"))?,
            );
        }
    }
    let length = length.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no length"))?;
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let value = serde_json::from_slice(&body)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some(value))
}

struct LspClient {
    child: Child,
    stdin: ChildStdin,
    receiver: Receiver<Value>,
    next_id: i64,
}

impl LspClient {
    fn start() -> Self {
        let mut child = Command::new(binary())
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ravum-lang lsp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_framed(&mut reader) {
                    Ok(Some(value)) => {
                        if sender.send(value).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });
        Self {
            child,
            stdin,
            receiver,
            next_id: 1,
        }
    }

    fn send(&mut self, value: &Value) {
        let body = serde_json::to_vec(value).expect("serialize");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
        self.stdin.write_all(&body).expect("write body");
        self.stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn incoming(&self) -> Value {
        self.receiver
            .recv_timeout(TIMEOUT)
            .expect("language server must answer within the timeout")
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        loop {
            let message = self.incoming();
            if message.get("id") == Some(&json!(id)) && message.get("method").is_none() {
                return message;
            }
        }
    }

    fn wait_notification(&self, method: &str) -> Value {
        loop {
            let message = self.incoming();
            if message.get("method").and_then(Value::as_str) == Some(method) {
                return message;
            }
        }
    }

    fn initialize(&mut self, root: &Path) -> Value {
        let response = self.request(
            "initialize",
            json!({
                "processId": serde_json::Value::Null,
                "rootUri": uri_for(root),
                "capabilities": {},
            }),
        );
        self.notify("initialized", json!({}));
        response
    }

    fn open(&mut self, uri: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "ravum",
                    "version": 1,
                    "text": text,
                }
            }),
        );
    }

    fn shutdown_and_exit(&mut self) {
        let response = self.request("shutdown", json!(null));
        assert_eq!(response.get("result"), Some(&Value::Null));
        self.notify("exit", json!(null));
        let status = self.child.wait().expect("wait for server exit");
        assert!(
            status.success(),
            "a clean shutdown must exit 0, got {status:?}"
        );
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Positions
// ---------------------------------------------------------------------------

fn offset_of(source: &str, needle: &str) -> u32 {
    source
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not found in source")) as u32
}

fn position_of(source: &str, needle: &str) -> Value {
    let offset = offset_of(source, needle) as usize;
    let before = &source[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let character: usize = source[line_start..offset].encode_utf16().count();
    json!({ "line": line, "character": character })
}

fn diagnostics_of(message: &Value) -> &Vec<Value> {
    message["params"]["diagnostics"]
        .as_array()
        .expect("publishDiagnostics.diagnostics must be an array")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

const SOURCE: &str = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\nfn main() {\n    let total: i32 = add(1, 2);\n    io::println(\"done\");\n}\n";

#[test]
fn initialize_advertises_a_real_language_server() {
    let package = TempPackage::new("init", SOURCE);
    let mut client = LspClient::start();
    let response = client.initialize(package.dir());
    let result = &response["result"];
    assert_eq!(result["serverInfo"]["name"], "ravum-lang-lsp");
    assert_eq!(result["capabilities"]["textDocumentSync"]["change"], 2);
    assert_eq!(result["capabilities"]["hoverProvider"], true);
    assert_eq!(result["capabilities"]["definitionProvider"], true);
    assert_eq!(result["capabilities"]["referencesProvider"], true);
    assert_eq!(
        result["capabilities"]["renameProvider"]["prepareProvider"], true,
        "rename advertises prepareRename, which VS Code requires before showing the box"
    );
    assert_eq!(result["capabilities"]["documentSymbolProvider"], true);
    assert_eq!(result["capabilities"]["documentFormattingProvider"], true);
    assert_eq!(result["positionEncoding"], "utf-16");
    client.shutdown_and_exit();
}

#[test]
fn clean_buffer_publishes_no_diagnostic() {
    let package = TempPackage::new("clean", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let published = client.wait_notification("textDocument/publishDiagnostics");
    assert_eq!(
        published["params"]["uri"].as_str().unwrap().to_lowercase(),
        package.main_uri().to_lowercase()
    );
    assert!(
        diagnostics_of(&published).is_empty(),
        "a clean program must publish no diagnostic, got {:?}",
        diagnostics_of(&published)
    );
    client.shutdown_and_exit();
}

#[test]
fn unsaved_buffer_is_analysed_without_touching_the_file() {
    let package = TempPackage::new("dirty", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    let broken = "fn main() {\n    let value: i32 = \"text\";\n}\n";
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": package.main_uri(), "version": 2 },
            "contentChanges": [ { "text": broken } ],
        }),
    );
    let published = client.wait_notification("textDocument/publishDiagnostics");
    let diagnostics = diagnostics_of(&published);
    assert!(
        !diagnostics.is_empty(),
        "an unsaved type error must produce a diagnostic"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["severity"] == 1),
        "the diagnostic must be an error"
    );
    // The file on disk is untouched: the server never writes the user's source.
    let on_disk = fs::read_to_string(package.main_path()).expect("read source");
    assert_eq!(on_disk, SOURCE);
    client.shutdown_and_exit();
}

#[test]
fn incremental_change_is_applied_to_the_buffer() {
    let package = TempPackage::new("incremental", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    // Replace `1` with `1 + ` in `add(1, 2)` -> `add(1 + , 2)` which cannot parse
    // if the range edit is applied; then restore it and expect clean output.
    let position = position_of(SOURCE, "add(1");
    let start = position["character"].as_u64().unwrap();
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": package.main_uri(), "version": 2 },
            "contentChanges": [{
                "range": {
                    "start": { "line": position["line"], "character": start + 4 },
                    "end": { "line": position["line"], "character": start + 4 },
                },
                "text": " + ",
            }],
        }),
    );
    let published = client.wait_notification("textDocument/publishDiagnostics");
    assert!(
        !diagnostics_of(&published).is_empty(),
        "an incomplete expression must be reported after an incremental edit"
    );
    client.shutdown_and_exit();
}

#[test]
fn hover_shows_the_builtin_signature() {
    let package = TempPackage::new("hover", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    let response = client.request(
        "textDocument/hover",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": position_of(SOURCE, "println"),
        }),
    );
    let value = response["result"]["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("hover must return markdown, got {response}"));
    assert!(
        value.contains("println"),
        "hover must describe `println`, got {value}"
    );
    assert!(
        value.contains("io::println") || value.contains("std::io::println"),
        "hover must show the resolved builtin path, got {value}"
    );
    client.shutdown_and_exit();
}

#[test]
fn definition_and_references_find_the_declaration() {
    let package = TempPackage::new("nav", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    // The call site `add(1, 2)`.
    let call_position = position_of(SOURCE, "add(1, 2)");
    let response = client.request(
        "textDocument/definition",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": call_position,
        }),
    );
    let locations = response["result"]
        .as_array()
        .unwrap_or_else(|| panic!("definition must return an array, got {response}"));
    assert_eq!(locations.len(), 1, "expected one definition: {locations:?}");
    assert_eq!(
        locations[0]["range"]["start"]["line"].as_u64(),
        Some(0),
        "the declaration of `add` is on line 0"
    );
    assert_eq!(
        locations[0]["range"]["start"]["character"].as_u64(),
        Some(3),
        "the declaration name starts after `fn `"
    );

    let response = client.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": call_position,
            "context": { "includeDeclaration": true },
        }),
    );
    let references = response["result"].as_array().expect("references array");
    assert_eq!(
        references.len(),
        2,
        "one declaration + one call, got {references:?}"
    );
    client.shutdown_and_exit();
}

#[test]
fn rename_rewrites_declaration_and_uses() {
    let package = TempPackage::new("rename", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    let response = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": position_of(SOURCE, "add(1, 2)"),
            "newName": "sum",
        }),
    );
    let edits = response["result"]["changes"]
        .as_object()
        .unwrap_or_else(|| panic!("rename must return a WorkspaceEdit, got {response}"));
    let all: Vec<&Value> = edits
        .values()
        .flat_map(|list| list.as_array().unwrap())
        .collect();
    assert_eq!(all.len(), 2, "declaration + call must both be renamed");
    assert!(all.iter().all(|edit| edit["newText"] == "sum"));

    // An invalid identifier is refused with a JSON-RPC error, never a silent
    // no-op.
    let response = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": position_of(SOURCE, "add(1, 2)"),
            "newName": "fn",
        }),
    );
    assert!(
        response.get("error").is_some(),
        "renaming to a keyword must be an error: {response}"
    );
    client.shutdown_and_exit();
}

#[test]
fn navigation_from_the_declaration_name_works() {
    let package = TempPackage::new("decl", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    // `fn add` starts at column 0; column 3 is inside the declared name.
    let mut declaration = position_of(SOURCE, "fn add");
    declaration["character"] = json!(declaration["character"].as_u64().unwrap() + 3);

    let response = client.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": declaration,
            "context": { "includeDeclaration": true },
        }),
    );
    let references = response["result"]
        .as_array()
        .unwrap_or_else(|| panic!("references must return an array, got {response}"));
    assert_eq!(
        references.len(),
        2,
        "declaration + one call, from the declaration site: {references:?}"
    );

    let response = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": declaration,
            "newName": "sum",
        }),
    );
    let edits = response["result"]["changes"]
        .as_object()
        .unwrap_or_else(|| panic!("rename must return a WorkspaceEdit, got {response}"));
    let all: Vec<&Value> = edits
        .values()
        .flat_map(|list| list.as_array().unwrap())
        .collect();
    assert_eq!(all.len(), 2, "declaration + call must both be renamed");
    assert!(all.iter().all(|edit| edit["newText"] == "sum"));

    // `textDocument/prepareRename` must answer on the declaration name too: a
    // client that sends it and gets `null` refuses the rename outright.
    let response = client.request(
        "textDocument/prepareRename",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": declaration,
        }),
    );
    let range = &response["result"];
    assert_eq!(
        range["start"]["character"].as_u64(),
        Some(3),
        "prepareRename must cover the declared name: {response}"
    );

    // Negative: a cursor with no symbol must yield `null`, never a fabricated
    // edit that would rename nothing (or the wrong thing).
    let blank = json!({ "line": 3, "character": 0 });
    let response = client.request(
        "textDocument/rename",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": blank,
            "newName": "sum",
        }),
    );
    assert!(
        response["result"].is_null(),
        "a blank line has no symbol to rename: {response}"
    );
    client.shutdown_and_exit();
}

#[test]
fn symbols_completion_and_formatting_answer() {
    let package = TempPackage::new("tools", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    client.open(&package.main_uri(), SOURCE);
    let _ = client.wait_notification("textDocument/publishDiagnostics");

    let response = client.request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": package.main_uri() } }),
    );
    let symbols = response["result"].as_array().expect("symbol array");
    let names: Vec<&str> = symbols
        .iter()
        .filter_map(|symbol| symbol["name"].as_str())
        .collect();
    assert!(names.contains(&"add"), "symbols were {names:?}");
    assert!(names.contains(&"main"), "symbols were {names:?}");

    let response = client.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "position": position_of(SOURCE, "total"),
        }),
    );
    let labels: Vec<&str> = response["result"]["items"]
        .as_array()
        .expect("completion items")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"fn"), "keywords must be offered");
    assert!(labels.contains(&"add"), "package items must be offered");

    let ugly = "fn main(){let x:i32=1;}\n";
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": package.main_uri(), "version": 2 },
            "contentChanges": [ { "text": ugly } ],
        }),
    );
    // No `publishDiagnostics` is required here: the server suppresses an
    // unchanged diagnostic set, and this buffer may produce none at all. The
    // request below is the real assertion.
    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": package.main_uri() },
            "options": { "tabSize": 4, "insertSpaces": true },
        }),
    );
    let edits = response["result"].as_array().expect("formatting edits");
    assert_eq!(edits.len(), 1, "formatting must rewrite the document once");
    let new_text = edits[0]["newText"].as_str().expect("newText");
    assert_ne!(
        new_text, ugly,
        "the formatter must change unformatted source"
    );
    assert!(new_text.contains("fn main()"));
    client.shutdown_and_exit();
}

#[test]
fn unknown_request_returns_method_not_found() {
    let package = TempPackage::new("unknown", SOURCE);
    let mut client = LspClient::start();
    client.initialize(package.dir());
    let response = client.request("textDocument/codeAction", json!({}));
    assert_eq!(response["error"]["code"], -32601);
    client.shutdown_and_exit();
}
