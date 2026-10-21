//! Bitcoin PSBT (BIP-174) parser for VisualSign.
//!
//! Decodes a base64 or hex PSBT into inputs, outputs and fee. The parser holds
//! no keys: it cannot tell change from payments, so outputs carrying
//! derivation data are shown as "claims change (unverified by parser)", and a
//! fee whose input amounts are not bound by a previous transaction is flagged
//! as unverified. See [`analysis`] for the exact rules.

mod analysis;
#[cfg(feature = "cli-plugin")]
pub mod cli_plugin;

#[cfg(feature = "cli-plugin")]
pub use cli_plugin::{BitcoinArgs, BitcoinPlugin};

pub use analysis::BitcoinParserError;

use analysis::{AmountSource, Analysis, Fee, InputInfo, OutputInfo};
use base64::{Engine as _, engine::general_purpose::STANDARD as b64};
use bitcoin::psbt::Psbt;
use bitcoin::{Amount, Network};
use visualsign::encodings::SupportedEncodings;
use visualsign::field_builders::{
    create_address_field, create_amount_field, create_preview_layout, create_text_field,
};
use visualsign::vsptrait::{
    ConversionResult, Transaction, TransactionParseError, VisualSignConverter,
    VisualSignConverterFromString, VisualSignError, VisualSignOptions,
};
use visualsign::{AnnotatedPayloadField, SignablePayload};

/// Upper bound on the encoded input. Real PSBTs with full previous
/// transactions are well below this.
const MAX_PSBT_CHARS: usize = 4 * 1024 * 1024;

/// Text shown next to outputs that carry derivation data.
const CLAIMS_CHANGE: &str = "claims change (unverified by parser)";

/// Parse a network name as accepted by `--bitcoin-network`.
pub fn parse_network(name: &str) -> Result<Network, String> {
    match name.to_ascii_lowercase().as_str() {
        "bitcoin" | "mainnet" => Ok(Network::Bitcoin),
        "testnet" | "testnet3" => Ok(Network::Testnet),
        "testnet4" => Ok(Network::Testnet4),
        "signet" => Ok(Network::Signet),
        "regtest" => Ok(Network::Regtest),
        other => Err(format!(
            "unknown Bitcoin network '{other}' (expected bitcoin, testnet, testnet4, signet or regtest)"
        )),
    }
}

fn network_label(network: Network) -> String {
    match network {
        Network::Bitcoin => "Bitcoin Mainnet".to_string(),
        Network::Testnet => "Bitcoin Testnet".to_string(),
        Network::Testnet4 => "Bitcoin Testnet4".to_string(),
        Network::Signet => "Bitcoin Signet".to_string(),
        Network::Regtest => "Bitcoin Regtest".to_string(),
    }
}

fn decode_psbt(data: &str) -> Result<Psbt, BitcoinParserError> {
    let data = data.trim();
    if data.len() > MAX_PSBT_CHARS {
        return Err(BitcoinParserError::FailedToDecodeTransaction(
            "PSBT is too large".into(),
        ));
    }
    let bytes = match SupportedEncodings::detect(data) {
        SupportedEncodings::Hex => visualsign::encodings::decode_hex(data).map_err(|e| {
            BitcoinParserError::FailedToDecodeTransaction(format!("Failed to decode hex: {e}"))
        })?,
        SupportedEncodings::Base64 => b64.decode(data).map_err(|e| {
            BitcoinParserError::FailedToDecodeTransaction(format!("Failed to decode base64: {e}"))
        })?,
    };
    Psbt::deserialize(&bytes).map_err(|e| {
        BitcoinParserError::FailedToDecodeTransaction(format!("Failed to parse PSBT: {e}"))
    })
}

/// Wrapper for a parsed PSBT.
#[derive(Debug, Clone)]
pub struct BitcoinTransactionWrapper {
    psbt: Psbt,
}

impl Transaction for BitcoinTransactionWrapper {
    fn from_string(data: &str) -> Result<Self, TransactionParseError> {
        let psbt =
            decode_psbt(data).map_err(|e| TransactionParseError::DecodeError(e.to_string()))?;
        Ok(Self { psbt })
    }

    fn transaction_type(&self) -> String {
        "Bitcoin".to_string()
    }
}

impl BitcoinTransactionWrapper {
    pub fn new(psbt: Psbt) -> Self {
        Self { psbt }
    }

    pub fn inner(&self) -> &Psbt {
        &self.psbt
    }
}

/// Converter for Bitcoin PSBTs. A PSBT does not name its network, so the
/// network (used for address encoding) is part of the converter.
#[derive(Debug, Clone, Copy)]
pub struct BitcoinVisualSignConverter {
    network: Network,
}

impl BitcoinVisualSignConverter {
    #[must_use]
    pub fn new(network: Network) -> Self {
        Self { network }
    }
}

impl Default for BitcoinVisualSignConverter {
    fn default() -> Self {
        Self::new(Network::Bitcoin)
    }
}

impl VisualSignConverter<BitcoinTransactionWrapper> for BitcoinVisualSignConverter {
    fn to_visual_sign_payload(
        &self,
        transaction_wrapper: BitcoinTransactionWrapper,
        options: VisualSignOptions,
    ) -> Result<ConversionResult, VisualSignError> {
        let analysis = analysis::analyze(transaction_wrapper.inner(), self.network)
            .map_err(|e| VisualSignError::ConversionError(e.to_string()))?;
        let payload = build_payload(&analysis, self.network, options)?;
        Ok(ConversionResult::new(payload))
    }
}

impl VisualSignConverterFromString<BitcoinTransactionWrapper> for BitcoinVisualSignConverter {}

/// Format satoshis as a fixed 8-decimal BTC string (valid for `create_amount_field`).
fn btc(amount: Amount) -> String {
    let sat = amount.to_sat();
    format!("{}.{:08}", sat / 100_000_000, sat % 100_000_000)
}

fn build_payload(
    analysis: &Analysis,
    network: Network,
    options: VisualSignOptions,
) -> Result<SignablePayload, VisualSignError> {
    let mut fields: Vec<AnnotatedPayloadField> =
        vec![create_text_field("Network", &network_label(network))?];

    for (i, input) in analysis.inputs.iter().enumerate() {
        fields.push(input_field(i, input)?);
    }
    for (i, output) in analysis.outputs.iter().enumerate() {
        fields.push(output_field(i, output)?);
    }

    match &analysis.fee {
        Fee::Verified(fee) => fields.push(create_amount_field("Fee", &btc(*fee), "BTC")?),
        Fee::Unverified { fee, reason } => fields.push(create_text_field(
            "Fee",
            &format!("{} BTC (unverified: {reason})", btc(*fee)),
        )?),
        Fee::Unknown => fields.push(create_text_field("Fee", "unknown (input amounts missing)")?),
    }

    if analysis.lock_time != 0 {
        fields.push(create_text_field(
            "Lock Time",
            &analysis.lock_time.to_string(),
        )?);
    }

    let title = options
        .transaction_name
        .unwrap_or_else(|| "Bitcoin Transaction".to_string());

    Ok(SignablePayload::new(
        0,
        title,
        None,
        fields
            .into_iter()
            .map(|af| af.signable_payload_field)
            .collect(),
        "BitcoinTx".to_string(),
    ))
}

fn input_field(index: usize, input: &InputInfo) -> Result<AnnotatedPayloadField, VisualSignError> {
    let mut expanded = vec![create_text_field(
        "Outpoint",
        &format!("{}:{}", input.outpoint.txid, input.outpoint.vout),
    )?];
    if let Some(addr) = &input.address {
        expanded.push(create_address_field(
            "Address", addr, None, None, None, None,
        )?);
    }
    if let Some(script_type) = input.script_type {
        expanded.push(create_text_field("Script Type", script_type)?);
    }
    let amount_text = match input.amount {
        Some(amount) => {
            expanded.push(create_amount_field("Amount", &btc(amount), "BTC")?);
            format!("{} BTC", btc(amount))
        }
        None => {
            expanded.push(create_text_field("Amount", "unknown (no UTXO data)")?);
            "amount unknown".to_string()
        }
    };
    expanded.push(create_text_field(
        "Amount Source",
        match input.source {
            AmountSource::Verified => "verified against previous transaction",
            AmountSource::Claimed => "claimed by witness_utxo only",
            AmountSource::Missing => "missing",
        },
    )?);
    if let Some(warning) = &input.sighash_warning {
        expanded.push(create_text_field("Sighash Type", warning)?);
    }
    let subtitle = match &input.address {
        Some(addr) => format!("{amount_text} from {addr}"),
        None => amount_text,
    };
    Ok(create_preview_layout(
        &format!("Input {index}"),
        subtitle,
        expanded,
    ))
}

fn output_field(
    index: usize,
    output: &OutputInfo,
) -> Result<AnnotatedPayloadField, VisualSignError> {
    let mut expanded = Vec::new();
    match &output.address {
        Some(addr) => expanded.push(create_address_field(
            "Address", addr, None, None, None, None,
        )?),
        None => expanded.push(create_text_field("Script", &output.script_hex)?),
    }
    expanded.push(create_text_field("Script Type", output.script_type)?);
    expanded.push(create_amount_field("Amount", &btc(output.amount), "BTC")?);
    if output.claims_change {
        expanded.push(create_text_field("Change", CLAIMS_CHANGE)?);
    }
    let target = match &output.address {
        Some(addr) => addr.clone(),
        None => format!("script {}", output.script_hex),
    };
    let mut subtitle = format!("{target} {} BTC", btc(output.amount));
    if output.claims_change {
        subtitle.push_str(&format!(" - {CLAIMS_CHANGE}"));
    }
    Ok(create_preview_layout(
        &format!("Output {index}"),
        subtitle,
        expanded,
    ))
}
