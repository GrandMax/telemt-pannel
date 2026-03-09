#!/usr/bin/env node

const net = require("node:net");

function parseArgs(argv) {
  const options = {
    host: "127.0.0.1",
    port: 443,
    clients: 10,
    duration: 15,
    connectTimeoutMs: 5000,
    holdMs: 250,
    payloadHex: "",
    maxFailures: Number.POSITIVE_INFINITY,
    maxTimeouts: Number.POSITIVE_INFINITY,
    minSuccess: 0,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = argv[i + 1];
    switch (arg) {
      case "--host":
        options.host = next;
        i += 1;
        break;
      case "--port":
        options.port = Number(next);
        i += 1;
        break;
      case "--clients":
        options.clients = Number(next);
        i += 1;
        break;
      case "--duration":
        options.duration = Number(next);
        i += 1;
        break;
      case "--connect-timeout-ms":
        options.connectTimeoutMs = Number(next);
        i += 1;
        break;
      case "--hold-ms":
        options.holdMs = Number(next);
        i += 1;
        break;
      case "--payload-hex":
        options.payloadHex = next;
        i += 1;
        break;
      case "--max-failures":
        options.maxFailures = Number(next);
        i += 1;
        break;
      case "--max-timeouts":
        options.maxTimeouts = Number(next);
        i += 1;
        break;
      case "--min-success":
        options.minSuccess = Number(next);
        i += 1;
        break;
      case "--help":
        console.log("Usage: node me_load.js [--host 127.0.0.1] [--port 443] [--clients 10] [--duration 15] [--connect-timeout-ms 5000] [--hold-ms 250] [--payload-hex deadbeef] [--max-failures 0] [--max-timeouts 0] [--min-success 1]");
        process.exit(0);
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return options;
}

function percentile(sortedValues, p) {
  if (sortedValues.length === 0) return 0;
  const index = Math.min(sortedValues.length - 1, Math.floor(sortedValues.length * p));
  return sortedValues[index];
}

async function oneAttempt(options, stats) {
  stats.attempts += 1;
  stats.inFlight += 1;

  return new Promise((resolve) => {
    const startedAt = performance.now();
    const socket = net.createConnection({ host: options.host, port: options.port });
    let settled = false;
    let connected = false;

    const finish = (kind, message) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      socket.destroy();
      const elapsed = performance.now() - startedAt;
      stats.inFlight -= 1;
      if (kind === "success") {
        stats.success += 1;
        stats.latencies.push(elapsed);
      } else if (kind === "timeout") {
        stats.timeouts += 1;
        stats.errors.push(`timeout:${message}`);
      } else {
        stats.failures += 1;
        stats.errors.push(message);
      }
      resolve();
    };

    const timeout = setTimeout(() => finish("timeout", `${options.host}:${options.port}`), options.connectTimeoutMs);

    socket.on("connect", () => {
      connected = true;
      if (options.payloadHex) {
        try {
          socket.write(Buffer.from(options.payloadHex, "hex"));
        } catch (error) {
          finish("error", `invalid-payload:${error.message}`);
          return;
        }
      }
      setTimeout(() => finish("success", "connected"), options.holdMs);
    });

    socket.on("error", (error) => {
      finish("error", connected ? `socket:${error.message}` : `connect:${error.message}`);
    });
  });
}

async function worker(workerId, options, stats, deadlineMs) {
  while (Date.now() < deadlineMs) {
    await oneAttempt(options, stats);
  }
  console.log(`worker ${workerId}: completed`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const stats = {
    attempts: 0,
    success: 0,
    failures: 0,
    timeouts: 0,
    inFlight: 0,
    latencies: [],
    errors: [],
  };
  const deadlineMs = Date.now() + options.duration * 1000;

  console.log(`step: ME load start host=${options.host} port=${options.port} clients=${options.clients} duration=${options.duration}s hold=${options.holdMs}ms`);

  const progressTimer = setInterval(() => {
    const sorted = [...stats.latencies].sort((a, b) => a - b);
    const avg = sorted.length ? sorted.reduce((sum, value) => sum + value, 0) / sorted.length : 0;
    console.log(
      `progress: attempts=${stats.attempts} success=${stats.success} failures=${stats.failures} timeouts=${stats.timeouts} in_flight=${stats.inFlight} avg_connect_ms=${avg.toFixed(1)} p95_ms=${percentile(sorted, 0.95).toFixed(1)}`
    );
  }, 1000);

  await Promise.all(
    Array.from({ length: options.clients }, (_, index) => worker(index + 1, options, stats, deadlineMs))
  );

  clearInterval(progressTimer);

  const sorted = [...stats.latencies].sort((a, b) => a - b);
  const avg = sorted.length ? sorted.reduce((sum, value) => sum + value, 0) / sorted.length : 0;
  const summary = {
    attempts: stats.attempts,
    success: stats.success,
    failures: stats.failures,
    timeouts: stats.timeouts,
    avg_connect_ms: Number(avg.toFixed(2)),
    p50_connect_ms: Number(percentile(sorted, 0.5).toFixed(2)),
    p95_connect_ms: Number(percentile(sorted, 0.95).toFixed(2)),
    error_samples: stats.errors.slice(0, 10),
  };

  console.log("summary:");
  console.log(JSON.stringify(summary, null, 2));

  if (
    stats.failures > options.maxFailures ||
    stats.timeouts > options.maxTimeouts ||
    stats.success < options.minSuccess
  ) {
    console.error(
      `threshold-check failed: success=${stats.success} failures=${stats.failures} timeouts=${stats.timeouts}`
    );
    process.exit(1);
  }
}

main().catch((error) => {
  console.error(`fatal: ${error.message}`);
  process.exit(1);
});
