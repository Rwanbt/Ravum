//! `ravum-batch` — collecteur de lots borné pour propositions multi-agents.
//!
//! Objectif (MVP-0c, L2 phase 2) : regrouper des propositions **textuellement
//! disjointes** sur une **même base** afin de remplacer N analyses par 1, avec
//! une fenêtre d'attente bornée, un pré-vol de conflits **sans rustc**, un repli
//! conservateur et une télémétrie exploitable.
//!
//! # Contrat de sûreté
//!
//! * **Aucune fusion hasardeuse** : un lot n'est formé que si toutes ses
//!   propositions sont compatibles (même base CAS, chemins disjoints, symboles
//!   disjoints). Une proposition incompatible avec les lots ouverts ouvre un
//!   lot séparé (isolation), elle n'est jamais forcée dans un lot.
//! * **Aucune acceptation partielle** : si l'analyseur refuse le lot, le lot
//!   n'accepte rien ; chaque proposition est alors ré-analysée individuellement
//!   (coût N+1) et seules les propositions acceptées individuellement sont
//!   acceptées.
//! * **Aucune perte** : chaque proposition soumise apparaît exactement une fois
//!   dans un rapport (acceptée ou refusée). Le compteur `items_reported` doit
//!   égaler `proposals_received` après `flush`.
//! * **Aucun rustc** : le crate ne dépend ni du daemon, ni du CLI, ni du
//!   coordinateur ; l'analyseur est injecté (`Analyzer`).
//!
//! # Temps et déterminisme
//!
//! Le collecteur ne lit **pas l'horloge** : tous les appels prennent un
//! `now_ms: u64` fourni par l'appelant (horloge monotone, par exemple
//! `Instant::elapsed().as_millis()`). Les tests utilisent des timestamps
//! injectés, donc aucun `sleep` et aucun test flaky.
//!
//! # API
//!
//! ```text
//! let mut collector = BatchCollector::new(BatchConfig::default(), analyzer)?;
//! let item = collector.submit(proposal, now_ms)?;   // non bloquant
//! let diag = collector.feedback(item)?;             // diagnostic immédiat (optionnel)
//! let reports = collector.poll(now_ms + 50);        // lots échus, analyses exécutées
//! let reports = collector.flush(now_ms);            // fin de flux
//! collector.telemetry();                            // lots, économies, latence, conflits
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Fenêtre d'attente par défaut, en millisecondes.
pub const DEFAULT_WINDOW_MS: u64 = 50;
/// Fenêtre d'attente maximale acceptée par la configuration.
pub const MAX_WINDOW_MS: u64 = 100;
/// Pas d'ajustement par défaut d'une fenêtre adaptative.
pub const DEFAULT_ADAPTIVE_STEP_MS: u64 = 25;
/// Taille de lot par défaut.
pub const DEFAULT_MAX_BATCH: usize = 6;
/// Taille de lot maximale acceptée par la configuration.
pub const MAX_BATCH: usize = 8;
/// Nombre maximum de propositions en attente (lots ouverts + lots prêts non exécutés).
pub const DEFAULT_MAX_PENDING: usize = 256;

/// Identifiant monotone de proposition, attribué par le collecteur.
pub type ItemId = u64;

/// Métadonnées nécessaires au pré-vol de conflits, sans rustc.
pub trait BatchProposal {
    /// Identifiant externe stable (ex. `agent/task`), non vide, unique.
    fn proposal_id(&self) -> &str;
    /// Chemins ciblés par la proposition (déjà canoniques côté workspace).
    fn paths(&self) -> &[String];
    /// Symboles touchés (noms ou ancres textuelles).
    fn symbols(&self) -> &[String];
    /// Identité de la base de comparaison (CAS), non vide.
    fn base_id(&self) -> &str;
}

/// Politique de fenêtre adaptative.
///
/// La fenêtre part de `min_ms` et s'ajuste entre `min_ms` et `max_ms`, par pas
/// de `step_ms` :
///
/// * une proposition qui rejoint un lot déjà ouvert prouve la densité → la
///   fenêtre grandit, pour regrouper davantage ;
/// * une fenêtre qui expire sans qu'aucune proposition ne l'ait rejointe prouve
///   la rareté → la fenêtre rétrécit, pour ne pas retarder un diagnostic isolé.
///
/// Les bornes restent `MAX_WINDOW_MS` au plus : la latence d'un développeur seul
/// n'est jamais sacrifiée au regroupement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveWindow {
    pub min_ms: u64,
    pub max_ms: u64,
    pub step_ms: u64,
}

impl AdaptiveWindow {
    pub fn new(min_ms: u64, max_ms: u64, step_ms: u64) -> Result<Self, ConfigError> {
        if min_ms > max_ms || max_ms > MAX_WINDOW_MS {
            return Err(ConfigError::InvalidAdaptiveWindow { min_ms, max_ms });
        }
        if step_ms == 0 {
            return Err(ConfigError::AdaptiveStepZero);
        }
        Ok(Self {
            min_ms,
            max_ms,
            step_ms,
        })
    }
}

/// Configuration du collecteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchConfig {
    window_ms: u64,
    max_batch: usize,
    max_pending: usize,
    /// `None` = fenêtre fixe historique ; `Some` = fenêtre adaptative.
    adaptive: Option<AdaptiveWindow>,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            window_ms: DEFAULT_WINDOW_MS,
            max_batch: DEFAULT_MAX_BATCH,
            max_pending: DEFAULT_MAX_PENDING,
            adaptive: None,
        }
    }
}

impl BatchConfig {
    /// Valide une configuration explicite. Fenêtre ∈ [0, 100] ms, lot ∈ [1, 8].
    pub fn new(window_ms: u64, max_batch: usize) -> Result<Self, ConfigError> {
        if window_ms > MAX_WINDOW_MS {
            return Err(ConfigError::WindowTooLarge(window_ms));
        }
        if max_batch == 0 || max_batch > MAX_BATCH {
            return Err(ConfigError::MaxBatchOutOfRange(max_batch));
        }
        Ok(Self {
            window_ms,
            max_batch,
            max_pending: DEFAULT_MAX_PENDING,
            adaptive: None,
        })
    }

    /// Configuration à fenêtre adaptative bornée.
    pub fn adaptive(
        min_ms: u64,
        max_ms: u64,
        step_ms: u64,
        max_batch: usize,
    ) -> Result<Self, ConfigError> {
        let adaptive = AdaptiveWindow::new(min_ms, max_ms, step_ms)?;
        if max_batch == 0 || max_batch > MAX_BATCH {
            return Err(ConfigError::MaxBatchOutOfRange(max_batch));
        }
        Ok(Self {
            window_ms: min_ms,
            max_batch,
            max_pending: DEFAULT_MAX_PENDING,
            adaptive: Some(adaptive),
        })
    }

    pub fn adaptive_policy(&self) -> Option<AdaptiveWindow> {
        self.adaptive
    }

    /// Ajuste la borne de propositions en attente (≥ 1).
    pub fn with_max_pending(mut self, max_pending: usize) -> Result<Self, ConfigError> {
        if max_pending == 0 {
            return Err(ConfigError::MaxPendingZero);
        }
        self.max_pending = max_pending;
        Ok(self)
    }

    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    pub fn max_batch(&self) -> usize {
        self.max_batch
    }

    pub fn max_pending(&self) -> usize {
        self.max_pending
    }
}

/// Erreur de configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    WindowTooLarge(u64),
    MaxBatchOutOfRange(usize),
    MaxPendingZero,
    InvalidAdaptiveWindow { min_ms: u64, max_ms: u64 },
    AdaptiveStepZero,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::WindowTooLarge(value) => write!(
                f,
                "window_ms={value} exceeds the bounded maximum {MAX_WINDOW_MS} ms"
            ),
            ConfigError::MaxBatchOutOfRange(value) => {
                write!(f, "max_batch={value} is outside 1..={MAX_BATCH}")
            }
            ConfigError::MaxPendingZero => write!(f, "max_pending must be >= 1"),
            ConfigError::InvalidAdaptiveWindow { min_ms, max_ms } => write!(
                f,
                "adaptive window needs 0 <= min_ms={min_ms} <= max_ms={max_ms} <= {MAX_WINDOW_MS}"
            ),
            ConfigError::AdaptiveStepZero => write!(f, "adaptive step_ms must be >= 1"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Verdict d'une analyse (lot ou proposition isolée).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

impl Verdict {
    pub fn passed() -> Self {
        Self {
            ok: true,
            diagnostics: Vec::new(),
        }
    }

    pub fn failed(diagnostic: impl Into<String>) -> Self {
        Self {
            ok: false,
            diagnostics: vec![diagnostic.into()],
        }
    }
}

/// Analyseur injecté (aucune dépendance rustc dans ce crate).
pub trait Analyzer {
    /// Analyse un lot (ou une proposition isolée) désigné par ses ids externes.
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict;

    /// Diagnostic immédiat d'une proposition, indépendant du lot.
    ///
    /// Par défaut, délègue à [`Analyzer::analyze`] sur un seul identifiant ; un
    /// analyseur réel peut brancher ici la lecture de diagnostics déjà calculés
    /// (feedback immédiat sans attendre la fenêtre).
    fn diagnose(&mut self, proposal_id: &str) -> Verdict {
        self.analyze(&[proposal_id])
    }
}

/// Implémentation utilitaire pour injecter un analyseur emprunté (`&mut A`).
impl<A: Analyzer + ?Sized> Analyzer for &mut A {
    fn analyze(&mut self, proposal_ids: &[&str]) -> Verdict {
        (**self).analyze(proposal_ids)
    }

    fn diagnose(&mut self, proposal_id: &str) -> Verdict {
        (**self).diagnose(proposal_id)
    }
}

/// Cause de fermeture d'un lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchReason {
    /// Le lot a atteint `max_batch` avant la fin de la fenêtre.
    Full,
    /// La fenêtre d'attente a expiré.
    WindowExpired,
    /// Fermeture explicite de fin de flux.
    Flush,
}

/// Résultat d'un lot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchOutcome {
    /// Le lot a été accepté en une seule analyse.
    AcceptedAll,
    /// Le lot a été refusé ; les propositions ont été ré-analysées une par une.
    FellBack { rejected_ids: Vec<String> },
}

/// Résultat d'une ré-analyse individuelle de repli.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndividualResult {
    pub proposal_id: String,
    pub verdict: Verdict,
}

/// Rapport d'un lot exécuté.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReport {
    pub batch_id: u64,
    pub reason: BatchReason,
    pub proposal_ids: Vec<String>,
    pub opened_ms: u64,
    pub closed_ms: u64,
    /// Latence d'attente réelle du lot (fermeture − première arrivée).
    pub wait_ms: u64,
    pub outcome: BatchOutcome,
    /// Verdict de l'analyse groupée (avant tout repli).
    pub batch_verdict: Verdict,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub individual: Vec<IndividualResult>,
    pub accepted_ids: Vec<String>,
    pub rejected_ids: Vec<String>,
    /// Analyses réellement consommées : 1 (succès) ou N+1 (repli).
    pub analyses_used: usize,
}

/// Télémétrie cumulative du collecteur (aucune estimation, uniquement des
/// événements observés).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchTelemetry {
    /// Propositions valides et acceptées dans la file.
    pub proposals_received: u64,
    /// Propositions rejetées avant file (id/base vide, id dupliqué, file pleine).
    pub proposals_rejected: u64,
    /// Rejets pour identifiant externe dupliqué.
    pub duplicates_rejected: u64,
    /// Rejets pour borne `max_pending` atteinte.
    pub queue_full_rejected: u64,
    /// Lots exécutés (succès ou repli).
    pub batches_formed: u64,
    /// Lots acceptés en une analyse.
    pub batches_accepted_all: u64,
    /// Lots ayant déclenché le repli individuel.
    pub batches_fell_back: u64,
    /// Analyses individuelles exécutées lors des replis.
    pub individual_analyses: u64,
    /// Analyses totales consommées (lot + replis) : le coût réel du collecteur.
    pub analyses_used: u64,
    /// Économie nette d'analyses par rapport à une analyse par proposition
    /// (positive pour les lots acceptés, négative pour un repli : N+1 > N).
    pub analyses_saved_net: i64,
    /// Diagnostics immédiats demandés via [`BatchCollector::feedback`].
    pub feedback_calls: u64,
    /// Propositions isolées faute de lot compatible (conflit textuel/base).
    pub preflight_isolations: u64,
    /// Situations où un chemin a empêché une fusion.
    pub path_conflicts: u64,
    /// Situations où un symbole a empêché une fusion.
    pub symbol_conflicts: u64,
    /// Situations où une base différente a empêché une fusion.
    pub base_mismatches: u64,
    /// Croissances de la fenêtre adaptative (densité observée).
    pub window_growths: u64,
    /// Rétrécissements de la fenêtre adaptative (fenêtre expirée sans arrivée).
    pub window_shrinks: u64,
    /// Fenêtre effective maximale atteinte pendant la campagne (ms).
    pub max_effective_window_ms: u64,
    /// Échantillons de latence d'attente des lots exécutés (ms).
    pub wait_samples_ms: Vec<u64>,
    /// Propositions effectivement rapportées (acceptées + refusées).
    pub items_reported: u64,
}

impl BatchTelemetry {
    /// Latence d'attente p50 (nearest-rank), `None` si aucun lot exécuté.
    pub fn wait_p50_ms(&self) -> Option<u64> {
        percentile(&self.wait_samples_ms, 50)
    }

    /// Latence d'attente p95 (nearest-rank), `None` si aucun lot exécuté.
    pub fn wait_p95_ms(&self) -> Option<u64> {
        percentile(&self.wait_samples_ms, 95)
    }

    /// Vérifie l'invariant de non-perte : toutes les propositions reçues ont été
    /// rapportées exactement une fois.
    pub fn no_loss(&self) -> bool {
        self.proposals_received == self.items_reported
    }
}

/// Percentile nearest-rank (pas d'interpolation) sur des échantillons u64.
pub fn percentile(values: &[u64], p: u64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let p = p.clamp(1, 100);
    let rank = ((p as f64 / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.saturating_sub(1).min(sorted.len() - 1)])
}

/// Erreur de soumission (non bloquante).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    EmptyProposalId,
    EmptyBaseId,
    DuplicateProposal(String),
    PendingLimit(usize),
}

impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubmitError::EmptyProposalId => write!(f, "proposal_id must not be empty"),
            SubmitError::EmptyBaseId => write!(f, "base_id must not be empty"),
            SubmitError::DuplicateProposal(id) => {
                write!(f, "proposal_id {id:?} is already known to this collector")
            }
            SubmitError::PendingLimit(limit) => {
                write!(f, "pending limit reached ({limit} proposals)")
            }
        }
    }
}

impl std::error::Error for SubmitError {}

/// Erreur de diagnostic immédiat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackError {
    UnknownItem(ItemId),
    AlreadyAnalyzed(ItemId),
}

impl fmt::Display for FeedbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedbackError::UnknownItem(id) => write!(f, "unknown item {id}"),
            FeedbackError::AlreadyAnalyzed(id) => {
                write!(f, "item {id} has already been analysed in a batch")
            }
        }
    }
}

impl std::error::Error for FeedbackError {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Conflicts {
    path: bool,
    symbol: bool,
    base: bool,
}

impl Conflicts {
    fn any(self) -> bool {
        self.path || self.symbol || self.base
    }
}

#[derive(Debug)]
struct Queued<P> {
    item_id: ItemId,
    proposal: P,
}

#[derive(Debug)]
struct OpenBatch<P> {
    batch_id: u64,
    opened_ms: u64,
    items: Vec<Queued<P>>,
}

#[derive(Debug)]
struct ClosedBatch<P> {
    batch_id: u64,
    reason: BatchReason,
    opened_ms: u64,
    closed_ms: u64,
    items: Vec<Queued<P>>,
}

enum JoinDecision {
    Join,
    Full,
    Conflicts(Conflicts),
}

/// Collecteur de lots borné, analyseur injecté.
///
/// `submit` ne bloque jamais et n'exécute aucune analyse. `poll`/`next_batch`
/// exécutent les lots échus et forment les rapports ; `flush` ferme le flux.
pub struct BatchCollector<P: BatchProposal, A: Analyzer> {
    config: BatchConfig,
    analyzer: A,
    open: Vec<OpenBatch<P>>,
    ready: Vec<ClosedBatch<P>>,
    seen_ids: HashSet<String>,
    next_item_id: u64,
    next_batch_id: u64,
    telemetry: BatchTelemetry,
    /// Propositions dans les lots ouverts + lots prêts non exécutés.
    pending_items: usize,
    /// Index item_id → id externe, conservé pour les diagnostics immédiats.
    item_index: HashMap<ItemId, String>,
    /// Items déjà exécutés dans un lot (diagnostic immédiat refusé explicitement).
    analyzed_items: HashSet<ItemId>,
    /// Fenêtre effective courante (égale à `config.window_ms()` si fixe).
    current_window_ms: u64,
}

impl<P: BatchProposal, A: Analyzer> BatchCollector<P, A> {
    /// Crée un collecteur ; la configuration a déjà été validée.
    pub fn new(config: BatchConfig, analyzer: A) -> Self {
        let current_window_ms = config.window_ms();
        Self {
            config,
            analyzer,
            open: Vec::new(),
            ready: Vec::new(),
            seen_ids: HashSet::new(),
            next_item_id: 1,
            next_batch_id: 1,
            telemetry: BatchTelemetry {
                max_effective_window_ms: current_window_ms,
                ..BatchTelemetry::default()
            },
            pending_items: 0,
            item_index: HashMap::new(),
            analyzed_items: HashSet::new(),
            current_window_ms,
        }
    }

    /// Fenêtre effectivement appliquée aux lots ouverts, en millisecondes.
    pub fn effective_window_ms(&self) -> u64 {
        self.current_window_ms
    }

    pub fn config(&self) -> &BatchConfig {
        &self.config
    }

    pub fn telemetry(&self) -> &BatchTelemetry {
        &self.telemetry
    }

    pub fn pending_batches(&self) -> usize {
        self.open.len() + self.ready.len()
    }

    pub fn pending_items(&self) -> usize {
        self.pending_items
    }

    /// Soumet une proposition ; retourne son identifiant monotone.
    ///
    /// Ne bloque pas et n'exécute aucune analyse. Le pré-vol décide si la
    /// proposition rejoint un lot ouvert compatible, sinon elle ouvre un lot
    /// séparé (isolation).
    pub fn submit(&mut self, proposal: P, now_ms: u64) -> Result<ItemId, SubmitError> {
        if proposal.proposal_id().is_empty() {
            self.telemetry.proposals_rejected += 1;
            return Err(SubmitError::EmptyProposalId);
        }
        if proposal.base_id().is_empty() {
            self.telemetry.proposals_rejected += 1;
            return Err(SubmitError::EmptyBaseId);
        }
        if self.seen_ids.contains(proposal.proposal_id()) {
            self.telemetry.proposals_rejected += 1;
            self.telemetry.duplicates_rejected += 1;
            return Err(SubmitError::DuplicateProposal(
                proposal.proposal_id().to_owned(),
            ));
        }
        if self.pending_items >= self.config.max_pending {
            self.telemetry.proposals_rejected += 1;
            self.telemetry.queue_full_rejected += 1;
            return Err(SubmitError::PendingLimit(self.config.max_pending));
        }

        // First-fit pré-vol, dans l'ordre de création des lots.
        let mut candidate: Option<usize> = None;
        let mut blocked_path = false;
        let mut blocked_symbol = false;
        let mut blocked_base = false;
        for (index, batch) in self.open.iter().enumerate() {
            match join_decision(&proposal, batch, self.config.max_batch) {
                JoinDecision::Join => {
                    candidate = Some(index);
                    break;
                }
                JoinDecision::Full => {}
                JoinDecision::Conflicts(conflicts) => {
                    blocked_path |= conflicts.path;
                    blocked_symbol |= conflicts.symbol;
                    blocked_base |= conflicts.base;
                }
            }
        }
        if blocked_path {
            self.telemetry.path_conflicts += 1;
        }
        if blocked_symbol {
            self.telemetry.symbol_conflicts += 1;
        }
        if blocked_base {
            self.telemetry.base_mismatches += 1;
        }

        let item_id = self.next_item_id;
        self.next_item_id += 1;
        let external_id = proposal.proposal_id().to_owned();
        self.seen_ids.insert(external_id.clone());
        self.item_index.insert(item_id, external_id);
        self.pending_items += 1;
        self.telemetry.proposals_received += 1;

        let joined_open_batch = candidate.is_some();
        match candidate {
            Some(index) => {
                let batch = &mut self.open[index];
                batch.items.push(Queued { item_id, proposal });
                if batch.items.len() >= self.config.max_batch {
                    let closed = self.close_open(index, BatchReason::Full, now_ms);
                    self.ready.push(closed);
                }
            }
            None => {
                if blocked_path || blocked_symbol || blocked_base {
                    self.telemetry.preflight_isolations += 1;
                }
                let batch_id = self.next_batch_id;
                self.next_batch_id += 1;
                self.open.push(OpenBatch {
                    batch_id,
                    opened_ms: now_ms,
                    items: vec![Queued { item_id, proposal }],
                });
                if self.config.max_batch == 1 {
                    let closed = self.close_open(self.open.len() - 1, BatchReason::Full, now_ms);
                    self.ready.push(closed);
                }
            }
        }
        // Densité observée : une proposition a rejoint un lot déjà ouvert, la
        // fenêtre peut grandir pour regrouper davantage sans retarder un isolé.
        if joined_open_batch {
            self.grow_window();
        }
        Ok(item_id)
    }

    fn grow_window(&mut self) {
        let Some(policy) = self.config.adaptive_policy() else {
            return;
        };
        let next = self
            .current_window_ms
            .saturating_add(policy.step_ms)
            .min(policy.max_ms);
        if next > self.current_window_ms {
            self.current_window_ms = next;
            self.telemetry.window_growths += 1;
            self.telemetry.max_effective_window_ms =
                self.telemetry.max_effective_window_ms.max(next);
        }
    }

    fn shrink_window(&mut self) {
        let Some(policy) = self.config.adaptive_policy() else {
            return;
        };
        let next = self
            .current_window_ms
            .saturating_sub(policy.step_ms)
            .max(policy.min_ms);
        if next < self.current_window_ms {
            self.current_window_ms = next;
            self.telemetry.window_shrinks += 1;
        }
    }

    /// Diagnostic immédiat d'une proposition soumise et pas encore analysée.
    ///
    /// Ce chemin est indépendant de la fenêtre : il permet d'afficher les
    /// diagnostics du snapshot courant sans attendre le lot. Il est compté dans
    /// `feedback_calls`, jamais comme une analyse de lot.
    pub fn feedback(&mut self, item_id: ItemId) -> Result<Verdict, FeedbackError> {
        if self.analyzed_items.contains(&item_id) {
            return Err(FeedbackError::AlreadyAnalyzed(item_id));
        }
        let Some(external_id) = self.item_index.get(&item_id).cloned() else {
            return Err(FeedbackError::UnknownItem(item_id));
        };
        self.telemetry.feedback_calls += 1;
        Ok(self.analyzer.diagnose(&external_id))
    }

    /// Exécute tous les lots échus à `now_ms` (fenêtre expirée ou lot plein).
    pub fn poll(&mut self, now_ms: u64) -> Vec<BatchReport> {
        self.expire_open(now_ms);
        let mut ready = std::mem::take(&mut self.ready);
        ready.sort_by_key(|batch| batch.batch_id);
        ready.into_iter().map(|batch| self.execute(batch)).collect()
    }

    /// Exécute au plus un lot échu (le plus ancien par `batch_id`).
    pub fn next_batch(&mut self, now_ms: u64) -> Option<BatchReport> {
        self.expire_open(now_ms);
        if self.ready.is_empty() {
            return None;
        }
        self.ready.sort_by_key(|batch| batch.batch_id);
        let batch = self.ready.remove(0);
        Some(self.execute(batch))
    }

    /// Ferme explicitement tous les lots ouverts et exécute tout ce qui est prêt.
    pub fn flush(&mut self, now_ms: u64) -> Vec<BatchReport> {
        let open = std::mem::take(&mut self.open);
        for batch in open {
            let closed_ms = now_ms.max(batch.opened_ms);
            self.ready.push(ClosedBatch {
                batch_id: batch.batch_id,
                reason: BatchReason::Flush,
                opened_ms: batch.opened_ms,
                closed_ms,
                items: batch.items,
            });
        }
        self.poll(now_ms)
    }

    fn expire_open(&mut self, now_ms: u64) {
        let window = self.current_window_ms;
        let mut still_open = Vec::with_capacity(self.open.len());
        let mut sparse_expiry = false;
        for batch in std::mem::take(&mut self.open) {
            if now_ms.saturating_sub(batch.opened_ms) >= window {
                let closed_ms = batch.opened_ms.saturating_add(window);
                if batch.items.len() == 1 {
                    sparse_expiry = true;
                }
                self.ready.push(ClosedBatch {
                    batch_id: batch.batch_id,
                    reason: BatchReason::WindowExpired,
                    opened_ms: batch.opened_ms,
                    closed_ms,
                    items: batch.items,
                });
            } else {
                still_open.push(batch);
            }
        }
        self.open = still_open;
        // Rareté observée : la fenêtre a expiré sans qu'aucune proposition ne
        // rejoigne le lot ; rétrécir évite de retarder le prochain isolé.
        if sparse_expiry {
            self.shrink_window();
        }
    }

    fn close_open(&mut self, index: usize, reason: BatchReason, now_ms: u64) -> ClosedBatch<P> {
        let batch = self.open.remove(index);
        ClosedBatch {
            batch_id: batch.batch_id,
            reason,
            opened_ms: batch.opened_ms,
            closed_ms: now_ms.max(batch.opened_ms),
            items: batch.items,
        }
    }

    fn execute(&mut self, batch: ClosedBatch<P>) -> BatchReport {
        let n = batch.items.len();
        self.pending_items = self.pending_items.saturating_sub(n);
        let wait_ms = batch.closed_ms.saturating_sub(batch.opened_ms);
        let proposal_ids: Vec<String> = batch
            .items
            .iter()
            .map(|item| item.proposal.proposal_id().to_owned())
            .collect();
        for item in &batch.items {
            self.item_index.remove(&item.item_id);
            self.analyzed_items.insert(item.item_id);
        }
        let id_refs: Vec<&str> = proposal_ids.iter().map(String::as_str).collect();
        let batch_verdict = self.analyzer.analyze(&id_refs);

        let (outcome, individual, accepted_ids, rejected_ids, analyses_used) = if batch_verdict.ok {
            (
                BatchOutcome::AcceptedAll,
                Vec::new(),
                proposal_ids.clone(),
                Vec::new(),
                1usize,
            )
        } else {
            let mut individual = Vec::with_capacity(n);
            let mut accepted = Vec::new();
            let mut rejected = Vec::new();
            for external_id in &proposal_ids {
                let verdict = self.analyzer.analyze(&[external_id.as_str()]);
                if verdict.ok {
                    accepted.push(external_id.clone());
                } else {
                    rejected.push(external_id.clone());
                }
                individual.push(IndividualResult {
                    proposal_id: external_id.clone(),
                    verdict,
                });
            }
            self.telemetry.batches_fell_back += 1;
            self.telemetry.individual_analyses += n as u64;
            (
                BatchOutcome::FellBack {
                    rejected_ids: rejected.clone(),
                },
                individual,
                accepted,
                rejected,
                n + 1,
            )
        };

        self.telemetry.batches_formed += 1;
        self.telemetry.analyses_used += analyses_used as u64;
        self.telemetry.analyses_saved_net += n as i64 - analyses_used as i64;
        self.telemetry.items_reported += n as u64;
        self.telemetry.wait_samples_ms.push(wait_ms);
        if matches!(outcome, BatchOutcome::AcceptedAll) {
            self.telemetry.batches_accepted_all += 1;
        }

        debug_assert_eq!(
            accepted_ids.len() + rejected_ids.len(),
            n,
            "no proposal may be lost: every item is accepted or rejected exactly once"
        );
        debug_assert!(
            !matches!(outcome, BatchOutcome::AcceptedAll) || rejected_ids.is_empty(),
            "a failed batch may never partially accept"
        );

        BatchReport {
            batch_id: batch.batch_id,
            reason: batch.reason,
            proposal_ids,
            opened_ms: batch.opened_ms,
            closed_ms: batch.closed_ms,
            wait_ms,
            outcome,
            batch_verdict,
            individual,
            accepted_ids,
            rejected_ids,
            analyses_used,
        }
    }

    /// Rend l'analyseur (utile pour inspecter des compteurs dans les tests).
    pub fn into_analyzer(self) -> A {
        self.analyzer
    }
}

fn join_decision<P: BatchProposal>(
    proposal: &P,
    batch: &OpenBatch<P>,
    max_batch: usize,
) -> JoinDecision {
    if batch.items.len() >= max_batch {
        return JoinDecision::Full;
    }
    let mut conflicts = Conflicts::default();
    for queued in &batch.items {
        if proposal.base_id() != queued.proposal.base_id() {
            conflicts.base = true;
        }
        if overlaps(proposal.paths(), queued.proposal.paths()) {
            conflicts.path = true;
        }
        if overlaps(proposal.symbols(), queued.proposal.symbols()) {
            conflicts.symbol = true;
        }
    }
    if conflicts.any() {
        JoinDecision::Conflicts(conflicts)
    } else {
        JoinDecision::Join
    }
}

fn overlaps(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|item| right.iter().any(|other| other == item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_match_the_measured_recommendation() {
        let config = BatchConfig::default();
        assert_eq!(config.window_ms(), 50);
        assert_eq!(config.max_batch(), 6);
        assert_eq!(MAX_WINDOW_MS, 100);
        assert_eq!(MAX_BATCH, 8);
    }

    #[test]
    fn config_bounds_are_enforced() {
        assert!(BatchConfig::new(100, 8).is_ok());
        assert!(BatchConfig::new(0, 1).is_ok());
        assert_eq!(
            BatchConfig::new(101, 6),
            Err(ConfigError::WindowTooLarge(101))
        );
        assert_eq!(
            BatchConfig::new(50, 0),
            Err(ConfigError::MaxBatchOutOfRange(0))
        );
        assert_eq!(
            BatchConfig::new(50, 9),
            Err(ConfigError::MaxBatchOutOfRange(9))
        );
        assert_eq!(
            BatchConfig::default().with_max_pending(0),
            Err(ConfigError::MaxPendingZero)
        );
    }

    #[test]
    fn percentile_is_nearest_rank() {
        assert_eq!(percentile(&[], 50), None);
        assert_eq!(percentile(&[50], 50), Some(50));
        assert_eq!(percentile(&[10, 20, 30, 40], 50), Some(20));
        assert_eq!(percentile(&[10, 20, 30, 40], 95), Some(40));
    }

    #[test]
    fn telemetry_no_loss_is_a_plain_invariant() {
        let mut telemetry = BatchTelemetry::default();
        telemetry.proposals_received = 3;
        telemetry.items_reported = 3;
        assert!(telemetry.no_loss());
        telemetry.items_reported = 2;
        assert!(!telemetry.no_loss());
    }
}
