//! The Bitcoin display engine as one WebAssembly export, [`display`].
//!
//! In: a base64 PSBT v0, the network, the owned-script list as JSON
//! (`[{"scriptHex": "...", "label": "..."}]`) and the policy. Out, as a string:
//!
//! - `{"payload":<SignablePayload JSON>,"digest":"<sha256 hex>"}`, where the
//!   payload is exactly the parser's serialization and the digest is SHA-256
//!   over exactly those payload bytes; or
//! - `{"refused":"<code>"}`.
//!
//! It never throws across the boundary: the export takes any JavaScript
//! values, and a missing or non-string argument is a refusal like any other
//! failure. There is no default network and no mainnet alias: the caller
//! names one of `bitcoin`, `testnet4` or `regtest`.

use bitcoin::Network;
use sha2::{Digest, Sha256};
use visualsign_bitcoin_owned::{OwnedScripts, Policy, Refusal};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

/// Base64 of the PSBT magic `psbt 0xff`: every base64 PSBT starts with it.
const PSBT_BASE64_PREFIX: &str = "cHNidP8";

/// The WebAssembly export `display(psbt_base64, network, owned_json, policy)`.
/// Any argument that is not a string is treated as missing.
#[wasm_bindgen(js_name = display)]
#[must_use]
pub fn display_js(
    psbt_base64: JsValue,
    network: JsValue,
    owned_json: JsValue,
    policy: JsValue,
) -> String {
    display(
        psbt_base64.as_string().as_deref(),
        network.as_string().as_deref(),
        owned_json.as_string().as_deref(),
        policy.as_string().as_deref(),
    )
}

/// Display `psbt_base64`, or say why not. See the crate documentation.
#[must_use]
pub fn display(
    psbt_base64: Option<&str>,
    network: Option<&str>,
    owned_json: Option<&str>,
    policy: Option<&str>,
) -> String {
    match render(psbt_base64, network, owned_json, policy) {
        Ok(out) => out,
        Err(code) => format!("{{\"refused\":\"{code}\"}}"),
    }
}

fn parse_network(name: &str) -> Option<Network> {
    match name {
        "bitcoin" => Some(Network::Bitcoin),
        "testnet4" => Some(Network::Testnet4),
        "regtest" => Some(Network::Regtest),
        _ => None,
    }
}

fn parse_policy(name: &str) -> Option<Policy> {
    match name {
        "none" => Some(Policy::None),
        "vault-withdrawal" => Some(Policy::VaultWithdrawal),
        _ => None,
    }
}

fn render(
    psbt_base64: Option<&str>,
    network: Option<&str>,
    owned_json: Option<&str>,
    policy: Option<&str>,
) -> Result<String, &'static str> {
    let network = network.and_then(parse_network).ok_or("bad-network")?;
    let policy = policy.and_then(parse_policy).ok_or("bad-policy")?;
    let psbt = psbt_base64
        .filter(|p| p.starts_with(PSBT_BASE64_PREFIX))
        .ok_or("not-base64-psbt")?;
    let owned =
        OwnedScripts::from_json(owned_json.ok_or("bad-owned-json")?).map_err(Refusal::code)?;
    let payload =
        visualsign_bitcoin_owned::display(psbt, network, &owned, policy).map_err(Refusal::code)?;
    let payload_json = payload.to_json().map_err(|_| "serialize-failed")?;
    Ok(envelope(&payload_json))
}

/// `{"payload":<payload_json>,"digest":"<sha256 of payload_json>"}`.
fn envelope(payload_json: &str) -> String {
    let digest = hex::encode(Sha256::digest(payload_json.as_bytes()));
    format!("{{\"payload\":{payload_json},\"digest\":\"{digest}\"}}")
}
