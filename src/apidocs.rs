use utoipa::ToSchema;

#[derive(ToSchema)]
/// An API error in JSON
#[allow(dead_code)]
pub struct ApiError {
    #[schema(rename = "@type")]
    /// The type of error, this will be "ApiError"
    r#type: String,

    /// The error name (MissingArgument, InternalError, NotFound, CustomNotFound, NotAcceptable, PermissionDenied)
    name: String,

    /// The error message
    message: String,
}
