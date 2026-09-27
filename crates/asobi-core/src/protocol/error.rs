//! Error mapping between [`ApiError`] and the wire error body.
//!
//! A failure over the wire is a non-2xx HTTP status with the body
//! `{"kind": "...", "message": "..."}` (ADR 0005). Each `ApiError` variant
//! has a fixed status and kind, so `RemoteStore` (WP5) rebuilds the same
//! variant from the kind and the CLI prints the same message as in local
//! mode; the status keeps failures visible to anything that only reads HTTP.
//! The statuses are plain numbers on purpose: neither WP4 nor WP5 needs an
//! HTTP crate to interpret them.

use crate::api::ApiError;
use serde::{Deserialize, Serialize};

/// The protocol error kinds that have no `ApiError` behind them.
pub mod kinds {
    pub const UNKNOWN_OPERATION: &str = "unknownOperation";
    pub const BAD_REQUEST: &str = "badRequest";
}

/// One RPC failure: the HTTP status to answer with and the JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub status: u16,
    pub kind: String,
    pub message: String,
}

impl ProtocolError {
    /// A protocol failure (unknown method, bad params, wrong HTTP verb).
    /// It has no `ApiError` behind it, so there is nothing to rebuild.
    pub fn protocol(status: u16, kind: &'static str, message: impl Into<String>) -> Self {
        ProtocolError {
            status,
            kind: kind.to_string(),
            message: message.into(),
        }
    }
}

/// The JSON body of an error response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorBody {
    pub kind: String,
    pub message: String,
}

impl ProtocolError {
    pub fn body(&self) -> ErrorBody {
        ErrorBody {
            kind: self.kind.clone(),
            message: self.message.clone(),
        }
    }
}

impl ApiError {
    /// The wire status and `kind` for this variant, per the ADR's table.
    pub fn rpc_shape(&self) -> (u16, &'static str) {
        match self {
            ApiError::NotFound(_) => (404, "notFound"),
            ApiError::Conflict(_) => (409, "conflict"),
            ApiError::Invalid(_) => (422, "invalid"),
            ApiError::Unsupported(_) => (501, "unsupported"),
            ApiError::Unavailable(_) => (503, "unavailable"),
            ApiError::Backend(_) => (500, "backend"),
        }
    }

    pub fn to_rpc_error(&self) -> ProtocolError {
        let (status, kind) = self.rpc_shape();
        ProtocolError {
            status,
            kind: kind.to_string(),
            message: self.to_string(),
        }
    }
}

/// Rebuild the `ApiError` variant an error body carries.
///
/// The message survives for every variant except `Unsupported`, whose payload
/// is a `&'static str` by type: over the wire only the variant can be
/// restored, with a fixed message. Callers that print a message read
/// `ErrorBody::message` (the server's own text) anyway.
pub fn error_body_to_api(body: &ErrorBody) -> Option<ApiError> {
    Some(match body.kind.as_str() {
        "notFound" => ApiError::NotFound(body.message.clone()),
        "conflict" => ApiError::Conflict(body.message.clone()),
        "unsupported" => ApiError::Unsupported(
            "reset is not available over the network; run `asobi reset` on the server host",
        ),
        "unavailable" => ApiError::Unavailable(body.message.clone()),
        "invalid" => ApiError::Invalid(body.message.clone()),
        "backend" => ApiError::Backend(body.message.clone()),
        _ => return None,
    })
}

impl From<ApiError> for ProtocolError {
    fn from(error: ApiError) -> Self {
        error.to_rpc_error()
    }
}
