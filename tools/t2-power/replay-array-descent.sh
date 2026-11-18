#!/usr/bin/env bash
set -euo pipefail

form=${1:?Specify a form from 10 to 13}
programs=${2:-2000}
case "$form" in
  10|11) audit=audit_shared_descent_slot_hoist_builds_and_runs ;;
  12|13) audit=audit_branch_and_nested_descents_build_and_run ;;
  *) exit 2 ;;
esac
[[ "$programs" =~ ^[1-9][0-9]*$ ]]

root=$(git rev-parse --show-toplevel)
output="$root/target/t2-power/$form"
mkdir -p "$output"
scratch=$(mktemp -d)
trap 'git -C "$root" worktree remove --force "$scratch/tree" 2>/dev/null || true; rm -rf "$scratch"' EXIT
git worktree add --detach "$scratch/tree" HEAD
cd "$scratch/tree"
git log -1 --format='base_sha=%H'
rustc --version
export RAVUM_TYPED_POWER_FORM="$form" RAVUM_TYPED_PROGRAMS="$programs" RAVUM_TYPED_RELEASE_EVERY=0

audit_test=(cargo test -p ravum-lang-codegen --test backend_audit "$audit" -- --exact --nocapture)
"${audit_test[@]}" > "$output/healthy-audit.log" 2>&1 || { cat "$output/healthy-audit.log"; exit 1; }
grep -F '1 passed; 0 failed' "$output/healthy-audit.log"
if grep -q 'SKIP ' "$output/healthy-audit.log"; then
  echo "The healthy audit was skipped" >&2
  exit 1
fi

git apply --check tools/t2-power/array-descent-controls.patch
git apply tools/t2-power/array-descent-controls.patch
sha256sum tools/t2-power/array-descent-controls.patch
export RAVUM_E7_INVERSE="$form"

set +e
"${audit_test[@]}" > "$output/inverse-audit.log" 2>&1
audit_status=$?
cargo test -p ravum-lang-codegen --test differential_oracle typed_oracle_long \
  -- --ignored --nocapture > "$output/campaign.log" 2>&1
campaign_status=$?
set -e
echo "form=$form audit_exit=$audit_status campaign_exit=$campaign_status"
[[ "$audit_status" -eq 101 && "$campaign_status" -eq 101 ]]
grep -m 1 '^error\[E0508\]' "$output/inverse-audit.log"

python3 - "$output/campaign.log" "$programs" <<'PYTHON'
import re
import sys
from pathlib import Path

log = Path(sys.argv[1]).read_text()
summaries = re.findall(r"^\[typed:long\] TOTAL (.*)$", log, re.MULTILINE)
if len(summaries) != 1:
    sys.exit("Expected exactly one completed campaign summary")
summary = summaries[0]
print(f"[typed:long] TOTAL {summary}")
counts = {key: int(value) for key, value in re.findall(r"(\w+)=(\d+)", summary)}
if counts["generated"] != 3 * int(sys.argv[2]):
    sys.exit("Incomplete campaign")
divergences = counts["divergences"]
if not (
    divergences > 0
    and counts["fault_rejected"] == counts["faults"]
    and counts["accepted"] == counts["rejected"] == 0
    and counts["generated"] == counts["faults"] + divergences
    and len(re.findall(r"^error\[E0508\]", log, re.MULTILINE)) == divergences
):
    sys.exit("Test results do not match the expected checked-program divergences")
PYTHON
