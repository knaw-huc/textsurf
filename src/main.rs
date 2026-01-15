use axum::{
    body::Body, extract::Path, extract::Query, extract::State, http::HeaderMap, http::HeaderValue,
    http::Request, routing::delete, routing::get, routing::post, routing::put, Router,
};
use clap::Parser;
use futures::StreamExt as _;
use std::sync::Arc;
use std::time::Duration;
use std::{borrow::Cow, convert::Infallible};
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{debug, error};

use serde::Deserialize;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod apidocs;
mod common;
mod textpool;
use common::{ApiError, ApiResponse};
use textpool::TextPool;
use walkdir::WalkDir;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const CONTENT_TYPE_JSON: &str = "application/json";
// 16KB
const CHUNK_SIZE: usize = 1 << 14;
const STREAM_THRESHOLD: usize = CHUNK_SIZE;

#[derive(Parser, Debug)]
struct Args {
    #[arg(
        short,
        long,
        default_value_os = "127.0.0.1:8080",
        help = "The host and port to bind to"
    )]
    bind: String,

    #[arg(
        short = 'd',
        long,
        default_value_os = ".",
        help = "The base directory to serve from"
    )]
    basedir: String,

    #[arg(
        short = 'e',
        long,
        default_value_os = "txt",
        help = "The file extension for plain text files. You can set this to empty if you want extensions in the URL or if you don't want a single file extension."
    )]
    extension: String,

    #[arg(
        long,
        default_value_t = 600,
        help = "Number of seconds before texts are unloaded again if unused"
    )]
    unload_time: u64,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "Allow upload and deletion of texts, otherwise everything is read-only"
    )]
    writable: bool,

    #[arg(
        short = 'L',
        long,
        default_value_t = false,
        help = "No line index; disables iquerying by line and makes for smaller indices"
    )]
    no_lines: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Output logging info on incoming requests"
    )]
    debug: bool,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        list_texts,
        create_text,
        get_text,
        delete_all,
        delete_text,
        stat_text,
        get_api2_with_region,
        get_api2_short,
        create_text_api2,
        delete_text_api2,
    ),
    tags(
        (name = "textsurf", description = "Webservice for efficiently serving multiple plain text documents or excerpts thereof (by unicode character offset), without loading everything into memory.")
    )
)]
pub struct ApiDoc;

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let textpool = TextPool::new(
        args.basedir,
        args.extension,
        !args.writable,
        !args.no_lines,
        args.unload_time,
    )
    .expect("Base directory must exist");

    if args.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    let textpool: Arc<TextPool> = textpool.into();
    let textpool_flush = textpool.clone();

    //launch a background thread that flushes texts out of the pool if they're not used for a while
    std::thread::spawn(move || loop {
        std::thread::sleep(FLUSH_INTERVAL);
        match textpool_flush.flush(false) {
            Err(e) => error!("Flush failed! {:?}", e),
            Ok(v) => {
                if args.debug {
                    debug!("Flushed {} text(s)", v.len());
                }
            }
        }
    });

    let app = Router::new()
        .route("/", get(list_texts))
        .route("/", delete(delete_all))
        .route("/stat/{*text_id}", get(stat_text))
        .route("/api2/{text_id}", get(get_api2_short))
        .route("/api2/{text_id}/{region}", get(get_api2_with_region)) //also used for info.json for stat
        .route("/api2/{text_id}", post(create_text_api2))
        .route("/api2/{text_id}", put(create_text_overwrite_api2))
        .route("/api2/{text_id}", delete(delete_text_api2))
        .route("/{*text_id}", get(get_text))
        .route("/{*text_id}", post(create_text))
        .route("/{*text_id}", put(create_text_overwrite))
        .route("/{*text_id}", delete(delete_text))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .with_state(textpool.clone());

    //allow trailing slashes as well: (conflicts with swagger-ui!)
    //let app = NormalizePathLayer::trim_trailing_slash().layer(app);

    eprintln!("[textsurf] listening on {}", args.bind);
    let listener = tokio::net::TcpListener::bind(args.bind).await.unwrap();
    axum::serve(
        listener, app,
        //ServiceExt::<axum::http::Request<Body>>::into_make_service(app),
    )
    .with_graceful_shutdown(shutdown_signal(textpool))
    .await
    .unwrap();
}

async fn shutdown_signal(textpool: Arc<TextPool>) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            textpool.flush(true).expect("Clean shutdown failed");
        }
        _ = terminate => {
            textpool.flush(true).expect("Clean shutdown failed");
        }
    }
}

#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, body = [String], description = "Returns a simple list of all available texts (recursively)"),
    )
)]
/// Returns all available texts, recursively
async fn list_texts(
    textpool: State<Arc<TextPool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    let store_ids: Vec<String> =
        file_index(textpool.basedir(), &format!(".{}", textpool.extension()));
    match negotiate_content_type(request.headers(), &[CONTENT_TYPE_JSON]) {
        Ok(CONTENT_TYPE_JSON) => {
            let store_ids: Vec<serde_json::Value> =
                store_ids.into_iter().map(|s| s.into()).collect();
            Ok(ApiResponse::JsonList(store_ids))
        }
        _ => Err(ApiError::NotAcceptable(
            "Accept header could not be satisfied (try application/json)",
        )),
    }
}

fn list_texts_subdir(
    path: String,
    textpool: State<Arc<TextPool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    for component in path.split('/') {
        if component.starts_with('.') {
            return Err(ApiError::NotFound("Invalid path"));
        }
    }

    let store_ids: Vec<String> = file_index(
        textpool.basedir().join(path.as_str()).as_path(),
        &format!(".{}", textpool.extension()),
    );
    match negotiate_content_type(request.headers(), &[CONTENT_TYPE_JSON]) {
        Ok(CONTENT_TYPE_JSON) => {
            let store_ids: Vec<serde_json::Value> =
                store_ids.into_iter().map(|s| s.into()).collect();
            Ok(ApiResponse::JsonList(store_ids))
        }
        _ => Err(ApiError::NotAcceptable(
            "Accept header could not be satisfied (try application/json)",
        )),
    }
}

#[utoipa::path(
    delete,
    path = "/",
    responses(
        (status = 204, description = "Returned when successfully deleted"),
    )
)]
/// Deletes all texts, recursively
async fn delete_all(textpool: State<Arc<TextPool>>) -> Result<ApiResponse, ApiError> {
    delete_subdir("", textpool)
}

#[utoipa::path(
    post,
    path = "/{*text_id}",
    request_body( content_type = "text/plain", content = String),
    params(
        ("text_id" = String, Path, description = "The identifier of the text. It may contain zero or more path components."),
    ),
    responses(
        (status = 201, description = "Returned when successfully created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the service is configured as read-only or the text already exists", content_type = "application/json")
    )
)]
/// Create (upload) a new text, the text is transferred in the request body and must be valid UTF-8. If the text exists already, 403 will be returned
async fn create_text(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
    text: String,
) -> Result<ApiResponse, ApiError> {
    if textpool.new_text(&text_id, text, false)? {
        Ok(ApiResponse::Created())
    } else {
        unreachable!("new_text without overwrite always returns true (or error)")
    }
}

#[utoipa::path(
    put,
    path = "/{*text_id}",
    request_body( content_type = "text/plain", content = String),
    params(
        ("text_id" = String, Path, description = "The identifier of the text. It may contain zero or more path components."),
    ),
    responses(
        (status = 200, description = "Returned when successfully updated"),
        (status = 201, description = "Returned when successfully newly created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the service is configured as read-only or the text already exists", content_type = "application/json")
    )
)]
/// Create (upload) a new text, the text is transferred in the request body and must be valid UTF-8. If the text exists already, it will be overwritten.
async fn create_text_overwrite(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
    text: String,
) -> Result<ApiResponse, ApiError> {
    if textpool.new_text(&text_id, text, true)? {
        Ok(ApiResponse::Created())
    } else {
        Ok(ApiResponse::Ok())
    }
}

#[utoipa::path(
    post,
    path = "/api2/{text_id}",
    request_body( content_type = "text/plain", content = String),
    params(
        ("text_id" = String, Path, description = "The identifier of the text. It may contain zero or more path components."),
    ),
    responses(
        (status = 201, description = "Returned when successfully created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the service is configured as read-only.", content_type = "application/json")
    )
)]
/// Create (upload) a new text, the text is transferred in the request body and must be valid UTF-8. If the text exists already, 403 will be returned
async fn create_text_api2(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
    text: String,
) -> Result<ApiResponse, ApiError> {
    textpool.new_text(&api2_decode_id(&text_id), text, false)?;
    Ok(ApiResponse::Created())
}

#[utoipa::path(
    put,
    path = "/api2/{text_id}",
    request_body( content_type = "text/plain", content = String),
    params(
        ("text_id" = String, Path, description = "The identifier of the text. It may contain zero or more path components."),
    ),
    responses(
        (status = 200, description = "Returned when successfully updated"),
        (status = 201, description = "Returned when successfully newly created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the service is configured as read-only or the text already exists", content_type = "application/json")
    )
)]
/// Create (upload) a new text, the text is transferred in the request body and must be valid UTF-8. If the text exists already, it will be overwritten.
async fn create_text_overwrite_api2(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
    text: String,
) -> Result<ApiResponse, ApiError> {
    if textpool.new_text(&api2_decode_id(&text_id), text, true)? {
        Ok(ApiResponse::Created())
    } else {
        Ok(ApiResponse::Ok())
    }
}

#[utoipa::path(
    delete,
    path = "/{*text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. It may contain zero or more path components. Important note: If this ends with a trailing slash, the identifier is interpreted as a path and all texts in that path are removed (recursively)!"),
    ),
    responses(
        (status = 204, description = "Returned when successfully deleted"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the text does not exist", content_type = "application/json"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied if the service is configured as read-only", content_type = "application/json")
    )
)]
/// Permanently delete a text
async fn delete_text(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    if text_id.ends_with('/') {
        //deletion of an entire subdir rather than a single text
        delete_subdir(text_id.as_str(), textpool)
    } else {
        textpool.delete_text(&text_id)?;
        Ok(ApiResponse::NoContent())
    }
}

#[utoipa::path(
    delete,
    path = "/api2/{text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. It may contain zero or more path components."),
    ),
    responses(
        (status = 204, description = "Returned when successfully deleted"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the text does not exist", content_type = "application/json"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied if the service is configured as read-only", content_type = "application/json")
    )
)]
/// Permanently delete a text
async fn delete_text_api2(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    textpool.delete_text(&api2_decode_id(&text_id))?;
    Ok(ApiResponse::NoContent())
}

enum Range {
    Chars(isize, isize),
    Lines(isize, isize),
}

fn get_text_chars(
    textpool: Arc<TextPool>,
    text_id: &str,
    range: Range,
    force_no_stream: bool,
) -> Result<ApiResponse, ApiError> {
    // get absolute start and end character positions
    let (begin, end) = match range {
        Range::Chars(begin, end) => textpool.absolute_pos(text_id, begin, end),
        Range::Lines(begin, end) => textpool.absolute_line_pos(text_id, begin, end),
    }?;

    // asked range is smaller than threshold, just send as non-streamed response
    if force_no_stream || (end - begin) < STREAM_THRESHOLD {
        return match range {
            Range::Chars(begin, end) => textpool.map(text_id, begin, end, |text| {
                Ok(ApiResponse::Text(text.to_string()))
            }),
            Range::Lines(begin, end) => textpool.map_lines(text_id, begin, end, |text| {
                Ok(ApiResponse::Text(text.to_string()))
            }),
        };
    }

    // this check doesn't happen in TextFrame::absolute_pos
    if end < begin {
        return Err(ApiError::ParameterError(
            "The range you requested has a negative length",
        ));
    }

    let char_count = (end - begin) as u64;

    let nb_chunks = char_count.div_ceil(CHUNK_SIZE as u64);

    let textpool = Arc::clone(&textpool);
    let text_id = text_id.to_string();

    let textstream = futures::stream::iter(0..nb_chunks).then(move |chunk| {
        let textpool = Arc::clone(&textpool);
        let text_id = text_id.clone();
        async move {
            // put textpool operation in a tokio blocking task
            // in order to not block this tokio executor on slow IO operations
            let chunk_data = tokio::task::block_in_place(|| {
                let begin_chunk = (begin as u64 + chunk * CHUNK_SIZE as u64) as isize;
                let end_chunk =
                    (begin as u64 + ((chunk + 1) * CHUNK_SIZE as u64)).min(end as u64) as isize;

                textpool
                    .map(
                        &text_id,
                        begin_chunk,
                        end_chunk,
                        |text| Ok(text.to_string()),
                    )
                    .unwrap()
            });
            Ok::<_, Infallible>(chunk_data)
        }
    });

    let body = Body::from_stream(textstream);

    Ok(ApiResponse::TextStream(body))
}

fn parse_range(input: &str) -> Result<(isize, isize), ApiError> {
    let mut fields = input.splitn(2, ',');

    let begin = fields.next().unwrap_or("");
    let end = fields.next().unwrap_or("");

    let parse = |field: &str| {
        if field.is_empty() {
            Ok(0)
        } else {
            field
                .parse()
                .map_err(|_| ApiError::ParameterError("range parameter must be an integer"))
        }
    };

    let begin = parse(begin)?;
    let end = parse(end)?;

    Ok((begin, end))
}

#[derive(Deserialize)]
struct TextParams {
    begin: Option<isize>,
    end: Option<isize>,
    char: Option<String>,
    line: Option<String>,
    length: Option<usize>,
    md5: Option<String>,
}

#[utoipa::path(
    get,
    path = "/{*text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. The identifier corresponds to the filename without extension on disk. It may contain zero or more path components."),
        ("begin" = Option<isize>, Query, description = "An integer indicating the begin offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors. The default value is 0."),
        ("end" = Option<isize>, Query, description = "An integer indicating the non-inclusive end offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors and `0` for actual end. The default value is 0."),
        ("char" = Option<isize>, Query, description = "Character range specification conforming to RFC5147, begin and end values are separated by a comma, 0-indexed, end is non-inclusive"),
        ("line" = Option<isize>, Query, description = "Line range specification conforming to RFC5147, begin and end values are separated by a comma, 0-indexed (first line is 0!), end is non-inclusive"),
        ("length" = Option<usize>, Query, description = "Optional length validity check (as in RFC5147, an encoding parameter is NOT supported though as textsurf only does UTF-8 anyway). This is not an alternative for `end`. If the check fails, a 403 will be returned."),
        ("md5" = Option<String>, Query, description = "MD5 checksum for the text that is being referenced (as defined by RFC5147). If the check fails, a 403 will be returned"),
    ),
    responses(
        (status = 200, description = "The text",content(
            (String = "text/plain"),
        )),
        (status = 403, body = apidocs::ApiError, description = "Return when an explicitly passed check (length,md5) fails", content_type = "application/json"),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
    )
)]
/// Returns a text given a text identifier. Returns either a full text or a portion thereof if offsets were specified.
/// If a path is specified (trailing slash), this returns an index of all files under that path instead (as JSON)
async fn get_text(
    Path(text_id): Path<String>,
    Query(params): Query<TextParams>,
    State(textpool): State<Arc<TextPool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    if text_id.ends_with('/') {
        //request for index rather than a text
        return list_texts_subdir(text_id, State(textpool), request);
    }

    let force_no_stream = params.length.is_some() || params.md5.is_some();

    let range = if let Some(char) = params.char {
        let (begin, end) = parse_range(&char)?;
        Range::Chars(begin, end)
    } else if let Some(line) = params.line {
        let (begin, end) = parse_range(&line)?;
        Range::Lines(begin, end)
    } else {
        // both begin and end default to 0 when they're not given
        let begin = params.begin.unwrap_or(0);
        let end = params.end.unwrap_or(0);
        Range::Chars(begin, end)
    };

    let response = get_text_chars(textpool, &text_id, range, force_no_stream);

    if let Ok(ApiResponse::Text(text)) = &response {
        if let Some(length) = params.length {
            if text.chars().count() != length {
                return Err(ApiError::PermissionDenied("length check failed"));
            }
        }
        if let Some(md5ref) = params.md5 {
            let checksum = format!("{:x}", md5::compute(text.as_bytes()));
            if checksum != md5ref {
                return Err(ApiError::PermissionDenied("md5 check failed"));
            }
        }
    }
    response
}

#[utoipa::path(
    get,
    path = "/stat/{*text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. The identifier corresponds to the filename without extension on disk. It may contain zero or more path components."),
    ),
    responses(
        (status = 200, description = "The text identifier",content(
            (String = "text/plain"),
        )),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
    )
)]
/// Returns metadata about a text. Returns a JSON response with fields "bytes" (filesize), "chars" (length in unicode characters), "checksum" (SHA-256) and "mtime" (unix timestamp for the file modification)
async fn stat_text(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    textpool.stat(&text_id)
}

#[utoipa::path(
    get,
    path = "/api2/{text_id}/{region}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. The identifier corresponds to the filename without extension on disk."),
        ("region" = isize, Path, description = "A region specification in the form: `[{prefix:}]{begin},{end}`. Where begin is an integer indicating the begin offset in unicode points (0-indexed, this may be a negative integer for end-aligned cursors). End is integer indicating the non-inclusive end offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors and `0` for actual end. Prefix can be `char` or `line`, the former is the default if omitted entirely, in the latter case begin and end arguments will be interpreted to be lines rather than characters (0-indexed, non-inclusive end). Instead of a range, you can also use the keyword `full` to get the full text, which is identical to just omitted the region parameter entirely. Last, instead of a region you can also specify `info.json` to get metadata about a text."),
    ),
    responses(
        (status = 200, description = "The requested text excerpt",content(
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
    )
)]
/// Returns a text or a text slice according to Text Referencing API 2
async fn get_api2_with_region(
    Path((text_id, region)): Path<(String, String)>,
    State(textpool): State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    let text_id = api2_decode_id(text_id.as_str());

    if region == "info.json" {
        textpool.stat_api2(&text_id)
    } else if let Some((prefix, remainder)) = region.split_once(':') {
        let (begin, end) = get_text_slice_helper(remainder)?;
        match prefix {
            "char" => get_text_chars(textpool, &text_id, Range::Chars(begin, end), false),
            "line" => get_text_chars(textpool, &text_id, Range::Lines(begin, end), false),
            _ => Err(ApiError::ParameterError(
                "invalid prefix for region parameter, must be 'char' or 'line'",
            )),
        }
    } else {
        let (begin, end) = get_text_slice_helper(region.as_str())?;

        get_text_chars(textpool, &text_id, Range::Chars(begin, end), false)
    }
}

#[utoipa::path(
    get,
    path = "/api2/{text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. The identifier corresponds to the filename without extension on disk."),
    ),
    responses(
        (status = 200, description = "The requested text excerpt",content(
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
    )
)]
async fn get_api2_short(
    Path(text_id): Path<String>,
    State(textpool): State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    get_text_chars(textpool, &text_id, Range::Chars(0, 0), false)
}

/// Extra patch to allow pipes as a substitute for slashes in URLs
fn api2_decode_id<'a>(s: &'a str) -> Cow<'a, str> {
    if s.find('|').is_some() {
        Cow::Owned(s.replace("|", "/"))
    } else {
        Cow::Borrowed(s)
    }
}

fn file_index(dir: &std::path::Path, extension: &str) -> Vec<String> {
    let mut store_ids: Vec<String> = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let filepath = entry
            .path()
            .strip_prefix(dir)
            .expect("prefix should be there");
        if !extension.is_empty() {
            if let Some(filepath_s) = filepath.to_str() {
                if let Some(pos) = filepath_s.find(extension) {
                    store_ids.push(filepath_s[0..pos].to_string());
                }
            }
        } else if filepath.is_file()
            && filepath.extension().map(|x| x.as_encoded_bytes()) != Some(b"index")
            && filepath
                .to_str()
                .map(|s| !s.starts_with("."))
                .unwrap_or(false)
        {
            store_ids.push(
                filepath
                    .to_str()
                    .expect("filename must be UTF-8")
                    .to_string(),
            );
        }
    }
    store_ids
}

fn get_text_slice_helper(s: &str) -> Result<(isize, isize), ApiError> {
    if s == "full" {
        return Ok((0, 0));
    }
    if let Some((begin, end)) = s.split_once(',') {
        let begin: isize = begin
            .parse()
            .map_err(|_| ApiError::ParameterError("region begin parameter must be an integer"))?;
        let end: isize = end
            .parse()
            .map_err(|_| ApiError::ParameterError("region end parameter must be an integer"))?;
        Ok((begin, end))
    } else {
        Err(ApiError::ParameterError(
            "region parameter must have a comma to express a range",
        ))
    }
}

fn negotiate_content_type(
    headers: &HeaderMap<HeaderValue>,
    offer_types: &[&'static str],
) -> Result<&'static str, ApiError> {
    if let Some(accept_types) = headers.get(axum::http::header::ACCEPT) {
        let mut match_accept_index = None;
        let mut matching_offer = None;
        for (i, accept_type) in accept_types
            .to_str()
            .map_err(|_| ApiError::NotAcceptable("Invalid Accept header"))
            .unwrap_or(CONTENT_TYPE_JSON)
            .split(",")
            .enumerate()
        {
            let accept_type = accept_type.split(";").next().unwrap();
            for offer_type in offer_types.iter() {
                if *offer_type == accept_type
                    || accept_type == "*/*"
                        && (match_accept_index.is_none()
                            || (match_accept_index.is_some() && match_accept_index.unwrap() > i))
                {
                    match_accept_index = Some(i);
                    matching_offer = Some(*offer_type);
                }
            }
        }
        if let Some(matching_offer) = matching_offer {
            Ok(matching_offer)
        } else {
            Err(ApiError::NotAcceptable("No matching content type on offer"))
        }
    } else {
        Ok(offer_types[0])
    }
}

fn delete_subdir(dir: &str, textpool: State<Arc<TextPool>>) -> Result<ApiResponse, ApiError> {
    for component in dir.split('/') {
        if component.starts_with('.') {
            return Err(ApiError::NotFound("Invalid path"));
        }
    }

    let dir: std::path::PathBuf = textpool.basedir().join(dir.trim_matches('/'));
    let extension = format!(".{}", textpool.extension());

    for entry in WalkDir::new(dir.as_path())
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let filepath = entry
            .path()
            .strip_prefix(textpool.basedir())
            .expect("prefix should be there");
        if !extension.is_empty() {
            if let Some(filepath_s) = filepath.to_str() {
                if filepath_s.contains(&extension) {
                    textpool.delete_text(filepath_s)?;
                }
            }
        } else if filepath.is_file()
            && filepath.extension().map(|x| x.as_encoded_bytes()) != Some(b"index")
            && filepath
                .to_str()
                .map(|s| !s.starts_with("."))
                .unwrap_or(false)
        {
            textpool.delete_text(filepath.to_str().expect("filename must be utf-8"))?;
        }
    }
    //TODO: clean up remaining empty directories
    Ok(ApiResponse::NoContent())
}
