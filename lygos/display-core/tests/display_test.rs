#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! `display` through its string boundary, on the VisualSign Bitcoin goldens
//! and on the owned-script shapes listed in `shapes.json`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as b64;
use display_core::{ENGINE, digest_hex, display};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

const PAYLOAD_PREFIX: &str = "{\"payload\":";
const ENGINE_MARK: &str = ",\"engine\":\"";
const DIGEST_MARK: &str = ",\"digest\":\"";
/// What this crate version must report as its engine.
const ENGINE_TEXT: &str = "display-core/0.1.0";

fn dir(rel: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(rel);
    path
}

fn bitcoin_fixtures() -> PathBuf {
    dir("../../src/chain_parsers/visualsign-bitcoin/tests/fixtures")
}

fn owned_fixtures() -> PathBuf {
    dir("../../src/chain_parsers/visualsign-bitcoin-owned/tests/fixtures")
}

fn read(path: PathBuf) -> String {
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path:?}"))
}

fn base64_of_hex(hex_text: &str) -> String {
    b64.encode(hex::decode(hex_text.trim()).unwrap())
}

fn call(psbt: &str, network: &str, owned: &str, policy: &str) -> String {
    display(Some(psbt), Some(network), Some(owned), Some(policy))
}

/// The payload text, engine and digest of a successful display.
fn split(out: &str) -> (&str, &str, &str) {
    let body = out
        .strip_prefix(PAYLOAD_PREFIX)
        .unwrap_or_else(|| panic!("{out}"));
    let at = body
        .rfind(ENGINE_MARK)
        .unwrap_or_else(|| panic!("no engine: {out}"));
    let (engine, rest) = body[at + ENGINE_MARK.len()..].split_once('"').unwrap();
    let digest = rest
        .strip_prefix(DIGEST_MARK)
        .and_then(|d| d.strip_suffix("\"}"))
        .unwrap();
    (&body[..at], engine, digest)
}

/// sha256 over `engine || "\n" || payload`.
fn engine_digest(engine: &str, payload: &str) -> String {
    sha256_hex(&format!("{engine}\n{payload}"))
}

fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn shape1_base64() -> String {
    base64_of_hex(&read(
        owned_fixtures().join("qsb-real-helper-whole-vault.input"),
    ))
}

fn owned_vault0() -> String {
    let script = read(owned_fixtures().join("qsb-vault-0.script"));
    format!(
        r#"[{{"scriptHex":"{}","label":"Your vault #0"}}]"#,
        script.trim()
    )
}

#[test]
fn every_visualsign_golden_reproduces() {
    let mut names: Vec<String> = fs::read_dir(bitcoin_fixtures())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".expected").map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(
        names.len(),
        14,
        "13 upstream goldens plus synthetic-bare-script-input"
    );
    for name in names {
        let psbt = base64_of_hex(&read(bitcoin_fixtures().join(format!("{name}.input"))));
        let out = call(&psbt, "bitcoin", "[]", "none");
        let (payload, engine, digest) = split(&out);
        let expected = read(bitcoin_fixtures().join(format!("{name}.expected")));
        assert_eq!(payload, expected.trim_end(), "{name}");
        assert_eq!(engine, ENGINE_TEXT, "{name}");
        assert_eq!(digest, engine_digest(engine, payload), "{name}");
    }
}

#[test]
fn every_owned_shape_golden_reproduces() {
    let manifest: serde_json::Value =
        serde_json::from_str(&read(owned_fixtures().join("shapes.json"))).unwrap();
    let cases = manifest.as_array().unwrap();
    assert_eq!(cases.len(), 13);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let owned: Vec<serde_json::Value> = case["owned"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let script = read(owned_fixtures().join(o["scriptFile"].as_str().unwrap()));
                serde_json::json!({"scriptHex": script.trim(), "label": o["label"]})
            })
            .collect();
        let psbt = base64_of_hex(&read(owned_fixtures().join(format!("{name}.input"))));
        let out = call(
            &psbt,
            case["network"].as_str().unwrap(),
            &serde_json::to_string(&owned).unwrap(),
            case["policy"].as_str().unwrap(),
        );
        let expected = read(owned_fixtures().join(format!("{name}.expected")));
        let expected = expected.trim_end();
        match expected.strip_prefix("refused: ") {
            Some(code) => assert_eq!(out, format!(r#"{{"refused":"{code}"}}"#), "{name}"),
            None => {
                let (payload, engine, digest) = split(&out);
                assert_eq!(engine, ENGINE_TEXT, "{name}");
                assert_eq!(payload, expected, "{name}");
                assert_eq!(digest, engine_digest(engine, payload), "{name}");
            }
        }
    }
}

#[test]
fn digest_is_sha256_of_the_exact_payload_text() {
    let out = call(
        &shape1_base64(),
        "regtest",
        &owned_vault0(),
        "vault-withdrawal",
    );
    let (payload, engine, digest) = split(&out);
    assert_eq!(engine, ENGINE_TEXT);
    assert_eq!(digest, engine_digest(engine, payload));
    assert_ne!(
        digest,
        sha256_hex(payload),
        "the engine must be in the digest"
    );
    assert_eq!(digest.len(), 64);
    // The payload is embedded verbatim: it parses as JSON, and the whole
    // output is ASCII JSON.
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["digest"], digest);
    assert_eq!(parsed["engine"], ENGINE_TEXT);
    // Keys in the order the contract states.
    let p = out.find("\"payload\":").unwrap();
    let e = out.rfind("\"engine\":").unwrap();
    let d = out.rfind("\"digest\":").unwrap();
    assert!(p < e && e < d, "{out}");
    assert!(out.is_ascii());
    assert!(payload.contains("Your vault #0"));
}

#[test]
fn network_must_be_named_and_known() {
    let psbt = shape1_base64();
    let owned = owned_vault0();
    for bad in [
        "", "mainnet", "testnet", "testnet3", "signet", "Bitcoin", "REGTEST", " regtest",
        "regtest ", "litecoin",
    ] {
        assert_eq!(
            call(&psbt, bad, &owned, "vault-withdrawal"),
            r#"{"refused":"bad-network"}"#,
            "{bad:?}"
        );
    }
    let missing = display(Some(&psbt), None, Some(&owned), Some("none"));
    assert_eq!(missing, r#"{"refused":"bad-network"}"#);
    for good in ["bitcoin", "testnet4", "regtest"] {
        assert!(call(&psbt, good, &owned, "vault-withdrawal").starts_with(PAYLOAD_PREFIX));
    }
}

#[test]
fn regtest_and_testnet4_encode_the_same_psbt_differently() {
    let psbt = shape1_base64();
    let owned = owned_vault0();
    let regtest = call(&psbt, "regtest", &owned, "vault-withdrawal");
    let testnet4 = call(&psbt, "testnet4", &owned, "vault-withdrawal");
    assert!(regtest.contains("bcrt1qtfqws6mhwa0hcmp8sdkqvp9ece9xs96gh6yx9l"));
    assert!(testnet4.contains("tb1qtfqws6mhwa0hcmp8sdkqvp9ece9xs96g"));
    assert!(!testnet4.contains("bcrt1"));
    assert_ne!(split(&regtest).2, split(&testnet4).2);
}

#[test]
fn policy_must_be_known() {
    let psbt = shape1_base64();
    let owned = owned_vault0();
    for bad in ["", "None", "VaultWithdrawal", "vault_withdrawal", "strict"] {
        assert_eq!(
            call(&psbt, "regtest", &owned, bad),
            r#"{"refused":"bad-policy"}"#,
            "{bad:?}"
        );
    }
    let missing = display(Some(&psbt), Some("regtest"), Some(&owned), None);
    assert_eq!(missing, r#"{"refused":"bad-policy"}"#);
}

#[test]
fn psbt_must_be_base64() {
    let owned = owned_vault0();
    let hex_psbt = read(owned_fixtures().join("qsb-real-helper-whole-vault.input"));
    for bad in [hex_psbt.trim(), "", "AAAA", " cHNidP8"] {
        assert_eq!(
            call(bad, "regtest", &owned, "none"),
            r#"{"refused":"not-base64-psbt"}"#,
            "{bad:?}"
        );
    }
    assert_eq!(
        display(None, Some("regtest"), Some(&owned), Some("none")),
        r#"{"refused":"not-base64-psbt"}"#
    );
    assert_eq!(
        call("cHNidP8AAAA", "regtest", "[]", "none"),
        r#"{"refused":"bad-psbt"}"#
    );
}

#[test]
fn owned_list_refusals_are_codes() {
    let psbt = shape1_base64();
    assert_eq!(
        display(Some(&psbt), Some("regtest"), None, Some("none")),
        r#"{"refused":"bad-owned-json"}"#
    );
    let cases = [
        ("", "bad-owned-json"),
        ("{}", "bad-owned-json"),
        (r#"[{"scriptHex":"51","label":"café"}]"#, "bad-label"),
        (r#"[{"scriptHex":"51","label":"a\nb"}]"#, "bad-label"),
        (r#"[{"scriptHex":"5g","label":"a"}]"#, "bad-script-hex"),
    ];
    for (owned, code) in cases {
        assert_eq!(
            call(&psbt, "regtest", owned, "none"),
            format!(r#"{{"refused":"{code}"}}"#),
            "{owned}"
        );
    }
    let long = format!(r#"[{{"scriptHex":"51","label":"{}"}}]"#, "v".repeat(49));
    assert_eq!(
        call(&psbt, "regtest", &long, "none"),
        r#"{"refused":"bad-label"}"#
    );
}

#[test]
fn vault_withdrawal_refusals_reach_the_boundary() {
    let psbt = shape1_base64();
    assert_eq!(
        call(&psbt, "regtest", "[]", "vault-withdrawal"),
        r#"{"refused":"unlabelled-script"}"#
    );
    assert!(call(&psbt, "regtest", "[]", "none").starts_with(PAYLOAD_PREFIX));
}

#[test]
fn a_different_engine_gives_a_different_digest() {
    assert_eq!(ENGINE, ENGINE_TEXT);
    assert!(
        ENGINE
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"./-".contains(&b))
    );
    let out = call(
        &shape1_base64(),
        "regtest",
        &owned_vault0(),
        "vault-withdrawal",
    );
    let (payload, engine, digest) = split(&out);
    assert_eq!(digest_hex(engine, payload), digest);
    for other in [
        "display-core/0.1.1",
        "display-core/0.2.0",
        "other-engine/0.1.0",
        "",
    ] {
        assert_ne!(digest_hex(other, payload), digest, "{other:?}");
    }
    // Same engine, payload one byte different: different digest.
    assert_ne!(digest_hex(engine, &format!("{payload} ")), digest);
}
