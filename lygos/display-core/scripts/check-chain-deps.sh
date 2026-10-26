#!/usr/bin/env bash
# Fails unless display-core links only the visualsign core and the Bitcoin
# parser: the fork's path crates display-core, visualsign-bitcoin-owned,
# visualsign-bitcoin, visualsign and generated (the core's protobuf types),
# and no other chain parser or chain SDK. With a .wasm argument it also fails
# if another chain's name appears in the built module.
#
# Usage: scripts/check-chain-deps.sh [path/to/display_core_bg.wasm]
set -euo pipefail

crate_dir=$(cd "$(dirname "$0")/.." && pwd -P)
cd "$crate_dir"

allowed_path=(display-core visualsign-bitcoin-owned visualsign-bitcoin visualsign generated)
# Chain parsers of the fork and common chain SDK crate names.
forbidden='^(visualsign-(ethereum|solana|sui|tron|near|unspecified|aptos|polkadot)|solana|spl-|anchor-|alloy|ethers|ethabi|revm|near-|sui-|move-|aptos|tron|polkadot|sp-|substrate|cosmos|tendermint)'

tree=$(cargo tree --locked -p display-core -e normal --target wasm32-unknown-unknown \
  --prefix none --format '{p}' | sed 's/ (\*)$//' | sort -u)
echo "display-core links $(wc -l <<< "$tree") packages (normal dependencies, wasm32)"

status=0
while read -r line; do
  name=${line%% *}
  if [[ "$line" == *" (/"* ]]; then
    if [[ " ${allowed_path[*]} " != *" $name "* ]]; then
      echo "FAIL: path crate $line is not the core or the Bitcoin parser" >&2
      status=1
    fi
    if [[ "$line" == *"/chain_parsers/"* && "$name" != visualsign-bitcoin && "$name" != visualsign-bitcoin-owned ]]; then
      echo "FAIL: chain parser $line" >&2
      status=1
    fi
  fi
  if [[ "$name" =~ $forbidden ]]; then
    echo "FAIL: other-chain crate $line" >&2
    status=1
  fi
done <<< "$tree"
grep '(/' <<< "$tree" | sed 's/^/  path crate: /'

if [ $# -ge 1 ]; then
  hits=$(grep -a -o -E 'Solana|SOLANA|Ethereum|ETHEREUM|NearMetadata|CHAIN_[A-Z]+|Sui[A-Z]|Tron[A-Z]' "$1" | sort | uniq -c || true)
  if [ -n "$hits" ]; then
    echo "FAIL: other-chain names inside $1:" >&2
    echo "$hits" >&2
    status=1
  else
    echo "no other-chain names inside $(basename "$1")"
  fi
fi

[ "$status" -eq 0 ] || exit 1
echo "OK: display-core links only the visualsign core and the Bitcoin parser"
