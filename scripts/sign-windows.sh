#!/usr/bin/env bash
set -euo pipefail

jsign_version="7.5"
jsign_sha256="602a51c3545a6dc4fb99bd2ea7152b26d1345916d0c93ddfbd5936cb735af91c"
if [ "$#" -eq 0 ]; then
  echo "usage: scripts/sign-windows.sh <binary>..." >&2
  exit 2
fi
missing=""
for name in AZURE_TENANT_ID AZURE_CLIENT_ID AZURE_TRUSTED_SIGNING_ENDPOINT AZURE_TRUSTED_SIGNING_ACCOUNT AZURE_TRUSTED_SIGNING_PROFILE; do
  [ -z "${!name:-}" ] && missing="$missing $name"
done
if [ -n "$missing" ]; then
  echo "::warning::no Azure Trusted Signing configuration ($missing ); the windows binaries stay unsigned"
  exit 0
fi

work="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/jerk-sign.XXXXXX")"
trap 'rm -rf "$work"' EXIT
sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}
token() {
  if [ -n "${AZURE_CLIENT_SECRET:-}" ]; then
    printf '%s|%s|%s' "$AZURE_TENANT_ID" "$AZURE_CLIENT_ID" "$AZURE_CLIENT_SECRET"
    return
  fi
  if [ -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ]; then
    echo "::error::no AZURE_CLIENT_SECRET and no GitHub OIDC token (id-token: write)" >&2
    exit 1
  fi
  local assertion
  assertion="$(curl -fsS -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
    "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=api://AzureADTokenExchange" | node -p 'JSON.parse(require("fs").readFileSync(0,"utf8")).value')"
  curl -fsS -X POST "https://login.microsoftonline.com/$AZURE_TENANT_ID/oauth2/v2.0/token" \
    --data-urlencode "client_id=$AZURE_CLIENT_ID" \
    --data-urlencode "scope=https://codesigning.azure.net/.default" \
    --data-urlencode "grant_type=client_credentials" \
    --data-urlencode "client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer" \
    --data-urlencode "client_assertion=$assertion" | node -p 'JSON.parse(require("fs").readFileSync(0,"utf8")).access_token'
}

jar="$work/jsign.jar"
curl -fsSL -o "$jar" "https://github.com/ebourg/jsign/releases/download/$jsign_version/jsign-$jsign_version.jar"
if [ "$(sha256 "$jar" | awk '{print $1}')" != "$jsign_sha256" ]; then
  echo "::error::jsign-$jsign_version.jar does not match its pinned SHA-256"
  exit 1
fi
endpoint="${AZURE_TRUSTED_SIGNING_ENDPOINT#https://}"
access="$(token)"
echo "::add-mask::$access"
echo "signing the windows binaries with Azure Trusted Signing"
java -jar "$jar" --storetype TRUSTEDSIGNING --keystore "${endpoint%/}" \
  --storepass "$access" \
  --alias "$AZURE_TRUSTED_SIGNING_ACCOUNT/$AZURE_TRUSTED_SIGNING_PROFILE" \
  --tsaurl http://timestamp.acs.microsoft.com --tsmode RFC3161 \
  --name jerk --url https://github.com/ai-ecoverse/slicc-jerk \
  "$@"
