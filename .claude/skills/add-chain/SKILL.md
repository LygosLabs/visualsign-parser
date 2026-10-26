---
name: add-chain
description: Add a new chain parser crate (visualsign-<chain>) and wire it into every place a chain must appear - proto, registry, parser_app, http-server, images, parser_cli, tests, CI metadata and docs. Covers the policy decisions the PR must state, the safety rules for untrusted input, the test standard, and the exact verify commands. Includes a read-only check-wiring.sh that reports every missing touch point.
user-invocable: true
---

# Add a chain parser

You are adding `src/chain_parsers/visualsign-<chain>/` and wiring it through the repo.
Recent chain PRs each missed 2-7 touch points (`.oss-scanner`, `cli-core/chains.rs`,
CLI fixtures, CODEOWNERS, docs). This skill exists so the next one misses none.

Names used below, for a chain called `foo`:

- `<chain>` = `foo`: crate dir suffix, Cargo feature, CLI `--chain` value, labeler and fixture prefix
- `<Variant>` = `Foo`: `visualsign::registry::Chain::Foo` and `generated::parser::Chain::Foo`
- `<CHAIN>` = `FOO`: proto `CHAIN_FOO`

`docs/adding-new-chain.mdx` is out of date (wrong codegen command, no parser_cli steps).
Follow this skill, not that page.

## Step 0: Before writing code

1. Read the closest existing crate. Bitcoin (`visualsign-bitcoin`, PSBT, UTXO) and NEAR
   (`visualsign-near`, local wire types + oracle) are the current reference shapes.
2. Claim a proto slot (see [wiring.md](wiring.md) step 3). Check open PRs first.
3. Decide the three policies below and write them into the PR description draft now.

## Policy decisions the PR must state

Put each under its own heading in the PR body. Reviewers reject a chain PR that leaves one implicit.

1. **Default-on or opt-in.** Every default-on chain grows the attested `parser_app` surface.
   Opt-in (feature defined, not in `default`) is the right choice until a downstream
   integration needs the chain. Whatever you choose, keep these five sets identical:
   `parser_app` `default`, `http-server` `default`, `CHAIN_FEATURES` in
   `images/parser_app/Containerfile`, `images/parser_http_server/Containerfile` and
   `.oss-scanner/Dockerfile`. `check-wiring.sh` compares them.
   If opt-in, `make test` (default features) never runs the parser_app registry test:
   add `cargo test --no-fail-fast -p parser_app --features <chain> --lib` to the `test`
   target in `src/Makefile`.
2. **Network trust.** Show a network as fact only if the signed bytes commit to it
   (EVM chain id, Cosmos `chain_id`, a genesis hash). If they don't, either omit it, or
   take it from the caller and label the field `Declared network (unsigned)`. Caller
   input comes from `ChainMetadata` (add a `<Variant>Metadata` arm to its `oneof` in
   `proto/parser/parser.proto`) and, in parser_cli, a `--<chain>-network` flag on the
   plugin's args. Never default silently to mainnet in a displayed field.
3. **C dependencies.** State whether the crate's normal+build graph compiles C:
   ```bash
   cd src && cargo tree -q -p visualsign-<chain> -e normal,build --target aarch64-apple-ios -i cc --depth 1
   ```
   Empty output = C-free. If `make -C src apple-silicon-check` exists (PR #587), add a
   C-free crate to `APPLE_NO_C_CRATES` in `src/Makefile`. If the crate needs C (e.g.
   `secp256k1-sys`), name the crate that pulls it in and why there's no pure-Rust option.

## Step 1: Write the crate

`src/chain_parsers/visualsign-<chain>/Cargo.toml` takes the shape of visualsign-bitcoin's:
`default = ["cli-plugin"]`, `cli-plugin = ["dep:clap", "dep:parser_cli_core", "dep:generated"]`,
`[lints] workspace = true`. Implement `Transaction`, `VisualSignConverter<T>`, and a
`cli_plugin.rs` with `<Variant>Args` (clap `Args`) and `<Variant>Plugin: ChainPlugin`.
Use `visualsign::encodings` for hex, `visualsign::field_builders` for fields, ASCII only.

### Safety rules (untrusted input)

Every rule below has a test. A missing test means the rule isn't met.

1. **Cap the encoded size before decoding.** Check `input.len()` against a `MAX_*_ENCODED`
   constant before hex/base64 decoding allocates. Capping after decoding is too late.
2. **Structural and recursion caps.** Bound counts (inputs, outputs, messages, actions)
   and nesting depth (authz, multisig, CPI-like wrappers). Refuse past the cap; don't truncate.
3. **Payload amplification.** Measure rendered `SignablePayload` JSON bytes against input
   bytes on the largest input your caps allow. State the ratio in the PR. Linear and small
   (Cosmos is ~2.9x) is fine; a quadratic blow-up isn't (a 131 KB Sui PTB renders to 15.6 MB).
4. **Show only what the signature commits to.** Refuse modes that sign less than is
   displayed (Bitcoin refuses `SIGHASH_NONE`/`SINGLE`). Never show unsigned claims
   ("change", network, labels from metadata) as fact; mark them `(unverified)` or drop them.
5. **Fail closed.** Unknown message types, unknown fields, trailing bytes, unsupported
   versions: return an error. Don't render a partial view with the rest omitted.
6. **No panics on input.** No `unwrap`/`expect`/indexing/`as` truncation on decoded data;
   the workspace lints deny the first three. Use checked arithmetic for amounts and fees.

## Step 2: Wire it

Follow [wiring.md](wiring.md), in order. It has the exact file and snippet for each of the
touch points. Then run:

```bash
.claude/skills/add-chain/check-wiring.sh <chain>        # add <Variant> if not just capitalised
```

It's read-only and exits 1 on any `MISSING`. `INFO` lines are the policy choices above;
copy them into the PR. Existing chains still fail some checks (e.g. bitcoin has no CLI
fixture, NEAR isn't in the README); that is debt on main, not a reason to skip yours.

## Step 3: Tests

Required in the chain PR:

- **Pinned-provenance vectors.** Each external vector names its source and an immutable
  pin (repo + commit, spec section, or tx hash + block). Vectors from an unmerged upstream
  PR are allowed only with the PR and commit pinned and the risk stated.
- **Goldens with `REGEN_FIXTURES`.** `tests/fixtures/<name>.input` + `.expected`;
  `REGEN_FIXTURES=1 cargo test -p visualsign-<chain>` rewrites them (copy the helper from
  `visualsign-bitcoin/tests/lib_test.rs`). Never regenerate an error into `.expected`.
- **`review_*` regression tests.** Each review finding that changed behaviour gets a test
  named `review_<what>` that fails on the old code.
- **One test per safety rule** above (oversized encoded input, depth/count cap, refused
  mode, unknown type, trailing bytes, truncated input).
- **Differential oracle** where an upstream implementation exists: add it as a
  `[dev-dependencies]` entry only, and compare decoded values (pattern: NEAR
  `tests/primitives_differential.rs` against `near-primitives`, merged in #583; Sui
  against `sui-sdk-types` in #576). A test that checks the parser against the same
  library call it uses internally is a mapping test, not an oracle; say so.
- **End-to-end:** a parser_app registry test and a parser_cli fixture (wiring.md 8 and 15).

Allowed as a tracked follow-up (open the issue and link it in the PR):

- `tests/proptest_<chain>.rs` (proptest dev-dependency) and a cargo-fuzz crate at
  `visualsign-<chain>/fuzz/` with at least one `fuzz_transaction_string` target, modelled
  on `visualsign-solana/fuzz/`, plus label-triggered workflows copied from
  `.github/workflows/proptest-solana.yml` and `fuzz-solana.yml`. Ad-hoc fuzzing done
  during development belongs in the repo as a target, not just in the PR text.

## Step 4: Verify

From the repo root, with protoc from `.github/protoc.env` (`scripts/install-protoc.sh`):

```bash
make -C src generated && git diff --exit-code     # CI fails on any codegen diff
make -C src lint
make -C src test
make -C src narrow-build-check                     # parser_app per chain + dev-signing-absent-check
make -C src parser-cli-narrow-build-check          # parser_cli --features <chain>, runs CLI fixtures
make -C src unused-deps                            # needs cargo-machete 0.9.2
make -C src apple-silicon-check                    # only if it exists (PR #587); macOS host
make -C src feature-matrix-check                   # local only, slow; run once before review
.claude/skills/add-chain/check-wiring.sh <chain>
```

Then decode a real vector through the CLI from `src/`:

```bash
cargo run -q --bin parser_cli -- decode --chain <chain> --output human -t <raw>
cargo run -q --bin parser_cli -- decode --chain <chain> --output json -t @path/to/tx.hex
cargo run -q -p parser_cli --no-default-features --features <chain> -- decode --chain <chain> -t <raw>
```

## Orchestration (optional)

For a large chain, run two subagents in parallel after Step 0 and the crate's public API
(`<Variant>Transaction`, `<Variant>VisualSignConverter`, `<Variant>Args`, `<Variant>Plugin`)
are fixed:

- **Wiring agent:** give it this skill's `wiring.md`, the chain names, the proto slot and
  the default-on/opt-in choice. It edits only files outside `visualsign-<chain>/` and
  finishes when `check-wiring.sh <chain>` exits 0 and `make -C src narrow-build-check` passes.
- **Test agent:** give it Step 3, the safety rules, and the vector sources. It edits only
  `visualsign-<chain>/tests/` (and `fuzz/` if in scope) and reports which safety rules have
  no test yet.

Review both diffs yourself before Step 4. Don't let either agent change proto slots or policy.

## PR description checklist

- Policy: default-on/opt-in, network trust, C deps (with the `cargo tree` output)
- Proto slot number and the open PRs you checked
- Amplification ratio and the caps chosen
- Vector provenance list; oracle used (or why none)
- Follow-up issue for proptest + fuzz if not in this PR
- `check-wiring.sh <chain>` output
