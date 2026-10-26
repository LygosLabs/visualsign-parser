#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Owned-script labels on real QSB vault bytes.
//!
//! Fixture provenance:
//! - `qsb-vault-0.script`: the real 9,923-byte Config A vault script funded by
//!   regtest transaction 3a002c9d... (vout 0) and spent in the mined
//!   transaction c81f5a5c... (vault at input 1).
//! - `qsb-funding-3a002c9d.tx`: that funding transaction, raw. Its vout 1 is
//!   the wallet output the mined spend used as its helper input, so every
//!   input below carries a real previous transaction.
//! - `qsb-spend-c81f5a5c.tx`: the mined spend, raw. `qsb-real-*` fixtures use
//!   its unsigned skeleton (scriptSigs and witnesses removed).
//! - `qsb-synthetic-*` fixtures are unsigned transactions built here from those
//!   real previous outputs. The second vault script ("Your new vault #1") is
//!   synthetic: vault 0 with every 20-byte commitment replaced, same size.
//!
//! `shapes.json` lists every case (owned scripts by fixture file, labels,
//! policy, network) so other runners, such as the WebAssembly build, replay the
//! same cases. `REGEN_FIXTURES=1 cargo test -p visualsign-bitcoin-owned`
//! rewrites this crate's `.input`, `.expected`, `shapes.json` and
//! `qsb-synthetic-vault-1.script` files.

use bitcoin::bip32::{DerivationPath, Fingerprint};
use bitcoin::consensus::deserialize;
use bitcoin::hashes::{Hash, hash160};
use bitcoin::psbt::raw;
use bitcoin::psbt::{Psbt, PsbtSighashType};
use bitcoin::script::Instruction;
use bitcoin::secp256k1::{PublicKey, XOnlyPublicKey};
use bitcoin::{
    Amount, EcdsaSighashType, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness, absolute,
};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use visualsign_bitcoin_owned::{OwnedScripts, Policy, Refusal, display};

const FUNDING_TXID: &str = "3a002c9dd08e243ed430f017fd98d5b9ede1d98642f2b406ff65fc90c3551aac";
const SPEND_TXID: &str = "c81f5a5c7b32bcd30fbf2d792c074b445b5667e4ad0b74e070e96c38bc8a80d3";

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push(name);
    path
}

fn regen() -> bool {
    std::env::var("REGEN_FIXTURES")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// After a regeneration run: fail, because nothing was checked.
fn fail_if_regenerated(what: &str) {
    assert!(
        !regen(),
        "REGEN_FIXTURES is set: {what} rewritten, nothing checked. Review the diff, \
         then run again without REGEN_FIXTURES."
    );
}

fn read_hex(name: &str) -> Vec<u8> {
    hex::decode(fs::read_to_string(fixture_path(name)).unwrap().trim()).unwrap()
}

// ---------------------------------------------------------------------------
// Real bytes
// ---------------------------------------------------------------------------

fn vault0() -> ScriptBuf {
    ScriptBuf::from_bytes(read_hex("qsb-vault-0.script"))
}

fn funding() -> Transaction {
    deserialize(&read_hex("qsb-funding-3a002c9d.tx")).unwrap()
}

fn mined_spend() -> Transaction {
    deserialize(&read_hex("qsb-spend-c81f5a5c.tx")).unwrap()
}

/// Vault 0 with each 20-byte commitment push replaced: a distinct bare
/// script of the same size and shape.
fn vault1() -> ScriptBuf {
    let v0 = vault0();
    let mut bytes = v0.to_bytes();
    let mut n = 0u32;
    for item in v0.instruction_indices() {
        let (i, ins) = item.unwrap();
        if let Instruction::PushBytes(b) = ins
            && b.len() == 20
        {
            assert_eq!(bytes[i], 0x14, "commitment push at {i} is not minimal");
            let h = hash160::Hash::hash(
                format!("lygos-s1 synthetic vault 1, commitment {n}").as_bytes(),
            );
            bytes[i + 1..i + 21].copy_from_slice(h.as_byte_array());
            n += 1;
        }
    }
    assert_eq!(n, 300, "Config A has 300 commitments");
    ScriptBuf::from_bytes(bytes)
}

fn recipient() -> ScriptBuf {
    mined_spend().output[0].script_pubkey.clone()
}

fn unsigned(mut tx: Transaction) -> Transaction {
    for input in &mut tx.input {
        input.script_sig = ScriptBuf::new();
        input.witness = Witness::new();
    }
    tx
}

/// Every input spends the funding transaction: attach it to each.
fn with_prev_txs(tx: Transaction) -> Psbt {
    let mut p = Psbt::from_unsigned_tx(tx).unwrap();
    for input in &mut p.inputs {
        input.non_witness_utxo = Some(funding());
    }
    p
}

fn txout(script: ScriptBuf, sat: u64) -> TxOut {
    TxOut {
        value: Amount::from_sat(sat),
        script_pubkey: script,
    }
}

#[test]
fn real_bytes_are_what_the_fixtures_say() {
    let f = funding();
    assert_eq!(f.compute_txid().to_string(), FUNDING_TXID);
    assert_eq!(vault0().len(), 9_923);
    assert_eq!(f.output[0].script_pubkey, vault0());
    assert_eq!(f.output[0].value, Amount::from_sat(100_000));
    assert!(f.output[1].script_pubkey.is_p2wpkh());
    let s = mined_spend();
    assert_eq!(s.compute_txid().to_string(), SPEND_TXID);
    assert_eq!(
        s.input[0].previous_output,
        OutPoint::new(f.compute_txid(), 1)
    );
    assert_eq!(
        s.input[1].previous_output,
        OutPoint::new(f.compute_txid(), 0)
    );
    assert_eq!(s.input[1].sequence, Sequence(0x8000_1563));
    let v1 = vault1();
    assert_eq!(v1.len(), 9_923);
    assert_ne!(v1, vault0());
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// The mined helper-backed whole-vault withdrawal, unsigned.
fn real_helper_whole_vault() -> Psbt {
    with_prev_txs(unsigned(mined_spend()))
}

/// Helper + vault in, recipient + change to a fresh vault out.
fn partial_with_change() -> Psbt {
    let mut tx = unsigned(mined_spend());
    tx.output = vec![txout(recipient(), 9_999_900_000), txout(vault1(), 60_000)];
    with_prev_txs(tx)
}

/// The vault alone, emptied to the recipient.
fn helper_free_whole_vault() -> Psbt {
    let s = mined_spend();
    let tx = Transaction {
        version: s.version,
        lock_time: absolute::LockTime::from_consensus(s.lock_time.to_consensus_u32()),
        input: vec![TxIn {
            previous_output: OutPoint::new(funding().compute_txid(), 0),
            script_sig: ScriptBuf::new(),
            sequence: s.input[1].sequence,
            witness: Witness::new(),
        }],
        output: vec![txout(recipient(), 90_000)],
    };
    with_prev_txs(tx)
}

/// The helper-free spend with only a `witness_utxo` for the vault.
fn claimed_vault_input() -> Psbt {
    let mut p = helper_free_whole_vault();
    p.inputs[0].non_witness_utxo = None;
    p.inputs[0].witness_utxo = Some(funding().output[0].clone());
    p
}

/// Change paid back to the vault being spent.
fn output_pays_spent_vault() -> Psbt {
    let mut p = partial_with_change();
    p.unsigned_tx.output[1].script_pubkey = vault0();
    p
}

type Owned = Vec<(String, String)>;

fn entry(script: &ScriptBuf, label: &str) -> (String, String) {
    (script.to_hex_string(), label.to_string())
}

fn owned_vault0() -> Owned {
    vec![entry(&vault0(), "Your vault #0")]
}

fn owned_both() -> Owned {
    vec![
        entry(&vault0(), "Your vault #0"),
        entry(&vault1(), "Your new vault #1"),
    ]
}

fn owned_non_ascii_label() -> Owned {
    vec![entry(&vault0(), "Your vault \u{e9}0")]
}

fn owned_49_char_label() -> Owned {
    vec![entry(&vault0(), &format!("Your {}", "v".repeat(44)))]
}

fn owned_control_char_label() -> Owned {
    vec![entry(&vault0(), "Your vault\n#0")]
}

struct Case {
    name: &'static str,
    psbt: fn() -> Psbt,
    owned: fn() -> Owned,
    policy: Policy,
    network: Network,
}

const CASES: [Case; 13] = [
    Case {
        name: "qsb-real-helper-whole-vault",
        psbt: real_helper_whole_vault,
        owned: owned_vault0,
        policy: Policy::VaultWithdrawal,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-partial-with-change",
        psbt: partial_with_change,
        owned: owned_both,
        policy: Policy::VaultWithdrawal,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-helper-free-whole-vault",
        psbt: helper_free_whole_vault,
        owned: owned_vault0,
        policy: Policy::VaultWithdrawal,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-helper-free-whole-vault-testnet4",
        psbt: helper_free_whole_vault,
        owned: owned_vault0,
        policy: Policy::VaultWithdrawal,
        network: Network::Testnet4,
    },
    Case {
        name: "qsb-synthetic-unlabelled-bare-output-vault-withdrawal",
        psbt: partial_with_change,
        owned: owned_vault0,
        policy: Policy::VaultWithdrawal,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-unlabelled-bare-output-none",
        psbt: partial_with_change,
        owned: owned_vault0,
        policy: Policy::None,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-claimed-vault-input-none",
        psbt: claimed_vault_input,
        owned: owned_vault0,
        policy: Policy::None,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-claimed-vault-input-vault-withdrawal",
        psbt: claimed_vault_input,
        owned: owned_vault0,
        policy: Policy::VaultWithdrawal,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-output-pays-spent-vault",
        psbt: output_pays_spent_vault,
        owned: owned_both,
        policy: Policy::VaultWithdrawal,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-bad-label-non-ascii",
        psbt: helper_free_whole_vault,
        owned: owned_non_ascii_label,
        policy: Policy::None,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-bad-label-49-chars",
        psbt: helper_free_whole_vault,
        owned: owned_49_char_label,
        policy: Policy::None,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-synthetic-bad-label-control-char",
        psbt: helper_free_whole_vault,
        owned: owned_control_char_label,
        policy: Policy::None,
        network: Network::Regtest,
    },
    Case {
        name: "qsb-real-helper-whole-vault-none",
        psbt: real_helper_whole_vault,
        owned: owned_vault0,
        policy: Policy::None,
        network: Network::Regtest,
    },
];

fn psbt_hex(p: &Psbt) -> String {
    hex::encode(p.serialize())
}

/// The payload JSON, or `refused: <code>`.
fn run(psbt: &str, owned: Owned, policy: Policy, network: Network) -> String {
    let result = OwnedScripts::new(owned)
        .and_then(|owned| display(psbt, network, &owned, policy))
        .map(|payload| payload.to_json().unwrap());
    match result {
        Ok(json) => json,
        Err(refusal) => format!("refused: {}", refusal.code()),
    }
}

fn run_case(case: &Case) -> String {
    run(
        &psbt_hex(&(case.psbt)()),
        (case.owned)(),
        case.policy,
        case.network,
    )
}

#[test]
fn shape_inputs_are_reproducible() {
    for case in &CASES {
        let built = psbt_hex(&(case.psbt)());
        let path = fixture_path(&format!("{}.input", case.name));
        if regen() {
            fs::write(&path, format!("{built}\n")).unwrap();
            continue;
        }
        let stored = fs::read_to_string(&path).unwrap();
        assert_eq!(built, stored.trim(), "fixture {} is stale", case.name);
    }
    fail_if_regenerated(".input files");
}

fn policy_name(policy: Policy) -> &'static str {
    match policy {
        Policy::None => "none",
        Policy::VaultWithdrawal => "vault-withdrawal",
    }
}

fn network_name(network: Network) -> &'static str {
    match network {
        Network::Regtest => "regtest",
        Network::Testnet4 => "testnet4",
        other => panic!("no case uses {other}"),
    }
}

fn script_file(script_hex: &str) -> &'static str {
    if script_hex == vault0().to_hex_string() {
        "qsb-vault-0.script"
    } else if script_hex == vault1().to_hex_string() {
        "qsb-synthetic-vault-1.script"
    } else {
        panic!("owned script without a fixture file")
    }
}

fn manifest() -> String {
    let cases: Vec<_> = CASES
        .iter()
        .map(|case| {
            let owned: Vec<_> = (case.owned)()
                .iter()
                .map(|(script_hex, label)| json!({"scriptFile": script_file(script_hex), "label": label}))
                .collect();
            json!({
                "name": case.name,
                "owned": owned,
                "policy": policy_name(case.policy),
                "network": network_name(case.network),
            })
        })
        .collect();
    serde_json::to_string_pretty(&cases).unwrap()
}

#[test]
fn shape_manifest_and_vault_1_are_reproducible() {
    let files = [
        ("shapes.json", manifest()),
        ("qsb-synthetic-vault-1.script", vault1().to_hex_string()),
    ];
    for (name, built) in files {
        let path = fixture_path(name);
        if regen() {
            fs::write(&path, format!("{built}\n")).unwrap();
            continue;
        }
        let stored = fs::read_to_string(&path).unwrap();
        assert_eq!(built, stored.trim_end(), "fixture {name} is stale");
    }
    fail_if_regenerated("shapes.json and the vault 1 script");
}

#[test]
fn shapes_match_their_goldens() {
    for case in &CASES {
        let actual = run_case(case);
        let path = fixture_path(&format!("{}.expected", case.name));
        if regen() {
            fs::write(&path, format!("{actual}\n")).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&path).unwrap();
        assert_eq!(
            actual,
            expected.trim_end(),
            "fixture '{}' differs",
            case.name
        );
    }
    fail_if_regenerated(".expected files");
}

#[test]
fn shape_outputs_are_ascii_only() {
    for case in &CASES {
        assert!(run_case(case).is_ascii(), "{}", case.name);
    }
}

// ---------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------

fn subtitle(json: &str, text: &str) -> bool {
    json.contains(&format!(r#""Subtitle":{{"Text":"{text}"}}"#))
}

#[test]
fn whole_vault_withdrawal_labels_the_verified_vault_input() {
    let json = run_case(&CASES[0]);
    assert!(
        subtitle(&json, "0.00100000 BTC from Your vault #0"),
        "{json}"
    );
    assert!(
        json.contains(&vault0().to_hex_string()),
        "raw script must stay"
    );
    assert!(json.contains(r#""Label":"Recognized By""#), "{json}");
    assert!(json.contains(r#""AmountV2":{"Abbreviation":"BTC","Amount":"0.00050000"},"FallbackText":"0.00050000 BTC","Label":"Fee""#), "{json}");
}

#[test]
fn partial_withdrawal_labels_the_fresh_change_vault() {
    let json = run_case(&CASES[1]);
    assert!(
        subtitle(&json, "0.00100000 BTC from Your vault #0"),
        "{json}"
    );
    assert!(
        subtitle(&json, "Your new vault #1 0.00060000 BTC"),
        "{json}"
    );
    assert!(
        json.contains(&vault1().to_hex_string()),
        "raw script must stay"
    );
}

#[test]
fn vault_withdrawal_refuses_an_unlabelled_bare_output_and_none_shows_it_raw() {
    assert_eq!(run_case(&CASES[4]), "refused: unlabelled-script");
    let json = run_case(&CASES[5]);
    assert!(
        subtitle(
            &json,
            &format!("script {} 0.00060000 BTC", vault1().to_hex_string())
        ),
        "{json}"
    );
    assert!(
        subtitle(&json, "0.00100000 BTC from Your vault #0"),
        "{json}"
    );
}

#[test]
fn claimed_vault_input_is_never_labelled() {
    let json = run_case(&CASES[6]);
    assert!(!json.contains("Your vault #0"), "{json}");
    assert!(json.contains(&vault0().to_hex_string()), "{json}");
    assert!(json.contains("claimed by witness_utxo only"), "{json}");
    assert_eq!(run_case(&CASES[7]), "refused: unverified-input");
}

#[test]
fn vault_withdrawal_refuses_paying_the_spent_vault_again() {
    assert_eq!(run_case(&CASES[8]), "refused: output-pays-spent-script");
}

#[test]
fn network_changes_only_the_address_encoding() {
    let regtest = run_case(&CASES[2]);
    let testnet4 = run_case(&CASES[3]);
    assert!(
        regtest.contains("bcrt1qtfqws6mhwa0hcmp8sdkqvp9ece9xs96gh6yx9l"),
        "{regtest}"
    );
    assert!(
        testnet4.contains("tb1qtfqws6mhwa0hcmp8sdkqvp9ece9xs96g"),
        "{testnet4}"
    );
    assert!(!testnet4.contains("bcrt1"), "{testnet4}");
    assert_ne!(regtest, testnet4);
}

#[test]
fn a_label_needs_the_exact_bytes() {
    let v0 = vault0().to_bytes();
    let mut flipped = v0.clone();
    *flipped.last_mut().unwrap() ^= 1;
    let mut longer = v0.clone();
    longer.push(0x51);
    let prefix = v0[..v0.len() - 1].to_vec();
    let leading = v0[..20].to_vec();
    for near in [flipped, longer, prefix, leading] {
        let owned = vec![(hex::encode(near), "Your vault #0".to_string())];
        let psbt = psbt_hex(&helper_free_whole_vault());
        let json = run(&psbt, owned.clone(), Policy::None, Network::Regtest);
        assert!(!json.contains("Your vault #0"), "near match labelled");
        assert_eq!(
            run(&psbt, owned, Policy::VaultWithdrawal, Network::Regtest),
            "refused: unlabelled-script"
        );
    }
}

#[test]
fn owned_scripts_must_be_bare() {
    // A script with an address form can never be owned, so a label can never
    // replace or sit beside an address.
    let witness_v2 = ScriptBuf::from_bytes([vec![0x52, 0x20], vec![7u8; 32]].concat());
    let anchor = ScriptBuf::from_bytes(vec![0x51, 0x02, 0x4e, 0x73]);
    let p2tr = ScriptBuf::from_bytes([vec![0x51, 0x20], vec![9u8; 32]].concat());
    let p2sh = ScriptBuf::from_bytes([vec![0xa9, 0x14], vec![3u8; 20], vec![0x87]].concat());
    for script in [recipient(), witness_v2, anchor, p2tr, p2sh] {
        let owned = vec![
            entry(&vault0(), "Your vault #0"),
            entry(&script, "Your vault #9"),
        ];
        assert_eq!(
            OwnedScripts::new(owned.clone()),
            Err(Refusal::OwnedScriptHasAddress),
            "{script:?}"
        );
        assert_eq!(
            run(
                &psbt_hex(&helper_free_whole_vault()),
                owned,
                Policy::None,
                Network::Regtest
            ),
            "refused: owned-script-has-address"
        );
    }
}

#[test]
fn an_output_with_an_address_is_never_labelled_and_unowned_ones_are_refused() {
    // Witness v2 output, not owned: shown by address under none, refused
    // under vault-withdrawal.
    let mut p = helper_free_whole_vault();
    p.unsigned_tx.output[0].value = Amount::from_sat(80_000);
    let v2 = ScriptBuf::from_bytes([vec![0x52, 0x20], vec![7u8; 32]].concat());
    p.unsigned_tx.output.push(txout(v2, 5_000));
    p.outputs.push(Default::default());
    let psbt = psbt_hex(&p);
    let json = run(&psbt, owned_vault0(), Policy::None, Network::Regtest);
    assert!(json.contains(r#""Subtitle":{"Text":"bcrt1z"#), "{json}");
    assert_eq!(json.matches("Recognized Script").count(), 1, "{json}");
    assert_eq!(
        run(
            &psbt,
            owned_vault0(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: unlabelled-script"
    );
}

#[test]
fn labels_follow_the_closed_pattern() {
    // ^Your (new )?vault #[0-9]{1,4}$, no leading zeros.
    for good in [
        "Your vault #0",
        "Your vault #7",
        "Your vault #12",
        "Your vault #9999",
        "Your new vault #1",
        "Your new vault #9999",
    ] {
        let owned = OwnedScripts::new([(vault0().to_hex_string(), good.to_string())]);
        assert!(owned.is_ok(), "{good:?} refused");
    }
    let long = format!("Your {}", "v".repeat(44));
    for bad in [
        "",
        long.as_str(),
        "Vault (old) - 2",
        "a",
        "caf\u{e9}",
        "tab\there",
        "new\nline",
        "nul\0",
        "a_b",
        "Your a_b",
        "Your a.b",
        "Your caf\u{e9}",
        "Your \u{7f}",
        "Your ",
        // Near the pattern but not it.
        "Your vault #",
        "Your vault #01",
        "Your vault #10000",
        "Your vault #-1",
        "Your vault #1a",
        "Your vault #O",
        "Your vault #0 ",
        " Your vault #0",
        "Your vault  #0",
        "Your  vault #0",
        "Your vault#0",
        "Your Vault #0",
        "your vault #0",
        "YOUR VAULT #0",
        "Your new  vault #1",
        "Your old vault #1",
        "Your new new vault #1",
        "Your vault #1\n",
        "Your vault #\u{0661}",
        // Words the old rules allowed.
        "Your change",
        "Your vault (old) - 2",
        "Your a",
        "Your 2nd vault",
        "Your mine",
    ] {
        let owned = OwnedScripts::new([(vault0().to_hex_string(), bad.to_string())]);
        assert_eq!(owned, Err(Refusal::BadLabel), "{bad:?} accepted");
    }
}

#[test]
fn labels_cannot_look_like_an_address_or_an_amount() {
    for bad in [
        "Your bc1qcyahasx62rxqx50028wkgwzpf9l78zrrg7husa",
        "Your tb1qcyahasx62rxqx50028wkgwzpf9l78zrrzcv0tw",
        "Your bcrt1qtfqws6mhwa0hcmp8sdkqvp9ece9xs96gh6yx9l",
        "Your BC1QCYAHASX62RXQX5",
        "Your bc1qcyah",
        "Your 1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2",
        "Your 3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
        "Your mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn",
        "Your n3GNqMveyvaPvUbH469vDRadqpJMPc84JA",
        "Your 2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc",
        "Your vault bc1qcyahasx62rx",
        "Your 1 BTC",
        "Your 2 btc",
        "Your 21000000BTC",
        "Your 5sat",
        "Your 100 sats",
        "Your vault 1 BTC",
        "Your vault #1 sat",
        // Re-audit R2-2: punctuation around an address or an amount.
        "Your -bc1qcyahasx62rxqx50028wkgwzpf9l78zrrg7husa",
        "Your #bc1qcyahasx62rxqx50028wkgwzpf9l78zrrg7husa",
        "Your (3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy)",
        "Your bc1qcyah-asx62rxqx50028wkgwzpf9l78zrrg7husa",
        "Your bc1qcya hasx62rxqx50028wkgwzpf9l78zrrg7husa",
        "Your 1 bitcoin",
        "Your 1(BTC)",
        "Your 1-BTC",
        "Your one BTC",
        "Your 1 B T C",
    ] {
        let owned = OwnedScripts::new([(vault0().to_hex_string(), bad.to_string())]);
        assert_eq!(owned, Err(Refusal::BadLabel), "{bad:?} accepted");
    }
}

#[test]
fn two_owned_scripts_cannot_share_a_label() {
    // Same vault number, with or without "new": the spent vault and the
    // change could not be told apart.
    for (a, b) in [
        ("Your vault #0", "Your vault #0"),
        ("Your vault #1", "Your new vault #1"),
        ("Your new vault #7", "Your vault #7"),
    ] {
        let owned = vec![entry(&vault0(), a), entry(&vault1(), b)];
        assert_eq!(
            OwnedScripts::new(owned.clone()),
            Err(Refusal::DuplicateLabel),
            "{a:?} {b:?}"
        );
        assert_eq!(
            run(
                &psbt_hex(&partial_with_change()),
                owned,
                Policy::VaultWithdrawal,
                Network::Regtest
            ),
            "refused: duplicate-label"
        );
    }
    // Re-audit R2-2: look-alikes of the same label are not labels at all.
    for b in ["Your vault #0 ", "Your vault  #0", "Your vault #O"] {
        let owned = vec![entry(&vault0(), "Your vault #0"), entry(&vault1(), b)];
        assert_eq!(OwnedScripts::new(owned), Err(Refusal::BadLabel), "{b:?}");
    }
}

#[test]
fn owned_json_is_strict() {
    let v0 = vault0().to_hex_string();
    let ok = format!(r#"[{{"scriptHex":"{v0}","label":"Your vault #0"}}]"#);
    assert!(
        OwnedScripts::from_json(&ok)
            .unwrap()
            .label_for(&vault0())
            .is_some()
    );
    assert_eq!(OwnedScripts::from_json("[]"), Ok(OwnedScripts::default()));
    let cases = [
        ("", Refusal::BadOwnedJson),
        ("{}", Refusal::BadOwnedJson),
        ("null", Refusal::BadOwnedJson),
        (r#"[{"scriptHex":"51"}]"#, Refusal::BadOwnedJson),
        (
            r#"[{"scriptHex":"51","label":"a","extra":1}]"#,
            Refusal::BadOwnedJson,
        ),
        (
            r#"[{"scriptHex":"51","label":"a","label":"b"}]"#,
            Refusal::BadOwnedJson,
        ),
        (r#"[{"scriptHex":51,"label":"a"}]"#, Refusal::BadOwnedJson),
        (r#"[{"scriptHex":"zz","label":"a"}]"#, Refusal::BadScriptHex),
        (
            r#"[{"scriptHex":"515","label":"a"}]"#,
            Refusal::BadScriptHex,
        ),
        (r#"[{"scriptHex":"","label":"a"}]"#, Refusal::BadScriptHex),
        (
            r#"[{"scriptHex":"51","label":"Your vault #1"},{"scriptHex":"51","label":"Your vault #2"}]"#,
            Refusal::DuplicateOwnedScript,
        ),
        (r#"[{"scriptHex":"51","label":"a b_c"}]"#, Refusal::BadLabel),
    ];
    for (json, want) in cases {
        assert_eq!(OwnedScripts::from_json(json), Err(want), "{json}");
    }
    let too_big = format!("[{}]", " ".repeat(1024 * 1024));
    assert_eq!(
        OwnedScripts::from_json(&too_big),
        Err(Refusal::BadOwnedJson)
    );
}

#[test]
fn owned_list_is_bounded() {
    let script = |n: usize| format!("{:02x}{}", n, "51".repeat(3));
    let sixteen: Owned = (0..16)
        .map(|n| (script(n), format!("Your vault #{n}")))
        .collect();
    assert!(OwnedScripts::new(sixteen.clone()).is_ok());
    let mut seventeen = sixteen;
    seventeen.push((script(16), "Your vault #16".to_string()));
    assert_eq!(
        OwnedScripts::new(seventeen),
        Err(Refusal::TooManyOwnedScripts)
    );
    let max = "51".repeat(10_000);
    assert!(OwnedScripts::new([(max, "Your vault #1".to_string())]).is_ok());
    let over = "51".repeat(10_001);
    assert_eq!(
        OwnedScripts::new([(over, "Your vault #1".to_string())]),
        Err(Refusal::BadScriptHex)
    );
}

#[test]
fn vault_withdrawal_needs_every_previous_transaction() {
    let owned = owned_vault0();
    // Helper input with only a witness_utxo.
    let mut p = real_helper_whole_vault();
    p.inputs[0].non_witness_utxo = None;
    p.inputs[0].witness_utxo = Some(funding().output[1].clone());
    let psbt = psbt_hex(&p);
    assert_eq!(
        run(
            &psbt,
            owned.clone(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: unverified-input"
    );
    // Under None the same PSBT displays, with the vault labelled (verified)
    // and the fee shown as unverified by the parser.
    let json = run(&psbt, owned.clone(), Policy::None, Network::Regtest);
    assert!(
        subtitle(&json, "0.00100000 BTC from Your vault #0"),
        "{json}"
    );
    assert!(json.contains("unknown (claimed 0.00050000 BTC"), "{json}");
    // A previous transaction that does not hash to the prevout txid.
    let mut p = real_helper_whole_vault();
    p.inputs[0].non_witness_utxo = Some(mined_spend());
    assert_eq!(
        run(
            &psbt_hex(&p),
            owned,
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: unverified-input"
    );
}

#[test]
fn vault_withdrawal_refuses_other_non_address_outputs() {
    let mut p = helper_free_whole_vault();
    let data = bitcoin::script::PushBytesBuf::try_from(b"memo".to_vec()).unwrap();
    p.unsigned_tx
        .output
        .push(txout(ScriptBuf::new_op_return(data), 0));
    p.outputs.push(Default::default());
    let psbt = psbt_hex(&p);
    assert_eq!(
        run(
            &psbt,
            owned_vault0(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: unlabelled-script"
    );
    assert!(run(&psbt, owned_vault0(), Policy::None, Network::Regtest).contains("OP_RETURN"));
}

#[test]
fn vault_withdrawal_refuses_anyonecanpay() {
    let mut p = real_helper_whole_vault();
    p.inputs[0].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::AllPlusAnyoneCanPay));
    assert_eq!(
        run(
            &psbt_hex(&p),
            owned_vault0(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: unsupported-sighash"
    );
    let mut p = real_helper_whole_vault();
    p.inputs[1].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::All));
    assert!(
        !run(
            &psbt_hex(&p),
            owned_vault0(),
            Policy::VaultWithdrawal,
            Network::Regtest
        )
        .starts_with("refused")
    );
}

#[test]
fn bad_psbt_and_parser_refusals_are_codes() {
    assert_eq!(
        run("AAAA", vec![], Policy::None, Network::Regtest),
        "refused: bad-psbt"
    );
    let mut p = helper_free_whole_vault();
    p.unsigned_tx.output[0].value = Amount::from_sat(100_001);
    assert_eq!(
        run(&psbt_hex(&p), vec![], Policy::None, Network::Regtest),
        "refused: psbt-rejected"
    );
}

#[test]
fn labels_must_start_with_your() {
    let psbt = psbt_hex(&helper_free_whole_vault());
    for bad in [
        "bc1qcyahasx62rxqx50028wkgwzpf9l78zrrg7husa",
        "tb1qcyahasx62rxqx50028wkgwzpf9l78zrrzcv0tw",
        "bcrt1qtfqws6mhwa0hcmp8sdkqvp9ece9xs96gh6yx9l",
        "your vault #0",
        "YOUR vault #0",
        "Your",
        "Yourvault #0",
        " Your vault #0",
    ] {
        let owned = vec![entry(&vault0(), bad)];
        assert_eq!(
            OwnedScripts::new(owned.clone()),
            Err(Refusal::BadLabel),
            "{bad:?} accepted"
        );
        for policy in [Policy::None, Policy::VaultWithdrawal] {
            assert_eq!(
                run(&psbt, owned.clone(), policy, Network::Regtest),
                "refused: bad-label",
                "{bad:?}"
            );
        }
    }
    // One good label next to a bad one still refuses the whole call.
    let mixed = vec![
        entry(&vault0(), "Your vault #0"),
        entry(&vault1(), "bc1qcyahasx62rxqx50028wkgwzpf9l78zrrg7husa"),
    ];
    assert_eq!(
        run(&psbt, mixed, Policy::None, Network::Regtest),
        "refused: bad-label"
    );
}

// ---------------------------------------------------------------------------
// Vault withdrawal: nothing rendered that is not signed or verified
// ---------------------------------------------------------------------------

const G: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

type Tamper = fn(&mut Psbt);

fn tampers() -> Vec<(&'static str, Tamper)> {
    vec![
        ("vault input witness_script", |p| {
            p.inputs[1].witness_script = Some(ScriptBuf::from_bytes(vec![0x51]));
        }),
        ("vault input redeem_script", |p| {
            p.inputs[1].redeem_script = Some(ScriptBuf::from_bytes(vec![0x51]));
        }),
        ("helper input final_script_sig", |p| {
            p.inputs[0].final_script_sig = Some(ScriptBuf::from_bytes(vec![0x00]));
        }),
        ("helper input final_script_witness", |p| {
            p.inputs[0].final_script_witness = Some(Witness::from_slice(&[vec![1u8]]));
        }),
        ("vault input tap_internal_key", |p| {
            p.inputs[1].tap_internal_key = Some(XOnlyPublicKey::from_str(&G[2..]).unwrap());
        }),
        ("helper input bip32_derivation", |p| {
            p.inputs[0].bip32_derivation.insert(
                PublicKey::from_str(G).unwrap(),
                (Fingerprint::default(), DerivationPath::master()),
            );
        }),
        ("helper input unknown key", |p| {
            p.inputs[0].unknown.insert(
                raw::Key {
                    type_value: 0x50,
                    key: vec![1],
                },
                b"Your vault #7".to_vec(),
            );
        }),
        ("output witness_script", |p| {
            p.outputs[0].witness_script = Some(ScriptBuf::from_bytes(vec![0x51]));
        }),
        ("output bip32_derivation", |p| {
            p.outputs[0].bip32_derivation.insert(
                PublicKey::from_str(G).unwrap(),
                (Fingerprint::default(), DerivationPath::master()),
            );
        }),
        ("output tap_internal_key", |p| {
            p.outputs[0].tap_internal_key = Some(XOnlyPublicKey::from_str(&G[2..]).unwrap());
        }),
    ]
}

#[test]
fn vault_withdrawal_refuses_psbt_fields_that_are_not_signed() {
    for (what, tamper) in tampers() {
        let mut p = real_helper_whole_vault();
        tamper(&mut p);
        assert_eq!(
            run(
                &psbt_hex(&p),
                owned_vault0(),
                Policy::VaultWithdrawal,
                Network::Regtest
            ),
            "refused: unsigned-psbt-field",
            "{what}"
        );
    }
    // A witness_utxo that agrees with the previous transaction and an
    // explicit ALL are allowed.
    let mut p = real_helper_whole_vault();
    p.inputs[0].witness_utxo = Some(funding().output[1].clone());
    p.inputs[1].sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::All));
    assert!(
        !run(
            &psbt_hex(&p),
            owned_vault0(),
            Policy::VaultWithdrawal,
            Network::Regtest
        )
        .starts_with("refused")
    );
}

#[test]
fn vault_withdrawal_refuses_an_owned_op_return() {
    let memo = ScriptBuf::from_bytes(vec![0x6a, 0x04, 0xde, 0xad, 0xbe, 0xef]);
    let mut p = helper_free_whole_vault();
    p.unsigned_tx.output.push(txout(memo.clone(), 0));
    p.outputs.push(Default::default());
    let owned = vec![
        entry(&vault0(), "Your vault #0"),
        entry(&memo, "Your vault #5"),
    ];
    let psbt = psbt_hex(&p);
    assert_eq!(
        run(
            &psbt,
            owned.clone(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: owned-script-not-vault"
    );
    // Under none it is a bare script like any other.
    assert!(run(&psbt, owned, Policy::None, Network::Regtest).contains("Your vault #5"));
}

#[test]
fn vault_withdrawal_refuses_an_owned_witness_program() {
    // A version 0 program of length 5 has no address form (only 20 and 32
    // are valid), so it can be owned, but it is no vault.
    let program = ScriptBuf::from_bytes(vec![0x00, 0x05, 1, 1, 1, 1, 1]);
    assert!(program.is_witness_program());
    let mut p = helper_free_whole_vault();
    p.unsigned_tx.output[0].value = Amount::from_sat(80_000);
    p.unsigned_tx.output.push(txout(program.clone(), 5_000));
    p.outputs.push(Default::default());
    let owned = vec![
        entry(&vault0(), "Your vault #0"),
        entry(&program, "Your new vault #1"),
    ];
    let psbt = psbt_hex(&p);
    assert_eq!(
        run(
            &psbt,
            owned.clone(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: owned-script-not-vault"
    );
    assert!(run(&psbt, owned, Policy::None, Network::Regtest).contains("Your new vault #1"));
}

#[test]
fn vault_withdrawal_refuses_a_duplicated_input_itself() {
    let mut p = real_helper_whole_vault();
    let dup = p.unsigned_tx.input[0].clone();
    p.unsigned_tx.input.push(dup);
    p.inputs.push(p.inputs[0].clone());
    // Keep the amounts balanced so only the duplicate is wrong.
    p.unsigned_tx.output[0].value = Amount::from_sat(19_999_810_000);
    let psbt = psbt_hex(&p);
    assert_eq!(
        run(
            &psbt,
            owned_vault0(),
            Policy::VaultWithdrawal,
            Network::Regtest
        ),
        "refused: duplicate-input"
    );
    assert_eq!(
        run(&psbt, owned_vault0(), Policy::None, Network::Regtest),
        "refused: psbt-rejected"
    );
}
