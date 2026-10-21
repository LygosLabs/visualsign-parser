//! Golden and behavior tests over committed PCZT fixtures.
//!
//! `REGEN_FIXTURES=1 cargo test -p visualsign-zcash --test fixtures golden`
//! rewrites every `.expected` from the current parser output. Review the diff.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/builder.rs"]
mod builder;

use std::fs;
use std::path::PathBuf;
use visualsign::SignablePayload;
use visualsign::vsptrait::{
    TransactionParseError, VisualSignConverterFromString, VisualSignError, VisualSignOptions,
};
use visualsign_zcash::{MAX_PCZT_BYTES, UNVERIFIED, ZcashVisualSignConverter};
use zcash_keys::address::UnifiedAddress;
use zcash_keys::encoding::AddressCodec;

const FIXTURES: [&str; 5] = [
    "orchard_send_v5",
    "ironwood_send_v6",
    "sapling_send_v5",
    "transparent_mix_v5",
    "keystone_v1_orchard_to_transparent",
];

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn input(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join(format!("{name}.input")))
        .unwrap()
        .trim()
        .to_string()
}

fn bytes(name: &str) -> Vec<u8> {
    hex::decode(input(name)).unwrap()
}

fn parse(data: &str) -> Result<SignablePayload, VisualSignError> {
    ZcashVisualSignConverter
        .to_visual_sign_payload_from_string(data, VisualSignOptions::default())
        .map(|r| r.payload)
}

fn parse_bytes(b: &[u8]) -> Result<SignablePayload, VisualSignError> {
    parse(&hex::encode(b))
}

fn text<'a>(p: &'a SignablePayload, label: &str) -> &'a str {
    p.fields
        .iter()
        .find(|f| f.label() == label)
        .unwrap_or_else(|| panic!("no field {label}: {:#?}", p.fields))
        .fallback_text()
}

fn has(p: &SignablePayload, label: &str) -> bool {
    p.fields.iter().any(|f| f.label() == label)
}

/// Fallback texts of every "Output n" field, in order.
fn outputs(p: &SignablePayload) -> Vec<String> {
    (1..)
        .map_while(|n| {
            has(p, &format!("Output {n}")).then(|| {
                format!(
                    "{} {}",
                    text(p, &format!("Output {n} amount")),
                    text(p, &format!("Output {n}"))
                )
            })
        })
        .collect()
}

fn ua(addr: orchard::Address) -> String {
    UnifiedAddress::from_receivers(Some(addr), None, None, None, None)
        .unwrap()
        .encode(&builder::MAIN)
}

fn zec(z: u64) -> String {
    format!("{}.{:08}", z / 100_000_000, z % 100_000_000)
}

fn sighash(b: &[u8]) -> String {
    let s = pczt::roles::signer::Signer::new(pczt::Pczt::parse(b).unwrap())
        .unwrap()
        .shielded_sighash();
    hex::encode(s)
}

/// Replace the unique occurrence of `from` in `bytes` with `to` (same length).
fn replace_once(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    assert_eq!(from.len(), to.len());
    let hits: Vec<usize> = (0..=bytes.len() - from.len())
        .filter(|&i| &bytes[i..i + from.len()] == from)
        .collect();
    assert_eq!(hits.len(), 1, "pattern must occur exactly once");
    let mut out = bytes.to_vec();
    out[hits[0]..hits[0] + to.len()].copy_from_slice(to);
    out
}

/// Postcard (LEB128) varint encoding of a u64.
fn varint(mut v: u64) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

#[test]
fn golden() {
    let regen = std::env::var("REGEN_FIXTURES").is_ok();
    for name in FIXTURES {
        let actual = parse(&input(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .to_json()
            .unwrap();
        let path = fixtures_dir().join(format!("{name}.expected"));
        if regen {
            fs::write(&path, format!("{actual}\n")).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing {path:?}; run with REGEN_FIXTURES=1"));
        assert_eq!(actual, expected.trim_end(), "{name}");
    }
}

#[test]
fn sighash_field_is_the_zip244_shielded_sighash_of_the_bytes() {
    for name in FIXTURES {
        let p = parse(&input(name)).unwrap();
        assert_eq!(
            text(&p, "Shielded sighash (ZIP-244)"),
            sighash(&bytes(name)),
            "{name}"
        );
    }
}

#[test]
fn orchard_send_shows_recipients_memos_fee_and_unverified_change() {
    let spec = &builder::fixtures()[0].1;
    let p = parse(&input("orchard_send_v5")).unwrap();
    assert_eq!(p.title, "Zcash Transaction");
    assert_eq!(p.payload_type, "ZcashTx");
    assert_eq!(text(&p, "Network"), "Zcash mainnet");

    // Without a viewing key every output is listed, change included; the
    // builder shuffles actions, so compare as a set.
    let to = ua(builder::foreign_orchard_address());
    let change = ua(builder::sender_orchard().address_at(0u32, orchard::keys::Scope::Internal));
    let mut got = outputs(&p);
    got.sort();
    let mut want = vec![
        format!("{} ZEC {to}", zec(100_000)),
        format!("{} ZEC {to}", zec(200_000)),
        format!("{} ZEC {change}", zec(builder::change_for(spec))),
    ];
    want.sort();
    assert_eq!(got, want);

    let memos: Vec<&str> = (1..=3)
        .filter(|n| has(&p, &format!("Output {n} memo")))
        .map(|n| text(&p, &format!("Output {n} memo")))
        .collect();
    assert!(memos.contains(&"thanks"), "{memos:?}");
    // Non-ASCII text is shown as its UTF-8 bytes ("caf\u{e9}").
    assert!(
        memos.contains(&"text memo, UTF-8 0x636166c3a9"),
        "{memos:?}"
    );
    assert_eq!(memos.len(), 2, "empty change memo is not shown");

    let fee = builder::fee_for(spec);
    assert_eq!(fee, 15_000);
    assert_eq!(text(&p, "Fee"), format!("{} ZEC", zec(fee)));
    assert_eq!(text(&p, "Fee check"), "equals the ZIP-317 conventional fee");
    assert!(text(&p, "Change").starts_with(UNVERIFIED));
    assert_eq!(
        text(&p, "Orchard spent"),
        format!("{} ZEC; spend ownership {UNVERIFIED}", zec(100_000_000))
    );
    let nu6_2 = u32::from(zcash_protocol::consensus::BranchId::Nu6_2);
    assert_eq!(
        text(&p, "Consensus branch"),
        format!("Nu6_2 (0x{nu6_2:08x})")
    );
}

#[test]
fn ironwood_send_is_handled_like_orchard() {
    let spec = &builder::fixtures()[1].1;
    let b = bytes("ironwood_send_v6");
    assert_eq!(*pczt::Pczt::parse(&b).unwrap().global().tx_version(), 6);
    let p = parse_bytes(&b).unwrap();
    let to = ua(builder::foreign_orchard_address());
    assert!(
        outputs(&p).contains(&format!("{} ZEC {to}", zec(1_000_000))),
        "{:?}",
        outputs(&p)
    );
    let n = (1..)
        .find(|n| text(&p, &format!("Output {n}")) == to)
        .unwrap();
    assert_eq!(text(&p, &format!("Output {n} memo")), "iw");
    let badge = match &p.fields.iter().find(|f| f.label() == "Output 1").unwrap() {
        visualsign::SignablePayloadField::AddressV2 { address_v2, .. } => {
            address_v2.badge_text.clone().unwrap()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(badge, "Ironwood");
    assert_eq!(
        text(&p, "Orchard spent"),
        format!("{} ZEC; spend ownership {UNVERIFIED}", zec(10_000_000))
    );
    assert_eq!(
        text(&p, "Fee"),
        format!("{} ZEC", zec(builder::fee_for(spec)))
    );
    assert_eq!(text(&p, "Fee check"), "equals the ZIP-317 conventional fee");
    // ZIP 326 same-address zero-value outputs are padding, not recipients.
    assert!(has(&p, "Padding outputs"));
}

#[test]
fn sapling_send() {
    let p = parse(&input("sapling_send_v5")).unwrap();
    let to = builder::foreign_sapling_address().encode(&builder::MAIN);
    let n = (1..)
        .find(|n| text(&p, &format!("Output {n}")) == to)
        .unwrap();
    assert_eq!(text(&p, &format!("Output {n} amount")), "0.01000000 ZEC");
    assert_eq!(text(&p, &format!("Output {n} memo")), "rent");
    assert_eq!(
        text(&p, "Sapling spent"),
        format!("{} ZEC; spend ownership {UNVERIFIED}", zec(50_000_000))
    );
    assert_eq!(text(&p, "Fee check"), "equals the ZIP-317 conventional fee");
    assert!(!has(&p, "Sapling proof key"));
}

#[test]
fn transparent_inputs_outputs_and_claimed_change() {
    let spec = &builder::fixtures()[3].1;
    let p = parse(&input("transparent_mix_v5")).unwrap();
    let ours = zcash_transparent::address::TransparentAddress::from_pubkey(
        &builder::sender_tkey().public_key(),
    );
    assert_eq!(
        text(&p, "Transparent input 1"),
        format!(
            "{} (P2PKH) {} ZEC",
            ours.encode(&builder::MAIN),
            zec(2_000_000)
        )
    );
    let all = outputs(&p);
    assert!(all.contains(&format!(
        "{} ZEC {}",
        zec(300_000),
        builder::foreign_taddr().encode(&builder::MAIN)
    )));
    let change = format!(
        "{} ZEC {}",
        zec(builder::change_for(spec)),
        ours.encode(&builder::MAIN)
    );
    let n = all.iter().position(|o| *o == change).unwrap() + 1;
    let badge = match p
        .fields
        .iter()
        .find(|f| *f.label() == format!("Output {n}"))
        .unwrap()
    {
        visualsign::SignablePayloadField::AddressV2 { address_v2, .. } => {
            address_v2.badge_text.clone().unwrap()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(
        badge,
        format!("transparent P2PKH, claimed change {UNVERIFIED}")
    );
    assert_eq!(
        text(&p, "Fee"),
        format!("{} ZEC", zec(builder::fee_for(spec)))
    );
    assert_eq!(text(&p, "Fee check"), "equals the ZIP-317 conventional fee");
}

#[test]
fn keystone_firmware_vector_parses() {
    let b = bytes("keystone_v1_orchard_to_transparent");
    assert_eq!(&b[4..8], &1u32.to_le_bytes());
    let p = parse_bytes(&b).unwrap();
    assert_eq!(text(&p, "Network"), "Zcash mainnet");
    assert!(has(&p, "Transparent input 1") || has(&p, "Orchard spent"));
    assert!(has(&p, "Output 1"));
}

#[test]
fn tampered_output_amount_is_refused() {
    let b = bytes("orchard_send_v5");
    let t = replace_once(&b, &varint(100_000), &varint(100_001));
    let e = parse_bytes(&t).unwrap_err();
    assert!(
        format!("{e}").contains("note commitment") || matches!(e, VisualSignError::ParseError(_)),
        "{e}"
    );
}

#[test]
fn tampered_spend_amount_is_refused() {
    let b = bytes("orchard_send_v5");
    let t = replace_once(&b, &varint(100_000_000), &varint(100_000_001));
    assert!(parse_bytes(&t).is_err());
}

#[test]
fn lowered_transparent_input_amount_is_refused() {
    let b = bytes("transparent_mix_v5");
    let t = replace_once(&b, &varint(2_000_000), &varint(1_000_000));
    assert!(parse_bytes(&t).is_err());
}

#[test]
fn user_address_must_contain_the_receiver() {
    let pczt = pczt::Pczt::parse(&bytes("orchard_send_v5")).unwrap();
    let to = builder::foreign_orchard_address();
    let send = pczt
        .orchard()
        .actions()
        .iter()
        .position(|a| *a.output().recipient() == Some(to.to_raw_address_bytes()))
        .unwrap();
    let set = |addr: String| {
        pczt::roles::updater::Updater::new(pczt.clone())
            .update_orchard_with(|mut u| {
                u.update_action_with(send, |mut a| {
                    a.set_output_user_address(addr);
                    Ok(())
                })
            })
            .unwrap()
            .finish()
            .serialize()
            .unwrap()
    };

    let good = ua(to);
    assert!(
        outputs(&parse_bytes(&set(good.clone())).unwrap())
            .iter()
            .any(|o| o.ends_with(&good))
    );

    let other = ua(builder::sender_orchard().address_at(5u32, orchard::keys::Scope::External));
    assert!(matches!(
        parse_bytes(&set(other)),
        Err(VisualSignError::ValidationError(_))
    ));

    let testnet = UnifiedAddress::from_receivers(Some(to), None, None, None, None)
        .unwrap()
        .encode(&zcash_protocol::consensus::Network::TestNetwork);
    assert!(matches!(
        parse_bytes(&set(testnet)),
        Err(VisualSignError::ValidationError(_))
    ));
}

#[test]
fn v2_encoding_is_accepted() {
    let pczt = pczt::Pczt::parse(&bytes("orchard_send_v5")).unwrap();
    let v2 = pczt::v2::Pczt::try_from(pczt).unwrap().serialize();
    assert_eq!(&v2[4..8], &2u32.to_le_bytes());
    let p = parse_bytes(&v2).unwrap();
    assert_eq!(
        text(&p, "Shielded sighash (ZIP-244)"),
        sighash(&bytes("orchard_send_v5"))
    );
    let mut trailing = v2.clone();
    trailing.push(0);
    assert!(parse_bytes(&trailing).is_err());
}

fn parse_error(r: Result<SignablePayload, VisualSignError>) -> TransactionParseError {
    match r {
        Err(VisualSignError::ParseError(e)) => e,
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[test]
fn malformed_input_is_refused() {
    let b = bytes("orchard_send_v5");
    parse_error(parse(""));
    parse_error(parse("zz not hex or base64 !!"));
    parse_error(parse_bytes(b"PCZT\x01\x00\x00\x00garbage"));
    parse_error(parse_bytes(&b[..b.len() / 2]));
    parse_error(parse_bytes(&b[..6]));
    let mut magic = b.clone();
    magic[0] = b'X';
    parse_error(parse_bytes(&magic));
    let mut version = b.clone();
    version[4] = 9;
    parse_error(parse_bytes(&version));
    let mut trailing = b.clone();
    trailing.push(0);
    parse_error(parse_bytes(&trailing));
    let big = vec![0u8; MAX_PCZT_BYTES + 1];
    assert!(matches!(
        parse_error(parse_bytes(&big)),
        TransactionParseError::InvalidFormat(_)
    ));
}

#[test]
fn base64_and_0x_hex_inputs_are_accepted() {
    use base64::Engine as _;
    let b = bytes("sapling_send_v5");
    let want = sighash(&b);
    for s in [
        base64::engine::general_purpose::STANDARD.encode(&b),
        format!("0x{}", hex::encode(&b)),
    ] {
        assert_eq!(
            text(&parse(&s).unwrap(), "Shielded sighash (ZIP-244)"),
            want
        );
    }
}
