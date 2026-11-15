//! The language server: document store, request dispatch, diagnostics.
//!
//! Analysis is synchronous and whole-package. That is a deliberate Core 1.0
//! trade-off: a package is small by construction (one crate root, no proc
//! macros) and the CLI already analyses a full package in milliseconds. The
//! server therefore *replaces* a previous result instead of racing two
//! analyses, which is why no stale result can ever be published: each reply and
//! each `publishDiagnostics` is computed from the buffer text as it is at that
//! moment.
//!
//! `$/cancelRequest` is accepted and ignored: there is nothing to cancel
//! because a request never outlives the message that triggered it. This is
//! stated in `docs/qualification/LSP.md` rather than silently dropped.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use ravum_lang_sema::Profile;
use ravum_lang_syntax::span::FileId;
use serde_json::{json, Value};

use crate::analysis::{override_key, package_root, Analysis};
use crate::convert::{diagnostic_json, TextIndex};
use crate::features;
use crate::paths::{path_to_uri, uri_to_path};
use crate::rpc::{self, Message};

pub const SERVER_NAME: &str = "ravum-lang-lsp";

#[derive(Debug, Clone)]
pub struct Document {
    pub uri: String,
    pub path: PathBuf,
    pub root: PathBuf,
    pub text: String,
    pub version: i64,
}

#[derive(Debug)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcError {
    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not implemented: {method}"),
        }
    }

    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }
}

pub struct Server {
    pub root: Option<PathBuf>,
    pub docs: BTreeMap<String, Document>,
    pub analyses: BTreeMap<PathBuf, Analysis>,
    /// Parse results reused across analyses (unchanged file => no re-parse).
    parse_cache: ravum_lang_sema::loader::ParseCache,
    /// Append-only session table `canonical path -> FileId` (T5): a file keeps
    /// its id — and the `NodeId`s that embed it — when another file is added or
    /// removed between two analyses.
    file_ids: BTreeMap<PathBuf, FileId>,
    /// Last diagnostics published per URI, so unchanged sets are not re-sent.
    published: BTreeMap<String, String>,
    initialized: bool,
    shutdown: bool,
    exit: bool,
    profile: Profile,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            root: None,
            docs: BTreeMap::new(),
            analyses: BTreeMap::new(),
            parse_cache: ravum_lang_sema::loader::ParseCache::new(),
            file_ids: BTreeMap::new(),
            published: BTreeMap::new(),
            initialized: false,
            shutdown: false,
            exit: false,
            profile: Profile::Core,
        }
    }

    pub fn expanded_exit(&self) -> bool {
        self.exit
    }

    /// Read/write loop. Returns on `exit` or EOF.
    pub fn run<R: BufRead, W: Write>(&mut self, mut reader: R, mut writer: W) -> io::Result<()> {
        loop {
            let message = match rpc::read_message(&mut reader) {
                Ok(Some(message)) => message,
                Ok(None) => return Ok(()),
                Err(error) => return Err(error),
            };
            let outgoing = self.handle(&message);
            for value in outgoing {
                rpc::write_message(&mut writer, &value)?;
            }
            if self.exit {
                return Ok(());
            }
        }
    }

    /// Handle one incoming message. Returns every outgoing message in order
    /// (a response first when the input was a request, then notifications).
    pub fn handle(&mut self, message: &Value) -> Vec<Value> {
        let Some(classified) = rpc::classify(message) else {
            return Vec::new();
        };
        match classified {
            Message::Notification { method, params } => self.notification(&method, &params),
            Message::Request { id, method, params } => {
                let outcome = self.request(&method, &params);
                let response = match outcome {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err(error) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": error.code, "message": error.message },
                    }),
                };
                vec![response]
            }
            Message::Response { .. } => Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Requests
    // -----------------------------------------------------------------------

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if method == "shutdown" {
            self.shutdown = true;
            return Ok(Value::Null);
        }
        if !self.initialized && method != "initialize" {
            return Err(RpcError {
                code: -32002,
                message: "server is not initialized; send `initialize` first".to_string(),
            });
        }
        match method {
            "initialize" => self.initialize(params),
            "textDocument/hover" => self.with_document(params, |analysis, file, offset| {
                Ok(features::hover(analysis, file, offset).unwrap_or(Value::Null))
            }),
            "textDocument/definition" => self.with_document(params, |analysis, file, offset| {
                Ok(features::definition(analysis, file, offset).unwrap_or(Value::Null))
            }),
            "textDocument/references" => {
                let include = params
                    .get("context")
                    .and_then(|context| context.get("includeDeclaration"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.with_document(params, |analysis, file, offset| {
                    Ok(
                        features::references(analysis, file, offset, include)
                            .unwrap_or(Value::Null),
                    )
                })
            }
            "textDocument/prepareRename" => self.with_document(params, |analysis, file, offset| {
                Ok(features::prepare_rename(analysis, file, offset).unwrap_or(Value::Null))
            }),
            "textDocument/rename" => {
                let new_name = params
                    .get("newName")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::invalid_params("`newName` is required"))?
                    .to_string();
                self.with_document(params, |analysis, file, offset| {
                    match features::rename(analysis, file, offset, &new_name) {
                        Ok(result) => Ok(result.unwrap_or(Value::Null)),
                        Err(message) => Err(RpcError {
                            code: -32603,
                            message,
                        }),
                    }
                })
            }
            "textDocument/completion" => {
                self.with_analysis(params, |analysis| Ok(features::completion(analysis)))
            }
            "textDocument/documentSymbol" => {
                let uri = uri_parameter(params)?;
                let analysis = self.analysis_for_uri(&uri)?;
                let file = self.file_id(&analysis, &uri)?;
                Ok(features::document_symbols(&analysis, file))
            }
            "workspace/symbol" => {
                let query = params
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let mut collected: Vec<Value> = Vec::new();
                for analysis in self.analyses.values() {
                    if let Value::Array(items) = features::workspace_symbols(analysis, &query) {
                        collected.extend(items);
                    }
                }
                Ok(Value::Array(collected))
            }
            "textDocument/formatting" => {
                let uri = uri_parameter(params)?;
                let document = self
                    .docs
                    .get(&uri)
                    .ok_or_else(|| RpcError::invalid_params(format!("document not open: {uri}")))?;
                Ok(features::formatting(&document.text).unwrap_or(Value::Null))
            }
            other => Err(RpcError::method_not_found(other)),
        }
    }

    fn initialize(&mut self, params: &Value) -> Result<Value, RpcError> {
        if self.initialized {
            return Err(RpcError {
                code: -32600,
                message: "`initialize` was already sent".to_string(),
            });
        }
        self.initialized = true;
        self.root = params
            .get("rootUri")
            .and_then(Value::as_str)
            .and_then(uri_to_path)
            .or_else(|| {
                params
                    .get("rootPath")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
            });
        Ok(json!({
            "capabilities": {
                "textDocumentSync": {
                    "openClose": true,
                    "change": 2,
                    "save": { "includeText": true },
                },
                "hoverProvider": true,
                "definitionProvider": true,
                "referencesProvider": true,
                "renameProvider": { "prepareProvider": true },
                "completionProvider": { "triggerCharacters": [".", ":"] },
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "documentFormattingProvider": true,
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
            },
            "positionEncoding": "utf-16",
        }))
    }

    fn with_document<F>(&mut self, params: &Value, run: F) -> Result<Value, RpcError>
    where
        F: FnOnce(&Analysis, u32, u32) -> Result<Value, RpcError>,
    {
        let uri = uri_parameter(params)?;
        let position = params
            .get("position")
            .ok_or_else(|| RpcError::invalid_params("`position` is required"))?;
        let line = position
            .get("line")
            .and_then(Value::as_u64)
            .ok_or_else(|| RpcError::invalid_params("`position.line` is required"))?
            as u32;
        let character = position
            .get("character")
            .and_then(Value::as_u64)
            .ok_or_else(|| RpcError::invalid_params("`position.character` is required"))?
            as u32;
        let document = self
            .docs
            .get(&uri)
            .ok_or_else(|| RpcError::invalid_params(format!("document not open: {uri}")))?;
        let text = document.text.clone();
        let index = TextIndex::new(&text);
        let offset = index.offset_at(&text, line, character);
        let analysis = self.analysis_for_uri(&uri)?;
        let file = self.file_id(&analysis, &uri)?;
        run(&analysis, file, offset)
    }

    fn with_analysis<F>(&mut self, params: &Value, run: F) -> Result<Value, RpcError>
    where
        F: FnOnce(&Analysis) -> Result<Value, RpcError>,
    {
        let uri = uri_parameter(params)?;
        let analysis = self.analysis_for_uri(&uri)?;
        run(&analysis)
    }

    fn analysis_for_uri(&self, uri: &str) -> Result<&Analysis, RpcError> {
        let path = uri_to_path(uri)
            .ok_or_else(|| RpcError::invalid_params(format!("unsupported URI: {uri}")))?;
        let root = self
            .docs
            .get(uri)
            .map(|document| document.root.clone())
            .unwrap_or_else(|| package_root(&path));
        self.analyses.get(&root).ok_or_else(|| {
            RpcError::internal(format!(
                "no analysis for package `{}` (open the file before querying it)",
                root.display()
            ))
        })
    }

    fn file_id(&self, analysis: &Analysis, uri: &str) -> Result<u32, RpcError> {
        let path = uri_to_path(uri)
            .ok_or_else(|| RpcError::invalid_params(format!("unsupported URI: {uri}")))?;
        let text = path.to_string_lossy();
        crate::convert::file_id_for_path(analysis.sources(), &text).ok_or_else(|| {
            RpcError::invalid_params(format!(
                "`{}` is not part of the analysed package",
                path.display()
            ))
        })
    }

    // -----------------------------------------------------------------------
    // Notifications
    // -----------------------------------------------------------------------

    fn notification(&mut self, method: &str, params: &Value) -> Vec<Value> {
        match method {
            "initialized" | "$/setTrace" | "workspace/didChangeConfiguration" => Vec::new(),
            "$/cancelRequest" => Vec::new(),
            "exit" => {
                self.exit = true;
                Vec::new()
            }
            "textDocument/didOpen" => self.did_open(params),
            "textDocument/didChange" => self.did_change(params),
            "textDocument/didSave" => Vec::new(),
            "textDocument/didClose" => self.did_close(params),
            "textDocument/didChangeWatchedFiles" => self.reanalyze_all(),
            "workspace/didChangeWatchedFiles" => self.reanalyze_all(),
            _ => Vec::new(),
        }
    }

    fn did_open(&mut self, params: &Value) -> Vec<Value> {
        let Some(item) = params.get("textDocument") else {
            return Vec::new();
        };
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        let version = item.get("version").and_then(Value::as_i64).unwrap_or(0);
        let root = package_root(&path);
        self.docs.insert(
            uri.to_string(),
            Document {
                uri: uri.to_string(),
                path,
                root: root.clone(),
                text: text.to_string(),
                version,
            },
        );
        self.analyze(&root)
    }

    fn did_change(&mut self, params: &Value) -> Vec<Value> {
        let Some(item) = params.get("textDocument") else {
            return Vec::new();
        };
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            return Vec::new();
        };
        let uri = uri.to_string();
        let Some(changes) = params.get("contentChanges").and_then(Value::as_array) else {
            return Vec::new();
        };
        let Some(document) = self.docs.get_mut(&uri) else {
            return Vec::new();
        };
        document.version = item
            .get("version")
            .and_then(Value::as_i64)
            .unwrap_or(document.version);
        for change in changes {
            if apply_change(&mut document.text, change).is_err() {
                // A malformed change must not corrupt the buffer: keep the last
                // good text and stop applying this batch.
                break;
            }
        }
        let root = document.root.clone();
        self.analyze(&root)
    }

    fn did_close(&mut self, params: &Value) -> Vec<Value> {
        let Some(uri) = params
            .get("textDocument")
            .and_then(|item| item.get("uri"))
            .and_then(Value::as_str)
        else {
            return Vec::new();
        };
        let removed = self.docs.remove(uri);
        let mut outgoing = Vec::new();
        if let Some(document) = removed {
            outgoing.push(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": uri, "diagnostics": [] },
            }));
            self.published.remove(uri);
            outgoing.extend(self.analyze(&document.root));
        }
        outgoing
    }

    fn reanalyze_all(&mut self) -> Vec<Value> {
        let roots: Vec<PathBuf> = self
            .analyses
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut outgoing = Vec::new();
        for root in roots {
            outgoing.extend(self.analyze(&root));
        }
        outgoing
    }

    // -----------------------------------------------------------------------
    // Analysis + diagnostics
    // -----------------------------------------------------------------------

    /// Re-analyse one package root and publish the diagnostic set.
    pub fn analyze(&mut self, root: &Path) -> Vec<Value> {
        let mut overrides: BTreeMap<PathBuf, String> = BTreeMap::new();
        for document in self.docs.values() {
            if document.root == root {
                overrides.insert(override_key(&document.path), document.text.clone());
            }
        }
        let analysis = Analysis::run_with_cache_and_file_ids(
            root,
            overrides,
            self.profile,
            &mut self.parse_cache,
            &mut self.file_ids,
        );
        let outgoing = self.publish(&analysis);
        self.analyses.insert(root.to_path_buf(), analysis);
        outgoing
    }

    fn publish(&mut self, analysis: &Analysis) -> Vec<Value> {
        let mut targets: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for (display, diagnostics) in analysis.by_file() {
            let Some(path) = features::document_path(analysis, &display) else {
                continue;
            };
            let uri = path_to_uri(&path);
            let converted = diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic_json(diagnostic, analysis.sources()))
                .collect::<Vec<_>>();
            targets.insert(uri, converted);
        }
        // Every open document of this package gets an answer, even when it has
        // no diagnostic, so stale squiggles disappear on fix.
        for (uri, document) in &self.docs {
            if document.root == analysis.root {
                targets.entry(uri.clone()).or_default();
            }
        }

        let mut outgoing = Vec::new();
        for (uri, diagnostics) in targets {
            let fingerprint = serde_json::to_string(&diagnostics).unwrap_or_default();
            if self.published.get(&uri) == Some(&fingerprint) {
                continue;
            }
            self.published.insert(uri.clone(), fingerprint);
            outgoing.push(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": uri, "diagnostics": diagnostics },
            }));
        }
        outgoing
    }
}

fn uri_parameter(params: &Value) -> Result<String, RpcError> {
    params
        .get("textDocument")
        .and_then(|item| item.get("uri"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| RpcError::invalid_params("`textDocument.uri` is required"))
}

fn apply_change(text: &mut String, change: &Value) -> Result<(), String> {
    let new_text = change
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| "change without `text`".to_string())?;
    match change.get("range") {
        Some(range) if !range.is_null() => {
            let index = TextIndex::new(text);
            let start = range
                .get("start")
                .ok_or_else(|| "range without `start`".to_string())?;
            let end = range
                .get("end")
                .ok_or_else(|| "range without `end`".to_string())?;
            let start_offset = index.offset_at(
                text,
                start.get("line").and_then(Value::as_u64).unwrap_or(0) as u32,
                start.get("character").and_then(Value::as_u64).unwrap_or(0) as u32,
            ) as usize;
            let end_offset = index.offset_at(
                text,
                end.get("line").and_then(Value::as_u64).unwrap_or(0) as u32,
                end.get("character").and_then(Value::as_u64).unwrap_or(0) as u32,
            ) as usize;
            if start_offset > end_offset || end_offset > text.len() {
                return Err("change range is not a valid span".to_string());
            }
            text.replace_range(start_offset..end_offset, new_text);
        }
        _ => *text = new_text.to_string(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_change(text: &str) -> Value {
        json!({ "text": text })
    }

    #[test]
    fn full_change_replaces_the_buffer() {
        let mut text = "old".to_string();
        apply_change(&mut text, &full_change("new")).unwrap();
        assert_eq!(text, "new");
    }

    #[test]
    fn incremental_change_replaces_only_the_range() {
        let mut text = "let x = 1;".to_string();
        let change = json!({
            "range": {
                "start": { "line": 0, "character": 8 },
                "end": { "line": 0, "character": 9 },
            },
            "text": "42",
        });
        apply_change(&mut text, &change).unwrap();
        assert_eq!(text, "let x = 42;");
    }

    #[test]
    fn invalid_range_is_refused_without_corrupting() {
        let mut text = "abc".to_string();
        let change = json!({
            "range": {
                "start": { "line": 99, "character": 0 },
                "end": { "line": 0, "character": 0 },
            },
            "text": "x",
        });
        assert!(apply_change(&mut text, &change).is_err());
        assert_eq!(text, "abc");
    }
}
