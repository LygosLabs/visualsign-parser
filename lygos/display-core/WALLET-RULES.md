# Rules for the wallet (S3) when it calls display-core

From the custody audit of `abccbf7` (AUDIT-custody-S1.md, section 7). Each
protection display-core gives holds only if the caller keeps these rules.

1. **Policy is a fixed constant for a vault spend.** Every vault withdrawal
   calls `display(..., policy = "vault-withdrawal")`, written as a compiled
   constant in the worker, never read from journal data or the page. Under
   `none`, an output paying the spent vault is labelled, claimed-only amounts
   are shown with an unverified fee, and the digest does not record which
   policy produced it.

2. **The owned list is exactly the spent vault and the fresh change.** Built
   by the signer from its own derivation: the vault being emptied, and the one
   fresh change vault (if any). Nothing else, never every journal vault.
   display-core accepts up to 16 entries and cannot tell which is which.

3. **Build the PSBT deterministically.** PSBT v0 from the unsigned
   transaction, with `non_witness_utxo` on every input and nothing else (an
   explicit `sighash_type` of ALL and a matching `witness_utxo` are tolerated;
   any other field is refused under vault-withdrawal). The same unsigned bytes
   must give the same PSBT at prepare time and at found time, or the found-time
   check refuses (before the burn, recoverable, but avoidable).

4. **Hash the embedded payload text, never a re-serialization.** The digest is
   SHA-256 of `engine || "\n" || payload`, where `payload` is the exact
   substring between `{"payload":` and `,"engine":"` of the returned string.
   Re-serializing parsed JSON happens to give the same bytes today; that is not
   a contract.

Also: pass the network from the wallet's own compiled network enum, never a
string from the page. `mutinynet` and `signet` are refused; showing vault
spends there needs a decision, not a fallback. The network argument is the
caller's assertion (a PSBT does not name its network), which is why the
payload's first field still says "transaction network unknown".
