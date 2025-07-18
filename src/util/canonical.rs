use blake2::{Blake2s256, Digest};
use serde_json::{Error, Map, Value};

use crate::error::ProtocolError;

pub fn to_canonical_json(v: &Value) -> Result<Vec<u8>, Error> {
    match v {
        Value::Array(arr) => {
            let mut buf = Vec::with_capacity(128);
            buf.extend_from_slice(b"[");

            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    buf.extend_from_slice(b",");
                }
                buf.extend_from_slice(&to_canonical_json(v)?);
            }
            buf.extend_from_slice(b"]");
            Ok(buf)
        }
        Value::Object(obj) => {
            // sort keys
            let mut entries: Vec<(&String, &Value)> = obj.iter().collect();
            entries.sort_by_key(|(k, _)| *k);

            let mut buf = Vec::with_capacity(128);
            buf.extend_from_slice(b"{");
            for (i, (k, val)) in entries.iter().enumerate() {
                if i > 0 {
                    buf.extend_from_slice(b",");
                }
                serde_json::to_writer(&mut buf, k)?;
                buf.extend_from_slice(b":");
                buf.extend_from_slice(&to_canonical_json(&val)?);
            }
            buf.extend_from_slice(b"}");

            Ok(buf)
        }
        _ => Ok(serde_json::to_vec(v)?),
    }
}

pub fn compute_object_id(v: &Value) -> Result<String, Error> {
    let canonical_json = to_canonical_json(&v)?;
    let mut hasher = Blake2s256::new();
    hasher.update(canonical_json);
    let hash = hasher.finalize();
    Ok(hex::encode(hash))
}

pub fn ensure_object_field(raw: &Value) -> Result<&Map<String, Value>, ProtocolError> {
    let obj = raw.as_object().ok_or(ProtocolError::InvalidFormat)?;
    let inner = obj.get("object").ok_or(ProtocolError::InvalidFormat)?;
    let map = inner.as_object().ok_or(ProtocolError::InvalidFormat)?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::NamedTempFile;

    use crate::{
        storage::{RedbStore, api::ObjectStore},
        util::canonical::compute_object_id,
    };

    use super::to_canonical_json;

    fn canon(s: &str) -> String {
        // helper: parse & return the utf-8 string
        String::from_utf8(to_canonical_json(&serde_json::from_str(s).unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn primitive_string() {
        assert_eq!(canon(r#""foo""#), r#""foo""#);
    }

    #[test]
    fn primitive_number_bool_null() {
        assert_eq!(canon("123"), "123");
        assert_eq!(canon("true"), "true");
        assert_eq!(canon("null"), "null");
    }

    #[test]
    fn simple_array() {
        assert_eq!(canon("[ 2 , 1, [3 ,4] ]"), "[2,1,[3,4]]");
    }

    #[test]
    fn simple_object() {
        let input = r#"{ "b":2, "a":1 }"#;
        assert_eq!(canon(input), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn nested_mixed() {
        let input = r#"
        {
          "z": [ 3,1 ],
          "a": { "y": false, "x": true }
        }
        "#;
        let expected = r#"{"a":{"x":true,"y":false},"z":[3,1]}"#;
        assert_eq!(canon(input), expected);
    }

    #[test]
    fn escapes_in_string() {
        let input = r#"{ "s": "hello\nworld", "t":"\u1234" }"#;
        let out = canon(input);
        assert!(out.contains(r#""hello\nworld""#));
        assert!(out.contains("ሴ"));
        assert_eq!(&out[0..3], r#"{"s"#);
    }

    #[test]
    fn test_canonical_ordering() {
        let v1 = json!({ "b": 2, "a": 1 });
        let v2 = json!({ "a": 1, "b": 2 });
        let c1 = to_canonical_json(&v1).unwrap();
        let c2 = to_canonical_json(&v2).unwrap();
        assert_eq!(c1, c2, "canonical json should be the same");
        assert_eq!(String::from_utf8(c1).unwrap(), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn test_store_and_retrieve_bytes_roundtrip() {
        let db = NamedTempFile::new().unwrap();
        let store = RedbStore::new(db.path()).unwrap();
        let store = std::sync::Arc::new(store);

        let canonical = br#"{"a":1,"b":[2,3]}"#; // canonical bytes

        let id = "dummy_id";
        let old = store.put(id, canonical).expect("put ok");
        assert!(old.is_none(), "no old content");

        assert!(store.has(id).unwrap(), "store should know the id");

        let got = store.get(id).expect("get ok").expect("key existing");
        assert_eq!(got.as_slice(), canonical, "bits should be identical");

        let new_val = br#"{"a":42}"#;
        let prev = store.put(id, new_val).expect("second put ok").expect("old content retrieved");
        assert_eq!(prev.as_slice(), canonical, "getting the old value back");

        let got2 = store.get(id).unwrap().unwrap();
        assert_eq!(got2.as_slice(), new_val, "new value is stored");
    }

    #[test]
    fn compute_object_correct_hash_1() {
        let genesis = json!({
            "object": {
                "height": 0,
                "outputs": [
                    {
                        "pubkey": "85acb336a150b16a9c6c8c27a4e9c479d9f99060a7945df0bb1b53365e98969b",
                        "value": 50000000000000u64
                    }
                ],
                "type": "transaction"
            },
            "type": "object"
        });
        let id = compute_object_id(&genesis).unwrap();
        assert_eq!(id, "5fdb2f396f0007512f4d5dfd2391acd7cbc7c3ba905106507a000426f417e06e");
    }

    #[test]
    fn compute_object_correct_hash_2() {
        // dummy json, values mean nothing
        let spend = json!({
            "object": {
                "inputs": [
                    {
                        "outpoint": {
                            "txid": "d46d09138f0251edc32e28f1a744cb0b7286850e4c9c777d7e3c6e459b289347",
                            "index": 0
                        },
                        "sig": "6204bbdbacc86b1f4357cfe45e6374b963f5455f26df0a86338310df33e50c15d7f04"
                    }
                ],
                "type": "transaction"
            },
            "type": "object"
        });
        let id = compute_object_id(&spend).unwrap();
        assert_eq!(id, "824d9b6d4f97eef50beb44a9af1830ba836954b40e2c4bbfd2e65b5417ffc58c");
    }
}
