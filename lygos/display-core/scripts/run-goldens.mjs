// Replays the native goldens through a built display-core package.
// Usage: node scripts/run-goldens.mjs <pkg-dir>
// Exit status 0 only when every golden matches and nothing throws.
import { readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const pkgDir = path.resolve(process.argv[2]);
const srcDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../src");
const btcFx = path.join(srcDir, "chain_parsers/visualsign-bitcoin/tests/fixtures");
const ownFx = path.join(srcDir, "chain_parsers/visualsign-bitcoin-owned/tests/fixtures");

const glue = await import(path.join(pkgDir, "display_core.js"));
glue.initSync({ module: readFileSync(path.join(pkgDir, "display_core_bg.wasm")) });

const read = (p) => readFileSync(p, "utf8");
const b64 = (hex) => Buffer.from(hex.trim(), "hex").toString("base64");
const sha256 = (s) => createHash("sha256").update(s, "utf8").digest("hex");
let failures = 0;
const fail = (msg) => { failures++; console.log("FAIL", msg); };

function call(...args) {
  try { return glue.display(...args); } catch (e) { fail(`threw ${e} for ${JSON.stringify(args).slice(0, 80)}`); return ""; }
}

// The payload text exactly as embedded, and the digest.
function split(out) {
  const pre = '{"payload":', mark = ',"digest":"';
  if (!out.startsWith(pre)) return [null, null];
  const at = out.lastIndexOf(mark);
  return [out.slice(pre.length, at), out.slice(at + mark.length, -2)];
}

function check(name, out, expected) {
  if (expected.startsWith("refused: ")) {
    const want = JSON.stringify({ refused: expected.slice(9) });
    out === want ? console.log("MATCH ", name) : fail(`${name}: ${out.slice(0, 80)}`);
    return;
  }
  const [payload, digest] = split(out);
  const ok = payload === expected && digest === sha256(payload);
  ok ? console.log("MATCH ", name) : fail(`${name} differs`);
  // A consumer that re-serializes the parsed payload gets the same bytes?
  const round = JSON.stringify(JSON.parse(out).payload) === payload;
  if (!round) console.log("NOTE  ", name, "JSON.parse/stringify round trip changes the payload bytes");
}

let upstream = 0, ours = 0;
for (const f of readdirSync(btcFx).filter((f) => f.endsWith(".expected")).sort()) {
  const name = f.slice(0, -9);
  const out = call(b64(read(path.join(btcFx, name + ".input"))), "bitcoin", "[]", "none");
  check("visualsign/" + name, out, read(path.join(btcFx, f)).trimEnd());
  name === "synthetic-bare-script-input" ? ours++ : upstream++;
}
const shapes = JSON.parse(read(path.join(ownFx, "shapes.json")));
for (const c of shapes) {
  const owned = c.owned.map((o) => ({ scriptHex: read(path.join(ownFx, o.scriptFile)).trim(), label: o.label }));
  const out = call(b64(read(path.join(ownFx, c.name + ".input"))), c.network, JSON.stringify(owned), c.policy);
  check("owned/" + c.name, out, read(path.join(ownFx, c.name + ".expected")).trimEnd());
  ours++;
}

// Refusals and hostile arguments: a refusal string every time, never a throw.
const psbt = b64(read(path.join(ownFx, "qsb-real-helper-whole-vault.input")));
const refusals = [
  [[psbt, undefined, "[]", "none"], "bad-network"],
  [[psbt, null, "[]", "none"], "bad-network"],
  [[psbt, "mainnet", "[]", "none"], "bad-network"],
  [[psbt, "signet", "[]", "none"], "bad-network"],
  [[psbt, "", "[]", "none"], "bad-network"],
  [[psbt, "regtest", "[]", undefined], "bad-policy"],
  [[undefined, "regtest", "[]", "none"], "not-base64-psbt"],
  [[psbt, "regtest", undefined, "none"], "bad-owned-json"],
  [[psbt, "regtest", "not json", "none"], "bad-owned-json"],
  [[psbt, "regtest", "[]", "vault-withdrawal"], "unlabelled-script"],
  [[], "bad-network"],
];
for (const [args, code] of refusals) {
  const out = call(...args);
  out === JSON.stringify({ refused: code }) ? console.log("REFUSE", code) : fail(`${JSON.stringify(args).slice(0, 60)} -> ${out}`);
}
for (const args of [[1, 2, 3, 4], [{}, [], 0, false], ["cHNidP8" + "A".repeat(5_000_000), "regtest", "[]", "none"]]) {
  const out = call(...args);
  out.startsWith('{"refused":"') ? console.log("REFUSE", out) : fail(`hostile -> ${out.slice(0, 80)}`);
}

console.log(`visualsign goldens matched: ${upstream} upstream + 1 new; owned shapes: ${shapes.length}; total checked ${upstream + ours}; failures ${failures}`);
process.exit(failures === 0 ? 0 : 1);
