#!/usr/bin/env node
// Print the release body for the version currently in release.json: the body of
// changelog/v<version>.md with its frontmatter stripped, or release.json.notes
// as a fallback when that file is absent. cut-release.yml pipes this into the
// GitHub release / latest.json notes.
//
// This lives in a file rather than an inline `node -e` in the workflow on
// purpose: embedding a multi-line script (with prose comments and apostrophes)
// inside a YAML shell heredoc is a quoting minefield — an apostrophe in a comment
// silently closed the single-quoted string and broke the gate. A real file has
// no shell-quoting surface and can be tested. See changelog/README.md.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const release = JSON.parse(readFileSync(join(root, "release.json"), "utf8"));

let body = "";
try {
  const raw = readFileSync(join(root, "changelog", `v${release.version}.md`), "utf8").replace(
    /\r\n/g,
    "\n",
  );
  const frontmatter = /^---\n[\s\S]*?\n---\n?/.exec(raw);
  body = (frontmatter ? raw.slice(frontmatter[0].length) : raw).trim();
} catch {
  // changelog file missing — fall back to release.json notes below.
}
if (!body) {
  body = release.notes || "";
}

// console.log appends the trailing newline the GITHUB_OUTPUT heredoc needs: the
// closing delimiter must sit on its own line.
console.log(body);
