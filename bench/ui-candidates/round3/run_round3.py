#!/usr/bin/env python3
"""U1-B tour 3 : la même scène dans chaque pile complète, sur une vraie machine.

À lancer sur les machines du responsable (UI-D9), pas en CI : il faut une
fenêtre et un GPU. Aucune dépendance hors de la bibliothèque standard.

Pour chaque pile (`egui`, `slint`, `modular`) :
  1. compilation `release` (`cargo build --release -p r3-<pile>`), taille du
     binaire ;
  2. N exécutions de l'application, chacune affichant la scène commune
     (`round3/common`) pendant `--frames` frames puis se fermant seule ;
  3. par exécution : rapport JSON de l'application (première frame,
     intervalles entre frames) et mesures externes du processus (temps
     écoulé, temps CPU utilisateur et système, pic de mémoire résidente).

Fail-closed : une exécution ne compte que si le processus sort avec le code
0 **et** a écrit un rapport valide du bon schéma, avec le bon nombre de
frames. Verdicts par pile : PASS (toutes les exécutions valides), FAIL (au
moins une invalide), NOT_RUN (compilation impossible ou interruption). Un
PASS dit seulement que les mesures ont été prises, pas qu'une pile « gagne ».

Sortie : un répertoire avec `report.json`, les journaux et les rapports
bruts. Codes de sortie : 0 si toutes les piles demandées sont PASS, 1 sinon.

Usage :
    python3 round3/run_round3.py --runs 5 --frames 600 --rows 10000
    python3 round3/run_round3.py --stacks modular,egui --out mesures-pc1

Notez la machine dans `--note` (GPU, écran, fréquence de rafraîchissement,
alimentation) : le script collecte ce qu'il peut, mais pas tout.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
WORKSPACE = HERE.parent  # bench/ui-candidates
STACKS = ["egui", "slint", "modular"]
SCHEMA = "ravum.ui.round3/v1"


def sh(cmd: list[str]) -> str:
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=60).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def host_facts() -> dict:
    facts = {
        "os": f"{platform.system()} {platform.release()} {platform.machine()}",
        "cpu": platform.processor() or "unknown",
        "python": platform.python_version(),
        "rustc": sh(["rustc", "--version"]) or "unknown",
        "commit": sh(["git", "-C", str(WORKSPACE), "rev-parse", "HEAD"]) or "unknown",
        "dirty": bool(sh(["git", "-C", str(WORKSPACE), "status", "--porcelain"])),
    }
    system = platform.system()
    if system == "Linux":
        gpu = sh(["sh", "-c", "lspci 2>/dev/null | grep -iE 'vga|3d|display'"])
        facts["gpu"] = gpu or "unknown (lspci unavailable)"
        facts["session"] = os.environ.get("XDG_SESSION_TYPE", "unknown")
    elif system == "Darwin":
        facts["gpu"] = sh(["sh", "-c", "system_profiler SPDisplaysDataType | grep -E 'Chipset|Resolution|Refresh'"]) or "unknown"
    elif system == "Windows":
        facts["gpu"] = sh(["powershell", "-NoProfile", "-Command",
                           "(Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join '; '"]) or "unknown"
    return facts


def binary(stack: str) -> Path:
    exe = ".exe" if platform.system() == "Windows" else ""
    target = Path(os.environ.get("CARGO_TARGET_DIR", WORKSPACE / "target"))
    return target / "release" / f"r3-{stack}{exe}"


# ---------------------------------------------------------------- process

def run_measured(cmd: list[str], env: dict, timeout: float) -> dict:
    """Run `cmd`; return exit code, wall time, CPU time and peak RSS."""
    t0 = time.perf_counter()
    if platform.system() == "Windows":
        return _run_windows(cmd, env, timeout, t0)
    p = subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    deadline = t0 + timeout
    while True:
        pid, status, ru = os.wait4(p.pid, os.WNOHANG)
        if pid:
            break
        if time.perf_counter() > deadline:
            p.kill()
            pid, status, ru = os.wait4(p.pid, 0)
            out = p.stdout.read().decode(errors="replace") if p.stdout else ""
            return {"exit": None, "timeout": True, "log": out}
        time.sleep(0.02)
    wall = time.perf_counter() - t0
    out = p.stdout.read().decode(errors="replace") if p.stdout else ""
    code = os.waitstatus_to_exitcode(status)
    # ru_maxrss: KiB on Linux, bytes on macOS.
    rss_kib = ru.ru_maxrss // 1024 if platform.system() == "Darwin" else ru.ru_maxrss
    return {"exit": code, "timeout": False, "wall_s": wall, "cpu_user_s": ru.ru_utime,
            "cpu_sys_s": ru.ru_stime, "peak_rss_kib": rss_kib, "log": out}


def _run_windows(cmd: list[str], env: dict, timeout: float, t0: float) -> dict:
    import ctypes
    from ctypes import wintypes

    p = subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        out, _ = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
        return {"exit": None, "timeout": True, "log": out.decode(errors="replace")}
    wall = time.perf_counter() - t0

    class PMC(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD),
                    ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                    ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
                    ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                    ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t)]

    handle = wintypes.HANDLE(int(p._handle))  # the process handle stays valid until Popen is collected
    ft = [wintypes.FILETIME() for _ in range(4)]
    ok_t = ctypes.windll.kernel32.GetProcessTimes(handle, *[ctypes.byref(f) for f in ft])
    pmc = PMC()
    pmc.cb = ctypes.sizeof(PMC)
    ok_m = ctypes.windll.psapi.GetProcessMemoryInfo(handle, ctypes.byref(pmc), pmc.cb)
    tick = lambda f: ((f.dwHighDateTime << 32) | f.dwLowDateTime) / 1e7  # 100 ns units
    res = {"exit": p.returncode, "timeout": False, "wall_s": wall, "log": out.decode(errors="replace")}
    if ok_t:
        res["cpu_sys_s"], res["cpu_user_s"] = tick(ft[2]), tick(ft[3])
    if ok_m:
        res["peak_rss_kib"] = pmc.PeakWorkingSetSize // 1024
    return res


# ---------------------------------------------------------------- verdicts

def judge(proc: dict, report_path: Path, frames: int, stack: str) -> dict:
    run = {k: v for k, v in proc.items() if k != "log"}
    run["verdict"], run["reason"] = "FAIL", ""
    if proc.get("timeout"):
        run["reason"] = "timeout"
        return run
    if proc.get("exit") != 0:
        run["reason"] = f"exit code {proc.get('exit')}"
        return run
    try:
        rep = json.loads(report_path.read_text())
    except (OSError, ValueError) as e:
        run["reason"] = f"no valid report: {e}"
        return run
    if rep.get("schema") != SCHEMA or rep.get("stack") != stack or rep.get("frames") != frames:
        run["reason"] = "report of the wrong schema, stack or frame count"
        return run
    run["app"] = rep
    for k in ("cpu_user_s", "cpu_sys_s", "peak_rss_kib", "wall_s"):
        if k not in proc:
            run["reason"] = f"missing external measure {k}"
            return run
    run["cpu_ms_per_frame"] = (proc["cpu_user_s"] + proc["cpu_sys_s"]) * 1e3 / frames
    run["verdict"] = "PASS"
    return run


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--stacks", default=",".join(STACKS))
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--frames", type=int, default=600)
    ap.add_argument("--rows", type=int, default=10_000)
    ap.add_argument("--timeout", type=float, default=300.0, help="seconds per run")
    ap.add_argument("--out", type=Path, default=None)
    ap.add_argument("--note", default="", help="machine notes: GPU, display, refresh rate, power")
    ap.add_argument("--no-build", action="store_true", help="use binaries already built")
    args = ap.parse_args(argv)

    stacks = [s for s in args.stacks.split(",") if s]
    unknown = [s for s in stacks if s not in STACKS]
    if unknown or args.runs <= 0 or args.frames <= 0:
        print(f"bad arguments: unknown stacks {unknown}, runs {args.runs}, frames {args.frames}", file=sys.stderr)
        return 1
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = (args.out or Path(f"round3-{stamp}")).resolve()
    out.mkdir(parents=True, exist_ok=True)
    report = {"schema": "ravum.ui.round3.campaign/v1", "date": stamp, "host": host_facts(),
              "note": args.note, "params": {"runs": args.runs, "frames": args.frames, "rows": args.rows},
              "stacks": {}}

    def save() -> None:
        (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")

    try:
        for stack in stacks:
            entry = report["stacks"].setdefault(stack, {"runs": []})
            if not args.no_build:
                t0 = time.perf_counter()
                b = subprocess.run(["cargo", "build", "--release", "--locked", "-p", f"r3-{stack}"],
                                   cwd=WORKSPACE, capture_output=True, text=True)
                (out / f"build-{stack}.log").write_text(b.stdout + b.stderr)
                entry["build_s"] = time.perf_counter() - t0
                if b.returncode != 0:
                    entry.update(verdict="NOT_RUN", reason="build failed (see the build log)")
                    save()
                    continue
            exe = binary(stack)
            if not exe.exists():
                entry.update(verdict="NOT_RUN", reason=f"binary not found: {exe}")
                save()
                continue
            entry["binary_bytes"] = exe.stat().st_size
            for i in range(1, args.runs + 1):
                rp = out / f"{stack}-run{i}.json"
                rp.unlink(missing_ok=True)
                env = dict(os.environ, R3_OUT=str(rp), R3_FRAMES=str(args.frames), R3_ROWS=str(args.rows))
                proc = run_measured([str(exe)], env, args.timeout)
                (out / f"{stack}-run{i}.log").write_text(proc.get("log", ""))
                entry["runs"].append(judge(proc, rp, args.frames, stack))
                save()
            bad = [r for r in entry["runs"] if r["verdict"] != "PASS"]
            entry["verdict"] = "FAIL" if bad else "PASS"
            entry["reason"] = bad[0]["reason"] if bad else f"{args.runs} valid run(s)"
            save()
    except KeyboardInterrupt:
        for s in stacks:
            report["stacks"].setdefault(s, {"runs": []}).setdefault("verdict", "NOT_RUN")
        report["interrupted"] = True
        save()
        print(f"interrupted — partial report: {out / 'report.json'}")
        return 1

    save()
    print(f"report: {out / 'report.json'}")
    for s in stacks:
        e = report["stacks"][s]
        line = f"{s:8} {e.get('verdict')}: {e.get('reason')}"
        good = [r for r in e.get("runs", []) if r["verdict"] == "PASS"]
        if good:
            med = lambda xs: sorted(xs)[len(xs) // 2]
            line += (f" | first frame {med([r['app']['first_frame_ms'] for r in good]):.0f} ms"
                     f" | interval p50 {med([r['app']['interval_ms']['p50'] for r in good]):.2f} ms"
                     f" p99 {med([r['app']['interval_ms']['p99'] for r in good]):.2f} ms"
                     f" | CPU {med([r['cpu_ms_per_frame'] for r in good]):.2f} ms/frame"
                     f" | peak RSS {med([r['peak_rss_kib'] for r in good]) // 1024} MiB")
        print(line)
    return 0 if all(report["stacks"][s].get("verdict") == "PASS" for s in stacks) else 1


if __name__ == "__main__":
    sys.exit(main())
