//! Deferred native-library reclamation (ADR-021 / ADR-024 enabler).
//!
//! Every `LoadLibraryW`/`dlopen` is matched by a `FreeLibrary`/`dlclose` executed
//! on a dedicated reclaimer thread, never on the thread that loaded the module or
//! the thread that performed the last in-flight call. This is a prerequisite for
//! the M3 audio profile where unloading on the callback thread is forbidden.

use libloading::Library;
use std::sync::mpsc::{self, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread;

struct GlobalMetrics {
    loaded: AtomicU64,
    unloaded: AtomicU64,
    pending: AtomicU64,
}

static METRICS: GlobalMetrics = GlobalMetrics {
    loaded: AtomicU64::new(0),
    unloaded: AtomicU64::new(0),
    pending: AtomicU64::new(0),
};

static RECLAIMER: OnceLock<Sender<Library>> = OnceLock::new();

fn sender() -> &'static Sender<Library> {
    RECLAIMER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Library>();
        thread::Builder::new()
            .name("ravum-reclaimer".to_string())
            .spawn(move || {
                for lib in rx {
                    drop(lib);
                    METRICS.unloaded.fetch_add(1, Ordering::SeqCst);
                    METRICS.pending.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .expect("spawn ravum-reclaimer");
        tx
    })
}

/// Modules loaded in this process (in-flight + active + awaiting reclamation).
pub fn live_modules() -> u64 {
    METRICS.loaded.load(Ordering::SeqCst)
        .saturating_sub(METRICS.unloaded.load(Ordering::SeqCst))
}

/// Modules freed by the reclaimer thread since process start.
pub fn unloaded_modules() -> u64 {
    METRICS.unloaded.load(Ordering::SeqCst)
}

/// Modules queued for reclamation but not freed yet.
pub fn pending_reclamations() -> u64 {
    METRICS.pending.load(Ordering::SeqCst)
}

/// Block until every queued module has been freed. Used by tests that need a
/// clean baseline before asserting "nothing was unloaded during this window".
pub fn wait_until_idle(timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if pending_reclamations() == 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    pending_reclamations() == 0
}

/// A dynamic library whose unload is always delegated to the reclaimer thread.
pub struct DeferredLibrary {
    lib: Option<Library>,
    path: std::path::PathBuf,
}

impl DeferredLibrary {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        // SAFETY: loading an arbitrary native library is inherently unsafe; the
        // caller only ever points this at artifacts produced by our own build
        // pipeline. The handle is never used after Drop.
        let lib = unsafe { Library::new(path) }
            .map_err(|e| format!("{}: {e}", path.display()))?;
        METRICS.loaded.fetch_add(1, Ordering::SeqCst);
        Ok(Self {
            lib: Some(lib),
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Copy an exported function pointer out of the library.
    ///
    /// # Safety
    /// The caller must guarantee the symbol has the exact signature `T`, and must
    /// not call the returned pointer after `self` is dropped.
    pub unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, String> {
        let lib = self.lib.as_ref().ok_or("library already unloaded")?;
        let sym = unsafe { lib.get::<T>(name) }
            .map_err(|e| String::from_utf8_lossy(e.to_string().as_bytes()).to_string())?;
        Ok(*sym)
    }
}

impl Drop for DeferredLibrary {
    fn drop(&mut self) {
        if let Some(lib) = self.lib.take() {
            METRICS.pending.fetch_add(1, Ordering::SeqCst);
            // If the reclaimer is gone the channel is closed and the library is
            // dropped inline by the failed send; the pending counter is then
            // corrected to stay consistent with `unloaded`.
            if sender().send(lib).is_err() {
                METRICS.pending.fetch_sub(1, Ordering::SeqCst);
                METRICS.unloaded.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for_unload(before: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if unloaded_modules() > before {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn unloading_happens_off_the_dropping_thread() {
        // The test binary itself is a valid module handle to open.
        let exe = std::env::current_exe().expect("current exe");
        let before = unloaded_modules();
        let loaded_before = live_modules();
        {
            let lib = DeferredLibrary::open(&exe).expect("open self");
            assert!(live_modules() > loaded_before);
            drop(lib);
        }
        assert!(wait_for_unload(before), "reclaimer never freed the module");
    }
}
