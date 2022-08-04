use brotli::enc::BrotliEncoderParams;
use brotli::BrotliCompress;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Instant;
use std::{fmt, fs, str};

use futures::{future::FutureExt, stream::StreamExt};
use headers::{Authorization, CacheControl, ContentType, ETag, Header, HeaderMapExt, IfNoneMatch};
use http::header::CACHE_CONTROL;
use hyper::StatusCode;
use log::{debug, error, info};
use parking_lot::{Mutex, RwLock};
use ring::hmac;
use rmp_serde;
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

pub use crate::api::{
    self, bootstrap, comparison, dashboard, github, graphs, info, self_profile, self_profile_raw,
    status, triage, ServerResult,
};
use crate::db::{self, ArtifactId};
use crate::load::{Config, SiteCtxt};
use crate::request_handlers;

pub type Request = http::Request<hyper::Body>;
pub type Response = http::Response<hyper::Body>;

macro_rules! check_http_method {
    ($lhs: expr, $rhs: expr) => {
        if $lhs != $rhs {
            return Ok(http::Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(hyper::Body::empty())
                .unwrap());
        }
    };
}

/// Server state
#[derive(Clone)]
struct Server {
    ctxt: Arc<RwLock<Option<Arc<SiteCtxt>>>>,
    updating: UpdatingStatus,
}

impl Server {
    fn new(ctxt: Arc<RwLock<Option<Arc<SiteCtxt>>>>) -> Self {
        Self {
            ctxt,
            updating: UpdatingStatus::new(),
        }
    }
}

#[derive(Clone)]
struct UpdatingStatus(Arc<AtomicBool>);

struct IsUpdating(Arc<AtomicBool>, hyper::body::Sender);

impl Drop for IsUpdating {
    fn drop(&mut self) {
        self.0.store(false, AtomicOrdering::SeqCst);
        if std::thread::panicking() {
            let _ = self.1.try_send_data("panicked, try again".into());
        } else {
            let _ = self.1.try_send_data("done".into());
        }
    }
}

impl UpdatingStatus {
    fn new() -> Self {
        UpdatingStatus(Arc::new(AtomicBool::new(false)))
    }

    // Returns previous state
    fn set_updating(&self) -> bool {
        match self
            .0
            .compare_exchange(false, true, AtomicOrdering::SeqCst, AtomicOrdering::SeqCst)
        {
            Ok(b) => b,
            Err(b) => b,
        }
    }

    fn release_on_drop(&self, channel: hyper::body::Sender) -> IsUpdating {
        IsUpdating(self.0.clone(), channel)
    }
}

impl Server {
    /// Handle a synchrnous HTTP GET request
    fn handle_get<F, S>(&self, req: &Request, handler: F) -> Result<Response, ServerError>
    where
        F: FnOnce(&SiteCtxt) -> S,
        S: Serialize,
    {
        check_http_method!(*req.method(), http::Method::GET);
        let ctxt = self.ctxt.clone();
        let ctxt = ctxt.read();
        let ctxt = ctxt.as_ref().unwrap();
        let result = handler(&ctxt);
        Ok(http::Response::builder()
            .header_typed(ContentType::json())
            .body(hyper::Body::from(serde_json::to_string(&result).unwrap()))
            .unwrap())
    }

    /// Handle an asynchrnous HTTP GET request
    async fn handle_get_async<F, R, S>(
        &self,
        req: &Request,
        handler: F,
    ) -> Result<Response, ServerError>
    where
        F: FnOnce(Arc<SiteCtxt>) -> R,
        R: std::future::Future<Output = S> + Send,
        S: Serialize,
    {
        check_http_method!(*req.method(), http::Method::GET);
        let ctxt = self.ctxt.clone();
        let ctxt = ctxt.read().as_ref().unwrap().clone();
        let result = handler(ctxt).await;
        Ok(http::Response::builder()
            .header_typed(ContentType::json())
            .body(hyper::Body::from(serde_json::to_string(&result).unwrap()))
            .unwrap())
    }

    async fn handle_fallible_get_async<F, R, S, E>(
        &self,
        req: &Request,
        compression: &Option<BrotliEncoderParams>,
        handler: F,
    ) -> Result<Response, ServerError>
    where
        F: FnOnce(Arc<SiteCtxt>) -> R,
        R: std::future::Future<Output = Result<S, E>> + Send,
        S: Serialize,
        E: Into<Vec<u8>>,
    {
        check_http_method!(*req.method(), http::Method::GET);
        let ctxt = self.ctxt.clone();
        let ctxt = ctxt.read().as_ref().unwrap().clone();
        let result = handler(ctxt).await;
        let response = match result {
            Ok(result) => {
                let response = http::Response::builder()
                    .header_typed(ContentType::json())
                    .header_typed(CacheControl::new().with_no_cache().with_no_store());
                let body = serde_json::to_vec(&result).unwrap();
                maybe_compressed_response(response, body, compression)
            }
            Err(err) => http::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header_typed(ContentType::text_utf8())
                .header_typed(CacheControl::new().with_no_cache().with_no_store())
                .body(hyper::Body::from(err.into()))
                .unwrap(),
        };
        Ok(response)
    }

    fn check_auth(&self, req: &http::request::Parts) -> bool {
        if let Some(auth) = req
            .headers
            .get(Authorization::<headers::authorization::Bearer>::name())
        {
            let ctxt = self.ctxt.read();
            let ctxt = ctxt.as_ref().unwrap();
            let auth = Authorization::<headers::authorization::Bearer>::decode(
                &mut Some(auth).into_iter(),
            )
            .unwrap();
            return auth.0.token() == *ctxt.config.keys.github_webhook_secret.as_ref().unwrap();
        }

        false
    }

    async fn handle_metrics(&self, _req: Request) -> Response {
        use prometheus::Encoder;
        let ctxt: Arc<SiteCtxt> = self.ctxt.read().as_ref().unwrap().clone();
        let idx = ctxt.index.load();

        let mut buffer = Vec::new();
        let r = prometheus::Registry::new();

        let missing_commits = ctxt.missing_commits().await;
        let queue_length =
            prometheus::IntGauge::new("rustc_perf_queue_length", "queue length").unwrap();
        queue_length.set(missing_commits.len() as i64);
        r.register(Box::new(queue_length)).unwrap();

        let queue_try_commits =
            prometheus::IntGauge::new("rustc_perf_queue_try_commits", "queued try commits")
                .unwrap();
        queue_try_commits.set(missing_commits.iter().filter(|(c, _)| c.is_try()).count() as i64);
        r.register(Box::new(queue_try_commits)).unwrap();

        if let Some(last_commit) = idx.commits().last().cloned() {
            let conn = ctxt.conn().await;
            let steps = conn.in_progress_steps(&ArtifactId::from(last_commit)).await;
            let g = prometheus::IntGaugeVec::new(
                prometheus::core::Opts {
                    namespace: format!("rustc_perf"),
                    subsystem: String::new(),
                    name: String::from("step_duration_seconds"),
                    help: String::from("step duration"),
                    const_labels: HashMap::new(),
                    variable_labels: vec![],
                },
                &["step"],
            )
            .unwrap();
            for step in steps {
                g.with_label_values(&[&step.name])
                    .set(step.expected.as_secs() as i64);
            }
            r.register(Box::new(g)).unwrap();
        }

        let encoder = prometheus::TextEncoder::new();
        let metric_families = r.gather();
        encoder.encode(&metric_families, &mut buffer).unwrap();

        Response::new(buffer.into())
    }

    async fn handle_push(&self, _req: Request) -> Response {
        lazy_static::lazy_static! {
            static ref LAST_UPDATE: Mutex<Option<Instant>> = Mutex::new(None);
        }

        let last = LAST_UPDATE.lock().clone();
        if let Some(last) = last {
            let min = 60 * 1; // 1 minutes
            let elapsed = last.elapsed();
            if elapsed < std::time::Duration::from_secs(min) {
                return http::Response::builder()
                    .status(StatusCode::OK)
                    .header_typed(ContentType::text_utf8())
                    .body(hyper::Body::from(format!(
                        "Refreshed too recently ({:?} ago). Please wait.",
                        elapsed
                    )))
                    .unwrap();
            }
        }
        *LAST_UPDATE.lock() = Some(Instant::now());

        // set to updating
        let was_updating = self.updating.set_updating();

        if was_updating {
            return http::Response::builder()
                .status(StatusCode::OK)
                .header_typed(ContentType::text_utf8())
                .body(hyper::Body::from("Already updating!"))
                .unwrap();
        }

        debug!("received onpush hook");

        let (channel, body) = hyper::Body::channel();

        let ctxt: Arc<SiteCtxt> = self.ctxt.read().as_ref().unwrap().clone();
        let _updating = self.updating.release_on_drop(channel);
        let mut conn = ctxt.conn().await;
        let index = db::Index::load(&mut *conn).await;
        eprintln!("index has {} commits", index.commits().len());
        ctxt.index.store(Arc::new(index));

        // Refresh the landing page
        ctxt.landing_page.store(Arc::new(None));

        // Spawn off a task to post the results of any commit results that we
        // are now aware of.
        tokio::spawn(async move {
            crate::github::post_finished(&ctxt).await;
        });

        Response::new(body)
    }
}

#[derive(Debug)]
struct ServerError(String);

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "server failed: {}", self.0)
    }
}

impl std::error::Error for ServerError {}

async fn serve_req(server: Server, req: Request) -> Result<Response, ServerError> {
    // Don't attempt to get lock if we're updating
    if server.ctxt.read().is_none() {
        return Ok(Response::new(hyper::Body::from("no data yet, please wait")));
    }

    if req.method() == http::Method::OPTIONS {
        return Ok(http::Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(hyper::Body::empty())
            .unwrap());
    }
    let path = req.uri().path().to_owned();
    let path = path.as_str();

    let allow_compression = req
        .headers()
        .get(hyper::header::ACCEPT_ENCODING)
        .map_or(false, |e| e.to_str().unwrap().contains("br"));

    let compression = if allow_compression {
        let mut brotli = BrotliEncoderParams::default();
        // In tests on /perf/graphs and /perf/get, quality = 2 reduces size by 20-40% compared to 0,
        // while quality = 4 takes 80% longer but reduces size by less than 5% compared to 2.
        // Around 4-5 is sometimes said to be "smaller and faster than gzip".
        // [Google's default is 6](https://github.com/google/ngx_brotli#brotli_comp_level),
        // higher levels offer only small size savings but are much slower.
        brotli.quality = 2;
        Some(brotli)
    } else {
        None
    };

    if let Some(response) = handle_fs_path(&req, path) {
        return Ok(response);
    }

    macro_rules! check {
        ($e:expr) => {
            match $e {
                Ok(v) => v,
                Err(e) => return Ok(e),
            }
        };
    }

    match path {
        "/perf/info" => return server.handle_get(&req, request_handlers::handle_info),
        "/perf/dashboard" => {
            return server
                .handle_get_async(&req, |c| request_handlers::handle_dashboard(c))
                .await;
        }
        "/perf/status_page" => {
            return server
                .handle_get_async(&req, |c| request_handlers::handle_status_page(c))
                .await;
        }
        "/perf/next_commit" => {
            return server
                .handle_get_async(&req, |c| request_handlers::handle_next_commit(c))
                .await;
        }
        "/perf/triage" if *req.method() == http::Method::GET => {
            let ctxt: Arc<SiteCtxt> = server.ctxt.read().as_ref().unwrap().clone();
            let input: triage::Request = check!(parse_query_string(req.uri()));
            return Ok(to_triage_response(
                crate::comparison::handle_triage(input, &ctxt).await,
            ));
        }
        "/perf/graph" => {
            let query = check!(parse_query_string(req.uri()));
            return server
                .handle_fallible_get_async(&req, &compression, |c| {
                    request_handlers::handle_graph(query, c)
                })
                .await;
        }
        "/perf/graphs" => {
            let query = check!(parse_query_string(req.uri()));
            return server
                .handle_fallible_get_async(&req, &compression, |c| {
                    request_handlers::handle_graphs(query, c)
                })
                .await;
        }
        "/perf/metrics" => {
            return Ok(server.handle_metrics(req).await);
        }
        "/perf/onpush" => {
            return Ok(server.handle_push(req).await);
        }
        "/perf/download-raw-self-profile" => {
            let ctxt: Arc<SiteCtxt> = server.ctxt.read().as_ref().unwrap().clone();
            let req = check!(parse_query_string(req.uri()));
            return Ok(request_handlers::handle_self_profile_raw_download(req, &ctxt).await);
        }
        "/perf/processed-self-profile" => {
            let ctxt: Arc<SiteCtxt> = server.ctxt.read().as_ref().unwrap().clone();
            let req = check!(parse_query_string(req.uri()));
            return Ok(request_handlers::handle_self_profile_processed_download(req, &ctxt).await);
        }
        _ if req.method() == http::Method::GET => return Ok(not_found()),
        _ => {}
    }

    // POST requests
    let (req, mut body_stream) = req.into_parts();
    check_http_method!(req.method, http::Method::POST);
    let ctxt: Arc<SiteCtxt> = server.ctxt.read().as_ref().unwrap().clone();
    let mut body = Vec::new();
    while let Some(chunk) = body_stream.next().await {
        let chunk = chunk.map_err(|e| ServerError(format!("failed to read chunk: {:?}", e)))?;
        body.extend_from_slice(&chunk);
        // More than 10 MB of data
        if body.len() > 1024 * 1024 * 10 {
            return Ok(http::Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .body(hyper::Body::empty())
                .unwrap());
        }
    }

    match path {
        "/perf/get" => Ok(to_response(
            crate::comparison::handle_compare(check!(parse_body(&body)), &ctxt)
                .await
                .map_err(|e| e.to_string()),
            &compression,
        )),
        "/perf/collected" => {
            if !server.check_auth(&req) {
                return Ok(http::Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(hyper::Body::empty())
                    .unwrap());
            }
            Ok(to_response(
                request_handlers::handle_collected().await,
                &compression,
            ))
        }
        "/perf/github-hook" => {
            if !verify_gh(&ctxt.config, &req, &body) {
                return Ok(http::Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(hyper::Body::empty())
                    .unwrap());
            }
            let event = req.headers.get("X-GitHub-Event").cloned();
            let event = event.and_then(|g| g.to_str().ok().map(|s| s.to_owned()));
            let event = match event {
                Some(v) => v,
                None => {
                    return Ok(http::Response::builder()
                        .status(StatusCode::OK)
                        .body(hyper::Body::from("missing event header"))
                        .unwrap())
                }
            };
            match event.as_str() {
                "issue_comment" => Ok(to_response(
                    request_handlers::handle_github(check!(parse_body(&body)), ctxt.clone()).await,
                    &compression,
                )),
                _ => Ok(http::Response::builder()
                    .status(StatusCode::OK)
                    .body(hyper::Body::from(format!("unknown event: {}", event)))
                    .unwrap()),
            }
        }
        "/perf/self-profile" => Ok(to_response(
            request_handlers::handle_self_profile(check!(parse_body(&body)), &ctxt).await,
            &compression,
        )),
        "/perf/self-profile-raw" => Ok(to_response(
            request_handlers::handle_self_profile_raw(check!(parse_body(&body)), &ctxt).await,
            &compression,
        )),
        "/perf/bootstrap" => Ok(
            match request_handlers::handle_bootstrap(check!(parse_body(&body)), &ctxt).await {
                Ok(result) => {
                    let mut response = http::Response::builder()
                        .header_typed(ContentType::json())
                        .header_typed(CacheControl::new().with_no_cache().with_no_store());
                    response.headers_mut().unwrap().insert(
                        hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                        hyper::header::HeaderValue::from_static("*"),
                    );
                    let body = serde_json::to_vec(&result).unwrap();
                    response.body(hyper::Body::from(body)).unwrap()
                }
                Err(err) => http::Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header_typed(ContentType::text_utf8())
                    .header_typed(CacheControl::new().with_no_cache().with_no_store())
                    .body(hyper::Body::from(err))
                    .unwrap(),
            },
        ),
        _ => Ok(http::Response::builder()
            .header_typed(ContentType::html())
            .status(StatusCode::NOT_FOUND)
            .body(hyper::Body::empty())
            .unwrap()),
    }
}

fn parse_body<D>(body: &[u8]) -> Result<D, Response>
where
    D: DeserializeOwned,
{
    match serde_json::from_slice(&body) {
        Ok(d) => Ok(d),
        Err(err) => {
            error!(
                "failed to deserialize request {}: {:?}",
                String::from_utf8_lossy(&body),
                err
            );
            return Err(http::Response::builder()
                .header_typed(ContentType::text_utf8())
                .status(StatusCode::BAD_REQUEST)
                .body(hyper::Body::from(format!(
                    "Failed to deserialize request: {:?}",
                    err
                )))
                .unwrap());
        }
    }
}

fn parse_query_string<D>(uri: &http::Uri) -> Result<D, Response>
where
    D: DeserializeOwned,
{
    let params: HashMap<String, String> = uri
        .query()
        .map(|v| {
            url::form_urlencoded::parse(v.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_else(HashMap::new);

    match serde_json::from_str(&serde_json::to_string(&params).unwrap()) {
        Ok(d) => Ok(d),
        Err(err) => {
            return Err(http::Response::builder()
                .header_typed(ContentType::text_utf8())
                .status(StatusCode::BAD_REQUEST)
                .body(hyper::Body::from(format!(
                    "Failed to deserialize request {}: {:?}",
                    uri, err,
                )))
                .unwrap());
        }
    }
}

lazy_static::lazy_static! {
    static ref VERSION_UUID: Uuid = Uuid::new_v4(); // random UUID used as ETag for cache revalidation
}

/// Handle the case where the path is to a static file
fn handle_fs_path(req: &Request, path: &str) -> Option<http::Response<hyper::Body>> {
    let fs_path = format!(
        "site/static{}",
        match path {
            "" | "/" => "/index.html",
            _ => path,
        }
    );

    if fs_path.contains("./") | fs_path.contains("../") {
        return Some(not_found());
    }

    if !Path::new(&fs_path).is_file() {
        return None;
    }

    let etag = ETag::from_str(&*format!(r#""{}""#, *VERSION_UUID)).unwrap();
    let mut response = http::Response::builder()
        .header_typed(etag.clone())
        .header(CACHE_CONTROL, "max-age=60, stale-while-revalidate=86400"); // tell client to use cached response for one day, but revalidate in background if older than one minute

    let if_none_match = req.headers().typed_get::<IfNoneMatch>();
    if let Some(if_none_match) = if_none_match {
        if !if_none_match.precondition_passes(&etag) {
            return Some(not_modified(response)); // tell client that the resource was not modified and to use cached response
        }
    }

    let source = fs::read(&fs_path).unwrap();
    let p = Path::new(&fs_path);
    match p.extension().and_then(|x| x.to_str()) {
        Some("html") => response = response.header_typed(ContentType::html()),
        Some("png") => response = response.header_typed(ContentType::png()),
        Some("json") => response = response.header_typed(ContentType::json()),
        Some("svg") => response = response.header("Content-Type", "image/svg+xml"),
        Some("css") => response = response.header("Content-Type", "text/css"),
        Some("js") => response = response.header("Content-Type", "application/javascript"),
        _ => (),
    }
    Some(response.body(hyper::Body::from(source)).unwrap())
}

fn not_modified(response: http::response::Builder) -> http::Response<hyper::Body> {
    response
        .status(StatusCode::NOT_MODIFIED)
        .body(hyper::Body::empty())
        .unwrap()
}

fn not_found() -> http::Response<hyper::Body> {
    http::Response::builder()
        .header_typed(ContentType::html())
        .status(StatusCode::NOT_FOUND)
        .body(hyper::Body::empty())
        .unwrap()
}

fn verify_gh(config: &Config, req: &http::request::Parts, body: &[u8]) -> bool {
    let gh_header = req.headers.get("X-Hub-Signature").cloned();
    let gh_header = gh_header.and_then(|g| g.to_str().ok().map(|s| s.to_owned()));
    let gh_header = match gh_header {
        Some(v) => v,
        None => {
            eprintln!("Could not find github header");
            return false
        }
    };
    verify_gh_sig(config, &gh_header, &body).unwrap_or(false)
}

fn verify_gh_sig(cfg: &Config, header: &str, body: &[u8]) -> Option<bool> {
    let key = hmac::Key::new(
        hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
        cfg.keys.github_webhook_secret.as_ref().unwrap().as_bytes(),
    );
    let sha = header.get(5..)?; // strip sha1=
    let sha = hex::decode(sha).ok()?;
    if let Ok(()) = hmac::verify(&key, body, &sha) {
        return Some(true);
    }
    eprintln!("HMAC verification failed");

    Some(false)
}

fn to_response<S>(result: ServerResult<S>, compression: &Option<BrotliEncoderParams>) -> Response
where
    S: Serialize,
{
    match result {
        Ok(result) => {
            let response = http::Response::builder()
                .header_typed(ContentType::octet_stream())
                .header_typed(CacheControl::new().with_no_cache().with_no_store());
            let body = rmp_serde::to_vec_named(&result).unwrap();
            maybe_compressed_response(response, body, compression)
        }
        Err(err) => http::Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header_typed(ContentType::text_utf8())
            .header_typed(CacheControl::new().with_no_cache().with_no_store())
            .body(hyper::Body::from(err))
            .unwrap(),
    }
}

fn maybe_compressed_response(
    response: http::response::Builder,
    body: Vec<u8>,
    compression: &Option<BrotliEncoderParams>,
) -> Response {
    match compression {
        None => response.body(hyper::Body::from(body)).unwrap(),
        Some(brotli_params) => {
            let compressed = compress_bytes(&body, brotli_params);
            let response = response.header(
                hyper::header::CONTENT_ENCODING,
                hyper::header::HeaderValue::from_static("br"),
            );
            response.body(hyper::Body::from(compressed)).unwrap()
        }
    }
}

fn compress_bytes(mut bytes: &[u8], brotli_params: &BrotliEncoderParams) -> Vec<u8> {
    let mut compressed = Vec::with_capacity(bytes.len());
    BrotliCompress(&mut bytes, &mut compressed, brotli_params).unwrap();
    compressed
}

fn to_triage_response(result: ServerResult<api::triage::Response>) -> Response {
    match result {
        Ok(result) => {
            let response = http::Response::builder().header_typed(ContentType::text());
            response.body(hyper::Body::from(result.0)).unwrap()
        }
        Err(err) => http::Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header_typed(ContentType::text_utf8())
            .body(hyper::Body::from(err.to_string()))
            .unwrap(),
    }
}

async fn run_server(ctxt: Arc<RwLock<Option<Arc<SiteCtxt>>>>, addr: SocketAddr) {
    let server = Server::new(ctxt);
    let svc = hyper::service::make_service_fn(move |_conn| {
        let ctx = server.clone();
        async move {
            Ok::<_, hyper::Error>(hyper::service::service_fn(move |req| {
                let start = std::time::Instant::now();
                let desc = format!("{} {}", req.method(), req.uri());
                serve_req(ctx.clone(), req)
                    .inspect(move |r| {
                        let dur = start.elapsed();
                        info!("{}: {:?} {:?}", desc, r.as_ref().map(|r| r.status()), dur)
                    })
                    .map(|mut r| {
                        if let Ok(r) = &mut r {
                            r.headers_mut().insert(
                                hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                                hyper::header::HeaderValue::from_static("*"),
                            );
                        }
                        r
                    })
            }))
        }
    });
    let server = hyper::server::Server::bind(&addr).serve(svc);
    if let Err(e) = server.await {
        eprintln!("server error: {:?}", e);
    }
}

pub async fn start(ctxt: Arc<RwLock<Option<Arc<SiteCtxt>>>>, port: u16) {
    let mut server_address: SocketAddr = "0.0.0.0:2346".parse().unwrap();
    server_address.set_port(port);
    run_server(ctxt, server_address).await;
}

pub trait ResponseHeaders {
    fn header_typed<T: headers::Header>(self, h: T) -> Self;
}

impl ResponseHeaders for http::response::Builder {
    fn header_typed<T: headers::Header>(mut self, h: T) -> Self {
        let mut v = vec![];
        h.encode(&mut v);
        for value in v {
            self = self.header(T::name(), value);
        }
        self
    }
}
