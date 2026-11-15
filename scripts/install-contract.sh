#!/usr/bin/env bash
# Contrat public de l'installateur POSIX `ravum-lang` (Linux).
#
# Exerce le contrat décrit dans docs/release/INSTALL-LINUX.md sur l'installateur
# réel scripts/install.sh (source de vérité) :
#   1. installation sous un préfixe temporaire (`--prefix DIR`) ;
#   2. binaire exécutable + `--version` réellement exécuté ;
#   3. invariants du manifeste `ravum-install.json` (analysé par python3, pas jq) ;
#   4. SHA-256 du manifeste == binaire installé == binaire construit ;
#   5. désinstallation, seconde désinstallation idempotente, réinstallation ;
#   6. erreurs d'usage de install.sh (codes 2 et 1 attendus, avec la raison).
#
#   bash scripts/install-contract.sh               # build release + contrat
#   bash scripts/install-contract.sh --skip-build  # binaire déjà dans target/release
#
# Réseau : uniquement ce que `cargo build --release --locked` peut demander (et
# rien du tout avec --skip-build). Aucun jq, aucune dépendance hors bash, python3,
# cargo et le binaire.
#
# Codes de sortie de CE script :
#   0  contrat tenu ;
#   1  contrat violé (invariant faux, code de sortie inattendu, désinstallation KO) ;
#   2  usage invalide de ce script ;
#   3  NOT_RUN : prérequis absent (hôte non Linux, python3 absent, installateur
#      absent, ou binaire absent avec --skip-build). Un NOT_RUN n'est jamais un PASS.
set -euo pipefail

EXIT_OK=0
EXIT_FAIL=1
EXIT_USAGE=2
EXIT_NOT_RUN=3

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
BUILT="$ROOT/target/release/ravum-lang"
SKIP_BUILD=0
PASS_COUNT=0

usage() {
    cat <<'EOF'
Usage: scripts/install-contract.sh [options]

  --skip-build   Ne pas lancer cargo build ; exiger target/release/ravum-lang.
  -h, --help     Affiche cette aide.

Le contrat s'exécute sur Linux (le manifeste et le binaire sont ceux de l'hôte).
Hors Linux, le script sort en 3 (NOT_RUN) : aucun PASS n'est simulé.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        -h|--help)
            usage
            exit "$EXIT_OK"
            ;;
        *)
            echo "install-contract.sh: error: argument inconnu: $1" >&2
            usage >&2
            exit "$EXIT_USAGE"
            ;;
    esac
done

not_run() {
    echo "install-contract: NOT_RUN: $*" >&2
    exit "$EXIT_NOT_RUN"
}

fail() {
    echo "install-contract: FAIL: $*" >&2
    exit "$EXIT_FAIL"
}

pass() {
    PASS_COUNT=$((PASS_COUNT + 1))
    printf 'PASS  %s (%s)\n' "$1" "$2"
}

flatten() {
    printf '%s' "$1" | tr '\r\n' '  ' | tr -s ' ' | cut -c1-400
}

LAST_OUT=""
LAST_CODE=0

run_capture() {
    # Échoue volontairement sans `set -e` : le code de sortie est l'objet du test.
    set +e
    LAST_OUT="$("$@" 2>&1)"
    LAST_CODE=$?
    set -e
}

expect_exit() {
    # expect_exit LABEL EXPECTED_CODE NEEDLE
    # NEEDLE vide : seul le code est vérifié. NEEDLE non vide : la sortie doit
    # contenir la raison attendue (un code juste pour une mauvaise raison échoue).
    local label="$1"
    local expected="$2"
    local needle="$3"
    if [ "$LAST_CODE" -ne "$expected" ]; then
        fail "$label: code=$LAST_CODE attendu=$expected sortie=$(flatten "$LAST_OUT")"
    fi
    if [ -n "$needle" ] && ! printf '%s' "$LAST_OUT" | grep -Fq -- "$needle"; then
        fail "$label: sortie ne contient pas '$needle' : $(flatten "$LAST_OUT")"
    fi
    pass "$label" "code=$expected"
}

# ------------------------------------------------------------- prérequis
if [ "$(uname -s 2>/dev/null || echo unknown)" != "Linux" ]; then
    not_run "hôte non Linux ($(uname -s 2>/dev/null || echo unknown)) : le contrat POSIX n'est qualifié que sur Linux (voir docs/release/INSTALL-LINUX.md)."
fi
command -v python3 >/dev/null 2>&1 || not_run "python3 est requis pour analyser ravum-install.json (aucun jq n'est utilisé)."
[ -f "$INSTALLER" ] || not_run "scripts/install.sh est absent de ce commit : rien à exercer."
if [ "$SKIP_BUILD" -eq 1 ] && [ ! -f "$BUILT" ]; then
    not_run "binaire release absent ($BUILT) : lancer sans --skip-build, ou construire d'abord 'cargo build -p ravum-lang-cli --release --locked'."
fi

TMP="$(mktemp -d "${TMPDIR:-/tmp}/ravum-install-contract.XXXXXX")"
# shellcheck disable=SC2329  # referenced by the EXIT trap below
cleanup() {
    rm -rf "$TMP"
}
trap cleanup EXIT

PREFIX="$TMP/prefix"
FIXTURE="$TMP/fixture-repo"
FIXTURE_PREFIX="$TMP/fixture-prefix"

echo "== install-contract: préfixe temporaire $PREFIX =="

# --------------------------------------------------- erreurs d'usage (avant)
# Aide : ne doit rien exiger, ne doit rien écrire, code 0.
run_capture bash "$INSTALLER" --help
expect_exit "install.sh --help" 0 "Usage: scripts/install.sh"

run_capture bash "$INSTALLER" --argument-inconnu
expect_exit "argument inconnu refusé (2)" 2 "argument inconnu"

run_capture bash "$INSTALLER" --prefix
expect_exit "--prefix sans valeur refusé (2)" 2 "--prefix exige une valeur"

# Le préfixe ne peut pas être la racine du dépôt : écrire là écraserait le
# binaire construit et le manifeste dans l'arbre de travail.
run_capture bash "$INSTALLER" --prefix "$ROOT"
expect_exit "préfixe = racine du dépôt refusé (2)" 2 "racine du depot"

run_capture bash "$INSTALLER" --prefix "$PREFIX" --remove-from-path
expect_exit "--remove-from-path sans --uninstall refusé (2)" 2 "--remove-from-path exige --uninstall"

# --skip-build sans binaire doit sortir en 1 (échec, pas usage). Le test ne
# supprime pas target/release : il copie l'installateur dans une racine
# minimale (Cargo.toml vide, aucun target/) et exerce le même code.
mkdir -p "$FIXTURE/scripts"
cp "$INSTALLER" "$FIXTURE/scripts/install.sh"
: > "$FIXTURE/Cargo.toml"
run_capture bash "$FIXTURE/scripts/install.sh" --prefix "$FIXTURE_PREFIX" --skip-build
expect_exit "--skip-build sans binaire échoue (1)" 1 "binaire construit introuvable"

# ------------------------------------------------------------- installation
if [ "$SKIP_BUILD" -eq 1 ]; then
    run_capture bash "$INSTALLER" --prefix "$PREFIX" --skip-build
else
    run_capture bash "$INSTALLER" --prefix "$PREFIX"
fi
if [ "$LAST_CODE" -ne 0 ]; then
    fail "installation: code=$LAST_CODE sortie=$(flatten "$LAST_OUT")"
fi
pass "installation sous préfixe temporaire" "code=0"

if [ -f "$PREFIX/bin/ravum-lang" ]; then
    pass "binaire installé présent" "$PREFIX/bin/ravum-lang"
else
    fail "binaire installé absent : $PREFIX/bin/ravum-lang"
fi
if [ -x "$PREFIX/bin/ravum-lang" ]; then
    pass "binaire installé exécutable" "chmod 755"
else
    fail "binaire installé non exécutable : $PREFIX/bin/ravum-lang"
fi
if [ -f "$PREFIX/ravum-install.json" ]; then
    pass "manifeste présent" "$PREFIX/ravum-install.json"
else
    fail "manifeste absent : $PREFIX/ravum-install.json"
fi

# Aucun staging ni sauvegarde ne doit survivre à une installation réussie.
LEFTOVERS="$(find "$PREFIX" -maxdepth 1 \( -name '.staging.*' -o -name 'ravum-lang.bak' -o -name 'ravum-install.json.bak' \) -print)"
if [ -z "$LEFTOVERS" ]; then
    pass "aucun résidu de staging ni de sauvegarde" "prefixe=propre"
else
    fail "résidus après installation : $(flatten "$LEFTOVERS")"
fi

# shellcheck disable=SC2012  # entrées contrôlées par l'installateur ; tout nom inattendu fait échouer le test
TOP_LEVEL="$(cd "$PREFIX" && ls -A | LC_ALL=C sort | tr '\n' ' ')"
if [ "$TOP_LEVEL" = "bin ravum-install.json " ]; then
    pass "préfixe limité à bin/ et au manifeste" "top-level=$TOP_LEVEL"
else
    fail "contenu inattendu à la racine du préfixe : '$TOP_LEVEL'"
fi

run_capture "$PREFIX/bin/ravum-lang" --version
expect_exit "binaire installé répond à --version" 0 "ravum-lang"
VERSION1="$LAST_OUT"

# Valeurs observées, transmises au vérificateur : le manifeste doit décrire
# l'installation réelle, pas seulement avoir la bonne forme.
COMMIT="UNKNOWN"
DIRTY="UNKNOWN"
if command -v git >/dev/null 2>&1 && git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
    if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then
        DIRTY=true
    else
        DIRTY=false
    fi
fi
PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
RUSTC_LINE="$(rustc -V 2>/dev/null || echo UNKNOWN)"
PINNED_CHANNEL=""
if [ -f "$ROOT/rust-toolchain.toml" ]; then
    PINNED_CHANNEL="$(awk -F'"' '/^channel/ { print $2; exit }' "$ROOT/rust-toolchain.toml")"
fi

set +e
PY_OUT="$(python3 - \
    "$PREFIX/ravum-install.json" \
    "$PREFIX/bin/ravum-lang" \
    "$BUILT" \
    "$PREFIX" \
    "$COMMIT" \
    "$DIRTY" \
    "$PLATFORM" \
    "$ARCH" \
    "$RUSTC_LINE" \
    "$PINNED_CHANNEL" <<'PY'
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys

(
    manifest_path,
    installed_bin,
    built_bin,
    prefix,
    expected_commit,
    expected_dirty,
    observed_platform,
    observed_arch,
    observed_rustc,
    pinned_channel,
) = sys.argv[1:11]

checks = 0
failures = []


def check(name, ok, detail=""):
    global checks
    checks += 1
    if ok:
        print("PASS  {} ({})".format(name, detail))
    else:
        failures.append(name)
        print("ERROR {} ({})".format(name, detail), file=sys.stderr)


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


EXPECTED_KEYS = {
    "schema_version",
    "name",
    "version",
    "package_version",
    "git_commit",
    "git_dirty",
    "installed_at_utc",
    "sha256",
    "rustc",
    "platform",
    "arch",
    "profile",
    "locked",
    "binary",
    "prefix",
    "installer",
}

try:
    with open(manifest_path, "rb") as handle:
        raw = handle.read()
except OSError as exc:
    print("ERROR manifest unreadable: {}".format(exc), file=sys.stderr)
    sys.exit(1)

check(
    "manifest is UTF-8 without BOM",
    not raw.startswith(b"\xef\xbb\xbf"),
    "{} bytes".format(len(raw)),
)

try:
    doc = json.loads(raw.decode("utf-8"))
except (UnicodeDecodeError, json.JSONDecodeError) as exc:
    print("ERROR manifest is not valid UTF-8 JSON: {}".format(exc), file=sys.stderr)
    sys.exit(1)

check("manifest is a JSON object", isinstance(doc, dict), type(doc).__name__)
if not isinstance(doc, dict):
    sys.exit(1)
check(
    "manifest fields are exactly the documented set",
    set(doc) == EXPECTED_KEYS,
    "missing={} unexpected={}".format(
        sorted(EXPECTED_KEYS - set(doc)), sorted(set(doc) - EXPECTED_KEYS)
    ),
)

version = doc.get("version")
check(
    "schema_version is the integer 1",
    doc.get("schema_version") == 1 and type(doc.get("schema_version")) is int,
    repr(doc.get("schema_version")),
)
check("name is ravum-lang", doc.get("name") == "ravum-lang", repr(doc.get("name")))
check(
    "version is a --version line",
    isinstance(version, str) and re.match(r"^ravum-lang \S+", version) is not None,
    repr(version),
)

try:
    proc = subprocess.run(
        [installed_bin, "--version"], capture_output=True, text=True
    )
except OSError as exc:
    print("ERROR cannot execute installed binary: {}".format(exc), file=sys.stderr)
    sys.exit(1)
stdout = proc.stdout.strip()
check(
    "installed binary --version exits 0",
    proc.returncode == 0,
    "exit={} stderr={}".format(proc.returncode, proc.stderr.strip()[:200]),
)
check(
    "manifest.version equals the installed --version output",
    version == stdout,
    "manifest={!r} installed={!r}".format(version, stdout),
)

parts = version.split() if isinstance(version, str) else []
package_version = doc.get("package_version")
check(
    "package_version is the version token",
    isinstance(package_version, str)
    and len(parts) >= 2
    and package_version == parts[1]
    and re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.\-]+)?", package_version)
    is not None,
    "{!r} vs {!r}".format(package_version, version),
)

commit = doc.get("git_commit")
if expected_commit != "UNKNOWN":
    check(
        "git_commit is a 40-hex commit",
        isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40}", commit) is not None,
        repr(commit),
    )
    check(
        "git_commit equals git rev-parse HEAD",
        commit == expected_commit,
        "manifest={} HEAD={}".format(commit, expected_commit),
    )
else:
    # Sans git, install.sh enregistre UNKNOWN : l'égalité avec HEAD est NOT_RUN,
    # mais le manifeste ne doit pas inventer un commit.
    check(
        "git_commit is UNKNOWN when git is unavailable",
        commit == "UNKNOWN",
        repr(commit),
    )
    print("NOTICE: git absent; git_commit == HEAD is NOT_RUN (not counted).")

dirty = doc.get("git_dirty")
check("git_dirty is a JSON boolean", type(dirty) is bool, repr(dirty))
if expected_dirty in ("true", "false"):
    check(
        "git_dirty matches the working tree state",
        dirty is (expected_dirty == "true"),
        "manifest={} observed={}".format(dirty, expected_dirty),
    )
else:
    print("NOTICE: git absent; git_dirty is NOT_RUN (not counted).")

installed_at = doc.get("installed_at_utc")
time_ok = False
if isinstance(installed_at, str) and re.fullmatch(
    r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", installed_at
):
    try:
        datetime.datetime.strptime(installed_at, "%Y-%m-%dT%H:%M:%SZ")
        time_ok = True
    except ValueError:
        time_ok = False
check("installed_at_utc is a UTC ISO-8601 timestamp", time_ok, repr(installed_at))

recorded_sha = doc.get("sha256")
check(
    "sha256 is a lowercase 64-hex digest",
    isinstance(recorded_sha, str)
    and re.fullmatch(r"[0-9a-f]{64}", recorded_sha) is not None,
    repr(recorded_sha),
)
try:
    installed_sha = sha256_file(installed_bin)
except OSError as exc:
    print("ERROR installed binary unreadable: {}".format(exc), file=sys.stderr)
    sys.exit(1)
check(
    "sha256 equals the installed binary digest",
    recorded_sha == installed_sha,
    "manifest={} installed={}".format(recorded_sha, installed_sha),
)
if os.path.isfile(built_bin):
    built_sha = sha256_file(built_bin)
    check(
        "installed binary is byte-identical to the release build",
        installed_sha == built_sha,
        "installed={} built={}".format(installed_sha, built_sha),
    )
else:
    print("NOTICE: release build absent; installed == built is NOT_RUN (not counted).")

rustc_line = doc.get("rustc")
check(
    "rustc is recorded",
    isinstance(rustc_line, str) and bool(rustc_line),
    repr(rustc_line),
)
if observed_rustc != "UNKNOWN":
    check(
        "rustc equals the observed toolchain",
        rustc_line == observed_rustc,
        "manifest={} observed={}".format(rustc_line, observed_rustc),
    )
    if pinned_channel:
        check(
            "observed toolchain matches the pinned channel",
            observed_rustc.startswith("rustc " + pinned_channel),
            "observed={} channel={}".format(observed_rustc, pinned_channel),
        )
    else:
        print("NOTICE: no pinned channel; pinned-toolchain is NOT_RUN (not counted).")
else:
    print("NOTICE: rustc absent; the rustc invariant is NOT_RUN (not counted).")

check("platform is linux", doc.get("platform") == "linux", repr(doc.get("platform")))
check(
    "platform equals the observed OS",
    doc.get("platform") == observed_platform,
    "manifest={} observed={}".format(doc.get("platform"), observed_platform),
)
check(
    "arch equals the observed architecture",
    isinstance(doc.get("arch"), str)
    and bool(doc.get("arch"))
    and doc.get("arch") == observed_arch,
    "manifest={} observed={}".format(doc.get("arch"), observed_arch),
)
check("profile is release", doc.get("profile") == "release", repr(doc.get("profile")))
check("locked is the JSON boolean true", doc.get("locked") is True, repr(doc.get("locked")))

binary_field = doc.get("binary")
check("binary is bin/ravum-lang", binary_field == "bin/ravum-lang", repr(binary_field))
check(
    "installed binary is a regular executable file",
    os.path.isfile(installed_bin) and os.access(installed_bin, os.X_OK),
    installed_bin,
)
declared_binary_path = (
    os.path.normpath(os.path.join(prefix, binary_field))
    if isinstance(binary_field, str)
    else ""
)
check(
    "binary field resolves to the installed path",
    declared_binary_path == os.path.normpath(installed_bin),
    "declared={} installed={}".format(declared_binary_path, installed_bin),
)
check(
    "prefix equals the installed prefix",
    doc.get("prefix") == prefix,
    "manifest={} expected={}".format(doc.get("prefix"), prefix),
)
check(
    "installer is scripts/install.sh",
    doc.get("installer") == "scripts/install.sh",
    repr(doc.get("installer")),
)

if failures:
    print(
        "FAILED: {} invariant(s) violated: {}".format(len(failures), ", ".join(failures)),
        file=sys.stderr,
    )
    sys.exit(1)
print("OK: {} manifest invariants hold.".format(checks))
PY
)"
PY_CODE=$?
set -e
if [ "$PY_CODE" -ne 0 ]; then
    printf '%s\n' "$PY_OUT" >&2
    fail "invariants du manifeste non tenus (python3 code=$PY_CODE)"
fi
printf '%s\n' "$PY_OUT"

# Le fichier témoin vérifie que la désinstallation ne touche que ses fichiers.
printf 'témoin : la désinstallation ne doit pas supprimer ce fichier\n' > "$PREFIX/keep-me.txt"

# ------------------------------------------------------------ désinstallation
run_capture bash "$INSTALLER" --prefix "$PREFIX" --uninstall
expect_exit "désinstallation" 0 ""

if [ ! -e "$PREFIX/bin/ravum-lang" ]; then
    pass "désinstallation a retiré le binaire" "$PREFIX/bin/ravum-lang"
else
    fail "binaire encore présent après désinstallation"
fi
if [ ! -e "$PREFIX/ravum-install.json" ]; then
    pass "désinstallation a retiré le manifeste" "$PREFIX/ravum-install.json"
else
    fail "manifeste encore présent après désinstallation"
fi
if [ ! -d "$PREFIX/bin" ]; then
    pass "répertoire bin/ vide retiré" "$PREFIX/bin"
else
    fail "répertoire bin/ laissé vide après désinstallation"
fi
if [ -f "$PREFIX/keep-me.txt" ]; then
    pass "fichier étranger du préfixe préservé" "keep-me.txt"
else
    fail "la désinstallation a supprimé un fichier étranger du préfixe"
fi

run_capture bash "$INSTALLER" --prefix "$PREFIX" --uninstall
expect_exit "seconde désinstallation idempotente" 0 "rien a desinstaller"
if [ ! -e "$PREFIX/bin/ravum-lang" ] && [ ! -e "$PREFIX/ravum-install.json" ]; then
    pass "seconde désinstallation ne recrée rien" "préfixe inchangé"
else
    fail "seconde désinstallation a laissé des fichiers"
fi

# Réinstallation sans rebuild : même binaire, même version.
run_capture bash "$INSTALLER" --prefix "$PREFIX" --skip-build
expect_exit "réinstallation sans rebuild" 0 ""
if [ -f "$PREFIX/bin/ravum-lang" ]; then
    pass "réinstallation a restauré le binaire" "$PREFIX/bin/ravum-lang"
else
    fail "réinstallation : binaire absent"
fi
run_capture "$PREFIX/bin/ravum-lang" --version
expect_exit "binaire réinstallé répond à --version" 0 "ravum-lang"
if [ "$LAST_OUT" = "$VERSION1" ]; then
    pass "version identique après réinstallation" "$(flatten "$LAST_OUT")"
else
    fail "version différente après réinstallation : $(flatten "$LAST_OUT") != $(flatten "$VERSION1")"
fi

run_capture bash "$INSTALLER" --prefix "$PREFIX" --uninstall
expect_exit "désinstallation finale" 0 ""

echo ""
echo "OK: contrat de l'installateur Linux tenu ($PASS_COUNT vérifications shell + invariants du manifeste ci-dessus)."
echo "NOT_RUN: signature, provenance hors manifeste, auto-update, musl/ARM/macOS (voir docs/release/INSTALL-LINUX.md)."
exit "$EXIT_OK"
