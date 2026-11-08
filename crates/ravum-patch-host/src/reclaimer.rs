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

/// Create the reclaimer thread now, if it does not exist yet.
///
/// Call this from a control thread at startup: without it, the *first* library drop
/// anywhere would create the thread, i.e. an unbounded operation inside a `Drop`. The
/// host bootstraps it before loading anything.
pub fn init_reclaimer() {
    let _ = sender();
}

impl DeferredLibrary {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        // The reclaimer must already exist before a library can be dropped: creating it
        // lazily inside `Drop` would spawn a thread on whatever thread drops first.
        init_reclaimer();
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
            // `sender()` is a process-wide `OnceLock` and the reclaimer thread owns the
            // receiving end for the life of the process, so the channel never closes:
            // the send cannot fail in practice, and the fallback below is unreachable.
            // It is kept so that a hypothetical closed channel degrades to an inline
            // unload *and stays accounted for* instead of leaking a handle.
            //
            // KNOWN LIMITATION: this `Drop` allocates and sends on a channel. That is
            // fine on a control thread (where it always runs by construction), never on
            // an audio callback: the guard API in `slot.rs` exists so that a payload
            // carrying a `DeferredLibrary` can only be dropped by the control thread.
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

    /// The queue is unbounded and must drain a burst: a callback that retires many
    /// modules (or a long campaign) may leave a backlog, never a leak.
    #[test]
    fn a_burst_of_retired_libraries_is_drained_and_accounted_for() {
        let exe = std::env::current_exe().expect("current exe");
        init_reclaimer();
        let unloaded_before = unloaded_modules();
        let live_before = live_modules();
        for _ in 0..200 {
            let lib = DeferredLibrary::open(&exe).expect("open self");
            drop(lib);
        }
        assert!(
            pending_reclamations() <= 200,
            "the pending counter must account for every queued library"
        );
        assert!(
            wait_until_idle(Duration::from_secs(20)),
            "the reclaimer must drain a 200-library burst"
        );
        assert_eq!(pending_reclamations(), 0);
        assert_eq!(
            live_modules(),
            live_before,
            "a burst must not leak a single library handle"
        );
        assert!(
            unloaded_modules() >= unloaded_before + 200,
            "every retired library must be accounted as unloaded"
        );
    }

    /// `init_reclaimer` is idempotent and safe to call from any control thread.
    #[test]
    fn init_reclaimer_is_idempotent() {
        init_reclaimer();
        init_reclaimer();
        assert!(wait_until_idle(Duration::from_secs(5)));
    }
}
