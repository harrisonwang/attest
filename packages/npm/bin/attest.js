#!/usr/bin/env node

const path = require("node:path");
const { spawnSync } = require("node:child_process");

const executable = process.env.ATTEST_BINARY || path.join(__dirname, "..", "vendor", process.platform === "win32" ? "attest.exe" : "attest");
const result = spawnSync(executable, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(`unable to launch attest: ${result.error.message}`);
  process.exit(2);
}
process.exit(result.status ?? 2);
