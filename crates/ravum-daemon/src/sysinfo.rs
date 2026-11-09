//! Machine-load observability: CPU busy ratio, free memory, process count and
//! the daemon's own working set.
//!
//! The reference machine is shared with other agents, so every measurement
//! campaign records the load it ran under. All observability here is read-only
//! and best-effort: `None` means "not observed", never "zero".
//!
//! Windows uses four documented Win32 calls (`GlobalMemoryStatusEx`,
//! `GetSystemTimes`, `GetPerformanceInfo`, `GetProcessMemoryInfo`). The system
//! FFI is confined to this module; every `unsafe` block carries a `SAFETY`
//! comment stating the invariant it relies on.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One load observation of the host.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemSnapshot {
    /// Busy CPU ratio over a short sampling window (all cores).
    pub cpu_busy_percent: Option<f64>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    /// Number of processes running on the machine (concurrent agents/builds).
    pub process_count: Option<u32>,
    /// Working set of the current process (the daemon, in the demo binary).
    pub daemon_working_set_bytes: Option<u64>,
    pub captured_unix: u64,
}

/// Captures a load snapshot, sampling CPU over `cpu_sample_ms`.
pub fn capture(cpu_sample_ms: u64) -> SystemSnapshot {
    let (cpu_busy_percent, memory_total_bytes, memory_available_bytes, process_count, working_set) =
        platform::capture(cpu_sample_ms);
    SystemSnapshot {
        cpu_busy_percent,
        memory_total_bytes,
        memory_available_bytes,
        process_count,
        daemon_working_set_bytes: working_set,
        captured_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    }
}

/// Spawns `rustc -vV` `samples` times and returns each wall time in
/// milliseconds. This is the machine-load calibration used by MVP-0a: it
/// measures process start + OS scheduling, not compiler analysis.
pub fn spawn_calibration(rustc: &Path, samples: usize) -> Vec<u64> {
    let mut out = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let _ = std::process::Command::new(rustc)
            .arg("-vV")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        out.push(started.elapsed().as_millis() as u64);
    }
    out
}

/// Sleeps `millis` while keeping the call site explicit about intent.
pub fn pause(millis: u64) {
    std::thread::sleep(Duration::from_millis(millis));
}

#[cfg(windows)]
mod platform {
    use std::mem::size_of;

    #[repr(C)]
    #[derive(Default)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    impl FileTime {
        fn as_u64(self) -> u64 {
            ((self.high as u64) << 32) | self.low as u64
        }
    }

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    impl Default for ProcessEntry32W {
        fn default() -> Self {
            Self {
                dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
                cnt_usage: 0,
                th32_process_id: 0,
                th32_default_heap_id: 0,
                th32_module_id: 0,
                cnt_threads: 0,
                th32_parent_process_id: 0,
                pc_pri_class_base: 0,
                dw_flags: 0,
                sz_exe_file: [0; 260],
            }
        }
    }

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
        fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
        fn GetCurrentProcess() -> isize;
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> isize;
        fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    /// Counts processes through a toolhelp snapshot. This is the number of
    /// concurrent processes on the shared machine, a load indicator only.
    fn count_processes() -> Option<u32> {
        // SAFETY: the snapshot handle is checked against INVALID_HANDLE_VALUE
        // and closed once; each call passes a live, correctly-sized entry.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry = ProcessEntry32W::default();
            let mut count: u32 = 0;
            let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
            while ok {
                count = count.saturating_add(1);
                entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
                ok = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            (count > 0).then_some(count)
        }
    }

    pub(super) fn capture(
        cpu_sample_ms: u64,
    ) -> (
        Option<f64>,
        Option<u64>,
        Option<u64>,
        Option<u32>,
        Option<u64>,
    ) {
        // SAFETY: the struct is `repr(C)`, fully initialized and its `length`
        // field is set to its own size, as the API requires.
        let mut memory = MemoryStatusEx {
            length: size_of::<MemoryStatusEx>() as u32,
            ..Default::default()
        };
        let memory_ok = unsafe { GlobalMemoryStatusEx(&mut memory) } != 0;
        let (memory_total, memory_available) = if memory_ok {
            (Some(memory.total_phys), Some(memory.avail_phys))
        } else {
            (None, None)
        };

        // SAFETY: all three pointers reference valid, writable, correctly-sized
        // `FileTime` values for the duration of the call.
        let first = {
            let (mut idle, mut kernel, mut user) = (
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
            );
            let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0;
            ok.then(|| (idle.as_u64(), kernel.as_u64(), user.as_u64()))
        };
        std::thread::sleep(std::time::Duration::from_millis(cpu_sample_ms.max(20)));
        // SAFETY: same invariant as the first sample.
        let second = {
            let (mut idle, mut kernel, mut user) = (
                FileTime::default(),
                FileTime::default(),
                FileTime::default(),
            );
            let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0;
            ok.then(|| (idle.as_u64(), kernel.as_u64(), user.as_u64()))
        };
        let cpu_busy_percent = match (first, second) {
            (Some(first), Some(second)) => {
                let idle = second.0.saturating_sub(first.0);
                let total = second
                    .1
                    .saturating_sub(first.1)
                    .saturating_add(second.2.saturating_sub(first.2));
                if total == 0 {
                    None
                } else {
                    Some(100.0 * (1.0 - idle as f64 / total as f64))
                }
            }
            _ => None,
        };

        let process_count = count_processes();

        // SAFETY: `counters` is correctly sized and `cb` is its size; the
        // pseudo-handle returned by `GetCurrentProcess` needs no release.
        let mut counters = ProcessMemoryCounters {
            cb: size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        let memory_info_ok =
            unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) } != 0;
        let working_set = memory_info_ok.then_some(counters.working_set_size as u64);

        (
            cpu_busy_percent,
            memory_total,
            memory_available,
            process_count,
            working_set,
        )
    }
}

#[cfg(not(windows))]
mod platform {
    pub(super) fn capture(
        _cpu_sample_ms: u64,
    ) -> (
        Option<f64>,
        Option<u64>,
        Option<u64>,
        Option<u32>,
        Option<u64>,
    ) {
        // Portable build: the Windows reference machine is where load is
        // measured; elsewhere the fields stay explicitly unobserved.
        (None, None, None, None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_returns_unix_timestamp() {
        let snapshot = capture(20);
        assert!(snapshot.captured_unix > 1_600_000_000);
    }

    #[cfg(windows)]
    #[test]
    fn windows_capture_observes_memory_and_processes() {
        let snapshot = capture(20);
        assert!(snapshot.memory_total_bytes.unwrap_or(0) > 0);
        assert!(
            snapshot.process_count.unwrap_or(0) > 0,
            "snapshot: {snapshot:?}"
        );
        assert!(snapshot.daemon_working_set_bytes.unwrap_or(0) > 0);
    }

    #[test]
    fn spawn_calibration_measures_rustc() {
        let rustc = std::env::var_os("RAVUM_RUSTC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("rustc"));
        let samples = spawn_calibration(&rustc, 2);
        assert_eq!(samples.len(), 2);
    }
}
/// Working set (RSS) of an arbitrary process, for external soak harnesses that
/// spawn the daemon binary. Windows only; `None` when the process is gone or the
/// query is refused.
#[cfg(windows)]
pub fn process_working_set(pid: u32) -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    const PROCESS_VM_READ: u32 = 0x0010;
    // SAFETY: the handle is checked against 0 and closed on every path; the
    // counters struct is `repr(C)`, zeroed and its `cb` carries its own size.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle == 0 {
            return None;
        }
        let mut counters = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Default::default()
        };
        let ok = GetProcessMemoryInfo(handle, &mut counters, counters.cb) != 0;
        CloseHandle(handle);
        ok.then_some(counters.working_set_size as u64)
    }
}

/// Non-Windows builds do not observe foreign-process memory.
#[cfg(not(windows))]
pub fn process_working_set(_pid: u32) -> Option<u64> {
    None
}
