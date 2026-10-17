import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:net";
import { loadEnvFile } from "node:process";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const root = fileURLToPath(new URL("../", import.meta.url));
if (existsSync(new URL("../.env", import.meta.url)))
  loadEnvFile(new URL("../.env", import.meta.url));
const windows = process.platform === "win32";
const host = process.env.MCAP_HOST || "127.0.0.1";

function port(value, fallback, name) {
  const parsed = Number(value || fallback);
  if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535)
    throw new Error(`${name} must be a port from 1 to 65535.`);
  return parsed;
}

function available(portNumber) {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once("error", () =>
      reject(
        new Error(
          `Port ${portNumber} on ${host} is already in use. Set a different port in .env.`,
        ),
      ),
    );
    server.listen(portNumber, host, () => server.close(resolve));
  });
}

const children = [];
let stopping = false;
function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  for (const child of children) {
    if (!child.pid) continue;
    if (windows) {
      spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
      });
    } else {
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {
        /* The process may already have stopped. */
      }
    }
  }
  // Give servers a moment to close files; force-stop surviving process groups afterwards.
  setTimeout(() => {
    if (!windows)
      for (const child of children) {
        try {
          process.kill(-child.pid, "SIGKILL");
        } catch {
          /* Already stopped. */
        }
      }
    process.exit(code);
  }, 750);
}

function launch(command, args, name, env) {
  const child = spawn(command, args, {
    cwd: root,
    stdio: "inherit",
    detached: !windows,
    shell: windows,
    env,
  });
  children.push(child);
  child.once("error", (error) => {
    console.error(`${name}: ${error.message}`);
    stop(1);
  });
  child.once("exit", (code, signal) => {
    if (!stopping) {
      console.error(
        `${name} stopped (${signal || code}). Closing both services.`,
      );
      stop(code || 1);
    }
  });
}

async function waitForBackend(portNumber) {
  const probeHost = host === "0.0.0.0" ? "127.0.0.1" : host === "::" ? "::1" : host;
  const address = probeHost.includes(":") ? `[${probeHost}]` : probeHost;
  const url = `http://${address}:${portNumber}/api/health`;
  while (!stopping) {
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(2000) });
      await response.body?.cancel();
      if (response.ok) return;
    } catch {
      // Cargo may still be compiling, or the backend may still be starting.
    }
    await delay(250);
  }
}

try {
  const backend = port(
    process.env.MCAP_BACKEND_PORT,
    8000,
    "MCAP_BACKEND_PORT",
  );
  const frontend = port(
    process.env.MCAP_FRONTEND_PORT,
    5173,
    "MCAP_FRONTEND_PORT",
  );
  if (frontend === backend)
    throw new Error("Frontend and backend ports must be different.");
  if (!existsSync(new URL("../frontend/node_modules", import.meta.url)))
    throw new Error(
      "Frontend dependencies are missing. Run npm run setup first.",
    );
  if (spawnSync("cargo", ["--version"], { stdio: "ignore" }).status !== 0)
    throw new Error(
      "Rust/Cargo is required. Install Rust with rustup, then run npm run setup.",
    );
  await Promise.all([available(backend), available(frontend)]);
  const env = {
    ...process.env,
    MCAP_HOST: host,
    MCAP_BACKEND_PORT: String(backend),
  };
  console.log(
    `Starting the Rust backend first.\nRust API → http://${host}:${backend}/api/health\nThe first backend build can take a few minutes. Ctrl+C stops both services.\n`,
  );
  process.once("SIGINT", () => stop(130));
  process.once("SIGTERM", () => stop(143));
  launch(
    "cargo",
    ["run", "--manifest-path", "backend/Cargo.toml", "--locked", "--release"],
    "Rust backend",
    env,
  );
  await waitForBackend(backend);
  if (!stopping) {
    console.log(`Backend ready. Starting the frontend → http://${host}:${frontend}\n`);
    launch(
      "npm",
      [
        "--prefix",
        "frontend",
        "run",
        "dev",
        "--",
        "--host",
        host,
        "--port",
        String(frontend),
        "--strictPort",
      ],
      "React frontend",
      env,
    );
  }
} catch (error) {
  console.error(error.message);
  stop(1);
}
