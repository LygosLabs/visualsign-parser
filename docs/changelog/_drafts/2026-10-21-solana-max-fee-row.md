---
category: solana
label: "Added Solana max fee cap and row"
description: ""
tags: ["Wallet API"]
---
Every Solana payload now shows a top-level "Max Fee" row in SOL right after the
header rows: the caller's new `SolanaMetadata.max_fee_lamports`
(`maxFeeLamports` on the HTTP gateway) when set, otherwise the parser's default
cap of 0.1 SOL. The parser rejects the request when the fee the transaction
commits to exceeds the cap in force: 5000 lamports per signature, counting the
ones precompile instructions verify, plus the compute budget's priority fee. A
compute budget instruction that does not decode leaves that fee unknown and is
rejected too, as is a transaction that repeats the compute unit price or limit,
which the runtime rejects. The row is therefore a true upper bound on what the
transaction pays. The CLI accepts the same value as `--max-fee-lamports`.

The Solana intermediate output moves to schema version 5 with two additions.
A `native_instruction_decoder` tag right after the version says which native
decoding the build has, so a consumer can tell a build without `json-parsed`
from a decode failure, and a build without `simulation` now marks a supplied
simulation `SimulationUnsupported` instead of refusing the request. A trailing
`max_fee_lamports` field carries the cap in force, so a consumer that checks
the transaction's fee reads the same number the signer saw. A decoder that
asserts the version must read both fields before it accepts the new output.

The `max_fee_lamports` field is part of the Borsh-encoded `ChainMetadata`, so
`metadata_digest` changes for every Solana request that sends `chain_metadata`,
even when the new field is absent. A client that recomputes the digest must add
the trailing field to its Borsh mirror. The parser and that mirror can ship in
either order, as long as both are live before the client sends Solana
`chain_metadata`.
