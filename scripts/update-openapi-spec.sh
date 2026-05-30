#!/usr/bin/env bash
#
# update-openapi-spec.sh — refresh (or check) the bundled Plex OpenAPI spec.
#
# Source of truth: the community-maintained Plex Media Server OpenAPI spec that
# powers https://plexapi.dev, published as YAML at
#   https://github.com/LukasParke/plex-api-spec  (file: plex-api-spec.yaml)
#
# We vendor it as openapi.json (sorted keys, pretty-printed) so future refreshes
# produce clean, reviewable diffs. Note: this is a *reference* artifact for
# cross-checking endpoint shapes — python-plexapi remains the behavioural spec,
# and some endpoints (e.g. the Plex Pass /statistics/* dashboard) are NOT in
# this spec at all.
#
# Usage:
#   scripts/update-openapi-spec.sh            # download + rewrite openapi.json
#   scripts/update-openapi-spec.sh --check    # report drift only (exit 1 if stale)
#   scripts/update-openapi-spec.sh --help
#
# Requires: curl, jq, and one of: python3(+pyyaml) | ruby | yq.
# (Behind a proxy, set the usual http_proxy/https_proxy env vars.)

set -euo pipefail

REPO="LukasParke/plex-api-spec"
BRANCH="main"
SPEC_PATH="plex-api-spec.yaml"
RAW_URL="https://raw.githubusercontent.com/${REPO}/${BRANCH}/${SPEC_PATH}"
API_URL="https://api.github.com/repos/${REPO}/contents/${SPEC_PATH}?ref=${BRANCH}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/openapi.json"

log()  { printf '%s\n' "$*" >&2; }
die()  { log "error: $*"; exit 2; }
need() { command -v "$1" >/dev/null 2>&1; }

usage() { sed -n '3,21p' "$0" | sed 's/^# \{0,1\}//'; }

# Emit JSON on stdout from a YAML file argument, trying converters in order of
# fidelity. python3+pyyaml and ruby ship a YAML parser in/near stdlib; yq is the
# fallback (mikefarah syntax).
yaml_to_json() {
  local f="$1"
  if need python3 && python3 -c 'import yaml' >/dev/null 2>&1; then
    python3 -c 'import sys,yaml,json; json.dump(yaml.safe_load(open(sys.argv[1])), sys.stdout)' "$f"
  elif need ruby; then
    ruby -ryaml -rjson -e \
      'f=ARGV[0]; d = YAML.respond_to?(:unsafe_load_file) ? YAML.unsafe_load_file(f) : YAML.load_file(f); print JSON.generate(d)' \
      "$f"
  elif need yq; then
    yq -o=json '.' "$f"
  else
    die "no YAML->JSON converter found; install one of: python3+pyyaml, ruby, yq"
  fi
}

mode="update"
case "${1:-}" in
  --check)      mode="check" ;;
  -h|--help)    usage; exit 0 ;;
  "")           mode="update" ;;
  *)            die "unknown argument: $1 (try --help)" ;;
esac

need curl || die "curl is required"
need jq   || die "jq is required"

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

log "→ fetching ${RAW_URL}"
curl -fsSL --retry 3 -o "${tmp}/spec.yaml" "$RAW_URL" || die "download failed"

# Canonicalise: sorted keys + pretty, so diffs across runs are minimal.
yaml_to_json "${tmp}/spec.yaml" | jq -S '.' > "${tmp}/spec.json" || die "YAML->JSON conversion failed"

new_ver="$(jq -r '.info.version // "?"'  "${tmp}/spec.json" | tr -d '[:space:]')"
new_paths="$(jq '.paths | length'        "${tmp}/spec.json")"
upstream_sha="$(curl -fsSL "$API_URL" 2>/dev/null | jq -r '.sha // empty' || true)"

if [[ -f "$OUT" ]]; then
  old_ver="$(jq -r '.info.version // "?"' "$OUT" 2>/dev/null | tr -d '[:space:]' || echo '?')"
  old_paths="$(jq '.paths | length'       "$OUT" 2>/dev/null || echo '?')"
else
  old_ver="(absent)"; old_paths="0"
fi

if [[ "$mode" == "check" ]]; then
  if [[ -f "$OUT" ]] && diff -q <(jq -S '.' "$OUT") "${tmp}/spec.json" >/dev/null 2>&1; then
    log "✓ openapi.json is up to date (upstream ${new_ver}, ${new_paths} paths)"
    exit 0
  fi
  log "✗ openapi.json is OUT OF DATE"
  log "    local:    version ${old_ver}, ${old_paths} paths"
  log "    upstream: version ${new_ver}, ${new_paths} paths${upstream_sha:+ (commit ${upstream_sha:0:12})}"
  log "  run 'scripts/update-openapi-spec.sh' to update."
  exit 1
fi

cp "${tmp}/spec.json" "$OUT"
log "✓ wrote ${OUT#"$ROOT"/}"
log "    ${old_ver} (${old_paths} paths)  ->  ${new_ver} (${new_paths} paths)"
log "    source: ${RAW_URL}${upstream_sha:+ @ ${upstream_sha:0:12}}"
