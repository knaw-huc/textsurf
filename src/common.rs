use axum::{
    http::HeaderValue,
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::value::Value;
use std::collections::BTreeMap;
use textframe;

#[derive(Debug)]
pub enum ApiResponse {
    Created(),
    NoContent(),
    Text(String), //TODO: Rework to work with a stream over a borrowed &str rather than needing this copy
    Stat {
        chars: u64,
        bytes: u64,
        mtime: u64,
        checksum: String,
    },
    JsonList(Vec<Value>),
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        let cors = (
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        match self {
            Self::Created() => (StatusCode::CREATED, "created").into_response(),
            Self::NoContent() => (StatusCode::NO_CONTENT, [cors], "deleted").into_response(),
            Self::Text(s) => (StatusCode::OK, [cors], s).into_response(),
            Self::JsonList(data) => (StatusCode::OK, [cors], Json(data)).into_response(),
            Self::Stat {
                chars,
                bytes,
                mtime,
                checksum,
            } => {
                let mut map: BTreeMap<&'static str, Value> = BTreeMap::new();
                map.insert("chars", chars.into());
                map.insert("bytes", bytes.into());
                map.insert("mtime", mtime.into());
                map.insert("checksum", checksum.into());
                (StatusCode::OK, [cors], Json(map)).into_response()
            }
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    InternalError(&'static str),
    NotFound(&'static str),
    NotAcceptable(&'static str),
    PermissionDenied(&'static str),
    TextError(textframe::Error),
}

impl Serialize for ApiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("ApiError", 3)?;
        state.serialize_field("@type", "ApiError")?;
        match self {
            Self::NotFound(s) => {
                state.serialize_field("name", "NotFound")?;
                state.serialize_field("message", s)?;
            }
            Self::NotAcceptable(s) => {
                state.serialize_field("name", "NotAcceptable")?;
                state.serialize_field("message", s)?;
            }
            Self::PermissionDenied(s) => {
                state.serialize_field("name", "PermissionDenied")?;
                state.serialize_field("message", s)?;
            }
            Self::InternalError(s) => {
                state.serialize_field("name", "InternalError")?;
                state.serialize_field("message", s)?;
            }
            Self::TextError(e) => {
                state.serialize_field("name", "TextError")?;
                let message: String = e.to_string();
                state.serialize_field("message", message.as_str())?;
            }
        }
        state.end()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let statuscode = match self {
            Self::InternalError(..) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PermissionDenied(..) => StatusCode::FORBIDDEN,
            Self::NotAcceptable(..) => StatusCode::NOT_ACCEPTABLE,
            _ => StatusCode::NOT_FOUND,
        };
        (statuscode, Json(self)).into_response()
    }
}

impl From<textframe::Error> for ApiError {
    fn from(value: textframe::Error) -> Self {
        Self::TextError(value)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound("file not found"),
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied("permission denied"),
            std::io::ErrorKind::NotSeekable => Self::InternalError("file not seekable"),
            std::io::ErrorKind::StorageFull => Self::InternalError("storage full"),
            std::io::ErrorKind::ReadOnlyFilesystem => Self::InternalError("read only filesystem"),
            _ => Self::InternalError("File I/O error"),
        }
    }
}

impl From<axum::Error> for ApiError {
    fn from(_value: axum::Error) -> Self {
        Self::InternalError("web framework error")
    }
}
