#!/usr/bin/env bash
# =====================================================================
# Pubblica LexLyrics su GitHub (account pipix92 di default)
#
# Il token arriva da GITHUB_TOKEN del vault (la passphrase la chiede il
# vault stesso): resta solo in memoria in questo processo, non finisce
# su disco, nella cronologia della shell o nella URL del remote.
#
# Uso:  scripts/publish_github.sh            repo pubblico pipix92/lexlyrics
#       PRIVATE=1 scripts/publish_github.sh  repo privato
#       OWNER=altro REPO=nome scripts/publish_github.sh
# =====================================================================
set -euo pipefail

OWNER="${OWNER:-pipix92}"
REPO="${REPO:-lexlyrics}"
PRIVATE="${PRIVATE:-0}"
VAULT="${VAULT:-/mnt/lm_models/DEBIAN_FOLDER/lex_secure_vault/vault.py}"
DESCRIPTION="Karaoke lyrics bar for Linux with live Italian translation (NLLB-200 + LLM refinement). Fork of Waylyrics."

cd "$(dirname "${BASH_SOURCE[0]}")/.."
BRANCH="$(git branch --show-current)"

echo "==> Leggo GITHUB_TOKEN dal vault"
TOKEN="$(python3 "$VAULT" get GITHUB_TOKEN | grep -oE '(ghp_|github_pat_|gho_)[A-Za-z0-9_]+' | tail -n1 || true)"
if [ -z "$TOKEN" ]; then
    echo "Token non trovato nell'output del vault." >&2
    exit 1
fi

api() {
    curl -fsS -H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" "$@"
}

if api "https://api.github.com/repos/$OWNER/$REPO" >/dev/null 2>&1; then
    echo "==> Il repository $OWNER/$REPO esiste già"
else
    echo "==> Creo $OWNER/$REPO ($([ "$PRIVATE" = 1 ] && echo privato || echo pubblico))"
    private_json=$([ "$PRIVATE" = 1 ] && echo true || echo false)
    api -X POST https://api.github.com/user/repos \
        -d "{\"name\":\"$REPO\",\"description\":\"$DESCRIPTION\",\"private\":$private_json,\"has_issues\":true,\"has_wiki\":false}" \
        >/dev/null
fi

# git chiede la password a questo helper, che la legge dall'ambiente
ASKPASS="$(mktemp)"
trap 'rm -f "$ASKPASS"' EXIT
printf '#!/bin/sh\nprintf "%%s" "$LEXLYRICS_GH_TOKEN"\n' > "$ASKPASS"
chmod 700 "$ASKPASS"

URL="https://$OWNER@github.com/$OWNER/$REPO.git"
echo "==> Push di $BRANCH su main e su $BRANCH"
LEXLYRICS_GH_TOKEN="$TOKEN" GIT_ASKPASS="$ASKPASS" GIT_TERMINAL_PROMPT=0 \
    git push "$URL" "HEAD:refs/heads/main" "HEAD:refs/heads/$BRANCH"

api -X PATCH "https://api.github.com/repos/$OWNER/$REPO" -d '{"default_branch":"main"}' >/dev/null || true
api -X PUT "https://api.github.com/repos/$OWNER/$REPO/topics" \
    -d '{"names":["lyrics","karaoke","linux","gtk4","rust","translation","nllb","cosmic","wayland"]}' >/dev/null || true

if git remote get-url origin >/dev/null 2>&1; then
    git remote set-url origin "https://github.com/$OWNER/$REPO.git"
else
    git remote add origin "https://github.com/$OWNER/$REPO.git"
fi

echo
echo "==> Fatto: https://github.com/$OWNER/$REPO"
