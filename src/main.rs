mod bridge;
mod store;
mod metrics;
mod webhook;

use std::collections::HashMap;
use std::convert::Infallible;
use std::error::Error;
use std::sync::{Arc};
use bytes::Bytes;
use log::info;
use prometheus::{Encoder, Registry, TextEncoder};
use warp::{Filter, reject, Rejection, Reply};
use warp::http::StatusCode;

use crate::bridge::{SSE, SSEConfig};

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let sse_config = SSEConfig::create_from_env("SSE");
    let allowed_headers = vec![
        "DNT", "X-CustomHeader",
        "Keep-Alive", "User-Agent",
        "X-Requested-With",
        "If-Modified-Since",
        "Cache-Control", "Content-Type", "Authorization", "Origin",
    ];
    
    let bridge_port = sse_config.bridge_port.clone();
    let metrics_port = sse_config.metrics_port.clone();


    let cors = if sse_config.enable_cors {
        warp::cors()
            .allow_any_origin()
            .allow_methods(vec!["GET", "POST", "OPTIONS"])
            .max_age(86400)
            .allow_headers(allowed_headers)
    } else {
        warp::cors()
    };

    let registry = Arc::new(Registry::new());
    let metrics = metrics::register_metrics(&registry);

    let metrics_route = warp::path("metrics")
        .and(warp::get())
        .and(warp::any().map(move || registry.clone()))
        .and_then(handle_metrics);

    let sse = SSE::new(sse_config, metrics);

    let sse_filter = warp::any().map(move || sse.clone());

    let options = warp::options()
        .map(warp::reply)
        .with(cors.clone());

    // example: bridge/events?client_id=123&last_event_id=0
    let get_events = warp::path("bridge")
        .and(warp::path("events"))
        .and(warp::get())
        .and(warp::query::<HashMap<String, String>>())
        .and(sse_filter.clone())
        .and(warp::header::<String>("host"))
        .and_then(handle_subscribe);

    // TODO: does it really need to be a POST?
    let post_events = warp::path("bridge")
        .and(warp::path("events"))
        .and(warp::post())
        .and(warp::query::<HashMap<String, String>>())
        .and(sse_filter.clone())
        .and(warp::header::<String>("host"))
        .and_then(handle_subscribe);


    // example: bridge/message?client_id=123&to=456&ttl=300
    let push_message = warp::path("bridge")
        .and(warp::path("message"))
        .and(warp::post())
        .and(warp::query::<HashMap<String, String>>())
        .and(warp::body::bytes())
        .and(sse_filter)
        .and(warp::header::<String>("host"))
        .and_then(handle_push);

    let sse_routes = options
        .or(get_events)
        .or(post_events)
        .or(push_message)
        .with(cors);


    let routes = sse_routes.recover(handle_rejection);
    let addr = ([0, 0, 0, 0], bridge_port);

    let (addr, server) = warp::serve(routes).bind_ephemeral(addr);

    info!("Bridge listening on http://{}", addr);

    // bind metrics server
    let metrics_addr = ([0, 0, 0, 0], metrics_port);
    let (metrics_addr, metrics_server) = warp::serve(metrics_route).bind_ephemeral(metrics_addr);

    info!("Metrics listening on http://{}", metrics_addr);

    tokio::select! {
        _ = server => {},
        _ = metrics_server => {},
    }
}

async fn handle_metrics(
    registry: Arc<Registry>,
) -> Result<impl Reply, Rejection> {
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    let mut buffer = Vec::new();

    encoder.encode(&metric_families, &mut buffer).unwrap();
    let response = String::from_utf8(buffer).unwrap();

    Ok(response)
}

async fn handle_subscribe(
    params: HashMap<String, String>,
    sse: Arc<SSE>,
    host: String,
) -> Result<impl Reply, Rejection> {
    let empty_client_ids = "".to_string();
    let client_ids = params.get("client_id").unwrap_or(&empty_client_ids);
    let default_last_event_id = "0".to_string();
    let last_event_id = params.get("last_event_id").unwrap_or(&default_last_event_id)
        .parse()
        .unwrap_or(0);

    sse.handle_subscribe(client_ids.to_owned(), Some(last_event_id), host).await
}

async fn handle_push(
    params: HashMap<String, String>,
    body: Bytes,
    sse: Arc<SSE>,
    host: String,
) -> Result<impl Reply, Rejection> {
    let empty_client_id = "".to_string();
    let client_id = params.get("client_id").unwrap_or(&empty_client_id);

    let empty_to = "".to_string();
    let to = params.get("to").unwrap_or(&empty_to);

    let default_ttl = "300".to_string();
    let ttl = params.get("ttl").unwrap_or(&default_ttl).parse().unwrap_or(0);
    let topic = params.get("topic");

    let body = body.to_vec();

    sse.handle_push(client_id.to_owned(), to.to_owned(), ttl, topic, body, host).await
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "NOT_FOUND";
    } else if let Some(e) = err.find::<warp::filters::body::BodyDeserializeError>() {
        // This error happens if the body could not be deserialized correctly
        // We can use the cause to analyze the error and customize the error message
        message = match e.source() {
            Some(cause) => {
                if cause.to_string().contains("denom") {
                    "FIELD_ERROR: denom"
                } else {
                    "BAD_REQUEST"
                }
            }
            None => "BAD_REQUEST",
        };
        code = StatusCode::BAD_REQUEST;
    } else if let Some(_) = err.find::<reject::MethodNotAllowed>() {
        // We can handle a specific error, here METHOD_NOT_ALLOWED,
        // and render it however we want
        code = StatusCode::METHOD_NOT_ALLOWED;
        message = "METHOD_NOT_ALLOWED";
    } else {
        // We should have expected this... Just log and say its a 500
        eprintln!("unhandled rejection: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "UNHANDLED_REJECTION";
    }

    let json = warp::reply::json(&serde_json::json!({ 
        "code": code.as_u16(),
        "message": message,
    }));

    Ok(warp::reply::with_status(json, code))
}