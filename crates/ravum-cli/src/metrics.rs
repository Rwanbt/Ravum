//! Measurement helpers: percentiles and (on Windows) current-process memory.

use serde_json::json;

/// Nearest-rank percentile over an owned, sortable sample set.
pub fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

pub fn stats(samples: &[u64]) -> serde_json::Value {
    if samples.is_empty() {
        return json!({"count": 0});
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let sum: u64 = sorted.iter().sum();
    json!({
        "count": sorted.len(),
        "min": sorted[0],
        "max": sorted[sorted.len() - 1],
        "mean": (sum as f64) / (sorted.len() as f64),
        "p50": percentile(&sorted, 50.0),
        "p95": percentile(&sorted, 95.0),
        "p99": percentile(&sorted, 99.0),
    })
}

pub fn env_info() -> serde_json::Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "parallelism": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "commit": std::env::var("RAVUM_COMMIT").unwrap_or_else(|_| "unknown".to_owned()),
    })
}

#[cfg(windows)]
mod win {
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
        fn GetCurrentProcess() -> isize;
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    /// Working set of the current process, in bytes.
    pub fn working_set_bytes() -> Option<u64> {
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        // SAFETY: `counters` is a valid, correctly-sized, writable struct and the
        // process handle is the pseudo-handle of the current process.
        let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
        (ok != 0).then_some(counters.working_set_size as u64)
    }
}

#[cfg(windows)]
pub fn working_set_bytes() -> Option<u64> {
    win::working_set_bytes()
}

#[cfg(not(windows))]
pub fn working_set_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        let v = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&v, 50.0), 5);
        assert_eq!(percentile(&v, 95.0), 10);
        assert_eq!(percentile(&v, 100.0), 10);
        assert_eq!(percentile(&[], 50.0), 0);
    }

    #[test]
    fn stats_reports_p50_p95_p99() {
        let s = stats(&[10, 30, 20, 40, 50]);
        assert_eq!(s["p50"], 30);
        assert_eq!(s["max"], 50);
        assert_eq!(s["count"], 5);
    }
}
