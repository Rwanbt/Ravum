#!/usr/bin/env python3
"""Build the native library of `packages/sqlite` (decision D-19-7).

The Ravum toolchain never compiles C (modules.md MOD-5.10). This repository
script does, once per platform:

1. it finds SQLite's amalgamation in the `libsqlite3-sys` crate that
   `Cargo.lock` pins (fetched by `cargo fetch` if absent), and refuses it unless
   `sqlite3.c` and `sqlite3.h` have the SHA-256 digests pinned below;
2. it compiles them with `native-src/ravum_sqlite.c` using the system C
   compiler into `native/<os>-<arch>/`: `cc` (or `$CC`) and `ar` give
   `libravum_sqlite.a` on Linux and macOS; on Windows, MSVC `cl` and `lib`
   (found with `vswhere`, environment from `vcvars64.bat`, `/Brepro` for
   reproducible objects) give `ravum_sqlite.lib`;
3. it writes `packages/sqlite/ravum.toml` from `ravum.toml.in`, pinning the
   SHA-256 of that library for this platform (`sha256-<os>-<arch>`).

The library and the generated manifest are build outputs and are not
committed: the trust anchors are the pinned SQLite source and this script.
"""

from __future__ import annotations

import hashlib
import os
import pathlib
import platform
import re
import shutil
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = REPO / "packages" / "sqlite"
CRATE = "libsqlite3-sys"
PINNED = {
    "sqlite3.c": "0c657666d819f261cc4007526ad57e1669de9949df7a8ea847aa284a2bf0ee52",
    "sqlite3.h": "d6c84557261893439b341d2e6c35187ae3ea24fbfd843ad94616f72d89242b8c",
}
SQLITE_VERSION = "3.54.1"
# Compile-time options: thread-safe, no extension loading, no deprecated API.
DEFINES = [
    "-DSQLITE_THREADSAFE=1",
    "-DSQLITE_OMIT_LOAD_EXTENSION",
    "-DSQLITE_OMIT_DEPRECATED",
    "-DSQLITE_DQS=0",
]


def host_platform() -> str:
    """`<os>-<arch>` as Rust's `std::env::consts` spell them."""
    system = {"Linux": "linux", "Darwin": "macos", "Windows": "windows"}.get(
        platform.system(), platform.system().lower()
    )
    machine = platform.machine().lower()
    arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(
        machine, machine
    )
    return f"{system}-{arch}"


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def pinned_crate_version() -> str:
    lock = (REPO / "Cargo.lock").read_text(encoding="utf-8")
    match = re.search(r'name = "libsqlite3-sys"\nversion = "([^"]+)"', lock)
    if not match:
        sys.exit("error: Cargo.lock does not pin libsqlite3-sys")
    return match.group(1)


def find_amalgamation(version: str) -> pathlib.Path:
    cargo_home = pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    for attempt in range(2):
        for index in sorted((cargo_home / "registry" / "src").glob("*")):
            candidate = index / f"{CRATE}-{version}" / "sqlite3"
            if (candidate / "sqlite3.c").is_file():
                return candidate
        if attempt == 0:
            subprocess.run(["cargo", "fetch", "--locked"], cwd=REPO, check=True)
    sys.exit(f"error: {CRATE} {version} is not in the cargo registry")


def vcvars() -> pathlib.Path:
    """`vcvars64.bat` of the newest Visual Studio with the C++ tools."""
    root = pathlib.Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)"))
    vswhere = root / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        sys.exit(f"error: {vswhere} not found; install Visual Studio Build Tools (C++)")
    found = subprocess.run(
        [
            str(vswhere),
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if not found:
        sys.exit("error: no Visual Studio installation with the C++ x64 tools")
    script = pathlib.Path(found) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    if not script.is_file():
        sys.exit(f"error: {script} not found")
    return script


def build_msvc(source: pathlib.Path, out_dir: pathlib.Path) -> pathlib.Path:
    library = out_dir / "ravum_sqlite.lib"
    if library.exists():
        library.unlink()
    defines = [d.replace("-D", "/D", 1) for d in DEFINES]
    with tempfile.TemporaryDirectory() as tmp:
        objects = []
        commands = []
        for unit in [source / "sqlite3.c", PACKAGE / "native-src" / "ravum_sqlite.c"]:
            obj = pathlib.Path(tmp) / (unit.stem + ".obj")
            objects.append(f'"{obj}"')
            commands.append(
                f'cl /nologo /O2 /MD /Brepro {" ".join(defines)} /I "{source}" /c "{unit}" /Fo"{obj}"'
            )
        commands.append(f'lib /nologo /Brepro /OUT:"{library}" {" ".join(objects)}')
        batch = pathlib.Path(tmp) / "build.bat"
        batch.write_text(
            "@echo off\r\n"
            + f'call "{vcvars()}" >nul || exit /b 1\r\n'
            + "".join(f"{command} || exit /b 1\r\n" for command in commands),
            encoding="utf-8",
        )
        subprocess.run(["cmd", "/d", "/c", str(batch)], check=True)
    return library


def main() -> int:
    target = host_platform()
    source = find_amalgamation(pinned_crate_version())
    for name, digest in PINNED.items():
        actual = sha256(source / name)
        if actual != digest:
            sys.exit(f"error: {source / name} has digest {actual}, pinned {digest}")
    out_dir = PACKAGE / "native" / target
    out_dir.mkdir(parents=True, exist_ok=True)
    if platform.system() == "Windows":
        library = build_msvc(source, out_dir)
    else:
        library = build_unix(source, out_dir)
    digest = sha256(library)
    template = (PACKAGE / "ravum.toml.in").read_text(encoding="utf-8")
    manifest = template.replace("@PLATFORM@", target).replace("@SHA256@", digest)
    (PACKAGE / "ravum.toml").write_text(manifest, encoding="utf-8")
    print(f"sqlite {SQLITE_VERSION}: {library.relative_to(REPO)}")
    print(f"sha256-{target} = {digest}")
    return 0


def build_unix(source: pathlib.Path, out_dir: pathlib.Path) -> pathlib.Path:
    library = out_dir / "libravum_sqlite.a"
    cc = os.environ.get("CC", "cc")
    with tempfile.TemporaryDirectory() as tmp:
        objects = []
        for unit in [source / "sqlite3.c", PACKAGE / "native-src" / "ravum_sqlite.c"]:
            obj = pathlib.Path(tmp) / (unit.stem + ".o")
            subprocess.run(
                [cc, "-O2", "-fPIC", *DEFINES, "-I", str(source), "-c", str(unit), "-o", str(obj)],
                check=True,
            )
            objects.append(str(obj))
        if library.exists():
            library.unlink()
        ar = os.environ.get("AR", "ar")
        # `D`: deterministic archive (no timestamps, uids or modes).
        flags = "rcsD" if platform.system() != "Darwin" else "rcs"
        subprocess.run([ar, flags, str(library), *objects], check=True)
    return library


if __name__ == "__main__":
    raise SystemExit(main())
