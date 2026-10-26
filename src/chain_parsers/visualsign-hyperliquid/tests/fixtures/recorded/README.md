# Recorded Hyperliquid testnet payloads

These four `unsigned_payload` envelopes were captured from a real end-to-end run,
not generated for the tests. They come from a second, independent producer next to
the Python-SDK vectors in `../../vectors/`.

- Producer: `@nktkas/hyperliquid` 0.33.3 (JavaScript SDK) built the actions.
- Signers: the Anchorage browser extension and an HSM build carrying the
  self-hosted EIP-712 exception for the Hyperliquid L1 envelope, running as a
  simulator with the published test seeds.
- Posted to the Hyperliquid testnet API on 2026-10-26 between 14:10 and 14:13 UTC,
  from the unfunded wallet `0xba66…e622`. The API answered "does not exist" for
  that user, which shows it recovered the expected signer from each signature.
- The `connectionId` of `a1-order` and `a2-cancel` was recomputed independently
  with Python msgpack 1.0.8 and pycryptodome keccak; both match.

| File | Kind |
| --- | --- |
| `a1-order.input.json` | L1 `order`, testnet |
| `a2-cancel.input.json` | L1 `cancel`, testnet |
| `b2-approve-agent-0x1.input.json` | user-signed `ApproveAgent`, chainId 1, no expiry |
| `e1-approve-expiring.input.json` | user-signed `ApproveAgent`, chainId 1, `valid_until` |

`<name>.expected` is the rendered payload, checked by `tests/vectors.rs`
(`REGEN_FIXTURES=1` to update).
