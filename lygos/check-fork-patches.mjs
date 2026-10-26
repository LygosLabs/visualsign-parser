// Fails unless lygos/FORK-PATCHES.json lists, in order, every commit on top
// of the upstream base pinned BELOW (not the one the manifest names), each
// with its exact files and subject. Commits that change only
// lygos/FORK-PATCHES.json (manifest updates) are not listed. Upstream-candidate
// commits may touch only the upstream Bitcoin crate and its doc page.
//
// The manifest is append-only: every version of it in git history (and the
// working copy) must keep every earlier entry, and the upstream block, byte for
// byte. So a later commit cannot move the base, rewrite a reason, flip
// upstreamCandidate or drop an entry. Changing the base means editing this
// script, which is itself a listed, reviewed commit.
// Usage: node lygos/check-fork-patches.mjs
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const git = (...args) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" }).trim();
const MANIFEST = "lygos/FORK-PATCHES.json";
const UPSTREAM_REPOSITORY = "https://github.com/anchorageoss/visualsign-parser";
const UPSTREAM_BASE = "5d18e229d65d4015865ea71dbe22b85578bc4c74";
const UPSTREAM_PATHS = ["src/chain_parsers/visualsign-bitcoin/", "docs/chains/bitcoin.mdx"];

let failures = 0;
const fail = (msg) => { failures++; console.log("FAIL", msg); };

const m = JSON.parse(readFileSync(path.join(repo, MANIFEST), "utf8"));
if (m.version !== 1) fail("version must be 1");
const up = m.upstream ?? {};
if (up.repository !== UPSTREAM_REPOSITORY) fail(`upstream.repository is ${up.repository}, pinned ${UPSTREAM_REPOSITORY}`);
if (up.baseCommit !== UPSTREAM_BASE) fail(`upstream.baseCommit is ${up.baseCommit}, pinned ${UPSTREAM_BASE}`);
if (git("rev-parse", UPSTREAM_BASE + "^{commit}") !== UPSTREAM_BASE) fail("pinned base commit not found");
try { git("merge-base", "--is-ancestor", UPSTREAM_BASE, "HEAD"); } catch { fail("pinned base commit is not an ancestor of HEAD"); }
if (git("log", "-1", "--format=%s", UPSTREAM_BASE) !== up.baseSubject) fail("upstream.baseSubject differs");

const files = (sha) => git("show", "--format=", "--name-only", sha).split("\n").filter(Boolean).sort();
const commits = git("log", "--reverse", "--format=%H", `${UPSTREAM_BASE}..HEAD`).split("\n").filter(Boolean);
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
// Append-only: compare each version of the manifest with the next.
const canonical = (v) => JSON.stringify(v, (k, x) =>
  x && typeof x === "object" && !Array.isArray(x)
    ? Object.fromEntries(Object.keys(x).sort().map((key) => [key, x[key]]))
    : x);
const versions = git("log", "--reverse", "--format=%H", "--", MANIFEST).split("\n").filter(Boolean)
  .map((sha) => ({ at: sha.slice(0, 7), m: JSON.parse(git("show", `${sha}:${MANIFEST}`)) }));
versions.push({ at: "working copy", m });
for (let i = 1; i < versions.length; i++) {
  const [prev, next] = [versions[i - 1], versions[i]];
  if (canonical(prev.m.upstream) !== canonical(next.m.upstream)) fail(`${next.at} changed the upstream block`);
  const [a, b] = [prev.m.patches ?? [], next.m.patches ?? []];
  if (b.length < a.length) fail(`${next.at} dropped entries (${a.length} to ${b.length})`);
  a.forEach((entry, j) => {
    if (canonical(entry) !== canonical(b[j])) fail(`${next.at} changed entry ${j} (${(entry.commit ?? "?").slice(0, 7)}): entries are append-only`);
  });
}

console.log(`fork patches: ${patches.length} listed, ${commits.length - listed.length} manifest-only, ${versions.length - 1} manifest versions append-only, failures ${failures}`);
process.exit(failures === 0 ? 0 : 1);
