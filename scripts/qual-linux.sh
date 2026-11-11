#!/usr/bin/env bash
# Qualification Linux locale — rejoue les gates du workflow
# `.github/workflows/linux-qual.yml` sur une machine Linux réelle.
#
# Ce script est un driver de reproduction locale, pas une preuve CI : la preuve
# officielle reste le run GitHub Actions ubuntu-latest (machine indépendante).
#
#   bash scripts/qual-linux.sh              # fmt + tests workspace + build + conformance + smoke
#   bash scripts/qual-linux.sh --quick      # fmt + build + smoke (sans tests workspace ni conformance)
#   bash scripts/qual-linux.sh --miri       # ajoute Miri (nightly + composant miri)
#   bash scripts/qual-linux.sh --wasm       # ajoute build + run wasm32-wasip1 (Node WASI)
#
# Codes de sortie : 0 = tout PASS ; 1 = au moins un FAIL ; 3 = au moins un
# NOT_RUN (outil indisponible, hôte non Linux) sans FAIL.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
QUICK=0
RUN_MIRI=0
RUN_WASM=0

usage() {
    cat <<'EOF'
Usage: qual-linux.sh [--quick] [--miri] [--wasm]

  --quick   Saute les tests workspace et le corpus de conformance.
  --miri    Exécute aussi `cargo +nightly miri test` (syntax + sema).
  --wasm    Exécute aussi la tentative wasm32-wasip1 (NOT_RUN si le CLI n'a
            pas de support `--target`).
  -h, --help
EOF
}

while (($# > 0)); do
    case "$1" in
        --quick)
            QUICK=1
            shift
            ;;
        --miri)
            RUN_MIRI=1
            shift
            ;;
        --wasm)
            RUN_WASM=1
            shift
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

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "NOT_RUN: qual-linux.sh qualifie Linux ; hôte détecté: $(uname -s)"
    echo "  (sous Windows, utilisez Git Bash uniquement pour 'bash -n' et le smoke"
    echo "   portable ; la qualification réelle est le run GitHub Actions ubuntu-latest.)"
    exit 3
fi

PASS=0
FAIL=0
NOT_RUN=0

record() {
    local status="$1" name="$2" detail="${3:-}"
    printf '[%s] %s %s\n' "$status" "$name" "$detail"
    case "$status" in
        PASS) PASS=$((PASS + 1)) ;;
        FAIL) FAIL=$((FAIL + 1)) ;;
        NOT_RUN) NOT_RUN=$((NOT_RUN + 1)) ;;
    esac
}

# run_step NAME CMD [ARGS...] : exécute une commande et enregistre son statut.
run_step() {
    local name="$1"
    shift
    printf '\n== %s ==\n' "$name"
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    set +e
    "$@"
    local code=$?
    set -e
    if ((code == 0)); then
        record PASS "$name"
    else
        record FAIL "$name" "exit=$code"
    fi
}

# require_tool NAME CMD... : enregistre NOT_RUN si l'outil n'est pas disponible.
require_tool() {
    local name="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        return 0
    fi
    record NOT_RUN "$name" "outil indisponible: $*"
    return 1
}

printf 'HOST: %s\n' "$(uname -srm)"
printf 'ROOT: %s\n' "$ROOT"

if ! require_tool "cargo" cargo --version; then
    printf '\nRESUME: %d PASS / %d FAIL / %d NOT_RUN\n' "$PASS" "$FAIL" "$NOT_RUN"
    exit 3
fi
printf 'rustc: %s\n' "$(rustc -vV | head -n 1)"
printf 'cargo: %s\n' "$(cargo --version)"

run_step "cargo fmt --all -- --check" cargo fmt --all -- --check

if ((QUICK == 0)); then
    run_step "cargo test --workspace --all-targets (--test-threads=2)" \
        cargo test --workspace --all-targets -- --test-threads=2
fi

run_step "build ravum-lang" cargo build -p ravum-lang-cli --bin ravum-lang

if ((QUICK == 0)); then
    printf '\n== conformance corpus ==\n'
    set +e
    cargo run -p ravum-conformance -- run --json target/conformance-report.json
    conformance_code=$?
    set -e
    case "$conformance_code" in
        0) record PASS "conformance corpus" ;;
        3) record NOT_RUN "conformance corpus" "exit=3 (NOT_RUN environnement, pas un PASS)" ;;
        *) record FAIL "conformance corpus" "exit=$conformance_code" ;;
    esac
fi

run_step "example smoke (scripts/lang-smoke.sh)" bash "$ROOT/scripts/lang-smoke.sh" --skip-build

if ((RUN_MIRI == 1)); then
    if require_tool "miri (nightly)" cargo +nightly miri --version; then
        set +e
        MIRIFLAGS="-Zmiri-disable-isolation" \
            cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests
        miri_code=$?
        set -e
        if ((miri_code == 0)); then
            record PASS "miri (syntax + sema)"
        else
            record FAIL "miri (syntax + sema)" "exit=$miri_code"
        fi
    fi
fi

if ((RUN_WASM == 1)); then
    if require_tool "rustup" rustup --version; then
        set +e
        rustup target add wasm32-wasip1
        add_code=$?
        set -e
        if ((add_code != 0)); then
            record NOT_RUN "wasm32-wasip1 target" "rustup target add a échoué (exit=$add_code)"
        elif "$ROOT/target/debug/ravum-lang" help build 2>&1 | grep -q -- '--target'; then
            run_step "wasm32-wasip1 build hello" \
                "$ROOT/target/debug/ravum-lang" build --target wasm32-wasip1 "$ROOT/examples/hello"
            if [[ ! -f "$ROOT/examples/hello/.ravum/bin/hello.wasm" ]]; then
                record FAIL "wasm32-wasip1 artifact" "examples/hello/.ravum/bin/hello.wasm absent"
            elif ! command -v node >/dev/null 2>&1; then
                record NOT_RUN "wasm32-wasip1 run hello" "Node.js absent (WASI runtime)"
            elif "$ROOT/target/debug/ravum-lang" help run 2>&1 | grep -q -- '--target'; then
                set +e
                wasm_out="$("$ROOT/target/debug/ravum-lang" run --target wasm32-wasip1 "$ROOT/examples/hello")"
                wasm_code=$?
                set -e
                printf '%s\n' "$wasm_out"
                if ((wasm_code != 0)); then
                    record FAIL "wasm32-wasip1 run hello" "exit=$wasm_code"
                elif [[ "$wasm_out" != *"hello, ravum"* ]]; then
                    record FAIL "wasm32-wasip1 run hello" "stdout inattendu"
                else
                    record PASS "wasm32-wasip1 run hello"
                fi
            else
                record NOT_RUN "wasm32-wasip1 run hello" "run --target non supporté par le CLI"
            fi
        else
            record NOT_RUN "wasm32-wasip1 build hello" \
                "NOT_APPLICABLE: le CLI Core 1.0 n'expose pas --target (help build)"
        fi
    fi
fi

printf '\nRESUME: %d PASS / %d FAIL / %d NOT_RUN\n' "$PASS" "$FAIL" "$NOT_RUN"
if ((FAIL > 0)); then
    exit 1
fi
if ((NOT_RUN > 0)); then
    exit 3
fi
exit 0
