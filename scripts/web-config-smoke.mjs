import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { resolveConfig } from "../frontend/node_modules/vite/dist/node/index.js";

const names = ["MCAP_API_PROXY_TARGET", "MCAP_HOST", "MCAP_BACKEND_PORT", "MCAP_FRONTEND_PORT"];
const saved = Object.fromEntries(names.map(name => [name, process.env[name]]));
const root = fileURLToPath(new URL("../frontend", import.meta.url));
try {
  process.env.MCAP_HOST = "0.0.0.0";
  process.env.MCAP_BACKEND_PORT = "8123";
  process.env.MCAP_FRONTEND_PORT = "5175";
  process.env.MCAP_API_PROXY_TARGET = "http://backend.example.test:9000";
  const remote = await resolveConfig({ root }, "serve");
  assert.equal(remote.server.proxy["/api"].target, "http://backend.example.test:9000");
  assert.equal(remote.server.host, "0.0.0.0");
  assert.equal(remote.server.port, 5175);
  delete process.env.MCAP_API_PROXY_TARGET;
  const local = await resolveConfig({ root }, "serve");
  assert.equal(local.server.proxy["/api"].target, "http://127.0.0.1:8123");
  console.log("Passed: remote API proxy target, root .env loading, configured frontend bind/port, and loopback target for wildcard binds.");
} finally {
  for (const name of names) {
    if (saved[name] === undefined) delete process.env[name];
    else process.env[name] = saved[name];
  }
}
