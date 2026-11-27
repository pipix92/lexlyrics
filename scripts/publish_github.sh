#!/usr/bin/env bash
# =====================================================================
# Pubblica LexLyrics su GitHub (pipix92/lexlyrics) + Release Assets
#
# - Se il remote SSH `git@github.com:...` è già autenticato, fa push di
#   `main`, del branch corrente e del tag `v<VERSION>` direttamente via SSH
#   (attivando anche la GitHub Action `.github/workflows/release.yml`).
# - Se è disponibile `GITHUB_TOKEN` nel vault (passphrase interattiva),
#   carica direttamente anche i pacchetti locali (.deb, .tar.gz, .zip)
#   sulla GitHub Release corrispondente.
# =====================================================================
set -euo pipefail

OWNER="${OWNER:-pipix92}"
REPO="${REPO:-lexlyrics}"
PRIVATE="${PRIVATE:-0}"
VAULT="${VAULT:-/mnt/lm_models/DEBIAN_FOLDER/lex_secure_vault/vault.py}"
DESCRIPTION="Karaoke lyrics bar for Linux & Windows with live Italian translation (NLLB-200 + LLM refinement)."

cd "$(dirname "${BASH_SOURCE[0]}")/.."
BRANCH="$(git branch --show-current)"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)"
TAG="v${VERSION}"

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "==> Creo tag $TAG"
    git tag -a "$TAG" -m "LexLyrics $TAG"
fi

if GIT_SSH_COMMAND="ssh -o BatchMode=yes -o ConnectTimeout=5" ssh -T git@github.com 2>&1 | grep -q "successfully authenticated"; then
    echo "==> Push via SSH su git@github.com:$OWNER/$REPO.git ($BRANCH, main, $TAG)"
    git remote set-url origin "git@github.com:$OWNER/$REPO.git" 2>/dev/null || \
        git remote add origin "git@github.com:$OWNER/$REPO.git"
    git push origin "HEAD:refs/heads/main" "HEAD:refs/heads/$BRANCH" "refs/tags/$TAG"
    echo "==> Codice e tag $TAG pubblicati! La GitHub Action creerà/aggiornerà la Release."
    if [ "${UPLOAD_LOCAL_ASSETS:-0}" != "1" ]; then
        echo "    (Per caricare anche gli asset locali via API usa: UPLOAD_LOCAL_ASSETS=1 ./scripts/publish_github.sh)"
        exit 0
    fi
fi

echo "==> Leggo GITHUB_TOKEN dal vault"
VAULT_OUT="$(python3 "$VAULT" get GITHUB_TOKEN || true)"
if [[ "$VAULT_OUT" == "Nessuna chiave"* ]]; then
    echo "Il vault non ha una chiave GITHUB_TOKEN (vedi: python3 $VAULT list)." >&2
    exit 1
fi
TOKEN="$(printf '%s\n' "$VAULT_OUT" | sed -n 's/^[[:space:]]*\(export[[:space:]]\+\)\?\(GITHUB_TOKEN[[:space:]]*=[[:space:]]*\)\?["'\'']\?\([^"'\''[:space:]]\+\).*/\3/p' | tail -n1)"
unset VAULT_OUT
if [ -z "$TOKEN" ]; then
    echo "Token non trovato nell'output del vault." >&2
    exit 1
fi

api() {
    curl -fsS -H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" "$@"
}

LOGIN="$(api https://api.github.com/user 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["login"])' 2>/dev/null || true)"
if [ -z "$LOGIN" ]; then
    echo "GitHub rifiuta il token (scaduto o revocato?)." >&2
    exit 1
fi
echo "==> Autenticato come $LOGIN"

if ! api "https://api.github.com/repos/$OWNER/$REPO" >/dev/null 2>&1; then
    private_json=$([ "$PRIVATE" = 1 ] && echo true || echo false)
    api -X POST https://api.github.com/user/repos \
        -d "{\"name\":\"$REPO\",\"description\":\"$DESCRIPTION\",\"private\":$private_json,\"has_issues\":true,\"has_wiki\":false}" \
        >/dev/null
fi

ASKPASS="$(mktemp)"
trap 'rm -f "$ASKPASS"' EXIT
printf '#!/bin/sh\nprintf "%%s" "$LEXLYRICS_GH_TOKEN"\n' > "$ASKPASS"
chmod 700 "$ASKPASS"

URL="https://$OWNER@github.com/$OWNER/$REPO.git"
LEXLYRICS_GH_TOKEN="$TOKEN" GIT_ASKPASS="$ASKPASS" GIT_TERMINAL_PROMPT=0 \
    git push "$URL" "HEAD:refs/heads/main" "HEAD:refs/heads/$BRANCH" "refs/tags/$TAG"

api -X PATCH "https://api.github.com/repos/$OWNER/$REPO" -d "{\"default_branch\":\"main\",\"description\":\"$DESCRIPTION\"}" >/dev/null || true
api -X PUT "https://api.github.com/repos/$OWNER/$REPO/topics" \
    -d '{"names":["lyrics","karaoke","linux","windows","gtk4","rust","translation","nllb","cosmic","wayland"]}' >/dev/null || true

NOTES_FILE="docs/release-notes/${TAG}.md"
NOTES_JSON="$(python3 -c 'import json,sys,pathlib; p=pathlib.Path(sys.argv[1]); print(json.dumps(p.read_text() if p.exists() else sys.argv[2]))' "$NOTES_FILE" "LexLyrics $TAG")"

REL_ID="$(api "https://api.github.com/repos/$OWNER/$REPO/releases/tags/$TAG" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' 2>/dev/null || true)"
if [ -z "$REL_ID" ]; then
    REL_ID="$(api -X POST "https://api.github.com/repos/$OWNER/$REPO/releases" \
        -d "{\"tag_name\":\"$TAG\",\"name\":\"LexLyrics $VERSION\",\"body\":$NOTES_JSON}" \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')"
fi

for asset in \
    "lexlyrics_${VERSION}-1_amd64.deb" \
    "lexlyrics-v${VERSION}-linux-x86_64.tar.gz" \
    "lexlyrics-v${VERSION}-windows-x86_64.zip"; do
    if [ -f "$asset" ]; then
        echo "==> Carico asset $asset sulla Release $TAG..."
        curl -fsS -X POST \
            -H "Authorization: Bearer $TOKEN" \
            -H "Content-Type: application/octet-stream" \
            --data-binary @"$asset" \
            "https://uploads.github.com/repos/$OWNER/$REPO/releases/$REL_ID/assets?name=$(basename "$asset")" >/dev/null || true
    fi
done

echo "==> Pubblicazione completata: https://github.com/$OWNER/$REPO/releases/tag/$TAG"
