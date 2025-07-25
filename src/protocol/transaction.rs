use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::{storage::api::ObjectStore, util::canonical::to_canonical_json};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    // height bloc value for mined utx0 (coinbase), none for usual tx
    #[serde(default)]
    pub height: Option<u64>,

    // utx0 inputs. should be empty for coinbase
    pub inputs: Vec<Input>,

    // can't be empty
    pub outputs: Vec<Output>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub outpoint: Outpoint,
    pub sig: String, // hex 64
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outpoint {
    pub txid: String, // hex 32
    pub index: usize, // position in outputs
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub pubkey: String, // hex 32)
    pub value: u64,     // >= 0
}

#[derive(Debug, Error)]
pub enum TxValidationError {
    #[error("invalid JSON format")]
    InvalidFormat,
    #[error("referenced object not found")]
    UnknownObject,
    #[error("invalid outpoint")]
    InvalidOutpoint,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("conservation law violated")]
    InvalidConservation,
}

pub fn validate_transaction<S: ObjectStore>(
    obj: &Value,
    store: S,
) -> Result<(), TxValidationError> {
    let tx_json = obj.get("object").cloned().ok_or(TxValidationError::InvalidFormat)?;
    let tx: Transaction =
        serde_json::from_value(tx_json).map_err(|_| TxValidationError::InvalidFormat)?;

    // coinbase tx?
    if tx.height.is_some() {
        // coinbase should have no inputs but outputs
        if !tx.inputs.is_empty() || tx.outputs.is_empty() {
            return Err(TxValidationError::InvalidFormat);
        }
        return Ok(());
    }

    // classical tx: inputs and outputs >=1
    if tx.inputs.is_empty() || tx.outputs.is_empty() {
        return Err(TxValidationError::InvalidFormat);
    }

    // conservation law
    let mut sum_inputs: u128 = 0;
    for input in &tx.inputs {
        // validate outpoint
        let parent_bytes = store
            .get(&input.outpoint.txid)
            .map_err(|_| TxValidationError::UnknownObject)?
            .ok_or(TxValidationError::InvalidOutpoint)?;

        let parent_json: Value =
            serde_json::from_slice(&parent_bytes).map_err(|_| TxValidationError::InvalidFormat)?;
        let parent_tx: Transaction = serde_json::from_value(
            parent_json.get("object").ok_or(TxValidationError::InvalidFormat)?.clone(),
        )
        .map_err(|_| TxValidationError::InvalidFormat)?;

        if input.outpoint.index >= parent_tx.outputs.len() {
            return Err(TxValidationError::InvalidOutpoint);
        }

        sum_inputs = sum_inputs
            .checked_add(parent_tx.outputs[input.outpoint.index].value as u128)
            .ok_or(TxValidationError::InvalidConservation)?;
    }

    // sum outputs
    let sum_outputs: u128 = tx
        .outputs
        .iter()
        .map(|o| o.value as u128)
        .try_fold(0u128, |acc, v| acc.checked_add(v))
        .ok_or(TxValidationError::InvalidConservation)?;

    if sum_inputs < sum_outputs {
        return Err(TxValidationError::InvalidConservation);
    }

    let mut sig_obj = obj.clone();
    if let Some(ref mut wrapper_map) = sig_obj.as_object_mut() {
        if let Some(inner_value) = wrapper_map.get_mut("object") {
            if let Some(inner_map) = inner_value.as_object_mut() {
                // inner_map is the transaction payload
                if let Some(Value::Array(inputs)) = inner_map.get_mut("inputs") {
                    for value in inputs {
                        if let Value::Object(map_inputs) = value {
                            map_inputs.insert("sig".to_string(), Value::String(String::new()));
                        }
                    }
                }
            }
        }
    }

    let canon_sig_obj =
        to_canonical_json(&sig_obj).map_err(|_| TxValidationError::InvalidFormat)?;

    for input in &tx.inputs {
        let parent_bytes =
            store.get(&input.outpoint.txid).map_err(|_| TxValidationError::UnknownObject)?.unwrap();
        let parent_json: Value =
            serde_json::from_slice(&parent_bytes).map_err(|_| TxValidationError::InvalidFormat)?;
        let tx_value =
            parent_json.get("object").cloned().ok_or(TxValidationError::InvalidFormat)?;
        let parent_tx: Transaction =
            serde_json::from_value(tx_value).map_err(|_| TxValidationError::InvalidFormat)?;

        let pub_key_hex = &parent_tx.outputs[input.outpoint.index].pubkey;
        let bytes_vec = hex::decode(pub_key_hex).map_err(|_| TxValidationError::InvalidFormat)?;

        let pub_key_array: [u8; 32] =
            bytes_vec.as_slice().try_into().map_err(|_| TxValidationError::InvalidFormat)?;
        let pub_key: VerifyingKey = VerifyingKey::from_bytes(&pub_key_array)
            .map_err(|_| TxValidationError::InvalidFormat)?;

        let sig_vec = hex::decode(&input.sig).map_err(|_| TxValidationError::InvalidFormat)?;
        let sig_array: [u8; 64] =
            sig_vec.as_slice().try_into().map_err(|_| TxValidationError::InvalidFormat)?;
        let sig: Signature = Signature::from_bytes(&sig_array);

        pub_key
            .verify_strict(&canon_sig_obj, &sig)
            .map_err(|_| TxValidationError::InvalidSignature)?;
    }
    Ok(())
}
