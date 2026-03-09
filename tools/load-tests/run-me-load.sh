#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-443}"
CLIENTS="${1:-10}"
DURATION="${2:-15}"
HOLD_MS="${3:-250}"
MAX_FAILURES="${MAX_FAILURES:-}"
MAX_TIMEOUTS="${MAX_TIMEOUTS:-}"
MIN_SUCCESS="${MIN_SUCCESS:-}"

if [[ "${BOOTSTRAP_DOCKER:-0}" == "1" ]]; then
  echo "step: bootstrapping docker stack with docker-compose up --build -d"
  (cd "$ROOT_DIR" && docker-compose up --build -d)
fi

echo "step: running ME transport load clients=${CLIENTS} duration=${DURATION}s hold=${HOLD_MS}ms host=${HOST} port=${PORT}"
cmd=(node "$ROOT_DIR/tools/load-tests/me_load.js" \
  --host "$HOST" \
  --port "$PORT" \
  --clients "$CLIENTS" \
  --duration "$DURATION" \
  --hold-ms "$HOLD_MS")

if [[ -n "$MAX_FAILURES" ]]; then
  cmd+=(--max-failures "$MAX_FAILURES")
fi
if [[ -n "$MAX_TIMEOUTS" ]]; then
  cmd+=(--max-timeouts "$MAX_TIMEOUTS")
fi
if [[ -n "$MIN_SUCCESS" ]]; then
  cmd+=(--min-success "$MIN_SUCCESS")
fi

"${cmd[@]}"
