#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const version = require("./package.json").version;
const targets = require("./platforms.json");
const baseUrl = `https://github.com/harrisonwang/attest/releases/download/v${version}`;

function resolveArtifact(platform = process.platform, arch = process.arch) {
  const artifact = targets[`${platform}-${arch}`];
  if (!artifact) {
    throw new Error(`attest does not provide a binary for ${platform}/${arch}`);
  }
  return artifact;
}

function parseChecksum(contents) {
  const checksum = contents.toString("utf8").trim().split(/\s+/)[0];
  if (!/^[0-9a-fA-F]{64}$/.test(checksum)) {
    throw new Error("release checksum is not a SHA-256 digest");
  }
  return checksum.toLowerCase();
}

function download(url, redirects = 0) {
  if (redirects > 5) return Promise.reject(new Error(`too many redirects: ${url}`));
  return new Promise((resolve, reject) => {
    const request = https.get(url, (response) => {
      if ([301, 302, 307, 308].includes(response.statusCode)) {
        const location = response.headers.location;
        response.resume();
        if (!location) return reject(new Error(`redirect missing location: ${url}`));
        return resolve(download(new URL(location, url).toString(), redirects + 1));
      }
      if (response.statusCode !== 200) {
        response.resume();
        return reject(new Error(`download failed (${response.statusCode}): ${url}`));
      }
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve(Buffer.concat(chunks)));
      response.on("error", reject);
    });
    request.setTimeout(30_000, () => {
      request.destroy(new Error(`download timed out: ${url}`));
    });
    request.on("error", reject);
  });
}

async function install() {
  if (process.env.ATTEST_BINARY) return;
  const artifact = resolveArtifact();
  const vendor = path.join(__dirname, "vendor");
  const archive = path.join(os.tmpdir(), `${artifact}-${process.pid}`);
  const executable = path.join(vendor, process.platform === "win32" ? "attest.exe" : "attest");
  fs.mkdirSync(vendor, { recursive: true });
  const [contents, checksumFile] = await Promise.all([
    download(`${baseUrl}/${artifact}`),
    download(`${baseUrl}/${artifact}.sha256`),
  ]);
  const expected = parseChecksum(checksumFile);
  const actual = crypto.createHash("sha256").update(contents).digest("hex");
  if (actual !== expected) throw new Error(`checksum mismatch for ${artifact}`);
  try {
    fs.rmSync(executable, { force: true });
    fs.writeFileSync(archive, contents);
    if (process.platform === "win32") {
      execFileSync("powershell", [
        "-NoProfile",
        "-Command",
        "Expand-Archive -Force -LiteralPath $args[0] -DestinationPath $args[1]",
        archive,
        vendor,
      ]);
    } else {
      execFileSync("tar", ["-xzf", archive, "-C", vendor]);
      fs.chmodSync(executable, 0o755);
    }
    if (!fs.statSync(executable).isFile()) {
      throw new Error(`archive did not contain ${path.basename(executable)}`);
    }
  } finally {
    fs.rmSync(archive, { force: true });
  }
}

module.exports = { install, parseChecksum, resolveArtifact };

if (require.main === module) {
  install().catch((error) => {
    console.error(`attest install failed: ${error.message}`);
    process.exit(1);
  });
}
