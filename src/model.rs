use serde_json::Value;
use serde_json::json;
use serde::Serialize;

pub(crate) struct OpenFaaSResponse {
    pub status: u16,
    pub body: Vec<u8>
}

#[derive(Clone, Serialize)]
pub (crate) enum BodyData {
    Integer(i32),
    Float(f64),
    String(String),
    Boolean(bool),
    Json(Value),
}


pub (crate) fn detect_and_parse_body(body: Vec<u8>) -> BodyData {
    // Convert Vec<u8> to String
    let string_body = match String::from_utf8(body.clone()) {
        Ok(s) => s,
        Err(_) => return BodyData::String("Invalid UTF-8 data".to_string()),
    };

    // Try to parse as JSON
    if let Ok(json_value) = serde_json::from_str::<Value>(&string_body) {
        return BodyData::Json(json_value);
    }

    // Try to parse as integer
    if let Ok(int_value) = string_body.parse::<i32>() {
        return BodyData::Integer(int_value);
    }

    // Try to parse as float
    if let Ok(float_value) = string_body.parse::<f64>() {
        return BodyData::Float(float_value);
    }

    // Try to parse as boolean
    if let Ok(bool_value) = string_body.parse::<bool>() {
        return BodyData::Boolean(bool_value);
    }

    // Default to string if not an integer, float, boolean, or JSON
    BodyData::String(string_body)
}

pub (crate) fn serialize_body(body: BodyData) -> serde_json::Value {
    let serialized_data: serde_json::Value = 
        match body {
            BodyData::Integer(val) => json!(val),
            BodyData::Float(val) => json!(val),
            BodyData::String(val) => json!(val),
            BodyData::Boolean(val) => json!(val),
            BodyData::Json(val) => val,
        };

    json!(serialized_data)
    
}