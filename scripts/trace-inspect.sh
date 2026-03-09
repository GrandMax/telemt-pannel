#!/usr/bin/env bash

set -euo pipefail

PANEL_URL="${PANEL_URL:-http://localhost:8080}"
PANEL_TOKEN="${PANEL_TOKEN:-}"

usage() {
  cat <<'EOF'
Usage:
  PANEL_URL=http://localhost:8080 PANEL_TOKEN=... ./scripts/trace-inspect.sh sessions [user] [dc]
  PANEL_URL=http://localhost:8080 PANEL_TOKEN=... ./scripts/trace-inspect.sh show <conn_id> [limit]
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
  sessions)
    require_token
    user_filter="${1:-}"
    dc_filter="${2:-}"
    query="limit=50"
    if [[ -n "$user_filter" ]]; then
      query="${query}&user=${user_filter}"
    fi
    if [[ -n "$dc_filter" ]]; then
      query="${query}&dc=${dc_filter}"
    fi
    echo "step: requesting trace sessions"
    curl -fsS \
      -H "Authorization: Bearer ${PANEL_TOKEN}" \
      "${PANEL_URL%/}/api/admin/trace/sessions?${query}"
    echo
    ;;
  show)
    require_token
    conn_id="${1:-}"
    limit="${2:-200}"
    if [[ -z "$conn_id" ]]; then
      echo "error: conn_id is required" >&2
      usage
      exit 1
    fi
    echo "step: requesting trace detail for conn_id=${conn_id}"
    curl -fsS \
      -H "Authorization: Bearer ${PANEL_TOKEN}" \
      "${PANEL_URL%/}/api/admin/trace/${conn_id}?limit=${limit}"
    echo
    ;;
  *)
    echo "error: unknown command: $command_name" >&2
    usage
    exit 1
    ;;
esac
