---
category: core                    # no dedicated bitcoin changelog stream yet
label: "Added Bitcoin PSBT parser"
description: ""
tags: ["Wallet API"]
---
Bitcoin PSBTs (BIP-174, base64 or hex) now decode into VisualSign payloads:
inputs and amounts, outputs and addresses, and the fee. Fees whose input
amounts are not bound by a previous transaction are flagged as unverified, and
outputs carrying derivation data are shown as "claims change (unverified by
parser)". See [Bitcoin](/chains/bitcoin).
