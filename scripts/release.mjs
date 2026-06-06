#!/usr/bin/env node
// Cut a release. Bumps the version in the three files that must agree
// (package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml + Cargo.lock),
// commits, tags `vX.Y.Z`, and pushes the tag. The pushed tag triggers
// .github/workflows/release.yml, which builds the macOS bundle, signs the
// updater artifacts, and publishes the GitHub Release. See docs/RELEASING.md.
//
//   pnpm release patch      # 0.1.0 -> 0.1.1
//   pnpm release minor      # 0.1.0 -> 0.2.0
//   pnpm release major      # 0.1.0 -> 1.0.0
//   pnpm release minor --dry-run   # show what would happen, change nothing
//
// (--patch/--minor/--major also work; pnpm forwards either form.)
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pkgPath = join(root, "package.json");
const confPath = join(root, "src-tauri", "tauri.conf.json");
const cargoPath = join(root, "src-tauri", "Cargo.toml");
const lockPath = join(root, "src-tauri", "Cargo.lock");

const RELEASE_BRANCH = "main";

function fail(msg) {
  console.error(`\x1b[31m✗ ${msg}\x1b[0m`);
  process.exit(1);
}

function git(args, opts = {}) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", ...opts }).trim();
}

// --- parse args ---------------------------------------------------------
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const kinds = args
  .map((a) => a.replace(/^--/, ""))
  .filter((a) => ["patch", "minor", "major"].includes(a));
if (kinds.length !== 1) {
  fail("usage: pnpm release <patch|minor|major> [--dry-run]");
}
const kind = kinds[0];

// --- preflight ----------------------------------------------------------
const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
if (branch !== RELEASE_BRANCH) {
  fail(`on branch "${branch}", but releases must be cut from "${RELEASE_BRANCH}".`);
}
if (git(["status", "--porcelain"]) !== "") {
  fail("working tree is dirty. Commit or stash before releasing.");
}
git(["fetch", "--tags", "origin", RELEASE_BRANCH], { stdio: "ignore" });
const local = git(["rev-parse", "HEAD"]);
const remote = git(["rev-parse", `origin/${RELEASE_BRANCH}`]);
if (local !== remote) {
  fail(`local ${RELEASE_BRANCH} is not in sync with origin. Pull/push first.`);
}

// --- compute next version ----------------------------------------------
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const current = pkg.version;
const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
if (!m) fail(`current version "${current}" is not plain semver X.Y.Z`);
let [maj, min, pat] = m.slice(1).map(Number);
if (kind === "major") [maj, min, pat] = [maj + 1, 0, 0];
else if (kind === "minor") [min, pat] = [min + 1, 0];
else pat += 1;
const next = `${maj}.${min}.${pat}`;
const tag = `v${next}`;

if (git(["tag", "--list", tag]) === tag) {
  fail(`tag ${tag} already exists.`);
}

console.log(`\x1b[36m${current} → ${next}  (${kind})\x1b[0m`);
if (dryRun) {
  console.log("--dry-run: no files changed, nothing committed or pushed.");
  process.exit(0);
}

// --- write the four files ----------------------------------------------
// package.json + tauri.conf.json: parse, set, re-serialize (2-space + newline).
pkg.version = next;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

const conf = JSON.parse(readFileSync(confPath, "utf8"));
conf.version = next;
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");

// Cargo.toml: only the version under [package], not any dependency version.
const cargo = readFileSync(cargoPath, "utf8").replace(
  /(\[package\][\s\S]*?\nversion = ")[^"]+(")/,
  `$1${next}$2`,
);
writeFileSync(cargoPath, cargo);

// Cargo.lock: the en-tu-cara package entry must match (avoids a dirty lock on
// the next cargo build / CI cache miss).
const lock = readFileSync(lockPath, "utf8").replace(
  /(name = "en-tu-cara"\nversion = ")[^"]+(")/,
  `$1${next}$2`,
);
writeFileSync(lockPath, lock);

// --- commit, tag, push --------------------------------------------------
git(["add", pkgPath, confPath, cargoPath, lockPath]);
git(["commit", "-m", `chore(release): ${tag}`]);
git(["tag", "-a", tag, "-m", `En Tu Cara ${tag}`]);
git(["push", "--follow-tags", "origin", RELEASE_BRANCH]);

const url = "https://github.com/fforres/en-tu-cara/actions";
console.log(`\x1b[32m✓ pushed ${tag}.\x1b[0m CI is building the release now:`);
console.log(`  ${url}`);
console.log(`  release will appear at https://github.com/fforres/en-tu-cara/releases/tag/${tag}`);
