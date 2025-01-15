use axum::{
    http::HeaderValue,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
};
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::value::Value;
use textframe;

#[derive(Debug)]
pub enum ApiResponse {
    Created(),
    Text(String), //TODO: Rework to work with a stream over a borrowed &str rather than needing this copy
    JsonList(Vec<Value>),
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        match self {
            Self::Created() => (
                StatusCode::CREATED,
                [(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    HeaderValue::from_static("*"),
                )],
                "created",
            )
                .into_response(),
            Self::Text(s) => (
                StatusCode::OK,
                [(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    HeaderValue::from_static("*"),
                )],
                s,
            )
                .into_response(),
            Self::JsonList(data) => (
                StatusCode::OK,
                [(
                    header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    HeaderValue::from_static("*"),
                )],
                Json(data),
            )
                .into_response(),
        }
    }
}

#[derive(Debug)]
pub enum ApiError {
    MissingArgument(&'static str),
    InternalError(&'static str),
    NotFound(&'static str),
    CustomNotFound(String),
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
            Self::MissingArgument(s) => {
                state.serialize_field("name", "MissingArgument")?;
                state.serialize_field("message", s)?;
            }
            Self::NotFound(s) => {
                state.serialize_field("name", "NotFound")?;
                state.serialize_field("message", s)?;
            }
            Self::CustomNotFound(s) => {
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
            _ => Self::InternalError("UFile I/O error"),
        }
    }
}

impl From<axum::Error> for ApiError {
    fn from(value: axum::Error) -> Self {
        Self::InternalError("web framework error")
    }
}
