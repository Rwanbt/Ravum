#!/usr/bin/env bash
# repro-build.sh — one deterministic build of `ravum-lang`, digest included.
#
# Why this file exists
# -------------------
# `scripts/repro-check.ps1` proves *path independence on one machine*: two
# builds from two absolute roots into two `target/` directories. It does not
# prove anything across machines: the reference digest is only compared with
# itself.
#
# The reproducibility gate also needs the opposite measurement: **the same tree
# built on two distinct runners must produce the same bytes**. This script
# performs exactly one build with the flags that make the output independent of
# the machine paths, and writes the digest as JSON. A workflow then compares the
# digests produced by two separate `ubuntu-latest` runners.
#
# Flags and why each one is required (measured on Windows in
# `docs/qualification/REPRODUCIBILITY.md`; the Linux side is what this script
# verifies):
#   * CARGO_INCREMENTAL=0  — incremental objects embed run-specific state;
#   * SOURCE_DATE_EPOCH=0  — pins any honoured build timestamp;
#   * --remap-path-prefix  — without it, absolute source/target paths leak into
#                            the binary (panic locations, debug metadata);
#   * --build-id=sha1      — the GNU linker's build-id must be content-derived,
#                            never random.
#
# What this script does NOT do: it does not compare anything. Comparison is the
# caller's job (`reproducibility.yml` compares two runners; `repro-check.ps1`
# compares two roots). It does not sign, tag or publish anything either.
#
# Usage:
#   bash scripts/repro-build.sh [--package ravum-lang-cli] [--profile release]
#                               [--output FILE]
#
# Exit codes: 0 measured, 1 build or measurement failed, 2 usage error.

set -euo pipefail

EXIT_FAIL=1
EXIT_USAGE=2

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
package="ravum-lang-cli"
profile="release"
output=""

usage() {
    cat <<'EOF'
Usage: bash scripts/repro-build.sh [options]

  --package NAME   Package to build (default: ravum-lang-cli).
  --profile NAME   Cargo profile (default: release).
  --output FILE    Write the JSON measurement to FILE instead of stdout.
  -h, --help       Show this help.

Environment overrides: CARGO_TARGET_DIR is honoured; everything else that
affects the digest (CARGO_INCREMENTAL, SOURCE_DATE_EPOCH, RUSTFLAGS, linker
build-id) is set by this script on purpose.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --package)
            [ $# -ge 2 ] || { echo "repro-build.sh: error: --package requires a value" >&2; exit "$EXIT_USAGE"; }
            package="$2"
            shift 2
            ;;
        --package=*)
            package="${1#*=}"
            shift
            ;;
        --profile)
            [ $# -ge 2 ] || { echo "repro-build.sh: error: --profile requires a value" >&2; exit "$EXIT_USAGE"; }
            profile="$2"
            shift 2
            ;;
        --profile=*)
            profile="${1#*=}"
            shift
            ;;
        --output)
            [ $# -ge 2 ] || { echo "repro-build.sh: error: --output requires a value" >&2; exit "$EXIT_USAGE"; }
            output="$2"
            shift 2
            ;;
        --output=*)
            output="${1#*=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "repro-build.sh: error: unknown argument: $1" >&2
            usage >&2
            exit "$EXIT_USAGE"
            ;;
    esac
done

[ -f "$root/Cargo.toml" ] || { echo "repro-build.sh: error: Cargo.toml not found in $root" >&2; exit "$EXIT_USAGE"; }

case "$profile" in
    dev) out_dir="debug" ;;
    release) out_dir="release" ;;
    *) out_dir="$profile" ;;
esac

# Remap the machine-specific prefixes to stable ones: the workspace root
# (critical) and the cargo registry cache (dependencies can mention it in panic
# locations). They are disjoint, so no prefix shadows another.
export CARGO_INCREMENTAL=0
export SOURCE_DATE_EPOCH=0
export RUSTFLAGS="--remap-path-prefix=$root=/rv --remap-path-prefix=${CARGO_HOME:-${HOME:-/home/runner}/.cargo}=/cargo -C link-arg=-Wl,--build-id=sha1"

echo "repro-build.sh: package=$package profile=$profile root=$root" >&2
echo "repro-build.sh: RUSTFLAGS=$RUSTFLAGS" >&2

cargo build --profile "$profile" --locked -p "$package" || exit "$EXIT_FAIL"

binary="$root/target/$out_dir/ravum-lang"
if [ ! -f "$binary" ] && [ -f "$binary.exe" ]; then
    binary="$binary.exe"
fi
[ -f "$binary" ] || { echo "repro-build.sh: error: built binary not found: $binary" >&2; exit "$EXIT_FAIL"; }

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "repro-build.sh: error: neither sha256sum nor shasum is available" >&2
        exit "$EXIT_FAIL"
    fi
}

sha256="$(sha256_of "$binary")"
size_bytes="$(wc -c < "$binary" | tr -d '[:space:]')"
git_commit="$(git -C "$root" rev-parse HEAD 2>/dev/null || echo UNKNOWN)"
git_dirty=false
if [ -n "$(git -C "$root" status --porcelain 2>/dev/null || true)" ]; then
    git_dirty=true
fi
rustc_line="$(rustc -V 2>/dev/null || echo UNKNOWN)"
os_name="$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]' || echo unknown)"
arch_name="$(uname -m 2>/dev/null || echo unknown)"
host_name="$(uname -n 2>/dev/null || echo unknown)"
built_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

json="$(cat <<EOF
{
  "schema_version": 1,
  "package": "$(printf '%s' "$package" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')",
  "profile": "$(printf '%s' "$profile" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')",
  "sha256": "$sha256",
  "size_bytes": $size_bytes,
  "git_commit": "$git_commit",
  "git_dirty": $git_dirty,
  "rustc": "$(printf '%s' "$rustc_line" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')",
  "os": "$os_name",
  "arch": "$arch_name",
  "hostname": "$(printf '%s' "$host_name" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')",
  "built_at_utc": "$built_at",
  "binary": "$(printf '%s' "$binary" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}
EOF
)"

echo "repro-build.sh: sha256=$sha256" >&2

if [ -n "$output" ]; then
    mkdir -p "$(dirname "$output")"
    printf '%s\n' "$json" > "$output"
    echo "repro-build.sh: wrote $output" >&2
else
    printf '%s\n' "$json"
fi

exit 0
