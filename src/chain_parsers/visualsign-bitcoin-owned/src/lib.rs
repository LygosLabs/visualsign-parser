//! Labels for bare scripts the caller owns, for the Bitcoin PSBT parser.
//!
//! A wallet knows scripts the parser cannot, such as its own bare vault
//! scripts. This crate names a bare script ONLY when its bytes are exactly
//! equal to a script the caller supplied together with a label. It never
//! parses or interprets a script, so a foreign script cannot be made to look
//! owned: it would have to equal a script the caller derived.
//!
//! - Outputs are the signed bytes: an owned output script is labelled.
//! - A spent (input) script is labelled only when the parser verified it
//!   through a `non_witness_utxo` that hashes to the prevout txid. A script
//!   claimed by `witness_utxo` alone is shown exactly as the parser shows it.
//! - Only bare scripts (no address form) can be owned, so a label never
//!   replaces or sits beside an address.
//! - Labels are 1 to 48 characters from letters, digits, space, `-`, `#`, `(`
//!   and `)`, start with `Your ` followed by something other than spaces, do
//!   not look like an address or an amount (see [`is_valid_label`]), and are
//!   unique ignoring ASCII case. Anything else refuses the whole call.
//!
//! [`Policy::VaultWithdrawal`] refuses the whole display unless every input is
//! verified through its previous transaction, every input and output script
//! is a standard address type or owned, no output pays a bare script that an
//! input spends, every input asks for SIGHASH_ALL (or nothing), no two inputs
//! spend the same outpoint, no owned script is an OP_RETURN or a witness
//! program, and the PSBT carries nothing that is unsigned and unneeded: inputs
//! hold only `non_witness_utxo`, a matching `witness_utxo` and `sighash_type`,
//! and outputs hold no fields at all.

use bitcoin::psbt::{Input, Output, Psbt, PsbtSighashType};
use bitcoin::{Address, EcdsaSighashType, Network, Script, ScriptBuf, TapSighashType};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use visualsign::SignablePayload;
use visualsign::vsptrait::{Transaction, VisualSignConverter, VisualSignOptions};
use visualsign_bitcoin::{
    BitcoinTransactionWrapper, BitcoinVisualSignConverter, RecognizedScript, ScriptRecognizer,
    ScriptSource,
};

/// Longest label, in characters.
pub const MAX_LABEL_CHARS: usize = 48;
/// Most owned scripts per call.
pub const MAX_OWNED_SCRIPTS: usize = 16;
/// Longest owned script: the consensus script size limit.
pub const MAX_SCRIPT_BYTES: usize = 10_000;
/// Longest owned-script JSON.
pub const MAX_OWNED_JSON_BYTES: usize = 1024 * 1024;

/// Every label starts with this.
const LABEL_PREFIX: &str = "Your ";

/// How Bitcoin addresses start: bech32 HRPs and the base58 version characters
/// of mainnet and testnet (P2PKH 1, m, n; P2SH 3, 2).
const ADDRESS_PREFIXES: &[&str] = &["bcrt1", "bc1", "tb1", "1", "3", "m", "n", "2"];

/// A word this long that starts like an address and is all letters and digits
/// reads as an address (or a truncated one) and is refused.
const ADDRESS_LIKE_MIN_CHARS: usize = 8;

/// The row added under every labelled script.
const MATCH_LABEL: &str = "Recognized By";
const MATCH_TEXT: &str = "exact match with a script the wallet supplied";

/// Why nothing is displayed. [`Refusal::code`] is stable and machine-readable.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Refusal {
    /// The owned list is not a JSON array of `{scriptHex, label}`, or too big.
    BadOwnedJson,
    TooManyOwnedScripts,
    /// An owned script is not hex, empty, or over 10,000 bytes.
    BadScriptHex,
    /// The same script appears twice in the owned list.
    DuplicateOwnedScript,
    BadLabel,
    /// The PSBT does not decode.
    BadPsbt,
    /// The parser refused the PSBT.
    Rejected,
    /// Vault withdrawal: an input has no previous transaction, or it does not
    /// hash to the prevout txid.
    UnverifiedInput,
    /// Vault withdrawal: a script is neither a standard address type nor owned.
    UnlabelledScript,
    /// Vault withdrawal: an output pays a bare script that an input spends.
    OutputPaysSpentScript,
    /// Vault withdrawal: an input asks for a sighash other than ALL / DEFAULT.
    UnsupportedSighash,
    /// An owned script has an address form; only bare scripts are labelled.
    OwnedScriptHasAddress,
    /// Two owned scripts have the same label (ignoring ASCII case).
    DuplicateLabel,
    /// Vault withdrawal: an owned script is an OP_RETURN or a witness program,
    /// which no vault is.
    OwnedScriptNotVault,
    /// Vault withdrawal: an input or output carries a PSBT field that is
    /// neither signed nor needed (only `non_witness_utxo`, a matching
    /// `witness_utxo` and `sighash_type` are allowed on inputs; nothing on
    /// outputs).
    UnsignedPsbtField,
    /// Vault withdrawal: two inputs spend the same outpoint.
    DuplicateInput,
}

impl Refusal {
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::BadOwnedJson => "bad-owned-json",
            Self::TooManyOwnedScripts => "too-many-owned-scripts",
            Self::BadScriptHex => "bad-script-hex",
            Self::DuplicateOwnedScript => "duplicate-owned-script",
            Self::BadLabel => "bad-label",
            Self::BadPsbt => "bad-psbt",
            Self::Rejected => "psbt-rejected",
            Self::UnverifiedInput => "unverified-input",
            Self::UnlabelledScript => "unlabelled-script",
            Self::OutputPaysSpentScript => "output-pays-spent-script",
            Self::UnsupportedSighash => "unsupported-sighash",
            Self::OwnedScriptHasAddress => "owned-script-has-address",
            Self::DuplicateLabel => "duplicate-label",
            Self::OwnedScriptNotVault => "owned-script-not-vault",
            Self::UnsignedPsbtField => "unsigned-psbt-field",
            Self::DuplicateInput => "duplicate-input",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Refusal {}

/// What the display must hold to.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Policy {
    /// The parser's own display, plus labels.
    None,
    /// A withdrawal from a vault: see the crate documentation.
    VaultWithdrawal,
}

/// Scripts the caller owns, each with its label.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct OwnedScripts {
    entries: Vec<(ScriptBuf, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedEntry {
    #[serde(rename = "scriptHex")]
    script_hex: String,
    label: String,
}

impl OwnedScripts {
    /// From `[{"scriptHex": "...", "label": "..."}]`.
    pub fn from_json(json: &str) -> Result<Self, Refusal> {
        if json.len() > MAX_OWNED_JSON_BYTES {
            return Err(Refusal::BadOwnedJson);
        }
        let entries: Vec<OwnedEntry> =
            serde_json::from_str(json).map_err(|_| Refusal::BadOwnedJson)?;
        Self::new(entries.into_iter().map(|e| (e.script_hex, e.label)))
    }

    /// From `(script hex, label)` pairs.
    pub fn new<I>(entries: I) -> Result<Self, Refusal>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut out: Vec<(ScriptBuf, String)> = Vec::new();
        for (script_hex, label) in entries {
            if out.len() == MAX_OWNED_SCRIPTS {
                return Err(Refusal::TooManyOwnedScripts);
            }
            let bytes = visualsign::encodings::decode_hex(&script_hex)
                .map_err(|_| Refusal::BadScriptHex)?;
            if bytes.is_empty() || bytes.len() > MAX_SCRIPT_BYTES {
                return Err(Refusal::BadScriptHex);
            }
            if !is_valid_label(&label) {
                return Err(Refusal::BadLabel);
            }
            let script = ScriptBuf::from_bytes(bytes);
            if has_address_form(&script) {
                return Err(Refusal::OwnedScriptHasAddress);
            }
            if out.iter().any(|(s, _)| *s == script) {
                return Err(Refusal::DuplicateOwnedScript);
            }
            if out.iter().any(|(_, l)| l.eq_ignore_ascii_case(&label)) {
                return Err(Refusal::DuplicateLabel);
            }
            out.push((script, label));
        }
        Ok(Self { entries: out })
    }

    fn scripts(&self) -> impl Iterator<Item = &Script> {
        self.entries.iter().map(|(s, _)| s.as_script())
    }

    /// The label of the owned script whose bytes equal `script`.
    #[must_use]
    pub fn label_for(&self, script: &Script) -> Option<&str> {
        self.entries
            .iter()
            .find(|(s, _)| s.as_script() == script)
            .map(|(_, label)| label.as_str())
    }
}

/// The parser shows this script as an address on some network.
fn has_address_form(script: &Script) -> bool {
    Address::from_script(script, Network::Bitcoin).is_ok()
}

/// A label is valid when it is 1 to [`MAX_LABEL_CHARS`] characters from the
/// allowlist, starts with [`LABEL_PREFIX`] followed by something other than
/// spaces, and after the prefix neither looks like an address nor like an
/// amount.
fn is_valid_label(label: &str) -> bool {
    (1..=MAX_LABEL_CHARS).contains(&label.len())
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'#' | b'(' | b')'))
        && label.strip_prefix(LABEL_PREFIX).is_some_and(|rest| {
            !rest.trim().is_empty() && !looks_like_address(rest) && !looks_like_amount(rest)
        })
}

/// Some word starts with an address prefix (any ASCII case) and is at least
/// [`ADDRESS_LIKE_MIN_CHARS`] letters and digits.
fn looks_like_address(text: &str) -> bool {
    text.split(' ').any(|word| {
        let lower = word.to_ascii_lowercase();
        word.len() >= ADDRESS_LIKE_MIN_CHARS
            && word.bytes().all(|b| b.is_ascii_alphanumeric())
            && ADDRESS_PREFIXES.iter().any(|p| lower.starts_with(p))
    })
}

/// A run of digits followed (after optional spaces) by `btc` or `sat`, in any
/// ASCII case.
fn looks_like_amount(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    (0..bytes.len()).any(|i| {
        let ends_digits =
            bytes[i].is_ascii_digit() && bytes.get(i + 1).is_none_or(|b| !b.is_ascii_digit());
        ends_digits && {
            let after = lower[i + 1..].trim_start_matches(' ');
            after.starts_with("btc") || after.starts_with("sat")
        }
    })
}

#[derive(Debug)]
struct OwnedRecognizer {
    owned: OwnedScripts,
}

impl ScriptRecognizer for OwnedRecognizer {
    fn recognize(&self, script_pubkey: &Script, source: ScriptSource) -> Option<RecognizedScript> {
        match source {
            ScriptSource::Output | ScriptSource::VerifiedPrevout => self
                .owned
                .label_for(script_pubkey)
                .map(|label| RecognizedScript {
                    name: label.to_string(),
                    fields: vec![(MATCH_LABEL.to_string(), MATCH_TEXT.to_string())],
                }),
            ScriptSource::ClaimedPrevout => None,
        }
    }
}

/// The parser's display of `psbt` (base64 or hex), with owned scripts
/// labelled, or why there is none.
pub fn display(
    psbt: &str,
    network: Network,
    owned: &OwnedScripts,
    policy: Policy,
) -> Result<SignablePayload, Refusal> {
    let wrapper = BitcoinTransactionWrapper::from_string(psbt).map_err(|_| Refusal::BadPsbt)?;
    if policy == Policy::VaultWithdrawal {
        check_vault_withdrawal(wrapper.inner(), owned)?;
    }
    let recognizer: Arc<dyn ScriptRecognizer> = Arc::new(OwnedRecognizer {
        owned: owned.clone(),
    });
    BitcoinVisualSignConverter::new(network)
        .with_recognizers(vec![recognizer])
        .to_visual_sign_payload(wrapper, VisualSignOptions::default())
        .map(|r| r.payload)
        .map_err(|_| Refusal::Rejected)
}

/// P2PKH, P2SH, P2WPKH, P2WSH or P2TR: a script the owner reads as an address.
fn is_address_type(script: &Script) -> bool {
    script.is_p2pkh()
        || script.is_p2sh()
        || script.is_p2wpkh()
        || script.is_p2wsh()
        || script.is_p2tr()
}

fn address_type_or_owned(script: &Script, owned: &OwnedScripts) -> Result<(), Refusal> {
    if is_address_type(script) || owned.label_for(script).is_some() {
        Ok(())
    } else {
        Err(Refusal::UnlabelledScript)
    }
}

fn check_vault_withdrawal(psbt: &Psbt, owned: &OwnedScripts) -> Result<(), Refusal> {
    let tx = &psbt.unsigned_tx;
    if tx.input.len() != psbt.inputs.len() || tx.output.len() != psbt.outputs.len() {
        return Err(Refusal::Rejected);
    }
    if owned
        .scripts()
        .any(|s| s.is_op_return() || s.is_witness_program())
    {
        return Err(Refusal::OwnedScriptNotVault);
    }
    let all = PsbtSighashType::from(EcdsaSighashType::All);
    let default = PsbtSighashType::from(TapSighashType::Default);
    let mut spent_bare: Vec<&Script> = Vec::with_capacity(tx.input.len());
    let mut outpoints = BTreeSet::new();
    for (txin, input) in tx.input.iter().zip(&psbt.inputs) {
        if !outpoints.insert(txin.previous_output) {
            return Err(Refusal::DuplicateInput);
        }
        let prev = input
            .non_witness_utxo
            .as_ref()
            .ok_or(Refusal::UnverifiedInput)?;
        if prev.compute_txid() != txin.previous_output.txid {
            return Err(Refusal::UnverifiedInput);
        }
        let prevout = usize::try_from(txin.previous_output.vout)
            .ok()
            .and_then(|v| prev.output.get(v))
            .ok_or(Refusal::UnverifiedInput)?;
        if input.sighash_type.is_some_and(|t| t != all && t != default) {
            return Err(Refusal::UnsupportedSighash);
        }
        if *input != signed_or_needed_only(input) {
            return Err(Refusal::UnsignedPsbtField);
        }
        address_type_or_owned(&prevout.script_pubkey, owned)?;
        if !is_address_type(&prevout.script_pubkey) {
            spent_bare.push(prevout.script_pubkey.as_script());
        }
    }
    if psbt.outputs.iter().any(|o| *o != Output::default()) {
        return Err(Refusal::UnsignedPsbtField);
    }
    for out in &tx.output {
        address_type_or_owned(&out.script_pubkey, owned)?;
        if spent_bare.contains(&out.script_pubkey.as_script()) {
            return Err(Refusal::OutputPaysSpentScript);
        }
    }
    Ok(())
}

/// `input` with every field dropped except the previous transaction, its
/// `witness_utxo` copy (the parser refuses one that disagrees) and the sighash
/// type (already limited to ALL / DEFAULT).
fn signed_or_needed_only(input: &Input) -> Input {
    Input {
        non_witness_utxo: input.non_witness_utxo.clone(),
        witness_utxo: input.witness_utxo.clone(),
        sighash_type: input.sighash_type,
        ..Input::default()
    }
}
