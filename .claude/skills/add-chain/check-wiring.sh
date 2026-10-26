#!/usr/bin/env bash
# Read-only check that every touch point for a chain mentions it.
#
#   .claude/skills/add-chain/check-wiring.sh <chain> [RustVariant]
#
# <chain> is the lowercase name used in the crate dir, Cargo feature and CLI
# (e.g. near, bitcoin). RustVariant defaults to <chain> with the first letter
# upper-cased (Near, Bitcoin); pass it when that is wrong.
#
# Prints one line per check: OK, MISSING, or INFO (a policy choice, not an
# error). Exits 1 if anything is MISSING. Never writes a file.
set -u

chain="${1:?usage: check-wiring.sh <chain> [RustVariant]}"
variant="${2:-$(printf '%s' "${chain:0:1}" | tr '[:lower:]' '[:upper:]')${chain:1}}"
upper="$(printf '%s' "$chain" | tr '[:lower:]' '[:upper:]')"
snake="${chain//-/_}"
root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)" || exit 2
cd "$root" || exit 2

missing=0
ok()   { printf '  OK       %s\n' "$1"; }
miss() { printf '  MISSING  %s\n' "$1"; missing=$((missing + 1)); }
info() { printf '  INFO     %s\n' "$1"; }
# check <description> <file> <extended regex>
check() {
  if [ -f "$2" ] && grep -Eq -- "$3" "$2"; then ok "$1"; else miss "$1 ($2)"; fi
}
# Space-separated feature list -> sorted, one per line.
norm() { tr ' ,"' '\n' | sed '/^$/d' | sort -u; }
# `default = [...]` of a Cargo.toml, minus non-chain features.
cargo_default() {
  sed -n 's/^default *= *\[\(.*\)\]/\1/p' "$1" | norm | grep -vxE 'diagnostics|cli-plugin|vsock'
}
chain_features_arg() { sed -n 's/^ARG CHAIN_FEATURES="\(.*\)"/\1/p' "$1" | norm; }

crate="src/chain_parsers/visualsign-$chain"
echo "Wiring for chain '$chain' (variant $variant, proto CHAIN_$upper)"

echo "Crate"
if [ -d "$crate" ]; then ok "$crate exists"; else miss "$crate exists"; fi
check "crate default = [\"cli-plugin\"]" "$crate/Cargo.toml" '^default *= *\["cli-plugin"\]'
check "crate [lints] workspace = true" "$crate/Cargo.toml" '^workspace *= *true'
check "workspace member" src/Cargo.toml "\"chain_parsers/visualsign-$chain\""

echo "Proto and core enum"
check "proto CHAIN_$upper slot" proto/parser/parser.proto "CHAIN_$upper *= *[0-9]+;"
check "generated code regenerated" src/generated/src/generated/parser.rs "\"CHAIN_$upper\""
check "registry Chain::$variant" src/visualsign/src/registry.rs "Chain::$variant => \"$variant\""
check "registry FromStr \"$chain\"" src/visualsign/src/registry.rs "\"$chain\" => Chain::$variant"
check "chain_conversion proto->registry" src/parser/app/src/chain_conversion.rs "ProtoChain::$variant => RegistryChain::$variant"

echo "parser_app / http-server"
check "parser_app feature" src/parser/app/Cargo.toml "^$chain *= *\[\"dep:visualsign-$chain\"\]"
check "parser_app registers converter" src/parser/app/src/registry.rs "feature = \"$chain\""
check "parser_app registry test" src/parser/app/src/registry.rs "convert_transaction\(&(visualsign::registry::)?Chain::$variant"
check "http-server feature" src/parser/http-server/Cargo.toml "^$chain *= *\[\"parser_app/$chain\"\]"

echo "Default set: parser_app default == http-server default == every CHAIN_FEATURES"
app_default="$(cargo_default src/parser/app/Cargo.toml)"
if printf '%s\n' "$app_default" | grep -qx "$chain"; then
  info "$chain is DEFAULT-ON in parser_app (state this in the PR)"
else
  info "$chain is OPT-IN in parser_app (state this in the PR)"
fi
# Word splitting on the feature lists is intended (one line in the message).
# shellcheck disable=SC2086
same() {
  if [ "$app_default" = "$2" ]; then ok "$1 matches parser_app default"
  else miss "$1 differs from parser_app default [$(printf "%s " $app_default)] vs [$(printf "%s " $2)]"; fi
}
same "http-server default" "$(cargo_default src/parser/http-server/Cargo.toml)"
for f in images/parser_app/Containerfile images/parser_http_server/Containerfile .oss-scanner/Dockerfile; do
  same "$f CHAIN_FEATURES" "$(chain_features_arg "$f")"
done

echo "parser_cli"
check "parser_cli feature" src/parser/cli/Cargo.toml "^$chain *= *\[\"dep:visualsign-$chain\""
check "parser_cli main.rs plugin" src/parser/cli/src/main.rs "visualsign_$snake::"
check "cli-core chains.rs mapping" src/parser/cli-core/src/chains.rs "\"$chain\", *Chain::$variant"
check "Makefile PARSER_CLI_CHAINS" src/Makefile "^PARSER_CLI_CHAINS *:=.*\\b$chain\\b"
check "cli_test.rs feature list" src/parser/cli/tests/cli_test.rs "feature = \"$chain\""
check "cli_test.rs disabled prefix" src/parser/cli/tests/cli_test.rs "^ *\"$chain\",$"
if ls src/parser/cli/tests/fixtures/"$chain"-*.input >/dev/null 2>&1; then
  ok "parser_cli fixture $chain-*.input"
else
  miss "parser_cli fixture (src/parser/cli/tests/fixtures/$chain-*.input)"
fi

echo "Repo metadata and docs"
check "labeler chain:$chain" .github/labeler.yml "^\"chain:$chain\":"
check "CODEOWNERS entry" .github/CODEOWNERS "^/src/chain_parsers/visualsign-$chain/ "
check "docs/chains/$chain.mdx" "docs/chains/$chain.mdx" '.'
check "docs.json nav" docs/docs.json "\"chains/$chain\""
check "chain-modules.mdx" docs/chain-modules.mdx "chains/$chain\)"
check "introduction.mdx" docs/introduction.mdx "chains/$chain\)"
check "parser-cli.mdx --chain list" docs/parser-cli.mdx "\`$chain\`"
check "README chain list" README.md "^- $variant\b"
if grep -rqil -- "$chain" docs/changelog/_drafts/*.md docs/changelog/*.mdx 2>/dev/null; then
  ok "changelog entry"
else
  miss "changelog entry (docs/changelog/_drafts/<date>-$chain-*.md)"
fi
check "CLAUDE.md crate list" CLAUDE.md "visualsign-\{[^}]*\b$chain\b"

if grep -q '^APPLE_NO_C_CRATES' src/Makefile; then
  if grep -Eq "^APPLE_NO_C_CRATES *:=.*visualsign-$chain\b" src/Makefile; then
    info "visualsign-$chain is in APPLE_NO_C_CRATES (C-free, checked on iOS)"
  else
    info "visualsign-$chain is NOT in APPLE_NO_C_CRATES (state its C deps in the PR)"
  fi
else
  info "no APPLE_NO_C_CRATES in src/Makefile yet; check C deps by hand (see SKILL.md)"
fi

echo
if [ "$missing" -gt 0 ]; then echo "$missing touch point(s) missing"; exit 1; fi
echo "all touch points present"
