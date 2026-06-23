#!/usr/bin/env node
// Bump the app version. release.json is the single source of truth; this writes
// the new number there AND propagates it to the files the build actually reads
// (package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml + Cargo.lock)
// so they can't drift apart.
//
// It does NOT tag, push, or publish. A release is cut when the release.json
// version CHANGE lands on `main` — see .github/workflows/cut-release.yml. So the
// flow is: bump here → review the diff → PR → merge to main → CI cuts vX.Y.Z.
//
//   pnpm release patch              # 0.1.0 -> 0.1.1
//   pnpm release minor              # 0.1.0 -> 0.2.0
//   pnpm release major              # 0.1.0 -> 1.0.0
//   pnpm release minor --dry-run    # show the bump, write nothing
//   pnpm release patch --commit     # also create the bump commit (no push)
//
// (--patch/--minor/--major also work.)
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const releasePath = join(root, "release.json");
const pkgPath = join(root, "package.json");
const confPath = join(root, "src-tauri", "tauri.conf.json");
const cargoPath = join(root, "src-tauri", "Cargo.toml");
const lockPath = join(root, "src-tauri", "Cargo.lock");

function fail(msg) {
  console.error(`\x1b[31m✗ ${msg}\x1b[0m`);
  process.exit(1);
}

function git(args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
}

// --- args ---------------------------------------------------------------
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
// Auto-commit the bump. Implied under CI — every CI provider sets `CI`, so a
// scripted bump stays non-interactive there.
const commit = args.includes("--commit") || Boolean(process.env.CI);
const kinds = args
  .map((a) => a.replace(/^--/, ""))
  .filter((a) => ["patch", "minor", "major"].includes(a));
if (kinds.length !== 1) {
  fail("usage: pnpm release <patch|minor|major> [--dry-run] [--commit]");
}
const kind = kinds[0];

// --- compute next version (release.json is the source of truth) ---------
const release = JSON.parse(readFileSync(releasePath, "utf8"));
const current = release.version;
const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(current ?? "");
if (!m) {
  fail(`release.json version "${current}" is not plain semver X.Y.Z`);
}
let [maj, min, pat] = m.slice(1).map(Number);
if (kind === "major") {
  [maj, min, pat] = [maj + 1, 0, 0];
} else if (kind === "minor") {
  [min, pat] = [min + 1, 0];
} else {
  pat += 1;
}
const next = `${maj}.${min}.${pat}`;
const tag = `v${next}`;

console.log(`\x1b[36m${current} → ${next}  (${kind})\x1b[0m`);
if (dryRun) {
  console.log("--dry-run: nothing written.");
  process.exit(0);
}

// --- write release.json (source) + the 4 files the build reads ----------
release.version = next;
// Clear the previous release's fallback notes so they can't ship verbatim under
// the new version (v0.3.0 published with 0.2.0's notes this way). The real
// release body now lives in changelog/v<next>.md (see changelog/README.md);
// release.json.notes is only a fallback for an un-migrated bump.
release.notes = "";
writeFileSync(releasePath, JSON.stringify(release, null, 2) + "\n");

const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = next;
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

const conf = JSON.parse(readFileSync(confPath, "utf8"));
conf.version = next;
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");

// Cargo.toml: only the version under [package], not any dependency version.
writeFileSync(
  cargoPath,
  readFileSync(cargoPath, "utf8").replace(
    /(\[package\][\s\S]*?\nversion = ")[^"]+(")/,
    `$1${next}$2`,
  ),
);

// Cargo.lock: the en-tu-cara package entry must match.
writeFileSync(
  lockPath,
  readFileSync(lockPath, "utf8").replace(
    /(name = "en-tu-cara"\nversion = ")[^"]+(")/,
    `$1${next}$2`,
  ),
);

// --- done ---------------------------------------------------------------
const files = [releasePath, pkgPath, confPath, cargoPath, lockPath];
if (commit) {
  git(["add", ...files]);
  git(["commit", "-m", `chore(release): ${tag}`]);
  console.log(`\x1b[32m✓ committed ${tag}.\x1b[0m Open a PR; merging to main cuts the release.`);
} else {
  console.log(`\x1b[32m✓ bumped to ${next}.\x1b[0m Review the diff, then:`);
  console.log(`  git commit -am "chore(release): ${tag}"   # PR + merge to main → CI cuts ${tag}`);
}
console.log(
  `\x1b[33m⚠ write ${tag}'s release notes in changelog/v${next}.md before merging — that file's body is what CI publishes (see changelog/README.md). Missing/empty → a generic body.\x1b[0m`,
);
