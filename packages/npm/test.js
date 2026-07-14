#!/usr/bin/env node

const assert = require("node:assert/strict");
const { parseChecksum, resolveArtifact } = require("./install.js");

assert.equal(resolveArtifact("darwin", "arm64"), "attest-macos-aarch64.tar.gz");
assert.equal(resolveArtifact("darwin", "x64"), "attest-macos-x86_64.tar.gz");
assert.equal(resolveArtifact("linux", "arm64"), "attest-linux-aarch64.tar.gz");
assert.equal(resolveArtifact("linux", "x64"), "attest-linux-x86_64.tar.gz");
assert.equal(resolveArtifact("win32", "arm64"), "attest-windows-aarch64.zip");
assert.equal(resolveArtifact("win32", "x64"), "attest-windows-x86_64.zip");
assert.throws(() => resolveArtifact("freebsd", "x64"), /does not provide a binary/);

const digest = "a".repeat(64);
assert.equal(parseChecksum(Buffer.from(`${digest}  attest.tar.gz\n`)), digest);
assert.equal(parseChecksum(Buffer.from(`${digest.toUpperCase()} *attest.zip\n`)), digest);
assert.throws(() => parseChecksum(Buffer.from("not-a-digest\n")), /SHA-256/);
