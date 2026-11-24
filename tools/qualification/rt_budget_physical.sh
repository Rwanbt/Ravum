#!/usr/bin/env bash
# V2-G10 : qualification temporelle T16 sur une machine physique.
# Lance le test rt_budget N fois (défaut 10) en release et garde les lignes
# T16 de chaque exécution dans un rapport daté. Sur une machine physique (ou
# non identifiée), les assertions de temps sont bloquantes : un échec est un
# FAIL réel, à rapporter tel quel. Aucun chiffre n'est retouché.
set -euo pipefail
runs="${1:-10}"
out="rt-budget-$(date -u +%Y%m%dT%H%M%SZ).log"
{
  echo "# rt_budget physique — $(date -u +%FT%TZ)"
  echo "# commit: $(git rev-parse HEAD)"
  echo "# os: $(uname -srm)"
  echo "# rustc: $(rustc --version)"
  if [ -r /proc/cpuinfo ]; then grep -m1 "model name" /proc/cpuinfo; fi
  if command -v sysctl >/dev/null 2>&1; then sysctl -n machdep.cpu.brand_string 2>/dev/null || true; fi
} > "$out"
fails=0
for i in $(seq 1 "$runs"); do
  if cargo test -q --release -p ravum-lang-jit --test rt_budget -- --nocapture --test-threads=1 2>&1 \
      | grep -E "^T16|test result|panicked" >> "$out"; then :; fi
  if tail -n 3 "$out" | grep -q "FAILED\|panicked"; then fails=$((fails + 1)); fi
done
echo "# runs=$runs fails=$fails" >> "$out"
echo "rapport : $out (runs=$runs, fails=$fails)"
[ "$fails" -eq 0 ]
