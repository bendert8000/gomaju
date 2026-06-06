#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const paths = {
  package: path.join(root, "package.json"),
  tauri: path.join(root, "src-tauri", "tauri.conf.json"),
  cargo: path.join(root, "src-tauri", "Cargo.toml"),
};

const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function readCargoPackageVersion() {
  const text = fs.readFileSync(paths.cargo, "utf8");
  const match = text.match(/(^\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m);
  if (!match) throw new Error("Could not find [package] version in src-tauri/Cargo.toml");
  return match[2];
}

function writeCargoPackageVersion(version) {
  const text = fs.readFileSync(paths.cargo, "utf8");
  const next = text.replace(
    /(^\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
    (_match, prefix, _oldVersion, suffix) => `${prefix}${version}${suffix}`,
  );
  if (next === text) throw new Error("Could not update [package] version in src-tauri/Cargo.toml");
  fs.writeFileSync(paths.cargo, next);
}

function versions() {
  return {
    package: readJson(paths.package).version,
    tauri: readJson(paths.tauri).version,
    cargo: readCargoPackageVersion(),
  };
}

function assertValid(version) {
  if (!SEMVER.test(version)) {
    throw new Error(`Invalid version '${version}'. Expected semver like 1.2.3 or 1.2.3-beta.1`);
  }
}

function sync(version) {
  assertValid(version);

  const pkg = readJson(paths.package);
  pkg.version = version;
  writeJson(paths.package, pkg);

  const tauri = readJson(paths.tauri);
  tauri.version = version;
  writeJson(paths.tauri, tauri);

  writeCargoPackageVersion(version);
}

function check() {
  const current = versions();
  assertValid(current.package);
  const mismatches = Object.entries(current).filter(([, version]) => version !== current.package);
  if (mismatches.length === 0) {
    console.log(`version ok: ${current.package}`);
    return;
  }
  const details = Object.entries(current)
    .map(([name, version]) => `  ${name}: ${version}`)
    .join("\n");
  throw new Error(`Version mismatch. package.json is canonical:\n${details}`);
}

function usage() {
  console.error("Usage: node scripts/version.mjs --check | --sync | --set <version>");
}

try {
  const [cmd, value] = process.argv.slice(2);
  if (cmd === "--check") {
    check();
  } else if (cmd === "--sync") {
    sync(versions().package);
    check();
  } else if (cmd === "--set" && value) {
    sync(value);
    check();
  } else {
    usage();
    process.exitCode = 2;
  }
} catch (err) {
  console.error(err instanceof Error ? err.message : String(err));
  process.exitCode = 1;
}
