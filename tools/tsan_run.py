#!/usr/bin/env python3
"""ThreadSanitizer qualification of Ravum's concurrent path (RFC-E11-CONCURRENCY §5).

Every `run-pass` conformance case whose source uses `thread::`, `sync::`,
`net::` or `http::` is built by `ravum-lang build` through a rustc wrapper that
adds `-Zsanitizer=thread`, then executed several times with
`TSAN_OPTIONS=halt_on_error=1`. Any ThreadSanitizer report, non-zero exit or
missing stdout line fails the run.

The run is never vacuous:
* a positive control (a Rust program with a real data race, built by the same
  wrapper) must be reported, otherwise the instrumentation is not working;
* the generated binaries must contain `__tsan_` symbols;
* zero selected cases is a failure, not a pass.

Limit, stated honestly: the standard library is not rebuilt with TSan
(`-Cunsafe-allow-abi-mismatch=sanitizer`, no `-Zbuild-std`). Races inside
`std` itself are not observed; races in the generated program and its
runtime prelude are. A green run is an observation on this corpus, host and
commit, not a proof of race freedom.

Usage: python3 tools/tsan_run.py [--ravum-lang PATH] [--runs N]
Exit codes: 0 = no report, 1 = report or failure, 2 = environment problem.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "conformance" / "cases"
MARKERS = ("thread::", "sync::", "net::", "http::")

RACE = """static mut COUNTER: u64 = 0;
fn main() {
    let t = std::thread::spawn(|| for _ in 0..100000 { unsafe { COUNTER += 1; } });
    for _ in 0..100000 { unsafe { COUNTER += 1; } }
    t.join().unwrap();
    println!("{}", unsafe { COUNTER });
}
"""


def write_wrapper(directory: Path, toolchain: str) -> Path:
    wrapper = directory / "rustc-tsan"
    wrapper.write_text(
        "#!/bin/sh\n"
        f'exec rustc +{toolchain} "$@" -Zsanitizer=thread '
        "-Cunsafe-allow-abi-mismatch=sanitizer\n",
        encoding="utf-8",
    )
    wrapper.chmod(wrapper.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return wrapper


def tsan_env() -> dict[str, str]:
    env = dict(os.environ)
    env["TSAN_OPTIONS"] = "halt_on_error=1 exitcode=66 second_deadlock_stack=1"
    return env


def selected_cases() -> list[Path]:
    chosen = []
    for case_json in sorted(CASES.glob("*/*/case.json")):
        case = json.loads(case_json.read_text(encoding="utf-8"))
        if case.get("mode") != "run-pass":
            continue
        sources = "".join(
            p.read_text(encoding="utf-8") for p in sorted(case_json.parent.rglob("*.rv"))
        )
        if any(marker in sources for marker in MARKERS):
            chosen.append(case_json.parent)
    return chosen


def positive_control(work: Path, wrapper: Path) -> str | None:
    source = work / "race.rs"
    source.write_text(RACE, encoding="utf-8")
    binary = work / "race"
    build = subprocess.run(
        [str(wrapper), "--edition", "2021", "-o", str(binary), str(source)],
        capture_output=True,
        text=True,
    )
    if build.returncode != 0:
        return f"the control does not build:\n{build.stderr}"
    run = subprocess.run([str(binary)], capture_output=True, text=True, env=tsan_env())
    if "ThreadSanitizer: data race" not in run.stderr:
        return "the control race was NOT reported: the instrumentation is not working"
    return None


def has_tsan_symbols(binary: Path) -> bool:
    data = binary.read_bytes()
    return b"__tsan_" in data


def run_case(case_dir: Path, work: Path, ravum: str, wrapper: Path, runs: int) -> str | None:
    case = json.loads((case_dir / "case.json").read_text(encoding="utf-8"))
    package = work / case["id"].replace("/", "__")
    shutil.copytree(case_dir, package)
    env = tsan_env()
    env["RAVUM_RUSTC"] = str(wrapper)
    env["RAVUM_NO_INCREMENTAL"] = "1"
    build = subprocess.run(
        [ravum, "build", str(package)], capture_output=True, text=True, env=env
    )
    if build.returncode != 0:
        return f"build failed:\n{build.stdout}{build.stderr}"
    built = next(
        (line[len("built: ") :].strip() for line in build.stdout.splitlines() if line.startswith("built: ")),
        None,
    )
    if built is None:
        return "no `built:` line"
    binary = Path(built)
    if not has_tsan_symbols(binary):
        return "the binary is not instrumented (no `__tsan_` symbol)"
    wanted = case.get("stdout_contains") or []
    if isinstance(wanted, str):
        wanted = [wanted]
    for attempt in range(runs):
        run = subprocess.run(
            [str(binary), *case.get("run_args", [])],
            capture_output=True,
            text=True,
            env=env,
            cwd=package,
            timeout=120,
        )
        if "ThreadSanitizer" in run.stderr:
            return f"run {attempt + 1}: ThreadSanitizer report:\n{run.stderr}"
        if run.returncode != 0:
            return f"run {attempt + 1}: exit {run.returncode}\n{run.stderr}"
        missing = [text for text in wanted if text not in run.stdout]
        if missing:
            return f"run {attempt + 1}: stdout lacks {missing!r}"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ravum-lang", default=str(ROOT / "target" / "debug" / "ravum-lang"))
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--toolchain", default=os.environ.get("RAVUM_TSAN_TOOLCHAIN", "nightly"))
    args = parser.parse_args()
    if not sys.platform.startswith("linux"):
        print("tsan_run: NOT_RUN — ThreadSanitizer is qualified on Linux only")
        return 2
    if not Path(args.ravum_lang).is_file():
        print(f"tsan_run: NOT_RUN — `{args.ravum_lang}` does not exist; build ravum-lang first")
        return 2
    with tempfile.TemporaryDirectory(prefix="ravum-tsan-") as tmp:
        work = Path(tmp)
        wrapper = write_wrapper(work, args.toolchain)
        problem = positive_control(work, wrapper)
        if problem:
            print(f"tsan_run: FAIL — positive control: {problem}")
            return 1
        print("tsan_run: positive control reported (instrumentation works)")
        cases = selected_cases()
        if not cases:
            print("tsan_run: FAIL — no concurrent run-pass case selected")
            return 1
        failures = 0
        for case_dir in cases:
            rel = case_dir.relative_to(CASES)
            problem = run_case(case_dir, work, args.ravum_lang, wrapper, args.runs)
            if problem:
                failures += 1
                print(f"FAIL  {rel}: {problem}")
            else:
                print(f"PASS  {rel} ({args.runs} runs, no report)")
        print(
            f"tsan_run: {len(cases)} case(s), {failures} failure(s), {args.runs} run(s) each; "
            "std is not instrumented (see the module docstring)"
        )
        return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
