use serde_json::{Error, Value};

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

#[cfg(test)]
mod tests {
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
}
