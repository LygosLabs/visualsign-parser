//! Recognition of the multisig script templates a PSBT can carry:
//! `wsh(multi/sortedmulti)` witness scripts and `tr(_, multi_a)` tap leaves.
//!
//! A script is only reported as a multisig when it is bound to the output it
//! belongs to (witness script hash, or taproot commitment through the control
//! block or the tap tree). Anything else is reported as unrecognized, with the
//! raw script, so a PSBT cannot make a spend look like a different policy.
//!
//! Bare scripts (no address form) have no template here. A caller that knows
//! such scripts can name them through a [`ScriptRecognizer`]; the parser tells
//! it where each script came from ([`ScriptSource`]), checks what it returns,
//! and still shows the raw bytes.

use crate::analysis::BitcoinParserError;
use bitcoin::bip32::{DerivationPath, Fingerprint};
use bitcoin::opcodes::all::{OP_CHECKMULTISIG, OP_CHECKSIG, OP_CHECKSIGADD, OP_NUMEQUAL};
use bitcoin::psbt::{Input, Output};
use bitcoin::script::Instruction;
use bitcoin::secp256k1::{PublicKey, Secp256k1, XOnlyPublicKey};
use bitcoin::taproot::{TapLeafHash, TaprootSpendInfo};
use bitcoin::{Script, ScriptBuf};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;

/// BIP-341 NUMS point: an internal key nobody can sign for.
const NUMS_HEX: &str = "50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0";
const MAX_WSH_KEYS: usize = 20;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Cosigner {
    pub key_hex: String,
    /// `fingerprint m/path` when the PSBT carries an origin for the key.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Multisig {
    pub kind: &'static str,
    pub threshold: usize,
    pub cosigners: Vec<Cosigner>,
    /// Taproot only: the internal key and whether it is the NUMS point.
    pub internal_key: Option<(String, bool)>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ScriptDetail {
    Multisig(Multisig),
    Unrecognized {
        script_hex: String,
        reason: &'static str,
    },
    /// A bare script named by a caller-supplied [`ScriptRecognizer`].
    Custom(RecognizedScript),
}

/// Where a script handed to a [`ScriptRecognizer`] comes from.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScriptSource {
    /// An output of the unsigned transaction: part of the bytes being signed.
    Output,
    /// The output an input spends, taken from a `non_witness_utxo` that hashes
    /// to the prevout txid.
    VerifiedPrevout,
    /// The output an input spends, taken from `witness_utxo` only. Nothing
    /// binds it to the real prevout, so it is a claim.
    ClaimedPrevout,
}

/// A name and optional label/value rows for a script, returned by a
/// [`ScriptRecognizer`]. The parser checks it as soon as it is returned and
/// fails the conversion if any text is empty, longer than
/// [`MAX_RECOGNIZED_TEXT`] bytes or not printable ASCII, if there are more than
/// [`MAX_RECOGNIZED_FIELDS`] rows, or if a row label repeats another row's or
/// is one the parser itself writes ([`RESERVED_LABELS`],
/// [`RESERVED_LABEL_PREFIXES`]; compared ignoring ASCII case and surrounding
/// spaces).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RecognizedScript {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

/// Most rows one [`RecognizedScript`] may carry.
pub const MAX_RECOGNIZED_FIELDS: usize = 8;
/// Longest text, in bytes, of a [`RecognizedScript`] name, label or value.
pub const MAX_RECOGNIZED_TEXT: usize = 256;

/// Every fixed label the parser writes, at the top level or inside an input or
/// output. A recognizer row may not use one.
pub const RESERVED_LABELS: &[&str] = &[
    "Address Encoding",
    "Outpoint",
    "Sequence",
    "Address",
    "Script",
    "Script Type",
    "Amount",
    "Amount Source",
    "Sighash Type",
    "Multisig",
    "Internal Key",
    "Internal Key Warning",
    "Script Status",
    "Recognized Script",
    "Fee",
    "Version",
    "Lock Time",
];

/// Prefixes of the numbered labels the parser writes (`Cosigner 1`,
/// `Input 0`, `Output 0`). A recognizer row label may not start with one.
pub const RESERVED_LABEL_PREFIXES: &[&str] = &["Cosigner ", "Input ", "Output "];

/// Names bare scripts: scripts with no address form, which the parser can
/// only show as raw hex.
///
/// Called for every output script and every spent (input prevout) script that
/// has no address form; never for a script the parser shows as an address.
/// The first recognizer that returns `Some` wins. The raw script is shown
/// either way, so a recognizer adds a name; it cannot hide bytes or replace an
/// address. `Send + Sync`, so a converter holding recognizers can be
/// registered like any other.
pub trait ScriptRecognizer: std::fmt::Debug + Send + Sync {
    fn recognize(&self, script_pubkey: &Script, source: ScriptSource) -> Option<RecognizedScript>;
}

/// The checked detail from the first recognizer that names `script_pubkey`.
pub fn recognize(
    recognizers: &[Arc<dyn ScriptRecognizer>],
    script_pubkey: &Script,
    source: ScriptSource,
) -> Result<Option<ScriptDetail>, BitcoinParserError> {
    match recognizers
        .iter()
        .find_map(|r| r.recognize(script_pubkey, source))
    {
        Some(recognized) => {
            check_recognized(&recognized)?;
            Ok(Some(ScriptDetail::Custom(recognized)))
        }
        None => Ok(None),
    }
}

fn rejected(why: String) -> BitcoinParserError {
    BitcoinParserError::RecognizerOutput(why)
}

fn check_text(what: &str, text: &str) -> Result<(), BitcoinParserError> {
    if text.is_empty()
        || text.len() > MAX_RECOGNIZED_TEXT
        || !text.bytes().all(|b| (0x20..=0x7e).contains(&b))
    {
        return Err(rejected(format!(
            "{what} must be 1 to {MAX_RECOGNIZED_TEXT} printable ASCII characters"
        )));
    }
    Ok(())
}

/// `label` with surrounding spaces removed and ASCII letters lowered.
fn label_key(label: &str) -> String {
    label.trim().to_ascii_lowercase()
}

fn is_reserved(label: &str) -> bool {
    let key = label_key(label);
    RESERVED_LABELS.iter().any(|r| key == label_key(r))
        || RESERVED_LABEL_PREFIXES
            .iter()
            .any(|p| key.starts_with(&p.to_ascii_lowercase()))
}

fn check_recognized(recognized: &RecognizedScript) -> Result<(), BitcoinParserError> {
    check_text("name", &recognized.name)?;
    if recognized.fields.len() > MAX_RECOGNIZED_FIELDS {
        return Err(rejected(format!("more than {MAX_RECOGNIZED_FIELDS} rows")));
    }
    let mut seen = BTreeSet::new();
    for (label, value) in &recognized.fields {
        check_text("row label", label)?;
        check_text("row value", value)?;
        if is_reserved(label) {
            return Err(rejected(format!(
                "row label '{label}' is one the parser writes"
            )));
        }
        if !seen.insert(label_key(label)) {
            return Err(rejected(format!("row label '{label}' repeats")));
        }
    }
    Ok(())
}

type Origins = BTreeMap<Vec<u8>, String>;

fn origin_text(fp: &Fingerprint, path: &DerivationPath) -> String {
    if path.is_empty() {
        format!("{fp} m")
    } else {
        format!("{fp} m/{path}")
    }
}

fn is_nums(key: &XOnlyPublicKey) -> bool {
    XOnlyPublicKey::from_str(NUMS_HEX).is_ok_and(|n| n == *key)
}

fn small_int(i: &Instruction<'_>) -> Option<usize> {
    match i {
        Instruction::Op(op) => {
            let b = op.to_u8();
            (0x51..=0x60).contains(&b).then(|| usize::from(b - 0x50))
        }
        Instruction::PushBytes(b) if b.len() == 1 => {
            let v = b.as_bytes()[0];
            (17..=127).contains(&v).then(|| usize::from(v))
        }
        Instruction::PushBytes(_) => None,
    }
}

fn instructions(script: &Script) -> Option<Vec<Instruction<'_>>> {
    script.instructions().collect::<Result<_, _>>().ok()
}

/// `<k> <pubkey>... <n> OP_CHECKMULTISIG`
fn parse_multi(script: &Script) -> Option<(usize, Vec<PublicKey>)> {
    let ins = instructions(script)?;
    let (last, rest) = ins.split_last()?;
    if *last != Instruction::Op(OP_CHECKMULTISIG) {
        return None;
    }
    let (n_ins, rest) = rest.split_last()?;
    let (k_ins, key_ins) = rest.split_first()?;
    let (k, n) = (small_int(k_ins)?, small_int(n_ins)?);
    let keys = key_ins
        .iter()
        .map(|i| match i {
            Instruction::PushBytes(b) if b.len() == 33 => PublicKey::from_slice(b.as_bytes()).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let unique = keys.iter().collect::<BTreeSet<_>>().len() == keys.len();
    (unique && keys.len() == n && (1..=n).contains(&k) && n <= MAX_WSH_KEYS).then_some((k, keys))
}

/// `<x1> OP_CHECKSIG (<xi> OP_CHECKSIGADD)* <k> OP_NUMEQUAL`
fn parse_multi_a(script: &Script) -> Option<(usize, Vec<XOnlyPublicKey>)> {
    let ins = instructions(script)?;
    let (last, rest) = ins.split_last()?;
    if *last != Instruction::Op(OP_NUMEQUAL) {
        return None;
    }
    let (k_ins, key_ins) = rest.split_last()?;
    let k = small_int(k_ins)?;
    if key_ins.is_empty() || key_ins.len() % 2 != 0 {
        return None;
    }
    let mut keys = Vec::new();
    for (idx, pair) in key_ins.chunks(2).enumerate() {
        let want = if idx == 0 {
            OP_CHECKSIG
        } else {
            OP_CHECKSIGADD
        };
        match pair {
            [Instruction::PushBytes(b), Instruction::Op(op)] if *op == want && b.len() == 32 => {
                keys.push(XOnlyPublicKey::from_slice(b.as_bytes()).ok()?);
            }
            _ => return None,
        }
    }
    let unique = keys.iter().collect::<BTreeSet<_>>().len() == keys.len();
    (unique && (1..=keys.len()).contains(&k)).then_some((k, keys))
}

fn unrecognized(script: &Script, reason: &'static str) -> ScriptDetail {
    ScriptDetail::Unrecognized {
        script_hex: script.to_hex_string(),
        reason,
    }
}

/// `redeem_script` is `Some` for the P2SH-wrapped form.
fn wsh_detail(
    witness_script: &Script,
    prevout_script: Option<&Script>,
    redeem_script: Option<&Script>,
    origins: &Origins,
) -> ScriptDetail {
    let wsh = ScriptBuf::new_p2wsh(&witness_script.wscript_hash());
    let (bound, kind) = match (prevout_script, redeem_script) {
        (Some(p), Some(r)) => (
            *r == wsh && *p == ScriptBuf::new_p2sh(&r.script_hash()),
            "P2SH-P2WSH",
        ),
        (Some(p), None) => (*p == wsh, "P2WSH"),
        (None, _) => (false, "P2WSH"),
    };
    if !bound {
        return unrecognized(witness_script, "witness script does not match the output");
    }
    match parse_multi(witness_script) {
        Some((threshold, keys)) => ScriptDetail::Multisig(Multisig {
            kind,
            threshold,
            cosigners: keys
                .iter()
                .map(|k| Cosigner {
                    key_hex: k.to_string(),
                    origin: origins.get(&k.serialize().to_vec()).cloned(),
                })
                .collect(),
            internal_key: None,
        }),
        None => unrecognized(witness_script, "unrecognized script"),
    }
}

fn tap_detail(
    leaf: &Script,
    internal: XOnlyPublicKey,
    bound: bool,
    origins: &Origins,
) -> ScriptDetail {
    if !bound {
        return unrecognized(leaf, "tap script is not committed to by the output");
    }
    match parse_multi_a(leaf) {
        Some((threshold, keys)) => ScriptDetail::Multisig(Multisig {
            kind: "P2TR script path",
            threshold,
            cosigners: keys
                .iter()
                .map(|k| Cosigner {
                    key_hex: k.to_string(),
                    origin: origins.get(&k.serialize().to_vec()).cloned(),
                })
                .collect(),
            internal_key: Some((internal.to_string(), is_nums(&internal))),
        }),
        None => unrecognized(leaf, "unrecognized script"),
    }
}

fn tap_origins(
    map: &BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, (Fingerprint, DerivationPath))>,
) -> Origins {
    map.iter()
        .map(|(k, (_, (fp, path)))| (k.serialize().to_vec(), origin_text(fp, path)))
        .collect()
}

fn ecdsa_origins(map: &BTreeMap<PublicKey, (Fingerprint, DerivationPath)>) -> Origins {
    map.iter()
        .map(|(k, (fp, path))| (k.serialize().to_vec(), origin_text(fp, path)))
        .collect()
}

fn p2tr_output_key(script: &Script) -> Option<XOnlyPublicKey> {
    script
        .is_p2tr()
        .then(|| XOnlyPublicKey::from_slice(&script.as_bytes()[2..34]).ok())
        .flatten()
}

/// Multisig details for the scripts an input carries. Empty when the input
/// carries no witness script or tap leaf script.
pub fn input_scripts(input: &Input, prevout_script: Option<&Script>) -> Vec<ScriptDetail> {
    let mut out = Vec::new();
    if let Some(ws) = &input.witness_script {
        out.push(wsh_detail(
            ws,
            prevout_script,
            input.redeem_script.as_deref(),
            &ecdsa_origins(&input.bip32_derivation),
        ));
    }
    let origins = tap_origins(&input.tap_key_origins);
    let secp = Secp256k1::verification_only();
    for (control, (leaf, _version)) in &input.tap_scripts {
        let bound = prevout_script
            .and_then(p2tr_output_key)
            .is_some_and(|key| control.verify_taproot_commitment(&secp, key, leaf));
        out.push(tap_detail(leaf, control.internal_key, bound, &origins));
    }
    out
}

/// Multisig details for the scripts an output carries.
pub fn output_scripts(output: &Output, script_pubkey: &Script) -> Vec<ScriptDetail> {
    let mut out = Vec::new();
    if let Some(ws) = &output.witness_script {
        out.push(wsh_detail(
            ws,
            Some(script_pubkey),
            output.redeem_script.as_deref(),
            &ecdsa_origins(&output.bip32_derivation),
        ));
    }
    if let (Some(tree), Some(internal)) = (&output.tap_tree, output.tap_internal_key) {
        let secp = Secp256k1::verification_only();
        let info = TaprootSpendInfo::from_node_info(&secp, internal, tree.clone().into_node_info());
        let committed = ScriptBuf::new_p2tr_tweaked(info.output_key());
        let bound = committed.as_script() == script_pubkey;
        let origins = tap_origins(&output.tap_key_origins);
        for leaf in tree.script_leaves() {
            out.push(tap_detail(leaf.script(), internal, bound, &origins));
        }
    }
    out
}
