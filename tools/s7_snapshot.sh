#!/usr/bin/env bash
# S7 snapshot: the Rust generated for every example and every conformance input
# that builds, so two toolchains can be compared byte for byte.
#
#   tools/s7_snapshot.sh <ravum-lang binary> <output directory>
#
# Writes <out>/<target>.rs for each target that builds, <out>.targets (the
# targets tried) and <out>.status (OK / FAIL per target). Compare two snapshots
# with `diff -r <out-a> <out-b>` and `diff <out-a>.status <out-b>.status`: both
# must be empty, a target that stops building is a difference too.
#
# The corpus is copied to a scratch directory first (the toolchain writes
# `.ravum/` and `ravum.lock` next to the sources); the repository is not touched.
# Run both snapshots with the same scratch path: it appears in no generated
# byte, but keeping it fixed removes the question.
set -u
if [ "$#" -ne 2 ]; then
    echo "usage: $0 <ravum-lang binary> <output directory>" >&2
    exit 2
fi
BIN="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
mkdir -p "$2"
OUT="$(cd "$2" && pwd)"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${S7_WORK:-${TMPDIR:-/tmp}/ravum-s7-work}"
rm -rf "$WORK" "$OUT"
mkdir -p "$WORK" "$OUT"
cp -r "$REPO/examples" "$WORK/examples"
cp -r "$REPO/conformance/cases" "$WORK/cases"
cd "$WORK" || exit 2

# Targets: every package directory, and every `.rv` file outside a package.
{
    find . -name ravum.toml -printf '%h\n'
    find . -name '*.rv' | while read -r file; do
        dir="$(dirname "$file")"
        packaged=0
        while [ "$dir" != "." ]; do
            if [ -f "$dir/ravum.toml" ]; then
                packaged=1
                break
            fi
            dir="$(dirname "$dir")"
        done
        [ "$packaged" = 0 ] && echo "$file"
    done
} | sort -u > "$OUT.targets"

snapshot_one() {
    target="$1"
    id="$(echo "$target" | sed 's|^\./||; s|/|__|g')"
    if timeout 120 "$BIN" build "$target" --emit-rust "$OUT/$id.rs" >/dev/null 2>&1; then
        echo "$id OK"
    else
        echo "$id FAIL($?)"
        rm -f "$OUT/$id.rs"
    fi
}
export -f snapshot_one
export BIN OUT
xargs -P "${S7_JOBS:-4}" -I{} bash -c 'snapshot_one "{}"' < "$OUT.targets" | sort > "$OUT.status"

emitted="$(grep -c ' OK$' "$OUT.status")"
total="$(wc -l < "$OUT.status")"
digest="$(cat "$OUT"/*.rs | sha256sum | cut -d' ' -f1)"
echo "s7: targets=$total emitted=$emitted sha256=$digest"
