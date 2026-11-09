//! `ravum batch-check` — collecteur de lots borné au service des propositions
//! multi-agents (MVP-0c, L2.4 → produit).
//!
//! Ce que fait la commande, concrètement :
//!
//! 1. lit un fichier de propositions JSON (plusieurs agents, même base) ;
//! 2. copie le workspace dans `--state` (aucune écriture dans le workspace
//!    utilisateur) et y applique les éditions ;
//! 3. confie les propositions au collecteur `ravum-batch` : pré-vol de conflits
//!    (chemins ou symboles), fenêtre bornée, repli conservateur N+1 ;
//! 4. exécute un vrai `cargo test --offline` par lot (et par proposition en cas
//!    de repli) avec un `CARGO_TARGET_DIR` partagé pour rester incrémental ;
//! 5. rapporte les lots, les verdicts, la télémétrie et vérifie qu'aucune
//!    proposition n'est perdue.
//!
//! La sûreté ne repose **pas** sur la seule segmentation textuelle : le pré-vol
//! est un filtre bon marché, et chaque lot est accepté uniquement après un
//! `cargo test` réussi. Un lot refusé repasse proposition par proposition.
//!
//! Codes de sortie : `0` toutes les propositions acceptées, `1` au moins une
//! proposition refusée, `2` usage/propositions invalides, `3` incertain
//! (workspace modifié pendant l'analyse, cargo indisponible, copie impossible).

use crate::unit_check::{copy_workspace, display_path, workspace_fingerprint};
use ravum_batch::{
    Analyzer, BatchCollector, BatchConfig, BatchOutcome, BatchProposal, BatchReason, Verdict,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const EXIT_OK: u8 = 0;
const EXIT_REJECTED: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_UNCERTAIN: u8 = 3;

const REPORT_SCHEMA_VERSION: u32 = 1;

/// Entry point for `ravum batch-check`.
pub fn batch_check(args: &[String]) -> u8 {
    if args
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        print_help();
        return EXIT_OK;
    }
    match run(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("batch-check: {}", error.message);
            error.code
        }
    }
}

#[derive(Debug)]
struct CommandError {
    code: u8,
    message: String,
}

impl CommandError {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self::new(EXIT_USAGE, message)
    }

    fn uncertain(message: impl Into<String>) -> Self {
        Self::new(EXIT_UNCERTAIN, message)
    }
}

type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Granularity {
    Symbols,
    Files,
}

#[derive(Debug)]
struct Options {
    workspace: PathBuf,
    proposals: PathBuf,
    state: Option<PathBuf>,
    window_ms: u64,
    max_batch: usize,
    granularity: Granularity,
    json: bool,
    out: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> CommandResult<Options> {
    let mut workspace: Option<PathBuf> = None;
    let mut proposals: Option<PathBuf> = None;
    let mut state: Option<PathBuf> = None;
    let mut window_ms = ravum_batch::DEFAULT_WINDOW_MS;
    let mut max_batch = ravum_batch::DEFAULT_MAX_BATCH;
    let mut granularity = Granularity::Symbols;
    let mut json = false;
    let mut out: Option<PathBuf> = None;
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--json" => json = true,
            "--proposals" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--proposals requires a JSON file"))?;
                proposals = Some(PathBuf::from(value));
            }
            "--state" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--state requires a directory"))?;
                state = Some(PathBuf::from(value));
            }
            "--out" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--out requires a file"))?;
                out = Some(PathBuf::from(value));
            }
            "--window-ms" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--window-ms requires milliseconds"))?;
                window_ms = value.parse().map_err(|_| {
                    CommandError::usage(format!("--window-ms is not a number: {value}"))
                })?;
            }
            "--max-batch" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--max-batch requires a count"))?;
                max_batch = value.parse().map_err(|_| {
                    CommandError::usage(format!("--max-batch is not a number: {value}"))
                })?;
            }
            "--granularity" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| CommandError::usage("--granularity requires symbols|files"))?;
                granularity = match value.as_str() {
                    "symbols" => Granularity::Symbols,
                    "files" => Granularity::Files,
                    other => {
                        return Err(CommandError::usage(format!(
                            "--granularity must be symbols or files, got {other}"
                        )))
                    }
                };
            }
            other if other.starts_with("--") => {
                return Err(CommandError::usage(format!("unknown option {other}")));
            }
            other => {
                if workspace.is_some() {
                    return Err(CommandError::usage(format!(
                        "unexpected extra argument {other:?} (one workspace only)"
                    )));
                }
                workspace = Some(PathBuf::from(other));
            }
        }
    }
    let workspace = workspace.ok_or_else(|| CommandError::usage("missing <workspace> argument"))?;
    let proposals =
        proposals.ok_or_else(|| CommandError::usage("missing --proposals <file> argument"))?;
    Ok(Options {
        workspace,
        proposals,
        state,
        window_ms,
        max_batch,
        granularity,
        json,
        out,
    })
}

fn print_help() {
    println!(
        "ravum batch-check <workspace> --proposals <file> [options]\n\
         \n\
         Regroupe des propositions multi-agents compatibles, applique les éditions\n\
         sur une copie du workspace et vérifie chaque lot avec un vrai\n\
         `cargo test --offline` (repli proposition par proposition si le lot échoue).\n\
         \n\
         Le workspace utilisateur n'est jamais modifié : l'analyse se fait dans\n\
         --state (défaut : <workspace>/target/ravum/batch-check).\n\
         \n\
         PROPOSALS FILE (JSON):\n\
         \x20 {{\n\
         \x20   \"base_id\": \"<identité de la base CAS>\",\n\
         \x20   \"proposals\": [\n\
         \x20     {{ \"id\": \"agent-1/t1\", \"files\": [\"src/lib.rs\"],\n\
         \x20       \"symbols\": [\"alpha\"],\n\
         \x20       \"edits\": [{{ \"path\": \"src/lib.rs\", \"old\": \"...\", \"new\": \"...\" }}] }}\n\
         \x20   ]\n\
         \x20 }}\n\
         \n\
         OPTIONS:\n\
         \x20 --proposals <file>     propositions JSON (obligatoire)\n\
         \x20 --state <dir>         répertoire de travail (copie + CARGO_TARGET_DIR)\n\
         \x20 --window-ms <n>       fenêtre du collecteur, 0..=100 (défaut 50)\n\
         \x20 --max-batch <n>       taille de lot, 1..=8 (défaut 6)\n\
         \x20 --granularity <mode>  symbols (défaut) : pré-vol par symboles, sûreté\n\
         \x20                      par cargo test ; files : pré-vol par fichiers\n\
         \x20 --json                rapport JSON sur stdout\n\
         \x20 --out <file>          écrit aussi le rapport JSON dans <file>\n\
         \n\
         EXIT: 0 tout accepté, 1 au moins un refus, 2 usage invalide, 3 incertain."
    );
}

/// One proposal as written in the JSON file.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct EditInput {
    path: String,
    old: String,
    new: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProposalInput {
    id: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    symbols: Vec<String>,
    edits: Vec<EditInput>,
}

/// The proposals file as a whole.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct ProposalsFile {
    base_id: String,
    proposals: Vec<ProposalInput>,
}

/// Proposal view consumed by `ravum-batch` pre-flight.
struct CliProposal {
    id: String,
    files: Vec<String>,
    symbols: Vec<String>,
    base_id: String,
    use_files: bool,
}

impl BatchProposal for CliProposal {
    fn proposal_id(&self) -> &str {
        &self.id
    }

    fn paths(&self) -> &[String] {
        if self.use_files {
            &self.files
        } else {
            &[]
        }
    }

    fn symbols(&self) -> &[String] {
        &self.symbols
    }

    fn base_id(&self) -> &str {
        &self.base_id
    }
}

/// Analyzer backed by a real `cargo test --offline` on a copy of the workspace.
struct CargoTestAnalyzer {
    pristine: PathBuf,
    work: PathBuf,
    target_dir: PathBuf,
    proposals: BTreeMap<String, ProposalInput>,
}

impl CargoTestAnalyzer {
    fn restore(&self, relative: &str) -> Result<(), String> {
        let source = self.pristine.join(relative);
        let destination = self.work.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::copy(&source, &destination)
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "cannot restore {} from the pristine copy: {error}",
                    destination.display()
                )
            })
    }

    fn apply(&self, edit: &EditInput) -> Result<(), String> {
        let path = self.work.join(&edit.path);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let occurrences = text.matches(&edit.old).count();
        if occurrences != 1 {
            return Err(format!(
                "edit on {} expects exactly one occurrence of the old text, found {occurrences}",
                edit.path
            ));
        }
        let updated = text.replacen(&edit.old, &edit.new, 1);
        std::fs::write(&path, updated)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))
    }

    fn run_cargo_test(&self) -> Verdict {
        let output = Command::new("cargo")
            .arg("test")
            .arg("--offline")
            .arg("--quiet")
            .current_dir(&self.work)
            .env("CARGO_TARGET_DIR", &self.target_dir)
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                return Verdict::failed(format!(
                    "cannot run `cargo test --offline` in {}: {error}",
                    self.work.display()
                ));
            }
        };
        if output.status.success() {
            return Verdict::passed();
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Verdict::failed(format!(
            "cargo test failed (exit {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.status.code(),
            tail(&stdout, 1500),
            tail(&stderr, 1500)
        ))
    }
}

impl Analyzer for CargoTestAnalyzer {
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict {
        let mut files = BTreeSet::new();
        for id in proposal_ids {
            match self.proposals.get(*id) {
                Some(proposal) => {
                    for file in &proposal.files {
                        files.insert(file.clone());
                    }
                }
                None => return Verdict::failed(format!("unknown proposal id {id}")),
            }
        }
        for file in &files {
            if let Err(error) = self.restore(file) {
                return Verdict::failed(error);
            }
        }
        for id in proposal_ids {
            let Some(proposal) = self.proposals.get(*id) else {
                return Verdict::failed(format!("unknown proposal id {id}"));
            };
            for edit in &proposal.edits {
                if let Err(error) = self.apply(edit) {
                    return Verdict::failed(error);
                }
            }
        }
        self.run_cargo_test()
    }
}

fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let start = text.len() - limit;
    format!("…{}", &text[start..])
}

fn validate_relative_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("edit path must not be empty".to_owned());
    }
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(format!("edit path must be relative: {path}"));
    }
    for component in candidate.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(format!("edit path must not contain `..`: {path}"));
        }
    }
    Ok(())
}

fn load_proposals(path: &Path) -> CommandResult<ProposalsFile> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CommandError::usage(format!("cannot read {}: {error}", path.display())))?;
    let mut file: ProposalsFile = serde_json::from_str(&text).map_err(|error| {
        CommandError::usage(format!(
            "{} is not a valid proposals file: {error}",
            path.display()
        ))
    })?;
    if file.base_id.trim().is_empty() {
        return Err(CommandError::usage("base_id must not be empty"));
    }
    if file.proposals.is_empty() {
        return Err(CommandError::usage("proposals must not be empty"));
    }
    let mut ids = BTreeSet::new();
    for proposal in &mut file.proposals {
        if proposal.id.trim().is_empty() {
            return Err(CommandError::usage("proposal id must not be empty"));
        }
        if !ids.insert(proposal.id.clone()) {
            return Err(CommandError::usage(format!(
                "duplicate proposal id {}",
                proposal.id
            )));
        }
        if proposal.edits.is_empty() {
            return Err(CommandError::usage(format!(
                "proposal {} has no edits",
                proposal.id
            )));
        }
        let mut files: BTreeSet<String> = proposal.files.iter().cloned().collect();
        for edit in &proposal.edits {
            validate_relative_path(&edit.path).map_err(CommandError::usage)?;
            if edit.old.is_empty() {
                return Err(CommandError::usage(format!(
                    "proposal {} has an empty old text for {}",
                    proposal.id, edit.path
                )));
            }
            files.insert(edit.path.clone());
        }
        proposal.files = files.into_iter().collect();
    }
    Ok(file)
}

fn run(args: &[String]) -> CommandResult<u8> {
    let options = parse_args(args)?;
    let config = BatchConfig::new(options.window_ms, options.max_batch)
        .map_err(|error| CommandError::usage(format!("invalid batch config: {error}")))?;
    let workspace = options
        .workspace
        .canonicalize()
        .map_err(|error| CommandError::usage(format!("workspace not found: {error}")))?;
    if !workspace.is_dir() {
        return Err(CommandError::usage(format!(
            "workspace is not a directory: {}",
            workspace.display()
        )));
    }
    let proposals_file = load_proposals(&options.proposals)?;
    let state_dir = options
        .state
        .clone()
        .unwrap_or_else(|| workspace.join("target/ravum/batch-check"));
    std::fs::create_dir_all(&state_dir).map_err(|error| {
        CommandError::uncertain(format!(
            "cannot create state directory {}: {error}",
            state_dir.display()
        ))
    })?;

    let fingerprint = workspace_fingerprint(&workspace, &state_dir).map_err(|error| {
        CommandError::uncertain(format!("cannot fingerprint workspace: {error}"))
    })?;
    let copy_root = state_dir.join("workspaces").join(&fingerprint[..12]);
    let pristine = copy_root.join("pristine");
    let work = copy_root.join("work");
    let target_dir = state_dir.join("cargo-target");
    if !pristine.is_dir() {
        copy_workspace(&workspace, &pristine, &state_dir)
            .map_err(|error| CommandError::uncertain(format!("cannot copy workspace: {error}")))?;
    }
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|error| {
            CommandError::uncertain(format!("cannot reset {}: {error}", work.display()))
        })?;
    }
    // The pristine copy lives inside `--state`, so it must not be excluded from
    // its own copy; pass the destination as the (irrelevant) exclusion root.
    copy_workspace(&pristine, &work, &work).map_err(|error| {
        CommandError::uncertain(format!("cannot prepare working copy: {error}"))
    })?;

    let mut proposal_map = BTreeMap::new();
    for proposal in &proposals_file.proposals {
        proposal_map.insert(proposal.id.clone(), proposal.clone());
    }
    let analyzer = CargoTestAnalyzer {
        pristine: pristine.clone(),
        work: work.clone(),
        target_dir: target_dir.clone(),
        proposals: proposal_map,
    };
    let mut collector = BatchCollector::new(config.clone(), analyzer);

    let mut submitted = Vec::new();
    for (index, proposal) in proposals_file.proposals.iter().enumerate() {
        let external = CliProposal {
            id: proposal.id.clone(),
            files: proposal.files.clone(),
            symbols: proposal.symbols.clone(),
            base_id: proposals_file.base_id.clone(),
            use_files: options.granularity == Granularity::Files,
        };
        collector
            .submit(external, index as u64)
            .map_err(|error| CommandError::usage(format!("cannot submit proposal: {error}")))?;
        submitted.push(proposal.id.clone());
    }
    let now = proposals_file.proposals.len() as u64 + options.window_ms + 1;
    let reports = collector.flush(now);
    let telemetry = collector.telemetry().clone();

    let mut accepted = BTreeSet::new();
    let mut rejected = BTreeSet::new();
    let mut batch_json = Vec::new();
    for report in &reports {
        for id in &report.accepted_ids {
            accepted.insert(id.clone());
        }
        for id in &report.rejected_ids {
            rejected.insert(id.clone());
        }
        batch_json.push(serde_json::to_value(report).map_err(|error| {
            CommandError::new(
                EXIT_USAGE,
                format!("cannot serialize batch report: {error}"),
            )
        })?);
    }
    let fingerprint_after = workspace_fingerprint(&workspace, &state_dir).map_err(|error| {
        CommandError::uncertain(format!("cannot re-fingerprint workspace: {error}"))
    })?;
    let workspace_unchanged = fingerprint_after == fingerprint;
    let all_accepted = rejected.is_empty() && accepted.len() == proposals_file.proposals.len();
    let no_loss = telemetry.proposals_received as usize == accepted.len() + rejected.len()
        && accepted.len() + rejected.len() == proposals_file.proposals.len();

    let payload = json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "command": "batch-check",
        "workspace": display_path(&workspace),
        "proposals_file": display_path(&options.proposals),
        "base_id": proposals_file.base_id,
        "config": {
            "window_ms": config.window_ms(),
            "max_batch": config.max_batch(),
            "granularity": match options.granularity { Granularity::Symbols => "symbols", Granularity::Files => "files" },
        },
        "proposals": proposals_file.proposals.iter().map(|proposal| json!({
            "id": proposal.id,
            "files": proposal.files,
            "symbols": proposal.symbols,
            "edits": proposal.edits.len(),
        })).collect::<Vec<_>>(),
        "batches": batch_json,
        "telemetry": telemetry,
        "summary": {
            "proposed": proposals_file.proposals.len(),
            "accepted": accepted.len(),
            "rejected": rejected.len(),
            "accepted_ids": accepted.iter().cloned().collect::<Vec<_>>(),
            "rejected_ids": rejected.iter().cloned().collect::<Vec<_>>(),
            "analyses_used": telemetry.analyses_used,
            "individual_analyses": telemetry.individual_analyses,
            "analyses_saved_net": telemetry.analyses_saved_net,
            "batches": telemetry.batches_formed,
            "fell_back": telemetry.batches_fell_back,
        },
        "validation": {
            "no_loss": no_loss,
            "workspace_unchanged": workspace_unchanged,
            "all_accepted": all_accepted,
            "submitted": submitted,
            "fingerprint_before": fingerprint,
            "fingerprint_after": fingerprint_after,
        },
        "analysis_copy": display_path(&work),
        "limits": [
            "la copie du workspace est refaite depuis la copie vierge pour chaque analyse ; le cache Cargo est partagé via CARGO_TARGET_DIR",
            "`cargo test --offline` est la seule vérification : pas de preuve formelle (aucun PROVED)",
            "le pré-vol est textuel : un conflit sémantique imprévu est détecté par le cargo test puis replié proposition par proposition",
            "granularity=symbols déclare des chemins vides au collecteur (sûreté par test) ; granularity=files isole les éditions d'un même fichier",
        ],
    });

    if let Some(out) = &options.out {
        std::fs::write(
            out,
            serde_json::to_string_pretty(&payload).unwrap_or_default(),
        )
        .map_err(|error| {
            CommandError::new(
                EXIT_USAGE,
                format!("cannot write {}: {error}", out.display()),
            )
        })?;
    }

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_owned())
        );
    } else {
        render_human(&payload, &reports);
    }

    if !workspace_unchanged {
        eprintln!("batch-check: workspace changed during the analysis (STALE)");
        return Ok(EXIT_UNCERTAIN);
    }
    if !no_loss {
        eprintln!("batch-check: internal error, a proposal is missing from the reports");
        return Ok(EXIT_UNCERTAIN);
    }
    Ok(if all_accepted { EXIT_OK } else { EXIT_REJECTED })
}

fn render_human(payload: &serde_json::Value, reports: &[ravum_batch::BatchReport]) {
    let summary = &payload["summary"];
    println!(
        "batch-check: {} proposition(s), {} acceptée(s), {} refusée(s)",
        summary["proposed"], summary["accepted"], summary["rejected"]
    );
    for report in reports {
        let outcome = match &report.outcome {
            BatchOutcome::AcceptedAll => "lot accepté".to_owned(),
            BatchOutcome::FellBack { rejected_ids } => {
                format!("repli individuel (refus: {})", rejected_ids.join(", "))
            }
        };
        let reason = match report.reason {
            BatchReason::Full => "lot plein",
            BatchReason::WindowExpired => "fenêtre expirée",
            BatchReason::Flush => "fin de flux",
        };
        println!(
            "  lot {} [{}] {} : {} proposition(s), attente {} ms, {} analyse(s)",
            report.batch_id,
            reason,
            outcome,
            report.proposal_ids.len(),
            report.wait_ms,
            report.analyses_used
        );
        for id in &report.accepted_ids {
            println!("    acceptée : {id}");
        }
        for id in &report.rejected_ids {
            println!("    refusée  : {id}");
        }
    }
    let telemetry = &payload["telemetry"];
    println!(
        "télémétrie: {} lot(s), {} analyse(s), économie nette {}, repli {}",
        telemetry["batches_formed"],
        telemetry["analyses_used"],
        telemetry["analyses_saved_net"],
        telemetry["batches_fell_back"]
    );
    println!(
        "validation: no_loss={}, workspace_unchanged={}",
        payload["validation"]["no_loss"], payload["validation"]["workspace_unchanged"]
    );
}
