#!/usr/bin/env python3
"""Generate one buildable Ravum program per *documented free function* of the stdlib.

The hand-written codegen sweep enumerates the documented *methods*; this tool
covers the free functions of `BUILTIN_FNS` so the surface is machine-counted
instead of sampled by hand.

Each generated program calls one free function with minimal literal arguments and
is only meant to be **built** (never run): `process::exit` and `panic` would end
the process.

Usage:
    python tools/stdlib_surface.py --out target/stdlib-surface
Writes one package per function plus `manifest.json`:

    {"schema_version": 1, "functions": [{"id": "io-println", "path": "std::io::println"}]}
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re

REPO = pathlib.Path(__file__).resolve().parent.parent
BUILTINS = REPO / "crates" / "ravum-lang-sema" / "src" / "builtins.rs"

# Minimal literal expression per parameter type. Generic `T` is instantiated with
# `i32` by passing an integer literal.
LITERALS = {
    "&str": '"x"',
    "str": '"x"',
    "i32": "1",
    "u32": "1",
    "i64": "1",
    "u64": "1",
    "i8": "1",
    "u8": "1",
    "i16": "1",
    "u16": "1",
    "isize": "1",
    "usize": "1",
    "f32": "1.0",
    "f64": "1.0",
    "bool": "true",
    "char": "'a'",
    "T": "1",
}

# Free functions whose value is deterministic for the generated arguments:
# the program prints the result and the sweep asserts it. Everything else is
# build-only (side effects, `!`, or a type with no `Display`).
EXPECTED_OUTPUT = {
    "std::fs::exists": "false",
    "std::string::from": "x",
    "std::math::abs": "1",
    "std::math::sqrt": "1",
    "std::math::floor": "1",
    "std::math::ceil": "1",
    "std::math::round": "1",
    "std::math::pow": "1",
    "std::math::min": "1",
    "std::math::max": "1",
    "std::io::println": "x",
    "std::io::print": "x",
    "String::from": "x",
    "String::new": "",
}

# Associated functions need an annotation (`Vec::new()` alone is ambiguous) and
# must not use the turbofish, which Core 1.0 refuses (E-PARSE-UNEXPECTED).
PRELUDE_STATEMENTS = {
    "String::from": 'let result = String::from("x");',
    "String::new": "let result = String::new();",
    "Vec::new": "let result: Vec<i32> = Vec::new();",
    "Vec::with_capacity": "let result: Vec<i32> = Vec::with_capacity(1);",
    "Error::new": 'let result = Error::new("x");',
    "drop": 'drop(String::from("x"));',
    "std::mem::drop": 'drop(String::from("x"));',
}

# Functions whose value can only be observed through a hand-written shape:
# `Result` has no `Display`, `Vec` needs `.len()`, and `()` needs a marker.
# Each entry is (full source, expected stdout substring).
CUSTOM_PROGRAMS = {
    "std::string::parse_i64": (
        'use std::io;\nuse std::string;\n\nfn main() {\n'
        '    let v = string::parse_i64("42").unwrap_or(-1);\n'
        '    io::println(&v.to_string());\n}\n',
        "42",
    ),
    "std::string::parse_f64": (
        'use std::io;\nuse std::string;\n\nfn main() {\n'
        '    let v = string::parse_f64("2.5").unwrap_or(-1.0);\n'
        '    io::println(&v.to_string());\n}\n',
        "2.5",
    ),
    "std::env::var": (
        'use std::io;\nuse std::env;\nuse std::string;\n\nfn main() {\n'
        '    let v = env::var("RAVUM_SURELY_UNSET_VARIABLE").unwrap_or(String::new());\n'
        '    let n = v.len();\n'
        '    io::println(&n.to_string());\n}\n',
        "0",
    ),
    "std::env::args": (
        'use std::io;\nuse std::env;\n\nfn main() {\n'
        '    let a = env::args();\n'
        '    let n = a.len();\n'
        '    if n > 0 {\n        io::println("args-ok");\n    }\n}\n',
        "args-ok",
    ),
    "std::fs::read_to_string": (
        'use std::io;\nuse std::fs;\nuse std::string;\n\nfn main() {\n'
        '    let v = fs::read_to_string("ravum-absent-file.txt").unwrap_or(String::new());\n'
        '    let n = v.len();\n'
        '    io::println(&n.to_string());\n}\n',
        "0",
    ),
    "std::fs::write_string": (
        'use std::io;\nuse std::fs;\n\nfn main() {\n'
        '    let r = fs::write_string("ravum-sweep-out.txt", "x");\n'
        '    if r.is_ok() {\n        io::println("write-ok");\n    } else {\n'
        '        io::println("write-err");\n    }\n}\n',
        "write-ok",
    ),
    "std::panic::assert": (
        'use std::io;\nuse std::panic;\n\nfn main() {\n'
        '    panic::assert(true);\n'
        '    io::println("assert-ok");\n}\n',
        "assert-ok",
    ),
    "std::panic::assert_eq": (
        'use std::io;\nuse std::panic;\n\nfn main() {\n'
        '    panic::assert_eq(1, 1);\n'
        '    io::println("assert-eq-ok");\n}\n',
        "assert-eq-ok",
    ),
    "drop": (
        'use std::io;\nuse std::string;\n\nfn main() {\n'
        '    drop(String::from("x"));\n'
        '    io::println("dropped");\n}\n',
        "dropped",
    ),
    "std::mem::drop": (
        'use std::io;\nuse std::string;\n\nfn main() {\n'
        '    drop(String::from("x"));\n'
        '    io::println("dropped");\n}\n',
        "dropped",
    ),
    "Vec::new": (
        'use std::io;\n\nfn main() {\n'
        "    let v: Vec<i32> = Vec::new();\n"
        '    let n = v.len();\n'
        '    io::println(&n.to_string());\n}\n',
        "0",
    ),
    "Vec::with_capacity": (
        'use std::io;\n\nfn main() {\n'
        "    let v: Vec<i32> = Vec::with_capacity(4);\n"
        '    let n = v.len();\n'
        '    io::println(&n.to_string());\n}\n',
        "0",
    ),
}

MODULE_OF = {
    "std::io": "io",
    "std::fs": "fs",
    "std::math": "math",
    "std::string": "string",
    "std::env": "env",
    "std::process": "process",
    "std::panic": "panic",
}


def parse_functions() -> list[dict]:
    text = BUILTINS.read_text(encoding="utf-8")
    start = text.index("pub static BUILTIN_FNS")
    block = text[start:]
    entries = []
    for match in re.finditer(
        r'path:\s*"([a-zA-Z0-9_:]+)",\s*generics:\s*&\[([^\]]*)\],\s*params:\s*&\[([^\]]*)\],\s*ret:\s*"([^"]*)"',
        block,
    ):
        path = match.group(1)
        params = re.findall(r'"([^"]+)"', match.group(3))
        entries.append({"path": path, "params": params, "ret": match.group(4)})
    return entries


def short_id(path: str) -> str:
    # Package names must match `[a-z][a-z0-9_-]*`; associated functions are
    # prefixed so `String::from` and `std::string::from` do not collide.
    identifier = path.replace("std::", "").replace("::", "-").lower()
    return identifier if path.startswith("std::") else f"assoc-{identifier}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="target/stdlib-surface")
    args = parser.parse_args()

    functions = parse_functions()
    if not functions:
        print("no free function parsed; the table shape changed")
        return 2

    import shutil

    out = (REPO / args.out).resolve()
    if out.exists():
        shutil.rmtree(out)

    manifest = []
    unsupported = []
    for entry in functions:
        path = entry["path"]
        literals = []
        for param in entry["params"]:
            literal = LITERALS.get(param)
            if literal is None:
                unsupported.append(f"{path}: unsupported parameter type {param!r}")
                literals = None
                break
            literals.append(literal)
        if literals is None:
            continue

        prelude = PRELUDE_STATEMENTS.get(path)
        module = MODULE_OF.get(path.rsplit("::", 1)[0])
        expected = EXPECTED_OUTPUT.get(path)
        custom = CUSTOM_PROGRAMS.get(path)
        if custom:
            source, expected = custom
        elif prelude:
            body = [f"    {prelude}"]
            if expected is not None and "let result" in prelude:
                # The assertion needs the value on stdout; `String` has no
                # `to_string` in Core 1.0, so print it directly.
                body.append("    io::println(&result);")
            source = "\n".join(["use std::io;", "", "fn main() {", *body, "}", ""])
        elif module is None:
            unsupported.append(f"{path}: unknown module")
            continue
        else:
            call = f"{path.split('::')[-2]}::{path.split('::')[-1]}({', '.join(literals)});"
            if entry["ret"] == "()":
                # The observable result is the side effect itself.
                body = [f"    {call}"]
            else:
                body = [f"    let result = {call}"]
                if expected is not None:
                    if entry["ret"] == "String":
                        # `String` has no `to_string` in Core 1.0.
                        body.append("    io::println(&result);")
                    else:
                        body.append("    io::println(&result.to_string());")
            source = "\n".join(
                [
                    *(["use std::io;"] if expected is not None and module != "io" else []),
                    f"use std::{module};",
                    "",
                    "fn main() {",
                    *body,
                    "}",
                    "",
                ]
            )

        # The value-asserting shapes are decided *before* this point (custom
        # programs, prelude statements, or the generic shape). An earlier version
        # rebuilt `source` here from a stale `call`, which silently wrote another
        # function's program into the wrong package.
        identifier = short_id(path)
        package = out / identifier
        (package / "src").mkdir(parents=True, exist_ok=True)
        (package / "ravum.toml").write_text(
            f'[package]\nname = "surface-{identifier}"\nversion = "0.1.0"\nedition = "core-1.0"\n',
            encoding="utf-8",
        )
        (package / "src" / "main.rv").write_text(source, encoding="utf-8")
        record = {"id": identifier, "path": path, "params": entry["params"]}
        if expected is not None:
            record["expect"] = expected
        manifest.append(record)

    ids = [entry["id"] for entry in manifest]
    duplicates = sorted({identifier for identifier in ids if ids.count(identifier) > 1})
    if duplicates:
        print(f"note: {len(duplicates)} duplicate table entr(y/ies) collapse onto one package: {', '.join(duplicates)}")

    (out / "manifest.json").write_text(
        json.dumps({"schema_version": 1, "functions": manifest}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"generated {len(manifest)} free-function program(s) in {out}")
    if unsupported:
        print(f"skipped {len(unsupported)}:")
        for line in unsupported:
            print(f"  {line}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
