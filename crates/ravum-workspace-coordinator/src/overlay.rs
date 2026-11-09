//! Lot B — overlays privés sur un état canonique partagé.
//!
//! Le problème mesuré par la campagne S8 : un répertoire partagé avec une simple
//! garde de version est plus lent que des copies privées, parce que les agents se
//! disputent le même arbre physique. Ce module implémente la troisième voie :
//!
//! ```text
//!   BaseSnapshot (immuable, content-adressé)
//!        ├── Overlay agent A  (écritures privées, aucune copie de l'arbre)
//!        ├── Overlay agent B
//!        └── Overlay agent C
//!                    ↓
//!          ChangeCoordinator (intégration séquentielle, CAS de base)
//!                    ↓
//!          snapshot_after (nouvel état canonique)
//! ```
//!
//! Propriétés démontrées, et seulement celles-là :
//!
//! * un agent ne modifie jamais la base : il écrit dans son overlay ;
//! * une proposition est refusée si sa base n'est pas la base courante (CAS) ;
//! * l'intégration est **tout ou rien** : un seul chemin déjà appliqué par un
//!   autre agent fait échouer la proposition entière, sans fusion sémantique ;
//! * aucun chemin absolu, remontant (`..`) ou `\` n'est accepté ;
//! * le coût de stockage est celui des fichiers écrits, jamais celui de l'arbre.
//!
//! Non démontré ici : fusion de deux modifications dans une même fonction
//! (refusée, `ConflictPath`), convergence CRDT, publication Git (assurée par
//! [`GitWriter`](crate::GitWriter)), et vérification sémantique (assurée par
//! [`SemanticOracle`](crate::SemanticOracle)).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Version du schéma des rapports d'intégration.
pub const OVERLAY_SCHEMA_VERSION: u32 = 1;

/// Erreur de validation d'un overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayError {
    /// Chemin vide.
    EmptyPath,
    /// Chemin absolu ou avec lecteur Windows : jamais accepté.
    AbsolutePath(String),
    /// Composant `.` ou `..`, ou antislash : jamais accepté.
    UnsafePath(String),
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayError::EmptyPath => write!(f, "overlay path is empty"),
            OverlayError::AbsolutePath(path) => {
                write!(f, "overlay path {path:?} must be relative to the workspace")
            }
            OverlayError::UnsafePath(path) => write!(
                f,
                "overlay path {path:?} must not contain `.`, `..` or a backslash"
            ),
        }
    }
}

impl std::error::Error for OverlayError {}

/// Valide un chemin relatif portable (`/` uniquement, aucun `..`).
pub fn validate_relative_path(path: &str) -> Result<(), OverlayError> {
    if path.trim().is_empty() {
        return Err(OverlayError::EmptyPath);
    }
    if path.starts_with('/')
        || path.starts_with('\\')
        || (path.len() >= 2 && path.as_bytes()[1] == b':')
    {
        return Err(OverlayError::AbsolutePath(path.to_owned()));
    }
    if path.contains('\\') {
        return Err(OverlayError::UnsafePath(path.to_owned()));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(OverlayError::UnsafePath(path.to_owned()));
        }
    }
    Ok(())
}

/// État canonique immuable, identifié par le contenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseSnapshot {
    id: String,
    files: BTreeMap<String, Vec<u8>>,
}

impl BaseSnapshot {
    /// Construit un snapshot depuis un répertoire : chemins relatifs en `/`,
    /// ordre déterministe, identité = SHA-256 des `(chemin, longueur, octets)`.
    pub fn from_directory(root: &Path) -> std::io::Result<Self> {
        let mut files = BTreeMap::new();
        collect_files(root, root, &mut files)?;
        let id = snapshot_id(&files);
        Ok(Self { id, files })
    }

    /// Snapshot explicite (tests, publication après intégration).
    pub fn from_files(files: BTreeMap<String, Vec<u8>>) -> Self {
        let id = snapshot_id(&files);
        Self { id, files }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }

    pub fn total_bytes(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }

    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }
}

fn snapshot_id(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ravum-base-snapshot-v1\0");
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(current)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == "target" {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, std::fs::read(&path)?);
        }
    }
    Ok(())
}

/// Écritures privées d'un agent, relatives à une base donnée.
#[derive(Debug, Clone)]
pub struct Overlay {
    agent: String,
    base_id: String,
    writes: BTreeMap<String, Vec<u8>>,
    deletes: BTreeSet<String>,
}

impl Overlay {
    /// Ouvre un overlay privé sur `base` pour `agent`.
    pub fn new(base: &BaseSnapshot, agent: &str) -> Self {
        Self {
            agent: agent.to_owned(),
            base_id: base.id().to_owned(),
            writes: BTreeMap::new(),
            deletes: BTreeSet::new(),
        }
    }

    pub fn agent(&self) -> &str {
        &self.agent
    }

    pub fn base_id(&self) -> &str {
        &self.base_id
    }

    /// Écrit un fichier dans l'overlay. Le chemin est validé, rien n'est touché
    /// sur le disque : la base reste intacte.
    pub fn write(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), OverlayError> {
        validate_relative_path(path)?;
        self.deletes.remove(path);
        self.writes.insert(path.to_owned(), bytes);
        Ok(())
    }

    /// Supprime un fichier dans l'overlay.
    pub fn delete(&mut self, path: &str) -> Result<(), OverlayError> {
        validate_relative_path(path)?;
        self.writes.remove(path);
        self.deletes.insert(path.to_owned());
        Ok(())
    }

    /// Chemins modifiés (écritures ∪ suppressions), ordre déterministe.
    pub fn changed_paths(&self) -> Vec<String> {
        let mut paths: BTreeSet<String> = self.writes.keys().cloned().collect();
        paths.extend(self.deletes.iter().cloned());
        paths.into_iter().collect()
    }

    /// Coût de stockage réel de l'overlay : uniquement les octets écrits.
    pub fn overlay_bytes(&self) -> usize {
        self.writes.values().map(Vec::len).sum()
    }

    pub fn writes(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.writes
    }

    pub fn deletes(&self) -> &BTreeSet<String> {
        &self.deletes
    }

    /// Vue logique : overlay d'abord, base ensuite.
    pub fn read(&self, base: &BaseSnapshot, path: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.writes.get(path) {
            return Some(bytes.clone());
        }
        if self.deletes.contains(path) {
            return None;
        }
        base.read(path).map(ToOwned::to_owned)
    }

    /// Matérialise la vue logique (base + overlay) dans `dir` pour un build.
    /// C'est une vue de compilation, jamais la base partagée.
    pub fn materialize(&self, base: &BaseSnapshot, dir: &Path) -> std::io::Result<usize> {
        let mut written = 0;
        for (path, bytes) in &base.files {
            if self.deletes.contains(path) || self.writes.contains_key(path) {
                continue;
            }
            write_relative(dir, path, bytes)?;
            written += 1;
        }
        for (path, bytes) in &self.writes {
            write_relative(dir, path, bytes)?;
            written += 1;
        }
        Ok(written)
    }
}

fn write_relative(root: &Path, path: &str, bytes: &[u8]) -> std::io::Result<()> {
    let target: PathBuf = path
        .split('/')
        .fold(root.to_path_buf(), |acc, part| acc.join(part));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, bytes)
}

/// Issue d'une proposition soumise au coordinateur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum IntegrationOutcome {
    /// Tous les changements ont été appliqués.
    Applied { agent: String, paths: Vec<String> },
    /// La base de la proposition n'est plus la base courante (CAS).
    RejectedStale {
        agent: String,
        expected: String,
        actual: String,
    },
    /// Au moins un chemin a déjà été modifié par un autre agent : rien n'est
    /// appliqué (conservateur, aucune fusion sémantique).
    ConflictPath { agent: String, paths: Vec<String> },
    /// Chemins invalides : rien n'est appliqué.
    RejectedInvalid { agent: String, reasons: Vec<String> },
}

impl IntegrationOutcome {
    pub fn is_applied(&self) -> bool {
        matches!(self, IntegrationOutcome::Applied { .. })
    }
}

/// Coût de stockage comparé à la stratégie « copie privée intégrale ».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageReport {
    pub schema_version: u32,
    pub agents: usize,
    pub base_bytes: usize,
    /// Octets réellement écrits par les overlays.
    pub overlay_bytes: usize,
    /// Octets qu'auraient coûtés `agents` copies privées complètes.
    pub full_copies_bytes: usize,
}

impl StorageReport {
    /// Économie relative (0.0 = aucune, 0.9 = 90 % de moins).
    pub fn saved_ratio(&self) -> f64 {
        if self.full_copies_bytes == 0 {
            return 0.0;
        }
        1.0 - (self.overlay_bytes as f64 / self.full_copies_bytes as f64)
    }
}

/// Intègre des propositions séquentiallement sur un état canonique.
#[derive(Debug, Clone)]
pub struct ChangeCoordinator {
    base: BaseSnapshot,
    applied: BTreeMap<String, String>,
    files: BTreeMap<String, Vec<u8>>,
    history: Vec<IntegrationOutcome>,
    total_overlay_bytes: usize,
    agents: BTreeSet<String>,
}

impl ChangeCoordinator {
    pub fn new(base: BaseSnapshot) -> Self {
        let files = base.files().clone();
        Self {
            base,
            applied: BTreeMap::new(),
            files,
            history: Vec::new(),
            total_overlay_bytes: 0,
            agents: BTreeSet::new(),
        }
    }

    pub fn base_id(&self) -> &str {
        self.base.id()
    }

    pub fn applied_paths(&self) -> &BTreeMap<String, String> {
        &self.applied
    }

    pub fn history(&self) -> &[IntegrationOutcome] {
        &self.history
    }

    pub fn record(&mut self, outcome: IntegrationOutcome) {
        self.history.push(outcome);
    }

    /// Intègre une proposition, tout ou rien.
    pub fn integrate(&mut self, overlay: &Overlay) -> IntegrationOutcome {
        let outcome = self.decide(overlay);
        if outcome.is_applied() {
            self.total_overlay_bytes += overlay.overlay_bytes();
            self.agents.insert(overlay.agent().to_owned());
        }
        self.history.push(outcome.clone());
        outcome
    }

    fn decide(&mut self, overlay: &Overlay) -> IntegrationOutcome {
        let mut invalid = Vec::new();
        for path in overlay.changed_paths() {
            if let Err(error) = validate_relative_path(&path) {
                invalid.push(format!("{path}: {error}"));
            }
        }
        if !invalid.is_empty() {
            return IntegrationOutcome::RejectedInvalid {
                agent: overlay.agent().to_owned(),
                reasons: invalid,
            };
        }
        if overlay.base_id() != self.base.id() {
            return IntegrationOutcome::RejectedStale {
                agent: overlay.agent().to_owned(),
                expected: overlay.base_id().to_owned(),
                actual: self.base.id().to_owned(),
            };
        }
        let conflicts: Vec<String> = overlay
            .changed_paths()
            .into_iter()
            .filter(|path| self.applied.contains_key(path))
            .collect();
        if !conflicts.is_empty() {
            return IntegrationOutcome::ConflictPath {
                agent: overlay.agent().to_owned(),
                paths: conflicts,
            };
        }
        let paths = overlay.changed_paths();
        for path in &paths {
            if let Some(bytes) = overlay.writes().get(path) {
                self.files.insert(path.clone(), bytes.clone());
            } else {
                self.files.remove(path);
            }
            self.applied
                .insert(path.clone(), overlay.agent().to_owned());
        }
        IntegrationOutcome::Applied {
            agent: overlay.agent().to_owned(),
            paths,
        }
    }

    /// Publie l'état intégré comme nouvelle base canonique, après publication
    /// Git réussie. Retourne le nouvel identifiant de base.
    ///
    /// Tant que `publish` n'est pas appelé, la base reste celle que les agents
    /// ont lue : une proposition indépendante peut donc encore être intégrée.
    /// Après publication, toute proposition construite sur l'ancienne base est
    /// refusée (`RejectedStale`) et doit être rejouée.
    pub fn publish(&mut self) -> String {
        let snapshot = self.snapshot_after();
        self.base = snapshot.clone();
        snapshot.id().to_owned()
    }

    /// Nouvel état canonique après intégration (nouvelle identité de contenu).
    pub fn snapshot_after(&self) -> BaseSnapshot {
        BaseSnapshot::from_files(self.files.clone())
    }

    /// Coût de stockage des overlays, comparé à des copies privées complètes.
    pub fn storage_report(&self) -> StorageReport {
        let base_bytes = self.base.total_bytes();
        let agents = self.agents.len();
        StorageReport {
            schema_version: OVERLAY_SCHEMA_VERSION,
            agents,
            base_bytes,
            overlay_bytes: self.total_overlay_bytes,
            full_copies_bytes: base_bytes.saturating_mul(agents),
        }
    }
}
