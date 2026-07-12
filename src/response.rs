//! Versioned JSON responses for the CLI's machine-readable output.

use schemars::JsonSchema;
use serde::Serialize;

/// Version of the CLI response envelope contract.
pub const RESPONSE_SCHEMA_VERSION: u32 = 1;

/// A versioned success or error response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Response<T> {
    /// Always [`RESPONSE_SCHEMA_VERSION`].
    pub schema_version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

/// Structured error information in a failed response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseError {
    pub kind: ErrorKind,
    pub message: String,
}

/// Stable machine-readable error categories.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotFound,
    Conflict,
    InvalidInput,
    Unsupported,
    Unavailable,
    Backend,
    Internal,
}

/// Serialize `data` in a success envelope to stdout.
pub fn emit<T: Serialize>(data: T) -> anyhow::Result<()> {
    let response = Response {
        schema_version: RESPONSE_SCHEMA_VERSION,
        ok: true,
        data: Some(data),
        error: None,
    };
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

/// Serialize an error envelope to stdout.
pub fn emit_err(err: &ResponseError) -> anyhow::Result<()> {
    let response = Response::<()> {
        schema_version: RESPONSE_SCHEMA_VERSION,
        ok: false,
        data: None,
        error: Some(err.clone()),
    };
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn success_response_uses_camel_case_and_omits_error() {
        let response = Response {
            schema_version: RESPONSE_SCHEMA_VERSION,
            ok: true,
            data: Some(json!({"entityCount": 1})),
            error: None,
        };

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["entityCount"], 1);
        assert!(value.get("error").is_none());
    }

    #[test]
    fn error_response_uses_snake_case_kind_and_omits_data() {
        let response = Response::<()> {
            schema_version: RESPONSE_SCHEMA_VERSION,
            ok: false,
            data: None,
            error: Some(ResponseError {
                kind: ErrorKind::InvalidInput,
                message: "bad request".to_string(),
            }),
        };

        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["kind"], "invalid_input");
        assert_eq!(value["error"]["message"], "bad request");
        assert!(value.get("data").is_none());
    }
}
