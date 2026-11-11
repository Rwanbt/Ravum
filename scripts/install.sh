#!/usr/bin/env bash
# Installateur POSIX de `ravum-lang` (compilateur Ravum Core 1.0).
#
# Construit `ravum-lang-cli` en release verrouillee, puis installe :
#   <prefix>/bin/ravum-lang
#   <prefix>/ravum-install.json
#
# Fail-closed : build/copie/remplacement en echec => version precedente restauree,
# code de sortie 1. Aucun reseau (--locked s'appuie sur Cargo.lock et le cache local).
# Aucune signature n'est produite : voir docs/release/PROVENANCE.md.
#
# Sorties : 0 succes, 1 echec, 2 usage invalide.
set -euo pipefail

EXIT_OK=0
EXIT_FAIL=1
EXIT_USAGE=2

PREFIX="${RAVUM_PREFIX:-$HOME/.local/share/ravum}"
DO_UNINSTALL=0
ADD_TO_PATH=0
REMOVE_FROM_PATH=0
SKIP_BUILD=0

usage() {
    cat <<'EOF'
Usage: scripts/install.sh [options]

  --prefix DIR            Prefixe d'installation (defaut: $RAVUM_PREFIX ou
                          $HOME/.local/share/ravum). Binaire dans DIR/bin.
  --uninstall             Supprime le binaire et le manifeste (idempotent).
  --add-to-path           N'ecrit rien : affiche la ligne `export PATH=...`.
  --remove-from-path      Avec --uninstall : affiche les instructions de retrait.
  --skip-build            Ne pas lancer cargo build (binaire deja dans target/release).
  -h, --help              Affiche cette aide.

Variables : SOURCE_DATE_EPOCH n'affecte que les scripts SBOM, pas l'installateur.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            [ $# -ge 2 ] || { echo "install.sh: error: --prefix exige une valeur" >&2; exit "$EXIT_USAGE"; }
            PREFIX="$2"
            shift 2
            ;;
        --prefix=*)
            PREFIX="${1#*=}"
            shift
            ;;
        --uninstall)
            DO_UNINSTALL=1
            shift
            ;;
        --add-to-path)
            ADD_TO_PATH=1
            shift
            ;;
        --remove-from-path)
            REMOVE_FROM_PATH=1
            shift
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        -h|--help)
            usage
            exit "$EXIT_OK"
            ;;
        *)
            echo "install.sh: error: argument inconnu: $1" >&2
            usage >&2
            exit "$EXIT_USAGE"
            ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
[ -f "$ROOT/Cargo.toml" ] || { echo "install.sh: error: Cargo.toml introuvable dans $ROOT" >&2; exit "$EXIT_USAGE"; }

# Git Bash / MSYS : accepter un chemin Windows (C:/... ou C:\...) via cygpath.
case "$PREFIX" in
    [A-Za-z]:[\\/]*)
        if command -v cygpath >/dev/null 2>&1; then
            PREFIX="$(cygpath -u "$PREFIX")"
        else
            echo "install.sh: error: chemin Windows non convertible (cygpath absent) : $PREFIX" >&2
            exit "$EXIT_USAGE"
        fi
        ;;
esac

case "$PREFIX" in
    /*) : ;;
    *) PREFIX="$(pwd)/$PREFIX" ;;
esac
PREFIX="${PREFIX%/}"
[ -n "$PREFIX" ] || { echo "install.sh: error: prefixe vide" >&2; exit "$EXIT_USAGE"; }
if [ "$PREFIX" = "$ROOT" ]; then
    echo "install.sh: error: le prefixe ne peut pas etre la racine du depot" >&2
    exit "$EXIT_USAGE"
fi
if [ "$REMOVE_FROM_PATH" -eq 1 ] && [ "$DO_UNINSTALL" -eq 0 ]; then
    echo "install.sh: error: --remove-from-path exige --uninstall" >&2
    exit "$EXIT_USAGE"
fi

BIN_DIR="$PREFIX/bin"
DEST="$BIN_DIR/ravum-lang"
MANIFEST="$PREFIX/ravum-install.json"

fail() {
    echo "install.sh: error: $*" >&2
    exit "$EXIT_FAIL"
}

info() {
    echo "install.sh: $*"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        fail "ni sha256sum ni shasum ne sont disponibles"
    fi
}

json_escape() {
    sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

# --------------------------------------------------------------- uninstall
if [ "$DO_UNINSTALL" -eq 1 ]; then
    removed=""
    if [ -f "$DEST" ]; then
        rm -f "$DEST"
        removed="$removed $DEST"
    fi
    if [ -f "$MANIFEST" ]; then
        rm -f "$MANIFEST"
        removed="$removed $MANIFEST"
    fi
    if [ -d "$BIN_DIR" ]; then
        rmdir "$BIN_DIR" 2>/dev/null || true
    fi
    if [ -n "$removed" ]; then
        info "desinstalle :$removed"
    else
        info "rien a desinstaller (prefixe : $PREFIX)"
    fi
    if [ "$REMOVE_FROM_PATH" -eq 1 ]; then
        echo "Retirer \"$BIN_DIR\" de PATH dans ~/.bashrc, ~/.zshrc ou ~/.profile," >&2
        echo "puis ouvrir un nouveau terminal." >&2
    fi
    exit "$EXIT_OK"
fi

# ------------------------------------------------------------------- build
BUILT="$ROOT/target/release/ravum-lang"
if [ "$SKIP_BUILD" -eq 0 ]; then
    info "cargo build -p ravum-lang-cli --release --locked"
    ( cd "$ROOT" && cargo build -p ravum-lang-cli --release --locked ) || fail "cargo build a echoue"
fi
[ -f "$BUILT" ] || fail "binaire construit introuvable : $BUILT"

# ------------------------------------------------------- metadonnees locales
version_line="$("$BUILT" --version)" || fail "ravum-lang --version a echoue"
[ -n "$version_line" ] || fail "sortie --version vide"
package_version="$(printf '%s\n' "$version_line" | sed -n 's/^ravum-lang \([^ ]*\).*/\1/p')"
[ -n "$package_version" ] || fail "sortie --version inattendue : $version_line"

commit="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo UNKNOWN)"
git_dirty=false
if [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null || true)" ]; then
    git_dirty=true
fi
rustc_line="$(rustc -V 2>/dev/null || echo UNKNOWN)"
sha256="$(sha256_of "$BUILT")"
installed_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
platform="$(uname -s 2>/dev/null | tr 'A-Z' 'a-z' || echo unknown)"
arch="$(uname -m 2>/dev/null || echo unknown)"

manifest_json="$(cat <<EOF
{
  "schema_version": 1,
  "name": "ravum-lang",
  "version": "$(printf '%s' "$version_line" | json_escape)",
  "package_version": "$(printf '%s' "$package_version" | json_escape)",
  "git_commit": "$(printf '%s' "$commit" | json_escape)",
  "git_dirty": $git_dirty,
  "installed_at_utc": "$installed_at",
  "sha256": "$sha256",
  "rustc": "$(printf '%s' "$rustc_line" | json_escape)",
  "platform": "$(printf '%s' "$platform" | json_escape)",
  "arch": "$(printf '%s' "$arch" | json_escape)",
  "profile": "release",
  "locked": true,
  "binary": "bin/ravum-lang",
  "prefix": "$(printf '%s' "$PREFIX" | json_escape)",
  "installer": "scripts/install.sh"
}
EOF
)"

# --------------------------------------------------- staging + move atomique
mkdir -p "$PREFIX"
STAGING="$(mktemp -d "$PREFIX/.staging.XXXXXX")" || fail "staging impossible dans $PREFIX"
cleanup() { rm -rf "$STAGING"; }
trap cleanup EXIT

BACKUP_EXE="$DEST.bak"
BACKUP_MANIFEST="$MANIFEST.bak"
had_exe=0
had_manifest=0

rollback() {
    if [ "$had_exe" -eq 1 ] && [ -f "$BACKUP_EXE" ]; then
        cp -f "$BACKUP_EXE" "$DEST" 2>/dev/null || true
    else
        rm -f "$DEST" 2>/dev/null || true
    fi
    if [ "$had_manifest" -eq 1 ] && [ -f "$BACKUP_MANIFEST" ]; then
        cp -f "$BACKUP_MANIFEST" "$MANIFEST" 2>/dev/null || true
    else
        rm -f "$MANIFEST" 2>/dev/null || true
    fi
}

mkdir -p "$STAGING/bin" "$BIN_DIR"
cp -f "$BUILT" "$STAGING/bin/ravum-lang" || fail "copie vers le staging echouee"
printf '%s\n' "$manifest_json" > "$STAGING/ravum-install.json" || fail "ecriture du manifeste echouee"

[ -f "$DEST" ] && { cp -f "$DEST" "$BACKUP_EXE" && had_exe=1; }
[ -f "$MANIFEST" ] && { cp -f "$MANIFEST" "$BACKUP_MANIFEST" && had_manifest=1; }

if ! mv -f "$STAGING/bin/ravum-lang" "$DEST"; then
    rollback
    fail "remplacement du binaire echoue (binaire en cours d'utilisation ?)"
fi
if ! mv -f "$STAGING/ravum-install.json" "$MANIFEST"; then
    rollback
    fail "remplacement du manifeste echoue"
fi
chmod 755 "$DEST" || fail "chmod 755 echoue sur $DEST"
rm -f "$BACKUP_EXE" "$BACKUP_MANIFEST"

installed_sha="$(sha256_of "$DEST")"
[ "$installed_sha" = "$sha256" ] || fail "SHA-256 installe different ($installed_sha != $sha256)"

info "installe : $version_line"
info "binaire  : $DEST"
info "manifeste: $MANIFEST"
info "sha256   : $sha256"
info "commit   : $commit"
if [ "$git_dirty" = true ]; then
    echo "install.sh: warning: arbre de travail modifie (git_dirty=true) : le binaire peut contenir des changements non commites." >&2
fi

if [ "$ADD_TO_PATH" -eq 1 ]; then
    echo ""
    echo "Ajouter au PATH (a coller dans ~/.bashrc, ~/.zshrc ou ~/.profile) :"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    echo "Puis ouvrir un nouveau terminal."
else
    info "PATH non modifie. Ajouter : $BIN_DIR (ou relancer avec --add-to-path)."
fi

exit "$EXIT_OK"
