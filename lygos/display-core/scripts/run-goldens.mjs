// Replays the native goldens through a built display-core package.
// Usage: node scripts/run-goldens.mjs <pkg-dir>
// Exit status 0 only when every expected golden is present and matches, the
// counts are exactly the ones pinned below, and nothing throws.
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

const ENGINE = "display-core/0.1.0";

// The payload text exactly as embedded, the engine and the digest.
function split(out) {
  const pre = '{"payload":', emark = ',"engine":"', dmark = '","digest":"';
  if (!out.startsWith(pre)) return [null, null, null];
  const e = out.lastIndexOf(emark), d = out.lastIndexOf(dmark);
  return [out.slice(pre.length, e), out.slice(e + emark.length, d), out.slice(d + dmark.length, -2)];
}

function check(name, out, expected) {
  if (expected.startsWith("refused: ")) {
    const want = JSON.stringify({ refused: expected.slice(9) });
    out === want ? console.log("MATCH ", name) : fail(`${name}: ${out.slice(0, 80)}`);
    return;
  }
  const [payload, engine, digest] = split(out);
  const ok = payload === expected && engine === ENGINE && digest === sha256(engine + "\n" + payload);
  ok ? console.log("MATCH ", name) : fail(`${name} differs`);
  // A consumer that re-serializes the parsed payload gets the same bytes?
  const round = JSON.stringify(JSON.parse(out).payload) === payload;
  if (!round) console.log("NOTE  ", name, "JSON.parse/stringify round trip changes the payload bytes");
}

// Pinned: the 13 upstream goldens, our 1 Bitcoin golden, our 13 shapes.
const UPSTREAM = [
  "bip174-p2pkh", "bip174-p2pkh-and-p2sh-p2wpkh-signed", "bip174-p2sh-p2wsh-multisig",
  "bip174-p2wsh-multisig", "bip174-unknown-types", "bip371-p2tr-keypath-input",
  "bip371-p2tr-keypath-output", "bip371-p2tr-scriptpath-input", "synthetic-p2tr-single-sig",
  "synthetic-p2wpkh-no-prev-tx", "synthetic-p2wpkh-single-sig", "synthetic-tr-multi-a-2of3",
  "synthetic-tr-multi-a-internal-key-spend",
];
const OURS = ["synthetic-bare-script-input"];
const SHAPES = 13;

function readOrFail(p) {
  try { return read(p); } catch (e) { fail(`missing ${p}`); return null; }
}

let upstream = 0, ours = 0, shapeCount = 0;
const onDisk = readdirSync(btcFx).filter((f) => f.endsWith(".expected")).map((f) => f.slice(0, -9)).sort();
const pinned = [...UPSTREAM, ...OURS].sort();
if (JSON.stringify(onDisk) !== JSON.stringify(pinned)) fail(`goldens on disk ${JSON.stringify(onDisk)} != pinned`);
for (const name of pinned) {
  const input = readOrFail(path.join(btcFx, name + ".input"));
  const expected = readOrFail(path.join(btcFx, name + ".expected"));
  if (input === null || expected === null) continue;
  check("visualsign/" + name, call(b64(input), "bitcoin", "[]", "none"), expected.trimEnd());
  UPSTREAM.includes(name) ? upstream++ : ours++;
}
const shapes = JSON.parse(readOrFail(path.join(ownFx, "shapes.json")) ?? "[]");
if (shapes.length !== SHAPES) fail(`shapes.json lists ${shapes.length} cases, pinned ${SHAPES}`);
for (const c of shapes) {
  const owned = c.owned.map((o) => ({ scriptHex: (readOrFail(path.join(ownFx, o.scriptFile)) ?? "").trim(), label: o.label }));
  const input = readOrFail(path.join(ownFx, c.name + ".input"));
  const expected = readOrFail(path.join(ownFx, c.name + ".expected"));
  if (input === null || expected === null) continue;
  check("owned/" + c.name, call(b64(input), c.network, JSON.stringify(owned), c.policy), expected.trimEnd());
  shapeCount++;
}
if (upstream !== UPSTREAM.length) fail(`ran ${upstream} upstream goldens, pinned ${UPSTREAM.length}`);
if (ours !== OURS.length) fail(`ran ${ours} of our Bitcoin goldens, pinned ${OURS.length}`);
if (shapeCount !== SHAPES) fail(`ran ${shapeCount} shapes, pinned ${SHAPES}`);

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

console.log(`visualsign goldens matched: ${upstream} upstream + ${ours} new; owned shapes: ${shapeCount}; total checked ${upstream + ours + shapeCount}; failures ${failures}`);
process.exit(failures === 0 ? 0 : 1);
