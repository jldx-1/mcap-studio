import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { loadEnvFile } from "node:process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
if (existsSync(new URL("../.env", import.meta.url)))
  loadEnvFile(new URL("../.env", import.meta.url));

const tasks = {
  setup: [
    ["npm", ["--prefix", "frontend", "ci"]],
    ["cargo", ["fetch", "--manifest-path", "backend/Cargo.toml", "--locked"]],
  ],
  check: [
    [
      "cargo",
      ["fmt", "--manifest-path", "backend/Cargo.toml", "--", "--check"],
    ],
    [
      "cargo",
      [
        "clippy",
        "--manifest-path",
        "backend/Cargo.toml",
        "--locked",
        "--all-targets",
        "--",
        "-D",
        "warnings",
      ],
    ],
    ["cargo", ["test", "--manifest-path", "backend/Cargo.toml", "--locked"]],
    ["npm", ["--prefix", "frontend", "run", "check"]],
    ["npm", ["--prefix", "frontend", "run", "build"]],
  ],
  build: [
    ["npm", ["--prefix", "frontend", "run", "build"]],
    [
      "cargo",
      [
        "build",
        "--manifest-path",
        "backend/Cargo.toml",
        "--locked",
        "--release",
      ],
    ],
  ],
  start: [
    [
      "cargo",
      ["run", "--manifest-path", "backend/Cargo.toml", "--locked", "--release"],
    ],
  ],
};

const task = process.argv[2];
if (!tasks[task]) {
  console.error(`Unknown task. Choose: ${Object.keys(tasks).join(", ")}`);
  process.exit(1);
}

for (const [command, args] of tasks[task]) {
  console.log(`\n> ${command} ${args.join(" ")}`);
  const child = spawn(command, args, {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
  const result = await new Promise((resolve) => {
    child.once("error", (error) => {
      console.error(error.message);
      resolve(1);
    });
    child.once("exit", (code, signal) => resolve(code ?? (signal ? 1 : 0)));
  });
  if (result !== 0) process.exit(result);
}
