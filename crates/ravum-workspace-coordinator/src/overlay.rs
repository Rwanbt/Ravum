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
//! * aucun chemin absolu, remontant (`..`), `\`, lecteur Windows, flux alternatif
//!   NTFS (`:`), nom réservé Windows, point/espace final, caractère de contrôle ou
//!   chemin trop long n'est accepté ;
//! * `materialize` revalide chaque chemin, refuse tout composant symlink/reparse
//!   point, écrit atomiquement (fichier temporaire + rename) et applique une
//!   police de tailles (nombre de fichiers, octets) pour borner une zip-bomb ;
//! * le coût de stockage est celui des fichiers écrits, jamais celui de l'arbre.
//!
//! # Frontière de confiance et limites TOCTOU
//!
//! `BaseSnapshot` et `Overlay` sont des valeurs en mémoire. `from_files` valide
//! les chemins avant de construire le snapshot, et `materialize` **revalide** :
//! un snapshot empoisonné (construit avant une évolution du validateur, désérialisé
//! par un futur format, ou reçu d'un autre composant) ne peut donc pas écrire hors
//! racine. La racine est canonicalisée avant toute écriture ; chaque composant
//! parent est vérifié avec `symlink_metadata` et n'est créé que s'il est absent.
//!
//! Limites résiduelles, explicitement **non** couvertes :
//!
//! * TOCTOU : entre le `symlink_metadata` d'un composant et le `rename` final, un
//!   processus local hostile peut remplacer un répertoire par un lien. Le module
//!   ne prétend pas être une barrière contre un attaquant qui contrôle le système
//!   de fichiers ; il empêche un chemin/snapshot hostile de traverser la racine.
//!   `openat`/`O_NOFOLLOW` portables n'existent pas dans `std` stable.
//! * Un reparse point Windows non-symlink (jonction montée) est détecté comme
//!   symlink par `FileType::is_symlink` pour les liens symboliques ; les jonctions
//!   (`IO_REPARSE_TAG_MOUNT_POINT`) ne sont pas distinguées par `std`. Le test de
//!   safety repose donc sur `is_symlink` ; un vrai durcissement nécessiterait
//!   `windows-sys` (dépendance non justifiée ici).
//! * L'atomicité est celle du `rename` (visibilité tout ou rien du fichier), pas
//!   une durabilité après coupure de courant : la vue matérialisée est un artefact
//!   de compilation, pas la base canonique.
//!
//! Non démontré ici : fusion de deux modifications dans une même fonction
//! (refusée, `ConflictPath`), convergence CRDT, publication Git (assurée par
//! [`GitWriter`](crate::GitWriter)), et vérification sémantique (assurée par
//! [`SemanticOracle`](crate::SemanticOracle)).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Version du schéma des rapports d'intégration.
pub const OVERLAY_SCHEMA_VERSION: u32 = 1;

/// Longueur maximale, en octets, d'un chemin relatif d'overlay.
///
/// La limite Windows historique (`MAX_PATH`, 260) s'applique aux chemins absolus ;
/// un chemin relatif de 1024 octets reste très en dessous de ce qu'un workspace
/// réel utilise, tout en refusant les chaînes démesurées utilisées pour sonder un
/// parseur ou dépasser un tampon interne.
pub const MAX_RELATIVE_PATH_BYTES: usize = 1024;

/// Longueur maximale, en octets, d'un composant de chemin.
///
/// 255 octets est la limite des systèmes de fichiers usuels (NTFS, ext4) : au-delà,
/// aucun fichier légitime ne peut être créé de toute façon.
pub const MAX_COMPONENT_BYTES: usize = 255;

/// Erreur de validation d'un overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayError {
    /// Chemin vide.
    EmptyPath,
    /// Chemin absolu, avec lecteur Windows, UNC ou verbatim : jamais accepté.
    AbsolutePath(String),
    /// Composant `.`/`..`/vide, antislash, caractère de contrôle, `:` (flux
    /// alternatif NTFS) ou point/espace final : jamais accepté.
    UnsafePath(String),
    /// Nom réservé Windows (CON, PRN, AUX, NUL, COM1-9, LPT1-9), y compris suivi
    /// d'une extension.
    ReservedName(String),
    /// Chemin ou composant trop long.
    TooLong { path: String, max: usize },
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
                "overlay path {path:?} must not contain `.`, `..`, a backslash, `:`, a control character, or a trailing dot/space"
            ),
            OverlayError::ReservedName(path) => {
                write!(f, "overlay path {path:?} uses a reserved Windows name")
            }
            OverlayError::TooLong { path, max } => write!(
                f,
                "overlay path {path:?} exceeds the {max}-byte limit"
            ),
        }
    }
}

impl std::error::Error for OverlayError {}

/// Valide un chemin relatif portable et l'écrit sous une forme canonique.
///
/// Règles (ordre significatif pour la classification d'erreur) :
/// 1. vide ou blanc → [`OverlayError::EmptyPath`] ;
/// 2. absolu (`/…`), lecteur (`C:…`), UNC (`\\\\serveur\\…`) ou verbatim
///    (`\\\\?\\…`) → [`OverlayError::AbsolutePath`] ;
/// 3. antislash, caractère de contrôle, `:` (ADS NTFS), composant vide, `.`/`..`,
///    point ou espace final → [`OverlayError::UnsafePath`] ;
/// 4. nom réservé Windows, même avec extension → [`OverlayError::ReservedName`] ;
/// 5. chemin ou composant trop long → [`OverlayError::TooLong`].
///
/// Le séparateur accepté est uniquement `/`. Aucune normalisation silencieuse
/// n'est faite : un chemin refusé ne doit jamais être « réparé » en chemin voisin.
pub fn validate_relative_path(path: &str) -> Result<(), OverlayError> {
    if path.trim().is_empty() {
        return Err(OverlayError::EmptyPath);
    }
    // Absolute forms: POSIX `/`, UNC/verbatim `\\`, Windows drive `C:`.
    if path.starts_with('/')
        || path.starts_with('\\')
        || (path.len() >= 2 && path.as_bytes()[1] == b':')
    {
        return Err(OverlayError::AbsolutePath(path.to_owned()));
    }
    if path.contains('\\') {
        return Err(OverlayError::UnsafePath(path.to_owned()));
    }
    if path
        .chars()
        .any(|c| c == '\0' || (c as u32) < 0x20 || (c as u32) == 0x7f)
    {
        return Err(OverlayError::UnsafePath(path.to_owned()));
    }
    if path.contains(':') {
        // NTFS alternate data stream (`file:stream`) or an ambiguous revision form.
        return Err(OverlayError::UnsafePath(path.to_owned()));
    }
    if path.len() > MAX_RELATIVE_PATH_BYTES {
        return Err(OverlayError::TooLong {
            path: path.to_owned(),
            max: MAX_RELATIVE_PATH_BYTES,
        });
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(OverlayError::UnsafePath(path.to_owned()));
        }
        if component.len() > MAX_COMPONENT_BYTES {
            return Err(OverlayError::TooLong {
                path: path.to_owned(),
                max: MAX_COMPONENT_BYTES,
            });
        }
        if component.ends_with('.') || component.ends_with(' ') {
            return Err(OverlayError::UnsafePath(path.to_owned()));
        }
        if is_windows_reserved_name(component) {
            return Err(OverlayError::ReservedName(path.to_owned()));
        }
    }
    Ok(())
}

/// Vrai pour un nom réservé Windows, y compris `CON.txt` et `NUL.tar.gz`.
///
/// Windows réserve le nom de base avant la première extension (`CON`, `con.txt`,
/// `COM1.log` sont tous refusés). `CONIN$`/`CONOUT$` sont ajoutés car ils nomment
/// aussi des périphériques de console.
fn is_windows_reserved_name(component: &str) -> bool {
    let base = component
        .split_once('.')
        .map(|(base, _)| base)
        .unwrap_or(component);
    let upper = base.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || is_reserved_numbered(&upper, "COM")
        || is_reserved_numbered(&upper, "LPT")
}

fn is_reserved_numbered(candidate: &str, prefix: &str) -> bool {
    let Some(rest) = candidate.strip_prefix(prefix) else {
        return false;
    };
    matches!(rest, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
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
    ///
    /// Les liens symboliques rencontrés sont ignorés : un snapshot ne doit pas
    /// capturer silencieusement du contenu extérieur à la racine.
    pub fn from_directory(root: &Path) -> io::Result<Self> {
        let mut files = BTreeMap::new();
        collect_files(root, root, &mut files)?;
        for path in files.keys() {
            validate_relative_path(path)
                .map_err(|error| invalid_input(format!("snapshot path {path:?}: {error}")))?;
        }
        Ok(Self::from_validated_files(files))
    }

    /// Snapshot explicite (tests, publication après intégration).
    ///
    /// Chaque chemin est validé : un snapshot empoisonné ne peut pas être construit
    /// par cette API. Les appelants internes qui ont déjà validé leurs chemins
    /// utilisent le constructeur privé.
    pub fn from_files(files: BTreeMap<String, Vec<u8>>) -> Result<Self, OverlayError> {
        for path in files.keys() {
            validate_relative_path(path)?;
        }
        Ok(Self::from_validated_files(files))
    }

    fn from_validated_files(files: BTreeMap<String, Vec<u8>>) -> Self {
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
) -> io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(current)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == "target" {
            continue;
        }
        // `symlink_metadata` does not follow: a link is skipped rather than read
        // from a possibly outside-of-root target.
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
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
    ///
    /// Chaque chemin est revalidé, la racine est canonicalisée, aucun composant
    /// symlink n'est suivi et les écritures sont atomiques. Voir
    /// [`Overlay::materialize_with_limits`] pour la police de tailles.
    pub fn materialize(&self, base: &BaseSnapshot, dir: &Path) -> io::Result<usize> {
        self.materialize_with_limits(base, dir, MaterializeLimits::default())
    }

    /// Variante explicite de [`Overlay::materialize`] avec des limites de taille.
    ///
    /// Une base ou un overlay hostile peut nommer beaucoup de fichiers ou de gros
    /// contenus ; les limites sont vérifiées **avant** chaque écriture, et le
    /// dépassement échoue sans continuer (aucune bombe locale silencieuse).
    pub fn materialize_with_limits(
        &self,
        base: &BaseSnapshot,
        dir: &Path,
        limits: MaterializeLimits,
    ) -> io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let root = std::fs::canonicalize(dir)?;
        let mut written = 0usize;
        let mut bytes_written = 0u64;
        for (path, bytes) in &base.files {
            if self.deletes.contains(path) || self.writes.contains_key(path) {
                continue;
            }
            write_one(
                &root,
                path,
                bytes,
                &limits,
                &mut written,
                &mut bytes_written,
            )?;
        }
        for (path, bytes) in &self.writes {
            write_one(
                &root,
                path,
                bytes,
                &limits,
                &mut written,
                &mut bytes_written,
            )?;
        }
        Ok(written)
    }
}

/// Bornes appliquées par [`Overlay::materialize`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializeLimits {
    /// Nombre maximal de fichiers matérialisés (base + overlay).
    pub max_files: usize,
    /// Total maximal d'octets matérialisés (base + overlay).
    pub max_bytes: u64,
}

impl Default for MaterializeLimits {
    fn default() -> Self {
        Self {
            max_files: 100_000,
            max_bytes: 1 << 30,
        }
    }
}

fn write_one(
    root: &Path,
    path: &str,
    bytes: &[u8],
    limits: &MaterializeLimits,
    written: &mut usize,
    bytes_written: &mut u64,
) -> io::Result<()> {
    // Defence in depth: the snapshot was validated at construction, but a poisoned
    // or future-deserialized snapshot must not be trusted by the writer.
    validate_relative_path(path)
        .map_err(|error| invalid_input(format!("refusing overlay path {path:?}: {error}")))?;
    if *written >= limits.max_files {
        return Err(invalid_input(format!(
            "materialize limit reached: more than {} files",
            limits.max_files
        )));
    }
    let next_total = bytes_written.saturating_add(bytes.len() as u64);
    if next_total > limits.max_bytes {
        return Err(invalid_input(format!(
            "materialize limit reached: more than {} bytes",
            limits.max_bytes
        )));
    }
    atomic_write_relative(root, path, bytes)?;
    *written += 1;
    *bytes_written = next_total;
    Ok(())
}

/// Écrit `path` sous `root` sans jamais suivre un lien existant.
///
/// Chaque composant parent est vérifié séparément ; un composant qui existe et
/// n'est pas un vrai répertoire fait échouer l'écriture. Le fichier est écrit dans
/// un temporaire du même répertoire puis `rename`-é sur la cible (atomicité de
/// visibilité, pas de durabilité après coupure de courant).
fn atomic_write_relative(root: &Path, path: &str, bytes: &[u8]) -> io::Result<()> {
    validate_relative_path(path)
        .map_err(|error| invalid_input(format!("refusing overlay path {path:?}: {error}")))?;
    let components: Vec<&str> = path.split('/').collect();
    let (file_name, directories) = components
        .split_last()
        .ok_or_else(|| invalid_input("overlay path has no component"))?;

    let mut current = root.to_path_buf();
    for component in directories {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid_input(format!(
                    "refusing to follow symbolic link {}",
                    current.display()
                )))
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(invalid_input(format!(
                    "{} exists and is not a directory",
                    current.display()
                )))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        // Race with another writer: only a real directory is acceptable.
                        let metadata = std::fs::symlink_metadata(&current)?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(invalid_input(format!(
                                "{} appeared and is not a real directory",
                                current.display()
                            )));
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }

    let target = current.join(file_name);
    match std::fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(invalid_input(format!(
                "refusing to replace symbolic link {}",
                target.display()
            )))
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(invalid_input(format!(
                "{} exists and is not a regular file",
                target.display()
            )))
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let parent = current;
    let temporary = unique_temporary_path(&parent);
    let result = (|| -> io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        drop(file);
        std::fs::rename(&temporary, &target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn unique_temporary_path(directory: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..64 {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let candidate = directory.join(format!(
            ".ravum-overlay-tmp-{}-{unique}-{nanos}",
            std::process::id()
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Extremely unlikely: let `create_new` report the collision instead of looping.
    directory.join(format!(".ravum-overlay-tmp-{}", std::process::id()))
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
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

    /// Base canonique courante (lecture seule). Les chemins qu'elle contient ont
    /// été validés à sa construction.
    pub fn base(&self) -> &BaseSnapshot {
        &self.base
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
    /// # Sémantique retenue
    ///
    /// * `base` devient [`ChangeCoordinator::snapshot_after`] : la nouvelle base
    ///   contient déjà toutes les intégrations publiées.
    /// * `applied` est **vidé** : il ne trace que les chemins modifiés dans le
    ///   cycle d'intégration courant. Le conserver ferait refuser en `ConflictPath`
    ///   une proposition légitime construite sur la nouvelle base et touchant un
    ///   chemin modifié au cycle précédent (l'ancien défaut : une fausse conflit
    ///   après chaque publication).
    /// * `total_overlay_bytes` et `agents` sont remis à zéro : ils mesurent le
    ///   cycle courant ([`StorageReport`]). Le coût du cycle précédent est déjà
    ///   reflété dans `base_bytes`.
    /// * `history` est **conservé** : c'est le journal d'audit de tous les
    ///   résultats, succès et refus, à travers les publications. Il n'est jamais
    ///   réinitialisé, pour ne pas perdre la trace d'un refus.
    pub fn publish(&mut self) -> String {
        let snapshot = self.snapshot_after();
        self.base = snapshot.clone();
        self.applied.clear();
        self.total_overlay_bytes = 0;
        self.agents.clear();
        snapshot.id().to_owned()
    }

    /// Nouvel état canonique après intégration (nouvelle identité de contenu).
    ///
    /// Les clés viennent de la base (validée) ou d'un overlay (validé à l'écriture
    /// et revalidé à l'intégration) : le constructeur privé évite une revalidation
    /// coûteuse tout en gardant l'invariant « toute clé est un chemin sûr ».
    pub fn snapshot_after(&self) -> BaseSnapshot {
        BaseSnapshot::from_validated_files(self.files.clone())
    }

    /// Coût de stockage des overlays du cycle courant, comparé à des copies
    /// privées complètes de la base courante.
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
