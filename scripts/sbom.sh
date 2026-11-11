#!/usr/bin/env bash
# Genere un SBOM SPDX 2.3 deterministe du workspace Ravum.
#
# 1. `cargo metadata --format-version 1 --locked` (local, sans reseau)
# 2. `node tools/sbom/sbom.mjs` (Node standard uniquement)
# 3. validation JSON + compteurs + SHA-256
#
# Determinisme : identique pour les memes metadonnees et le meme horodatage.
# Sans --timestamp, `created` vaut l'heure UTC courante ; utiliser
# SOURCE_DATE_EPOCH=0 (ou --timestamp 1970-01-01T00:00:00Z) pour un fichier
# reproductible octet pour octet.
#
# Sorties : 0 succes, 1 echec, 2 usage invalide.
set -euo pipefail

EXIT_OK=0
EXIT_FAIL=1
EXIT_USAGE=2

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT="$ROOT/target/sbom/ravum.spdx.json"
NAME="ravum-workspace"
TIMESTAMP=""

usage() {
    cat <<'EOF'
Usage: scripts/sbom.sh [options]

  --output FILE       Fichier SPDX JSON (defaut: target/sbom/ravum.spdx.json).
  --name NAME         Nom du document SPDX (defaut: ravum-workspace).
  --timestamp ISO     Horodatage fixe, ex. 1970-01-01T00:00:00Z.
  --repo-root DIR     Racine du depot (defaut: parent du script).
  -h, --help          Affiche cette aide.

Environnement : SOURCE_DATE_EPOCH (secondes) fixe `created` si --timestamp est absent.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --output) [ $# -ge 2 ] || { echo "sbom.sh: error: --output exige une valeur" >&2; exit "$EXIT_USAGE"; }; OUTPUT="$2"; shift 2 ;;
        --output=*) OUTPUT="${1#*=}"; shift ;;
        --name) [ $# -ge 2 ] || { echo "sbom.sh: error: --name exige une valeur" >&2; exit "$EXIT_USAGE"; }; NAME="$2"; shift 2 ;;
        --name=*) NAME="${1#*=}"; shift ;;
        --timestamp) [ $# -ge 2 ] || { echo "sbom.sh: error: --timestamp exige une valeur" >&2; exit "$EXIT_USAGE"; }; TIMESTAMP="$2"; shift 2 ;;
        --timestamp=*) TIMESTAMP="${1#*=}"; shift ;;
        --repo-root) [ $# -ge 2 ] || { echo "sbom.sh: error: --repo-root exige une valeur" >&2; exit "$EXIT_USAGE"; }; ROOT="$(cd "$2" && pwd)"; shift 2 ;;
        --repo-root=*) ROOT="$(cd "${1#*=}" && pwd)"; shift ;;
        -h|--help) usage; exit "$EXIT_OK" ;;
        *) echo "sbom.sh: error: argument inconnu: $1" >&2; usage >&2; exit "$EXIT_USAGE" ;;
    esac
done

fail() {
    echo "sbom: error: $*" >&2
    exit "$EXIT_FAIL"
}

# Git Bash / MSYS : convertir une sortie Windows (C:/...) en chemin POSIX.
case "$OUTPUT" in
    [A-Za-z]:[\\/]*)
        if command -v cygpath >/dev/null 2>&1; then
            OUTPUT="$(cygpath -u "$OUTPUT")"
        fi
        ;;
esac

[ -f "$ROOT/Cargo.toml" ] || fail "Cargo.toml introuvable dans $ROOT"
GENERATOR="$ROOT/tools/sbom/sbom.mjs"
[ -f "$GENERATOR" ] || fail "generateur introuvable : $GENERATOR"
command -v node >/dev/null 2>&1 || fail "node introuvable dans PATH"
command -v cargo >/dev/null 2>&1 || fail "cargo introuvable dans PATH"

command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || fail "sha256sum/shasum absent"

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

OUTDIR="$(dirname "$OUTPUT")"
mkdir -p "$OUTDIR"

TMP="$(mktemp "${TMPDIR:-/tmp}/ravum-metadata.XXXXXX")"
cleanup() { rm -f "$TMP"; }
trap cleanup EXIT

( cd "$ROOT" && cargo metadata --format-version 1 --locked --manifest-path "$ROOT/Cargo.toml" > "$TMP" ) \
    || fail "cargo metadata a echoue"
[ -s "$TMP" ] || fail "cargo metadata a produit une sortie vide"

COMMIT="$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || echo nogit)"

NODE_ARGS=( "$GENERATOR" --metadata-file "$TMP" --out "$OUTPUT" --name "$NAME" --commit "$COMMIT" )
if [ -n "$TIMESTAMP" ]; then
    NODE_ARGS+=( --timestamp "$TIMESTAMP" )
fi
node "${NODE_ARGS[@]}" || fail "ravum-sbom a echoue"

[ -f "$OUTPUT" ] || fail "fichier SBOM absent apres generation : $OUTPUT"

SUMMARY="$(node -e '
const fs = require("node:fs");
const doc = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
if (doc.spdxVersion !== "SPDX-2.3") { console.error("spdxVersion inattendu: " + doc.spdxVersion); process.exit(1); }
if (!Array.isArray(doc.packages) || doc.packages.length === 0) { console.error("aucun package"); process.exit(1); }
const rel = Array.isArray(doc.relationships) ? doc.relationships.length : 0;
process.stdout.write(`spdxVersion=${doc.spdxVersion} packages=${doc.packages.length} relationships=${rel} created=${doc.creationInfo.created}`);
' "$OUTPUT")" || fail "SBOM produit invalide : $OUTPUT"

SHA="$(sha256_of "$OUTPUT")"
echo "sbom: wrote $OUTPUT"
echo "sbom: $SUMMARY"
echo "sbom: sha256=$SHA"
exit "$EXIT_OK"
