# Load Testing

## Overview

The repository now includes lightweight transport-level load scripts under `tools/load-tests/`:

- `me_load.js`: repeated concurrent client connections against the public proxy port, with optional payload write and short hold time.
- `direct_load.js`: repeated concurrent client connections that intentionally keep sockets idle for longer, useful for stress around direct-relay accept and timeout paths.
- `run-me-load.sh`: shell wrapper for `me_load.js`.
- `run-direct-load.sh`: shell wrapper for `direct_load.js`.

These scripts are designed to be dependency-free and interactive. They print live progress every second and a JSON summary at the end.
They can also fail the process when thresholds are exceeded via `--max-failures`, `--max-timeouts`, and `--min-success`.

## Important limitation

These scripts are **transport-level stress tools**, not full MTProto client emulators. They measure:

- TCP accept/connect success rate
- connection setup latency
- socket timeout/error rate
- stability under many short-lived or idle client connections

For true end-to-end MTProto protocol benchmarking, use a dedicated Telegram client simulator or capture/replay tool in addition to these scripts.

## Preparing a local stand

If you want to test against the local Docker stack:

```bash
docker-compose up --build -d
```

Default exposed ports from `docker-compose.yml`:

- `443`: telemt public listener
- `9090`: metrics endpoint

## Running the scenarios

### ME / public listener load

```bash
chmod +x tools/load-tests/run-me-load.sh
./tools/load-tests/run-me-load.sh 20 15 250
```

Arguments:

- `20`: concurrent clients
- `15`: duration in seconds
- `250`: hold time per successful connection in milliseconds

Environment overrides:

- `HOST=127.0.0.1`
- `PORT=443`
- `BOOTSTRAP_DOCKER=1` to run `docker-compose up --build -d` before the scenario

Direct script form:

```bash
node tools/load-tests/me_load.js --host 127.0.0.1 --port 443 --clients 20 --duration 15 --hold-ms 250
```

Threshold-gated example:

```bash
node tools/load-tests/me_load.js --host 127.0.0.1 --port 443 --clients 20 --duration 15 --hold-ms 250 --max-failures 0 --max-timeouts 0 --min-success 1
```

### Direct relay / idle-pressure load

```bash
chmod +x tools/load-tests/run-direct-load.sh
./tools/load-tests/run-direct-load.sh 20 15 5000
```

Arguments:

- `20`: concurrent clients
- `15`: duration in seconds
- `5000`: idle socket hold time in milliseconds

Direct script form:

```bash
node tools/load-tests/direct_load.js --host 127.0.0.1 --port 443 --clients 20 --duration 15 --stall-ms 5000
```

Threshold-gated example:

```bash
node tools/load-tests/direct_load.js --host 127.0.0.1 --port 443 --clients 20 --duration 15 --stall-ms 5000 --max-failures 0 --max-timeouts 0 --min-success 1
```

## What to watch during a run

- Console progress from the script:
  - attempts
  - successes
  - failures
  - timeouts
  - in-flight connections
  - average and p95 connect latency
- `docker-compose logs -f telemt`
- `curl http://127.0.0.1:9090/metrics`
- CPU, memory and open file descriptors on the host/container

## Success criteria

A test run is considered healthy when:

- the proxy process stays up for the whole run
- error and timeout rates stay within expected limits for the scenario
- connect latency does not grow without bound during the run
- there is no reconnect storm or obvious resource exhaustion in logs

## Quick verification commands

```bash
node tools/load-tests/me_load.js --host 127.0.0.1 --port 443 --clients 2 --duration 3 --hold-ms 100
node tools/load-tests/direct_load.js --host 127.0.0.1 --port 443 --clients 2 --duration 3 --stall-ms 1000
```
