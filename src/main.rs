use axum::{
    body::Body, extract::Path, extract::Query, extract::State, http::HeaderMap, http::HeaderValue,
    http::Request, routing::get, routing::post, Form, Router,
};
use clap::Parser;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{debug, error};

use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

mod apidocs;
mod common;
mod textpool;
use common::{ApiError, ApiResponse};
use textpool::TextPool;

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const CONTENT_TYPE_JSON: &'static str = "application/json";
const CONTENT_TYPE_TEXT: &'static str = "text/plain";

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

    #[arg(short = 'u', long, help = "The public-facing base URL.")]
    baseurl: Option<String>,

    #[arg(
        short = 'e',
        long,
        default_value_os = "txt",
        help = "The extension for plain text files"
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
        help = "Read-only, disallow uploads of texts"
    )]
    readonly: bool,

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
        get_text_slice,
    ),
    tags(
        (name = "textsurf", description = "Webservice for efficiently serving multiple plain text documents or excerpts thereof (by unicode character offset), without everything into memory.")
    )
)]
pub struct ApiDoc;

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let textpool = TextPool::new(
        args.basedir,
        if let Some(baseurl) = args.baseurl.as_ref() {
            baseurl.to_string()
        } else {
            format!("http://{}/", args.bind)
        },
        args.extension,
        args.readonly,
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
        .route("/{text_id}/{begin}/{end}", get(get_text_slice))
        .route("/{text_id}", get(get_text))
        .route("/{text_id}", post(create_text))
        .merge(SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .with_state(textpool.clone());

    //allow trailing slashes as well: (conflicts with swagger-ui!)
    //let app = NormalizePathLayer::trim_trailing_slash().layer(app);

    eprintln!("[textrepo] listening on {}", args.bind);
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
        (status = 200, body = [String], description = "Returns a simple list of all available texts"),
    )
)]
/// Returns all available texts
async fn list_texts(
    textpool: State<Arc<TextPool>>,
    request: Request<Body>,
) -> Result<ApiResponse, ApiError> {
    let extension = format!(".{}", textpool.extension());
    let mut store_ids: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(textpool.basedir())
        .map_err(|_| ApiError::InternalError("Unable to read base directory"))?
    {
        let entry = entry.unwrap();
        if let Some(filename) = entry.file_name().to_str() {
            if let Some(pos) = filename.find(&extension) {
                store_ids.push(filename[0..pos].to_string());
            }
        }
    }
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
    post,
    path = "/{text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text"),
    ),
    responses(
        (status = 201, description = "Returned when successfully created"),
        (status = 403, body = apidocs::ApiError, description = "Returned with name `PermissionDenied` when permission is denied, for instance the store is configured as read-only or the store already exists", content_type = "application/json")
    )
)]
/// Create (upload) a new text
async fn create_text(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    textpool.new_text(&text_id)?;
    Ok(ApiResponse::Created())
}

#[utoipa::path(
    get,
    path = "/{text_id}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. The identifier corresponds to the filename without extension on disk."),
    ),
    responses(
        (status = 200, description = "The text identifier",content(
            (String = "text/plain"),
        )),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
    )
)]
/// Returns a full text given a text identifier
async fn get_text(
    Path(text_id): Path<String>,
    textpool: State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    textpool.map(
        &text_id,
        0,
        0,
        |text| Ok(ApiResponse::Text(text.to_string())), //TODO: work away the String clone
    )
}

#[utoipa::path(
    get,
    path = "/{text_id}/{begin}/{end}",
    params(
        ("text_id" = String, Path, description = "The identifier of the text. The identifier corresponds to the filename without extension on disk."),
        ("begin" = isize, Path, description = "An integer indicating the begin offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors."),
        ("end" = isize, Path, description = "An integer indicating the non-inclusive end offset in unicode points (0-indexed). This may be a negative integer for end-aligned cursors and `0` for actual end."),
    ),
    responses(
        (status = 200, description = "The text",content(
            (String = "text/plain"),
        )),
        (status = 406, body = apidocs::ApiError, description = "This is returned if the requested content-type (Accept) could not be delivered", content_type = "application/json"),
        (status = 404, body = apidocs::ApiError, description = "An ApiError with name 'NotFound` is returned if the store or resource does not exist", content_type = "application/json"),
    )
)]
/// Returns a text slice given a text identifier and an offset
async fn get_text_slice(
    Path((text_id, begin, end)): Path<(String, isize, isize)>,
    textpool: State<Arc<TextPool>>,
) -> Result<ApiResponse, ApiError> {
    textpool.map(
        &text_id,
        begin,
        end,
        |text| Ok(ApiResponse::Text(text.to_string())), //TODO: work away the String clone
    )
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
                if *offer_type == accept_type || accept_type == "*/*" {
                    if match_accept_index.is_none()
                        || (match_accept_index.is_some() && match_accept_index.unwrap() > i)
                    {
                        match_accept_index = Some(i);
                        matching_offer = Some(*offer_type);
                    }
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
