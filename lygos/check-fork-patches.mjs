// Fails unless lygos/FORK-PATCHES.json lists, in order, every commit on top
// of the pinned upstream base, each with its exact files and subject.
// Commits that change only lygos/FORK-PATCHES.json (manifest updates) are not
// listed. Upstream-candidate commits may touch only the upstream Bitcoin crate
// and its doc page.
// Usage: node lygos/check-fork-patches.mjs
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const git = (...args) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" }).trim();
const MANIFEST = "lygos/FORK-PATCHES.json";
const UPSTREAM_PATHS = ["src/chain_parsers/visualsign-bitcoin/", "docs/chains/bitcoin.mdx"];

let failures = 0;
const fail = (msg) => { failures++; console.log("FAIL", msg); };

const m = JSON.parse(readFileSync(path.join(repo, MANIFEST), "utf8"));
if (m.version !== 1) fail("version must be 1");
const up = m.upstream ?? {};
if (up.repository !== "https://github.com/anchorageoss/visualsign-parser") fail(`upstream.repository is ${up.repository}`);
if (!/^[0-9a-f]{40}$/.test(up.baseCommit ?? "")) fail("upstream.baseCommit must be a full sha");
else {
  if (git("rev-parse", up.baseCommit + "^{commit}") !== up.baseCommit) fail("base commit not found");
  try { git("merge-base", "--is-ancestor", up.baseCommit, "HEAD"); } catch { fail("base commit is not an ancestor of HEAD"); }
  if (git("log", "-1", "--format=%s", up.baseCommit) !== up.baseSubject) fail("upstream.baseSubject differs");
}

const files = (sha) => git("show", "--format=", "--name-only", sha).split("\n").filter(Boolean).sort();
const commits = git("log", "--reverse", "--format=%H", `${up.baseCommit}..HEAD`).split("\n").filter(Boolean);
const listed = commits.filter((sha) => JSON.stringify(files(sha)) !== JSON.stringify([MANIFEST]));
const patches = m.patches ?? [];
if (JSON.stringify(patches.map((p) => p.commit)) !== JSON.stringify(listed)) {
  fail(`manifest lists ${patches.length} commits, git has ${listed.length} on top of the base (manifest-only commits excluded)`);
}
for (const p of patches) {
  const tag = (p.commit ?? "?").slice(0, 7);
  if (!listed.includes(p.commit)) { fail(`${tag} is not on top of the base`); continue; }
  if (git("log", "-1", "--format=%s", p.commit) !== p.subject) fail(`${tag} subject differs`);
  if (JSON.stringify([...(p.files ?? [])].sort()) !== JSON.stringify(files(p.commit))) fail(`${tag} files differ`);
  if (typeof p.reason !== "string" || p.reason.length < 10) fail(`${tag} needs a reason`);
  if (typeof p.upstreamCandidate !== "boolean") fail(`${tag} needs upstreamCandidate true or false`);
  if (p.upstreamCandidate && !files(p.commit).every((f) => UPSTREAM_PATHS.some((u) => f.startsWith(u)))) {
    fail(`${tag} is marked upstreamCandidate but touches files outside the upstream Bitcoin crate`);
  }
}
console.log(`fork patches: ${patches.length} listed, ${commits.length - listed.length} manifest-only, failures ${failures}`);
process.exit(failures === 0 ? 0 : 1);
