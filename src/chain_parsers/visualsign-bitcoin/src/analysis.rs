//! Key-free analysis of a PSBT: what is spent, what is paid, and how much of
//! that the parser can actually vouch for.
//!
//! The parser holds no keys, so it cannot tell which outputs are change. It
//! reports what the PSBT claims and flags every claim as unverified.
//!
//! Amounts: a segwit v0 sighash commits only to the amount of its own input, so
//! a lying `witness_utxo` can hide an arbitrary fee. An input amount is
//! therefore only `Verified` when a `non_witness_utxo` hashes to the prevout
//! txid. The fee is verified when every input is verified, or when every input
//! spends a taproot output (a BIP-341 signature commits to all input amounts,
//! so whichever input gets signed binds the rest).

use crate::scripts::{self, ScriptDetail};
use bitcoin::psbt::{Input, Output, Psbt};
use bitcoin::{Address, Amount, Network, OutPoint, Script, ScriptBuf, TxIn, TxOut};

/// Why the PSBT cannot be analysed.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum BitcoinParserError {
    #[error("Failed to decode transaction: {0}")]
    FailedToDecodeTransaction(String),
    #[error("Malformed PSBT: {0}")]
    Malformed(String),
}

/// How an input amount is known.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AmountSource {
    /// From a previous transaction that hashes to the prevout txid.
    Verified,
    /// From `witness_utxo` only; nothing binds it to the real prevout.
    Claimed,
    /// Not provided.
    Missing,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InputInfo {
    pub outpoint: OutPoint,
    pub amount: Option<Amount>,
    pub source: AmountSource,
    pub address: Option<String>,
    pub script_type: Option<&'static str>,
    /// Set when the PSBT asks for a sighash type other than ALL / DEFAULT.
    pub sighash_warning: Option<String>,
    /// Multisig / script details from the witness script or tap leaf scripts.
    pub scripts: Vec<ScriptDetail>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OutputInfo {
    pub amount: Amount,
    pub address: Option<String>,
    pub script_hex: String,
    pub script_type: &'static str,
    /// The PSBT carries derivation data for this output, i.e. it claims the
    /// output returns to the signer's wallet. The parser cannot check that.
    pub claims_change: bool,
    pub scripts: Vec<ScriptDetail>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Fee {
    Verified(Amount),
    Unverified { fee: Amount, reason: &'static str },
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Analysis {
    pub inputs: Vec<InputInfo>,
    pub outputs: Vec<OutputInfo>,
    pub fee: Fee,
    pub lock_time: u32,
}

const UNVERIFIED_REASON: &str =
    "input amounts come from witness_utxo without a matching previous transaction";

pub fn analyze(psbt: &Psbt, network: Network) -> Result<Analysis, BitcoinParserError> {
    let tx = &psbt.unsigned_tx;
    if tx.input.len() != psbt.inputs.len() || tx.output.len() != psbt.outputs.len() {
        return Err(BitcoinParserError::Malformed(
            "input/output maps do not match the unsigned transaction".into(),
        ));
    }

    let mut inputs = Vec::with_capacity(tx.input.len());
    let mut prevout_scripts = Vec::with_capacity(tx.input.len());
    for (txin, input) in tx.input.iter().zip(&psbt.inputs) {
        let (prevout, source) = resolve_prevout(txin, input)?;
        let is_taproot = prevout.is_some_and(|o| o.script_pubkey.is_p2tr());
        prevout_scripts.push(prevout.map(|o| o.script_pubkey.clone()));
        inputs.push(InputInfo {
            outpoint: txin.previous_output,
            amount: prevout.map(|o| o.value),
            source,
            address: prevout.and_then(|o| address(&o.script_pubkey, network)),
            script_type: prevout.map(|o| script_type(&o.script_pubkey)),
            sighash_warning: sighash_warning(input, is_taproot),
            scripts: scripts::input_scripts(input, prevout.map(|o| o.script_pubkey.as_script())),
        });
    }

    let mut outputs = Vec::with_capacity(tx.output.len());
    let mut output_total = Amount::ZERO;
    for (out, meta) in tx.output.iter().zip(&psbt.outputs) {
        output_total = output_total
            .checked_add(out.value)
            .ok_or_else(|| BitcoinParserError::Malformed("output total overflows".into()))?;
        outputs.push(OutputInfo {
            amount: out.value,
            address: address(&out.script_pubkey, network),
            script_hex: out.script_pubkey.to_hex_string(),
            script_type: script_type(&out.script_pubkey),
            claims_change: claims_change(meta),
            scripts: scripts::output_scripts(meta, &out.script_pubkey),
        });
    }

    let fee = compute_fee(&inputs, &prevout_scripts, output_total)?;
    Ok(Analysis {
        inputs,
        outputs,
        fee,
        lock_time: tx.lock_time.to_consensus_u32(),
    })
}

fn compute_fee(
    inputs: &[InputInfo],
    prevout_scripts: &[Option<ScriptBuf>],
    output_total: Amount,
) -> Result<Fee, BitcoinParserError> {
    let mut input_total = Amount::ZERO;
    for info in inputs {
        let Some(amount) = info.amount else {
            return Ok(Fee::Unknown);
        };
        input_total = input_total
            .checked_add(amount)
            .ok_or_else(|| BitcoinParserError::Malformed("input total overflows".into()))?;
    }
    let fee = input_total
        .checked_sub(output_total)
        .ok_or_else(|| BitcoinParserError::Malformed("outputs exceed inputs".into()))?;

    let all_verified = inputs.iter().all(|i| i.source == AmountSource::Verified);
    let all_taproot = !prevout_scripts.is_empty()
        && prevout_scripts
            .iter()
            .all(|s| s.as_ref().is_some_and(|s| s.is_p2tr()));
    Ok(if all_verified || all_taproot {
        Fee::Verified(fee)
    } else {
        Fee::Unverified {
            fee,
            reason: UNVERIFIED_REASON,
        }
    })
}

/// The output `txin` spends, and how well that is known. A `non_witness_utxo`
/// must hash to the prevout txid and agree with any `witness_utxo`.
fn resolve_prevout<'a>(
    txin: &TxIn,
    input: &'a Input,
) -> Result<(Option<&'a TxOut>, AmountSource), BitcoinParserError> {
    if let Some(prev) = &input.non_witness_utxo {
        if prev.compute_txid() != txin.previous_output.txid {
            return Err(BitcoinParserError::Malformed(
                "non_witness_utxo does not match the prevout txid".into(),
            ));
        }
        let out = usize::try_from(txin.previous_output.vout)
            .ok()
            .and_then(|v| prev.output.get(v))
            .ok_or_else(|| BitcoinParserError::Malformed("prevout index out of range".into()))?;
        if let Some(w) = &input.witness_utxo
            && (w.value != out.value || w.script_pubkey != out.script_pubkey)
        {
            return Err(BitcoinParserError::Malformed(
                "witness_utxo disagrees with non_witness_utxo".into(),
            ));
        }
        return Ok((Some(out), AmountSource::Verified));
    }
    Ok(match &input.witness_utxo {
        Some(w) => (Some(w), AmountSource::Claimed),
        None => (None, AmountSource::Missing),
    })
}

fn claims_change(meta: &Output) -> bool {
    !meta.bip32_derivation.is_empty() || !meta.tap_key_origins.is_empty()
}

fn sighash_warning(input: &Input, is_taproot: bool) -> Option<String> {
    let ty = input.sighash_type?;
    if is_taproot {
        match ty.taproot_hash_ty() {
            Ok(bitcoin::TapSighashType::Default | bitcoin::TapSighashType::All) => None,
            Ok(other) => Some(other.to_string()),
            Err(_) => Some(format!("invalid ({})", ty.to_u32())),
        }
    } else {
        match ty.ecdsa_hash_ty() {
            Ok(bitcoin::EcdsaSighashType::All) => None,
            Ok(other) => Some(other.to_string()),
            Err(_) => Some(format!("invalid ({})", ty.to_u32())),
        }
    }
}

fn address(script: &Script, network: Network) -> Option<String> {
    Address::from_script(script, network)
        .ok()
        .map(|a| a.to_string())
}

fn script_type(script: &Script) -> &'static str {
    if script.is_p2pkh() {
        "P2PKH"
    } else if script.is_p2sh() {
        "P2SH"
    } else if script.is_p2wpkh() {
        "P2WPKH"
    } else if script.is_p2wsh() {
        "P2WSH"
    } else if script.is_p2tr() {
        "P2TR"
    } else if script.is_op_return() {
        "OP_RETURN"
    } else {
        "non-standard"
    }
}
