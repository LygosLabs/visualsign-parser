---
category: solana
label: "Added Solana max fee row"
description: ""
tags: ["Wallet API"]
---
`SolanaMetadata` gains an optional `max_fee_lamports` field (`maxFeeLamports`
on the HTTP gateway). When set, the payload shows it as a top-level "Max Fee"
row in SOL right after the header rows. The CLI accepts the same value as
`--max-fee-lamports`.

The field is part of the Borsh-encoded `ChainMetadata`, so `metadata_digest`
changes for every Solana request that sends `chain_metadata`, even when the
new field is absent. A client that recomputes the digest must update its
encoding in the same release.
