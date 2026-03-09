#!/usr/bin/env bash

set -euo pipefail

PANEL_URL="${PANEL_URL:-http://localhost:8080}"
PANEL_TOKEN="${PANEL_TOKEN:-}"

usage() {
  cat <<'EOF'
Usage:
  PANEL_URL=http://localhost:8080 PANEL_TOKEN=... ./scripts/export-import.sh export
  PANEL_URL=http://localhost:8080 PANEL_TOKEN=... ./scripts/export-import.sh import <snapshot.json> [merge|replace]
EOF
}

require_token() {
  if [[ -z "$PANEL_TOKEN" ]]; then
    echo "error: PANEL_TOKEN is required" >&2
    exit 1
  fi
}

command_name="${1:-}"
if [[ -z "$command_name" ]]; then
  usage
  exit 1
fi
shift || true

case "$command_name" in
  export)
    require_token
    echo "step: requesting export snapshot"
    curl -fsS \
      -H "Authorization: Bearer ${PANEL_TOKEN}" \
      "${PANEL_URL%/}/api/admin/export"
    echo
    ;;
  import)
    require_token
    snapshot_path="${1:-}"
    mode="${2:-merge}"
    if [[ -z "$snapshot_path" ]]; then
      echo "error: snapshot path is required" >&2
      usage
      exit 1
    fi
    if [[ "$mode" != "merge" && "$mode" != "replace" ]]; then
      echo "error: mode must be merge or replace" >&2
      exit 1
    fi
    echo "step: importing snapshot path=${snapshot_path} mode=${mode}"
    curl -fsS \
      -X POST \
      -H "Authorization: Bearer ${PANEL_TOKEN}" \
      -H "Content-Type: application/json" \
      --data-binary @"${snapshot_path}" \
      "${PANEL_URL%/}/api/admin/import?mode=${mode}"
    echo
    ;;
  *)
    echo "error: unknown command: $command_name" >&2
    usage
    exit 1
    ;;
esac
