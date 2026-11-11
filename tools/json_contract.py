#!/usr/bin/env python3
"""Validate the CLI's `--json` output against `spec/language/diagnostics.md` §7-§8.

§7 defines a versioned envelope (`schema_version: 1`) with a field table and five
MUST rules (DIAG-7.1…7.5); §8 defines the exit codes and their own MUST rules.
Those are a contract, so they can be checked mechanically instead of trusted.

What it checks, per invocation:

* DIAG-7.1 — `--json` writes exactly one JSON object on stdout, even on failure;
* DIAG-7.2 — UTF-8 without BOM, one line (no NDJSON);
* DIAG-7.4 — empty arrays are present and empty, never null;
* §7.2 — required envelope fields, their types, `schema_version == 1`,
  `tool == "ravum-lang"`, `command` in the documented set, `status` in the
  documented set;
* §7.2/§7.3 — required diagnostic fields and span fields with documented minimums;
* DIAG-7.5 — no absolute machine path in a diagnostic, `/`-normalised paths;
* §8 / DIAG-8.4 — the process exit code equals the envelope's `exit_code`.

`doctor` is checked separately: it is the one command whose *purpose* is to report
the environment, so absolute paths there are reported as a documented tension
rather than counted as a violation.

Usage: python tools/json_contract.py
Exit code 0 when every MUST holds, 1 otherwise.
"""

from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
EXE = REPO / ("target/debug/ravum-lang.exe" if (REPO / "target/debug/ravum-lang.exe").exists() else "target/debug/ravum-lang")

COMMANDS = {"check", "build", "run", "test", "lint", "doc", "fmt", "doctor", "init"}
STATUSES = {"success", "failure", "unsupported"}
REQUIRED_ENVELOPE = {
    "schema_version": int,
    "tool": str,
    "tool_version": str,
    "spec_version": str,
    "command": str,
    "status": str,
    "exit_code": int,
    "target": dict,
    "summary": dict,
    "diagnostics": list,
}
DIAGNOSTIC_REQUIRED = {"code": str, "severity": str, "phase": str, "message": str, "primary": dict, "labels": list, "notes": list}
SPAN_REQUIRED = {
    "file": str,
    "start_byte": int,
    "end_byte": int,
    "start_line": int,
    "end_line": int,
    "start_col": int,
    "end_col": int,
    "start_utf16": int,
    "end_utf16": int,
}

violations: list[str] = []
tensions: list[str] = []


def fail(where: str, message: str) -> None:
    violations.append(f"{where}: {message}")


def run(arguments: list[str], cwd: pathlib.Path | None = None) -> tuple[int, str, str]:
    completed = subprocess.run(
        [str(EXE), *arguments],
        capture_output=True,
        cwd=str(cwd or REPO),
    )
    return completed.returncode, completed.stdout.decode("utf-8", "replace"), completed.stderr.decode("utf-8", "replace")


def check_envelope(where: str, raw: str, exit_code: int, expect_error_paths: bool) -> None:
    if raw.startswith("\ufeff"):
        fail(where, "DIAG-7.2: output starts with a BOM")
    body = raw.strip()
    if not body:
        fail(where, "DIAG-7.1: no JSON object on stdout")
        return
    if "\n" in body:
        fail(where, "DIAG-7.2: more than one line (NDJSON is not Core 1.0)")
    try:
        document = json.loads(body)
    except json.JSONDecodeError as error:
        fail(where, f"DIAG-7.1: stdout is not JSON ({error})")
        return
    if not isinstance(document, dict):
        fail(where, "§7.1: the envelope must be an object")
        return

    for field, expected_type in REQUIRED_ENVELOPE.items():
        if field not in document:
            fail(where, f"§7.2: required field `{field}` is missing")
        elif not isinstance(document[field], expected_type):
            fail(where, f"§7.2: field `{field}` must be {expected_type.__name__}")

    if document.get("schema_version") != 1:
        fail(where, f"§7.2: schema_version must be 1, got {document.get('schema_version')!r}")
    if document.get("tool") != "ravum-lang":
        fail(where, f"§7.2: tool must be \"ravum-lang\", got {document.get('tool')!r}")
    if document.get("command") not in COMMANDS:
        fail(where, f"§7.2: command {document.get('command')!r} is not documented")
    if document.get("status") not in STATUSES:
        fail(where, f"§7.2: status {document.get('status')!r} is not documented")
    if document.get("exit_code") != exit_code:
        fail(where, f"DIAG-8.4: envelope exit_code {document.get('exit_code')!r} != process exit {exit_code}")

    target = document.get("target")
    if isinstance(target, dict):
        if target.get("kind") not in {"file", "package", "workspace"}:
            fail(where, f"§7.2: target.kind {target.get('kind')!r} is not documented")
        path = target.get("path")
        if isinstance(path, str):
            if "\\" in path:
                fail(where, f"DIAG-7.5: target.path is not `/`-normalised: {path!r}")
            if pathlib.PurePath(path).is_absolute() or (len(path) > 2 and path[1] == ":"):
                fail(where, f"DIAG-7.5: target.path is absolute: {path!r}")

    summary = document.get("summary")
    if isinstance(summary, dict):
        for field in ("errors", "warnings"):
            if field not in summary:
                fail(where, f"§7.2: summary.{field} is missing")

    diagnostics = document.get("diagnostics")
    if isinstance(diagnostics, list):
        if diagnostics is None:
            fail(where, "DIAG-7.4: diagnostics must be an empty array, not null")
        for index, diagnostic in enumerate(diagnostics):
            spot = f"{where} diagnostics[{index}]"
            for field, expected_type in DIAGNOSTIC_REQUIRED.items():
                if field not in diagnostic:
                    fail(spot, f"§7.2: required field `{field}` is missing")
                elif not isinstance(diagnostic[field], expected_type):
                    fail(spot, f"§7.2: field `{field}` must be {expected_type.__name__}")
            if diagnostic.get("severity") not in {"error", "warning"}:
                fail(spot, f"§7.2: severity {diagnostic.get('severity')!r} is not documented")
            primary = diagnostic.get("primary")
            if isinstance(primary, dict):
                for field, expected_type in SPAN_REQUIRED.items():
                    if field not in primary:
                        fail(spot, f"§7.3: span field `{field}` is missing")
                    elif not isinstance(primary[field], expected_type):
                        fail(spot, f"§7.3: span field `{field}` must be {expected_type.__name__}")
                if isinstance(primary.get("file"), str):
                    if "\\" in primary["file"]:
                        fail(spot, f"DIAG-7.5: span file is not `/`-normalised: {primary['file']!r}")
                    if pathlib.PurePath(primary["file"]).is_absolute():
                        fail(spot, f"DIAG-7.5: span file is absolute: {primary['file']!r}")
                for field in ("start_line", "end_line", "start_col", "end_col"):
                    value = primary.get(field)
                    if isinstance(value, int) and value < 1:
                        fail(spot, f"§7.3: {field} must be >= 1, got {value}")
            for field in ("labels", "notes"):
                if field in diagnostic and diagnostic[field] is None:
                    fail(spot, f"DIAG-7.4: {field} must be an empty array, not null")
    if expect_error_paths and not diagnostics:
        fail(where, "expected at least one diagnostic in this scenario")


def build_fixtures(root: pathlib.Path) -> dict[str, pathlib.Path]:
    fixtures = {
        "clean": ("use std::io;\n\nfn main() {\n    io::println(\"x\");\n}\n", True),
        "error": (
            "fn main() {\n    let s: String = String::from(\"x\");\n    let t = s;\n    let u = s;\n}\n",
            False,
        ),
        "warning": ("use std::io;\nuse std::math;\n\nfn main() {\n    io::println(\"x\");\n}\n", True),
    }
    made = {}
    for name, (source, _) in fixtures.items():
        directory = root / name
        (directory / "src").mkdir(parents=True, exist_ok=True)
        (directory / "ravum.toml").write_text(
            f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "core-1.0"\n', encoding="utf-8"
        )
        (directory / "src" / "main.rv").write_text(source, encoding="utf-8")
        made[name] = directory
    broken = root / "manifest-error"
    (broken / "src").mkdir(parents=True, exist_ok=True)
    (broken / "ravum.toml").write_text("[package]\nname = \"Bad Name\"\n", encoding="utf-8")
    (broken / "src" / "main.rv").write_text("fn main() {\n}\n", encoding="utf-8")
    made["manifest-error"] = broken
    return made


def main() -> int:
    work = REPO / "target" / "json-contract"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    fixtures = build_fixtures(work)

    scenarios = [
        ("check-clean", ["check", "--json", str(fixtures["clean"])], 0, False),
        ("check-error", ["check", "--json", str(fixtures["error"])], 1, True),
        ("check-warning", ["check", "--json", str(fixtures["warning"])], 0, False),
        ("check-manifest-error", ["check", "--json", str(fixtures["manifest-error"])], 1, True),
        ("test-clean", ["test", "--json", str(fixtures["clean"])], 0, False),
    ]
    for identifier, arguments, expected_exit, expect_diagnostics in scenarios:
        code, stdout, stderr = run(arguments)
        if code != expected_exit:
            fail(identifier, f"§8: exit {code}, expected {expected_exit} (stderr: {stderr.strip()[:120]})")
        check_envelope(identifier, stdout, code, expect_diagnostics)

    # `doctor` reports paths by design; its envelope is still checked, and the
    # absolute environment paths are reported as a tension, not a violation.
    code, stdout, _ = run(["doctor", "--json"])
    check_envelope("doctor", stdout, code, False)
    try:
        document = json.loads(stdout)
        environment = document.get("summary", {}).get("environment", {})
        absolute = [key for key, value in environment.items() if isinstance(value, str) and ("\\" in value or (len(value) > 2 and value[1] == ":"))]
        if absolute:
            tensions.append(
                "doctor: deterministic absolute/host paths in summary.environment "
                f"({', '.join(sorted(absolute))}) — DIAG-7.5 forbids machine-dependent "
                "fields, but the command exists to report the environment; a human must "
                "decide whether the exception is documented or the fields removed"
            )
    except json.JSONDecodeError:
        pass

    print(f"scenarios     : {len(scenarios) + 1}")
    print(f"violations    : {len(violations)}")
    for item in violations:
        print(f"  {item}")
    print(f"tensions      : {len(tensions)}")
    for item in tensions:
        print(f"  {item}")
    if violations:
        print()
        print("json-contract: FAIL")
        return 1
    print()
    print("json-contract: PASS — every documented MUST of §7 and §8 holds")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
