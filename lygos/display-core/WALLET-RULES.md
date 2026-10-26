# Rules for the wallet (S3) when it calls display-core

From the custody audit of `abccbf7` (AUDIT-custody-S1.md, section 7) and the
re-audit of `8a91389` (AUDIT-custody-S1-R2.md, contracts for S3). Each
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
   display-core accepts up to 16 entries and cannot tell which is which; it
   labels any bare owned script. Labels must match
   `^Your (new )?vault #[0-9]{1,4}$` exactly (for example `Your vault #3` for
   the spent vault and `Your new vault #4` for the change), with a different
   number on each; anything else refuses the call. display-core does not tie
   `new` to inputs or outputs, so the wallet must: never `new` on the vault
   being spent, always `new` on the fresh change.

3. **Build the PSBT deterministically, with nothing but the previous
   transactions.** Under vault-withdrawal every input carries
   `non_witness_utxo` (the full previous transaction) and nothing else: an
   explicit `sighash_type` of ALL and a `witness_utxo` that matches the
   previous transaction are tolerated, and every other input field refuses
   the call (`unsigned-psbt-field`). That includes `bip32_derivation`,
   `partial_sigs`, `witness_script`, `redeem_script`, tap fields, final
   script fields and unknown or proprietary keys, which many PSBT builders
   add by default. Outputs carry no fields at all. The same unsigned bytes
   must give the same PSBT at prepare time and at found time, or the
   found-time check refuses (before the burn, recoverable, but avoidable).
   S3's first test must run the real builder's PSBT through
   vault-withdrawal (re-audit R2, contracts for S3).

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
