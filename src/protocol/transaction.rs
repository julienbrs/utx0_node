use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::{storage::api::ObjectStore, util::canonical::to_canonical_json};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    #[serde(rename = "type")]
    kind: String,
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

        let sig_vec = hex::decode(&input.sig).map_err(|_| TxValidationError::InvalidSignature)?;
        let sig_array: [u8; 64] =
            sig_vec.as_slice().try_into().map_err(|_| TxValidationError::InvalidSignature)?;

        let sig: Signature = Signature::from_bytes(&sig_array);

        pub_key
            .verify_strict(&canon_sig_obj, &sig)
            .map_err(|_| TxValidationError::InvalidSignature)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::storage::error::StorageError;
    use crate::util::canonical::compute_object_id;

    use super::*;
    use ed25519_dalek::{SigningKey, VerifyingKey};
    use rand::rngs::OsRng;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct DummyStore {
        map: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl DummyStore {
        fn new() -> Self {
            DummyStore { map: Mutex::new(HashMap::new()) }
        }
        fn insert(&self, txid: &str, bytes: Vec<u8>) {
            self.map.lock().unwrap().insert(txid.to_string(), bytes);
        }
    }

    impl ObjectStore for &DummyStore {
        fn has(&self, id: &str) -> Result<bool, StorageError> {
            Ok(self.map.lock().unwrap().contains_key(id))
        }

        fn get(&self, id: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(self.map.lock().unwrap().get(id).cloned())
        }

        fn put(&self, _id: &str, _data: &[u8]) -> Result<Option<Vec<u8>>, StorageError> {
            unimplemented!()
        }
    }

    ///  valid coinbase
    #[test]
    fn coinbase_ok() {
        let store = DummyStore::new();
        let coinbase = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "height": 42,
                "inputs": [],
                "outputs": [
                  { "pubkey": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "value": 100 }
                ]
            }
        });
        assert!(validate_transaction(&coinbase, &store).is_ok());
    }

    #[test]
    fn revert_coinbase_non_empty_input() {
        let store = DummyStore::new();
        let coinbase = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "height": 42,
                "inputs": [
                    { "outpoint": { "txid": "dummy", "index": 1 }, "sig": "" }
                  ],
                "outputs": [
                  { "pubkey": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "value": 100 }
                ]
            }
        });
        let e = validate_transaction(&coinbase, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidFormat));
    }

    #[test]
    fn revert_coinbase_no_height() {
        let store = DummyStore::new();
        let coinbase = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "height": "",
                "inputs": [],
                "outputs": [
                  { "pubkey": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "value": 100 }
                ]
            }
        });
        let e = validate_transaction(&coinbase, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidFormat));
    }

    /// invalid format if no object
    #[test]
    fn missing_object_format_err() {
        let store = DummyStore::new();
        let bad = json!({ "type": "object" });
        let e = validate_transaction(&bad, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidFormat));
    }

    /// UnknownObject if store.get returns none for an input
    #[test]
    fn unknown_object_err() {
        let store = DummyStore::new();
        let tx = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "inputs": [
                  { "outpoint": { "txid": "deadbeef", "index": 0 }, "sig": "" }
                ],
                "outputs": [ { "pubkey": "", "value": 1 } ]
            }
        });
        let e = validate_transaction(&tx, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidOutpoint));
    }

    /// InvalidOutpoint if index out of bounds
    #[test]
    fn invalid_outpoint_err() {
        // parent with a single output
        let parent = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "inputs": [],
                "outputs": [ { "pubkey": "", "value": 50 } ]
            }
        });
        let bytes = to_canonical_json(&parent).unwrap();
        let parent_id = compute_object_id(&parent).unwrap();

        let store = DummyStore::new();
        store.insert(&parent_id, bytes);

        let bad = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "inputs": [
                  { "outpoint": { "txid": parent_id, "index": 1 }, "sig": "" }
                ],
                "outputs": [ { "pubkey": "", "value": 1 } ]
            }
        });
        let e = validate_transaction(&bad, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidOutpoint));
    }

    /// Conservation error if sum inputs < sum outputs
    #[test]
    fn conservation_err() {
        // parent à 1
        let parent = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "inputs": [],
                "outputs": [ { "pubkey": "", "value": 1 } ]
            }
        });
        let bytes = to_canonical_json(&parent).unwrap();
        let parent_id = compute_object_id(&parent).unwrap();

        let store = DummyStore::new();
        store.insert(&parent_id, bytes);

        // spend 1 + create output of 2 => conservation error
        let tx = json!({
            "type": "object",
            "object": {
                "type": "transaction",
                "inputs": [
                  { "outpoint": { "txid": parent_id, "index": 0 }, "sig": "" }
                ],
                "outputs": [ { "pubkey": "", "value": 2 } ]
            }
        });
        let e = validate_transaction(&tx, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidConservation));
    }

    /// invalid signature
    #[test]
    fn invalid_signature_err() {
        // parent got 5
        let mut csprng = OsRng;
        let signing = SigningKey::generate(&mut csprng);
        let verifying: VerifyingKey = signing.verifying_key();

        // parent transaction
        let parent = json!({
            "type":"object","object":{
                "type":"transaction",
                "inputs":[],
                "outputs":[{"pubkey": hex::encode(verifying.as_bytes()), "value":5}]
            }
        });
        let bytes = to_canonical_json(&parent).unwrap();
        let parent_id = compute_object_id(&parent).unwrap();

        let store = DummyStore::new();
        store.insert(&parent_id, bytes);

        // not signing the tx
        let tx_obj = json!({
            "type":"object","object":{
                "type":"transaction",
                "inputs":[{"outpoint":{ "txid": parent_id, "index": 0},"sig": ""}],
                "outputs":[{"pubkey":"00","value":5}]
            }
        });
        let e = validate_transaction(&tx_obj, &store).unwrap_err();
        assert!(matches!(e, TxValidationError::InvalidSignature));
    }
}
