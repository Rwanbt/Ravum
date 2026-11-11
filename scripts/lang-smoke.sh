#!/usr/bin/env bash
# Smoke test de la chaîne Ravum (port fidèle de scripts/lang-smoke.ps1).
#
# Exécute la vraie chaîne `ravum-lang` sur les projets `examples/` et vérifie
# stdout/exit codes. Aucun résultat simulé : un échec est un échec, une commande
# absente ou un binaire introuvable est NOT_RUN (exit 3).
#
#   bash scripts/lang-smoke.sh               # build (si nécessaire) + smoke
#   bash scripts/lang-smoke.sh --skip-build  # utilise le binaire existant
#   bash scripts/lang-smoke.sh --bin PATH    # binaire explicite
#
# Ce script est portable : Linux (CI ubuntu-latest) et Git Bash sous Windows.
# Il ne suppose aucun outil hors `bash`, `cargo` et le binaire `ravum-lang`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SKIP_BUILD=0
BIN="${RAVUM_LANG_BIN:-}"

usage() {
    cat <<'EOF'
Usage: lang-smoke.sh [--skip-build] [--bin PATH]

  --skip-build   Ne pas compiler ravum-lang ; exiger un binaire déjà présent.
  --bin PATH     Binaire ravum-lang à utiliser (défaut : target/debug/ravum-lang).
  -h, --help     Affiche cette aide.
EOF
}

while (($# > 0)); do
    case "$1" in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --bin)
            if (($# < 2)); then
                echo "error: --bin requires a path" >&2
                exit 2
            fi
            BIN="$2"
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

TEMP_DIRS=()
cleanup() {
    local directory
    for directory in ${TEMP_DIRS[@]+"${TEMP_DIRS[@]}"}; do
        rm -rf "$directory"
    done
}
trap cleanup EXIT

new_temp_dir() {
    # Sets NEW_TEMP instead of echoing: a command substitution would run in a
    # subshell and lose the TEMP_DIRS registration used by the EXIT trap.
    NEW_TEMP="$(mktemp -d "${TMPDIR:-/tmp}/ravum-smoke.XXXXXX")"
    TEMP_DIRS+=("$NEW_TEMP")
}

resolve_binary() {
    if [[ -n "$BIN" ]]; then
        return 0
    fi
    local candidate
    for candidate in "$ROOT/target/debug/ravum-lang" "$ROOT/target/debug/ravum-lang.exe"; do
        if [[ -x "$candidate" ]]; then
            BIN="$candidate"
            return 0
        fi
    done
    return 0
}

resolve_binary

if [[ ! -x "$BIN" && "$SKIP_BUILD" -eq 0 ]]; then
    echo "== build ravum-lang =="
    (cd "$ROOT" && cargo build -p ravum-lang-cli --bin ravum-lang)
    BIN=""
    resolve_binary
fi

if [[ ! -x "$BIN" ]]; then
    echo "NOT_RUN: binaire introuvable: ${BIN:-$ROOT/target/debug/ravum-lang (ou .exe)}"
    exit 3
fi

RESULT_CASE=()
RESULT_STATUS=()
RESULT_DETAIL=()
LAST_STDOUT=""

record() {
    local case_name="$1" status="$2" detail="$3"
    RESULT_CASE+=("$case_name")
    RESULT_STATUS+=("$status")
    RESULT_DETAIL+=("$detail")
    printf '[%s] %s %s\n' "$status" "$case_name" "$detail"
}

flatten() {
    printf '%s' "$1" | tr '\r\n' '  ' | tr -s ' ' | cut -c1-400
}

# invoke_case NAME DIR EXPECT_STDOUT EXPECT_EXIT CMD [ARGS...]
#
# Exécute le binaire dans DIR (répertoire temporaire si vide), capture stdout et
# stderr, vérifie le code de sortie puis le sous-texte attendu. Retourne 0 sur
# PASS et 1 sur FAIL (le résultat est toujours enregistré). LAST_STDOUT contient
# la sortie standard observée, même en cas d'échec.
invoke_case() {
    local case_name="$1" directory="$2" expect_stdout="$3" expect_exit="$4"
    shift 4

    local tmp
    new_temp_dir
    tmp="$NEW_TEMP"
    local work="$directory"
    if [[ -z "$work" ]]; then
        work="$tmp"
    fi
    local out_file="$tmp/stdout.txt"
    local err_file="$tmp/stderr.txt"
    local exit_code=0

    # `set -e` est désactivé le temps de la commande : un code non nul est une
    # donnée à vérifier, pas une raison d'abandonner le reste de la suite.
    set +e
    (cd "$work" && "$BIN" "$@" >"$out_file" 2>"$err_file")
    exit_code=$?
    set -e

    local stdout="" stderr=""
    if [[ -f "$out_file" ]]; then
        stdout="$(cat "$out_file")"
    fi
    if [[ -f "$err_file" ]]; then
        stderr="$(cat "$err_file")"
    fi
    LAST_STDOUT="$stdout"

    local failed=0
    if [[ "$exit_code" -ne "$expect_exit" ]]; then
        record "$case_name" "FAIL" "exit=$exit_code attendu=$expect_exit stderr=$(flatten "$stderr")"
        failed=1
    elif [[ -n "$expect_stdout" && "$stdout" != *"$expect_stdout"* ]]; then
        record "$case_name" "FAIL" "stdout ne contient pas '$expect_stdout' : $(flatten "$stdout")"
        failed=1
    else
        record "$case_name" "PASS" ""
    fi

    rm -rf "$tmp"
    return "$failed"
}

# --- commandes de base ---
if ! invoke_case "version" "" "ravum-lang" 0 --version; then
    echo "STOP: le binaire ne répond pas à --version"
    printf 'RESUME: 0 PASS / 1 FAIL / 0 NOT_RUN\n'
    exit 1
fi

invoke_case "doctor" "" "rustc" 0 doctor || true

# --- init + hello de zéro ---
new_temp_dir
init_dir="$NEW_TEMP"
if invoke_case "init" "" "" 0 init "$init_dir"; then
    if [[ -f "$init_dir/ravum.toml" ]]; then
        record "init-files" "PASS" ""
    else
        record "init-files" "FAIL" "ravum.toml absent"
    fi
else
    record "init-files" "FAIL" "init a échoué"
fi
invoke_case "init-check" "$init_dir" "" 0 check || true
invoke_case "init-build" "$init_dir" "" 0 build || true
invoke_case "init-run" "$init_dir" "hello" 0 run || true

# --- exemples ---
run_example() {
    local name="$1" expect="$2"
    shift 2
    local directory="$ROOT/examples/$name"
    invoke_case "check:$name" "$directory" "" 0 check || true
    invoke_case "build:$name" "$directory" "" 0 build || true
    local args=(run)
    if (($# > 0)); then
        args+=(--)
        args+=("$@")
    fi
    invoke_case "run:$name" "$directory" "$expect" 0 "${args[@]}" || true
}

run_example "hello" "hello, ravum"
run_example "fib" "55"
run_example "calculator" "8" 5 + 3
run_example "stats" "2.5" "$ROOT/examples/stats/sample.txt"
run_example "dsp-gain-iir" "0.1264"
run_example "package-demo/app" "42"
run_example "ffi-abs" "42"
invoke_case "test:tests-demo" "$ROOT/examples/tests-demo" "passed" 0 test || true
invoke_case "fmt-check:hello" "$ROOT/examples/hello" "" 0 fmt --check || true

# --- échecs attendus (diagnostics) ---
new_temp_dir
bad_dir="$NEW_TEMP"
printf '%s\n' 'fn main() { let x: i32 = "text"; }' >"$bad_dir/bad.rv"
invoke_case "check-fail-json" "$bad_dir" "E-TYP-MISMATCH" 1 check --json || true

# --- résumé ---
pass=0
fail=0
not_run=0
for status in ${RESULT_STATUS[@]+"${RESULT_STATUS[@]}"}; do
    case "$status" in
        PASS) pass=$((pass + 1)) ;;
        FAIL) fail=$((fail + 1)) ;;
        NOT_RUN) not_run=$((not_run + 1)) ;;
    esac
done
printf '\nRESUME: %d PASS / %d FAIL / %d NOT_RUN\n' "$pass" "$fail" "$not_run"
if ((fail > 0)); then
    exit 1
fi
exit 0
