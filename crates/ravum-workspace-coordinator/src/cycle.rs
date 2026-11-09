//! Cycle transactionnel de bout en bout : overlay → vérification → publication Git.
//!
//! Le coordinateur d'overlays ([`ChangeCoordinator`]) décide en mémoire, et le
//! [`GitWriter`] publie de façon transactionnelle ; ce module est le seul endroit
//! qui les enchaîne **dans le bon ordre** et refuse d'avancer la base canonique
//! sans publication réussie. Il est additif : aucune API existante n'est tordue.
//!
//! ```text
//!   Overlay + ChangeProposal
//!         │  1. CAS base ↔ HEAD (chemins modifiés)
//!         ▼
//!   ChangeCoordinator (clone de travail)
//!         │  2. integrate (tout ou rien)
//!         ▼
//!   snapshot_after()
//!         │  3. SemanticOracle sur (avant, après, patch déclaré)
//!         ▼
//!   GitWriter::publish_request (prepare + CAS + journal durable)
//!         │  4. succès seulement
//!         ▼
//!   coordinator.publish()  → nouvelle base canonique
//! ```
//!
//! # Garanties réellement démontrées (et seulement celles-là)
//!
//! * **Aucun état fantôme** : l'intégration est faite sur un clone du
//!   coordinateur. Un refus de vérification ou de publication laisse la base,
//!   `applied`, la comptabilité de stockage et l'historique exactement inchangés.
//! * **CAS base ↔ HEAD** : pour chaque chemin modifié, le contenu de la base doit
//!   être exactement le blob de `HEAD`. Sinon le cycle refuse (`StaleBase`) : on ne
//!   publie jamais un diff calculé contre une base qui n'est pas l'état Git.
//! * **CAS de publication** : `expected_head` est épinglé et le writer compare-échange
//!   la référence. Un writer concurrent perd sans corruption.
//! * **Journal durable obligatoire** : sans [`PublicationJournal`], le cycle refuse
//!   de s'exécuter. C'est ce qui rend la reprise après crash possible.
//! * **Reprise / idempotence** : `recover()` résout d'abord les intentions
//!   pendantes ; un `publication_id` déjà `PUBLISHED`/`RECOVERED_PUBLISHED` est
//!   refusé (`AlreadyPublished`) sans seconde publication. Un rejeu après un refus
//!   avant le point de commit est permis et publie une seule fois.
//!
//! # Limites honnêtes
//!
//! * Le cycle **ne prouve pas** que la `BaseSnapshot` fournie décrit l'arbre
//!   complet de `HEAD` : il vérifie les chemins modifiés. Un fichier non modifié
//!   divergent n'est pas inspecté.
//! * L'oracle par défaut (`textual-semantics-v1`) est textuel : `Clear` n'est pas
//!   `PROVED`. Un contenu non UTF-8 ne peut pas être vérifié par le modèle
//!   `SourceSnapshot` actuel ; le cycle échoue fermé (`VerificationUnavailable`)
//!   plutôt que de publier sans vérification.
//! * La reprise automatique (`recover`) lit le journal à chaque cycle. Une
//!   intention écrite par un autre cycle vivant, mais pas encore enregistrée comme
//!   vivante, peut être classée `RECOVERED_UNPUBLISHED` par un observateur
//!   concurrent ; le dernier `OUTCOME` fait foi et le writer refuse toujours une
//!   double publication. Cette fenêtre est documentée, pas masquée.
//! * Les crochets Git (`pre-commit`, `commit-msg`, `reference-transaction`) restent
//!   exécutés : un crochet qui refuse annule la publication, jamais l'intégration
//!   en mémoire (elle n'est pas adoptée).

use crate::git_writer::{
    CommitResult, GitError, GitWriter, PathUpdate, PublishRequest, RecoveryReport,
};
use crate::oracle::{OracleReport, OracleRequest, OracleVerdict, SemanticOracle};
use crate::overlay::{BaseSnapshot, ChangeCoordinator, IntegrationOutcome, Overlay};
use crate::patch;
use crate::publication_journal::PublicationStatus;
use crate::records::MergeCheckKind;
use ravum_protocol::{sha256_hex, ChangeProposal};
use ravum_snapshot::SourceSnapshot;

/// Demande d'un cycle complet. Toutes les décisions de publication sont
/// explicites : identifiant d'idempotence, en-tête Git attendu, opt-in généré.
#[derive(Debug, Clone)]
pub struct CycleRequest<'a> {
    /// Écritures privées de l'agent, relatives à la base courante du coordinateur.
    pub overlay: &'a Overlay,
    /// Proposition qui décrit la même modification pour l'oracle (patch, symboles,
    /// dépendances déclarées). Son `base_snapshot_id` doit être la base courante.
    pub proposal: &'a ChangeProposal,
    /// Sujet du commit publié.
    pub subject: String,
    /// Trailer d'attribution (par ex. `Ravum-Agent`).
    pub trailers: Vec<(String, String)>,
    /// Clé d'idempotence durable, obligatoire.
    pub publication_id: String,
    /// `HEAD` attendu avant publication. `None` épingle le `HEAD` observé au début
    /// du cycle (le CAS du writer le vérifie ensuite).
    pub expected_head: Option<String>,
    /// Décision humaine explicite autorisant la publication d'un chemin généré.
    pub allow_generated: bool,
}

impl<'a> CycleRequest<'a> {
    pub fn new(
        overlay: &'a Overlay,
        proposal: &'a ChangeProposal,
        publication_id: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            overlay,
            proposal,
            subject: subject.into(),
            trailers: Vec::new(),
            publication_id: publication_id.into(),
            expected_head: None,
            allow_generated: false,
        }
    }

    pub fn with_trailer(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.trailers.push((key.into(), value.into()));
        self
    }

    pub fn with_expected_head(mut self, head: impl Into<String>) -> Self {
        self.expected_head = Some(head.into());
        self
    }

    pub fn allowing_generated_paths(mut self) -> Self {
        self.allow_generated = true;
        self
    }
}

/// Résultat d'un cycle. Chaque variante est un état observable, jamais une
/// promotion silencieuse.
#[derive(Debug)]
pub enum CycleOutcome {
    /// La vérification a passé et le commit est publié ; le coordinateur a été
    /// avancé sur la nouvelle base.
    Published {
        base_id: String,
        commit: CommitResult,
        oracle: OracleReport,
        warnings: Vec<String>,
    },
    /// L'intégration en mémoire a été refusée (CAS, conflit de chemin, invalide).
    /// Aucun état n'a été adopté.
    Rejected { integration: IntegrationOutcome },
    /// Demande incohérente : rien n'a été tenté.
    InvalidRequest { reasons: Vec<String> },
    /// Une décision humaine est requise (contrat impacté). Rien n'a été publié.
    HumanRequired { reasons: Vec<String> },
    /// L'oracle a produit au moins un constat bloquant. Rien n'a été publié.
    VerificationRefused {
        oracle: OracleReport,
        reasons: Vec<String>,
    },
    /// L'oracle n'a pas pu répondre (fail-closed). Rien n'a été publié.
    VerificationUnavailable { reasons: Vec<String> },
    /// La base de la proposition n'est pas l'état de `HEAD` (ou une précondition
    /// CAS déclarée est fausse). Rien n'a été publié.
    StaleBase { reasons: Vec<String> },
    /// La publication a été refusée ou a échoué avant le point de commit.
    /// Le coordinateur n'a pas été avancé.
    PublicationRefused {
        error: GitError,
        /// Identité de base qu'aurait produite le cycle, pour l'audit.
        staged_base_id: String,
        /// Résolution des intentions pendantes observée au début du cycle.
        recovery: Option<RecoveryReport>,
    },
    /// Le `publication_id` est déjà publié (journal durable). Aucune seconde
    /// publication n'a été tentée.
    AlreadyPublished {
        publication_id: String,
        commit: String,
    },
}

impl CycleOutcome {
    pub fn is_published(&self) -> bool {
        matches!(self, CycleOutcome::Published { .. })
    }

    /// Identité de la base canonique après un cycle réussi.
    pub fn published_base_id(&self) -> Option<&str> {
        match self {
            CycleOutcome::Published { base_id, .. } => Some(base_id),
            _ => None,
        }
    }
}

/// Exécute un cycle transactionnel complet.
///
/// `coordinator` n'est muté qu'après une publication réussie. `writer` doit
/// porter un journal durable ([`GitWriter::with_publication_journal`]) : sans lui,
/// le cycle refuse de démarrer car la reprise après crash ne serait pas démontrable.
pub fn commit_cycle(
    coordinator: &mut ChangeCoordinator,
    writer: &GitWriter,
    oracle: &dyn SemanticOracle,
    request: &CycleRequest<'_>,
) -> CycleOutcome {
    // 0. Le journal durable est la condition de la reprise : pas de journal, pas
    //    de cycle.
    if writer.journal().is_none() {
        return CycleOutcome::InvalidRequest {
            reasons: vec![
                "commit_cycle requires a durable publication journal; attach one with \
                 GitWriter::with_publication_journal"
                    .to_owned(),
            ],
        };
    }
    if let Err(reasons) = validate_request(coordinator, request) {
        return CycleOutcome::InvalidRequest { reasons };
    }

    // 1. Résolution des intentions pendantes puis refus d'un rejeu déjà publié.
    let recovery = match writer.recover() {
        Ok(report) => report,
        Err(error) => {
            return CycleOutcome::PublicationRefused {
                error,
                staged_base_id: coordinator.base_id().to_owned(),
                recovery: None,
            }
        }
    };
    if let Some(journal) = writer.journal() {
        match journal.published_commit_of(&request.publication_id) {
            Ok(Some(commit)) => {
                return CycleOutcome::AlreadyPublished {
                    publication_id: request.publication_id.clone(),
                    commit,
                }
            }
            Ok(None) => {}
            Err(error) => {
                return CycleOutcome::PublicationRefused {
                    error: GitError::Journal(error.to_string()),
                    staged_base_id: coordinator.base_id().to_owned(),
                    recovery: Some(recovery),
                }
            }
        }
        // A previous attempt whose reference state cannot be explained from the
        // recorded commits is not replayed: the journal keeps the facts and a human
        // must inspect the repository. Guessing here is how a "recovery" would
        // silently publish on top of an unknown state.
        match journal.status_of(&request.publication_id) {
            Ok(Some(PublicationStatus::RecoveredUnknown)) => {
                return CycleOutcome::PublicationRefused {
                    error: GitError::Journal(format!(
                        "publication {:?} was left in an unexplained state by a previous attempt; refusing to publish on top of it",
                        request.publication_id
                    )),
                    staged_base_id: coordinator.base_id().to_owned(),
                    recovery: Some(recovery),
                }
            }
            Ok(_) => {}
            Err(error) => {
                return CycleOutcome::PublicationRefused {
                    error: GitError::Journal(error.to_string()),
                    staged_base_id: coordinator.base_id().to_owned(),
                    recovery: Some(recovery),
                }
            }
        }
    }

    // 2. Contrats impactés : décision humaine, jamais une intégration automatique
    //    (INV-02).
    if !request.proposal.impacted_contracts.is_empty() {
        return CycleOutcome::HumanRequired {
            reasons: vec![
                "the proposal declares an impacted contract: contract changes require human arbitration (INV-02)"
                    .to_owned(),
            ],
        };
    }

    // 3. CAS base ↔ HEAD pour chaque chemin modifié. `HEAD` est épinglé une seule
    //    fois : si la référence bouge pendant le cycle, la publication CAS échoue
    //    au lieu de publier sur une base différente de celle vérifiée.
    let head = match writer.head() {
        Ok(head) => head,
        Err(error) => {
            return CycleOutcome::PublicationRefused {
                error,
                staged_base_id: coordinator.base_id().to_owned(),
                recovery: Some(recovery),
            }
        }
    };
    if let Some(expected) = &request.expected_head {
        if expected != &head {
            return CycleOutcome::PublicationRefused {
                error: GitError::ConcurrentWriter {
                    expected: expected.clone(),
                    found: head,
                },
                staged_base_id: coordinator.base_id().to_owned(),
                recovery: Some(recovery),
            };
        }
    }
    match check_base_against_head(coordinator.base(), writer, request.overlay) {
        Ok(()) => {}
        Err(BaseAgainstHead::Mismatch(reasons)) => {
            return CycleOutcome::StaleBase { reasons };
        }
        Err(BaseAgainstHead::Error(error)) => {
            return CycleOutcome::PublicationRefused {
                error,
                staged_base_id: coordinator.base_id().to_owned(),
                recovery: Some(recovery),
            }
        }
    }

    // 4. Intégration sur un clone : aucun état fantôme si la suite échoue.
    let mut staged = coordinator.clone();
    let integration = staged.integrate(request.overlay);
    if !integration.is_applied() {
        return CycleOutcome::Rejected { integration };
    }
    let candidate = staged.snapshot_after();

    // 5. Vérification sémantique sur l'exact candidat.
    let before_snapshot = match to_source_snapshot(coordinator.base()) {
        Ok(snapshot) => snapshot,
        Err(reason) => {
            return CycleOutcome::VerificationUnavailable {
                reasons: vec![reason],
            }
        }
    };
    let after_snapshot = match to_source_snapshot(&candidate) {
        Ok(snapshot) => snapshot,
        Err(reason) => {
            return CycleOutcome::VerificationUnavailable {
                reasons: vec![reason],
            }
        }
    };
    let oracle_request = OracleRequest {
        proposal_id: request.proposal.proposal_id(),
        agent_id: request.proposal.agent_id.clone(),
        snapshot_before: before_snapshot,
        snapshot_after: after_snapshot,
        changed_paths: request.overlay.changed_paths(),
        declared_symbols: request.proposal.target_symbols.clone(),
        declared_dependencies: request.proposal.declared_dependencies.clone(),
        impacted_contracts: request.proposal.impacted_contracts.clone(),
        unified_patch: request.proposal.unified_patch.clone(),
        required_checks: vec![MergeCheckKind::MetadataCheck],
    };
    let verdict = oracle.evaluate(&oracle_request);
    let report = OracleReport::from_verdict(oracle.name(), &verdict);
    if let OracleVerdict::Unavailable { reason } = &verdict {
        return CycleOutcome::VerificationUnavailable {
            reasons: vec![
                format!("semantic oracle unavailable: {reason}"),
                "fail-closed: no publication without a semantic verdict".to_owned(),
            ],
        };
    }
    if report.has_blocking() {
        let reasons = report
            .blocking_findings()
            .map(|finding| {
                format!(
                    "oracle finding [{}]: {} (evidence {})",
                    finding.kind.as_str(),
                    finding.message,
                    finding.evidence_id
                )
            })
            .collect();
        return CycleOutcome::VerificationRefused {
            oracle: report,
            reasons,
        };
    }

    // 6. Publication Git transactionnelle, avec CAS et journal.
    let updates = match build_updates(coordinator.base(), request.overlay) {
        Ok(updates) => updates,
        Err(error) => {
            return CycleOutcome::PublicationRefused {
                error,
                staged_base_id: candidate.id().to_owned(),
                recovery: Some(recovery),
            }
        }
    };
    let mut publish_request = PublishRequest::new(request.subject.clone())
        .with_changes(updates)
        .with_publication_id(request.publication_id.clone())
        .with_expected_head(head);
    for (key, value) in &request.trailers {
        publish_request = publish_request.with_trailer(key.clone(), value.clone());
    }
    if request.allow_generated {
        publish_request = publish_request.allowing_generated_paths();
    }
    let commit = match writer.publish_request(&publish_request) {
        Ok(commit) => commit,
        Err(error) => {
            return CycleOutcome::PublicationRefused {
                error,
                staged_base_id: candidate.id().to_owned(),
                recovery: Some(recovery),
            }
        }
    };

    // 7. Publication réussie : la nouvelle base est adoptée.
    let base_id = staged.publish();
    *coordinator = staged;
    let mut warnings: Vec<String> = report
        .warning_findings()
        .map(|finding| {
            format!(
                "oracle warning [{}]: {} (evidence {})",
                finding.kind.as_str(),
                finding.message,
                finding.evidence_id
            )
        })
        .collect();
    warnings.extend(commit.warnings.iter().cloned());
    CycleOutcome::Published {
        base_id,
        commit,
        oracle: report,
        warnings,
    }
}

fn validate_request(
    coordinator: &ChangeCoordinator,
    request: &CycleRequest<'_>,
) -> Result<(), Vec<String>> {
    let mut reasons = Vec::new();
    if request.publication_id.trim().is_empty() {
        reasons.push("publication_id must not be empty".to_owned());
    } else if request
        .publication_id
        .chars()
        .any(|c| c.is_control() || c.is_whitespace())
    {
        reasons.push("publication_id must not contain whitespace or control characters".to_owned());
    }
    if request.subject.trim().is_empty() {
        reasons.push("subject must not be empty".to_owned());
    }
    if request.overlay.agent() != request.proposal.agent_id {
        reasons.push(format!(
            "overlay agent {:?} differs from proposal agent {:?}: attribution would be wrong",
            request.overlay.agent(),
            request.proposal.agent_id
        ));
    }
    if request.proposal.base_snapshot_id.as_str() != coordinator.base_id() {
        reasons.push(format!(
            "proposal base {} is not the coordinator base {}: the oracle would verify a different base",
            request.proposal.base_snapshot_id.short(),
            &coordinator.base_id()[..coordinator.base_id().len().min(12)]
        ));
    }
    let mut declared = request.proposal.target_paths.clone();
    declared.sort();
    declared.dedup();
    let actual = request.overlay.changed_paths();
    if declared != actual {
        reasons.push(format!(
            "proposal targets {declared:?} differ from overlay changes {actual:?}: the oracle would verify a different change"
        ));
    }
    match patch::parse(&request.proposal.unified_patch) {
        Err(error) => reasons.push(format!(
            "proposal unified patch does not parse: {error}; the semantic oracle cannot verify it"
        )),
        Ok(files) => {
            let patch_paths: std::collections::BTreeSet<String> =
                files.into_iter().map(|file| file.path).collect();
            let declared_paths: std::collections::BTreeSet<String> =
                declared.iter().cloned().collect();
            if patch_paths != declared_paths {
                reasons.push(format!(
                    "patch files {patch_paths:?} differ from declared target paths {declared_paths:?}: the oracle would verify a different change"
                ));
            }
        }
    }
    // Preconditions declared by the agent are checked against the exact base.
    for (path, expected) in &request.proposal.expected_hashes {
        let actual = coordinator
            .base()
            .read(path)
            .map(sha256_hex)
            .unwrap_or_default();
        if &actual != expected {
            reasons.push(format!(
                "stale precondition on {path}: declared {expected}, base holds {}",
                if actual.is_empty() {
                    "<absent>".to_owned()
                } else {
                    actual
                }
            ));
        }
    }
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons)
    }
}

enum BaseAgainstHead {
    Mismatch(Vec<String>),
    Error(GitError),
}

fn check_base_against_head(
    base: &BaseSnapshot,
    writer: &GitWriter,
    overlay: &Overlay,
) -> Result<(), BaseAgainstHead> {
    let mut reasons = Vec::new();
    for path in overlay.changed_paths() {
        let base_hash = base.read(&path).map(sha256_hex);
        let head_hash = writer
            .head_file_sha256(&path)
            .map_err(BaseAgainstHead::Error)?;
        if base_hash != head_hash {
            reasons.push(format!(
                "{path}: base holds {}, HEAD holds {}; the overlay was built on a base that is not the Git state",
                base_hash.as_deref().unwrap_or("<absent>"),
                head_hash.as_deref().unwrap_or("<absent>")
            ));
        }
    }
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(BaseAgainstHead::Mismatch(reasons))
    }
}

fn to_source_snapshot(snapshot: &BaseSnapshot) -> Result<SourceSnapshot, String> {
    let mut files = Vec::with_capacity(snapshot.files().len());
    for (path, bytes) in snapshot.files() {
        let text = String::from_utf8(bytes.clone()).map_err(|_| {
            format!(
                "{path}: non-UTF-8 content cannot be semantically verified by the current SourceSnapshot model"
            )
        })?;
        files.push((path.clone(), text));
    }
    Ok(SourceSnapshot::new(files))
}

fn build_updates(base: &BaseSnapshot, overlay: &Overlay) -> Result<Vec<PathUpdate>, GitError> {
    let mut updates = Vec::with_capacity(overlay.changed_paths().len());
    for path in overlay.changed_paths() {
        let base_hash = base.read(&path).map(sha256_hex);
        match overlay.writes().get(&path) {
            Some(bytes) => {
                let content = String::from_utf8(bytes.clone())
                    .map_err(|_| GitError::NotUtf8 { path: path.clone() })?;
                updates.push(
                    PathUpdate::write(path, content)
                        .with_expected_old_sha256(base_hash.unwrap_or_default()),
                );
            }
            None => {
                // Deleting a path absent from the base would be a no-op or, worse,
                // delete an untracked file that the agent never saw. Refuse.
                let Some(hash) = base_hash else {
                    return Err(GitError::InvalidRequest(format!(
                        "{path}: delete targets a path absent from the base snapshot"
                    )));
                };
                updates.push(PathUpdate::delete(path).with_expected_old_sha256(hash));
            }
        }
    }
    Ok(updates)
}
