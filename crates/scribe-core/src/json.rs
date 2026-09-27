//! Small accessors over `serde_json::Value` so the builder reads like the
//! transcript it parses instead of like a chain of `and_then`s.

use serde_json::{Map, Value};

pub fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn bool_of(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub fn u64_of(v: &Value, key: &str) -> u64 {
    v.get(key)
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
        .unwrap_or(0)
}

pub fn obj_of<'a>(v: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    v.get(key).and_then(Value::as_object)
}

pub fn arr_of<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// The row's message content, as a list of blocks. A string content becomes
/// one text block so callers never branch on the shape.
pub fn blocks(row: &Value) -> Vec<Value> {
    let Some(message) = row.get("message").and_then(Value::as_object) else {
        return Vec::new();
    };
    match message.get("content") {
        Some(Value::String(s)) => vec![serde_json::json!({"type": "text", "text": s})],
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

pub fn block_type(b: &Value) -> &str {
    str_of(b, "type")
}
