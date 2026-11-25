//! `oe serve`: the ledger as a small JSON API on this computer only, for the
//! browser extension and the local app (ADR 0010, 1, option A).
//!
//! * Bound to 127.0.0.1, never another address.
//! * Every endpoint except `GET /v1/health` needs `Authorization: Bearer
//!   <token>`. The token is made once, kept in `data/serve/token.txt` and
//!   printed at start; `--new-token` replaces it.
//! * The `Host` header must name 127.0.0.1 or localhost with the port (a web
//!   page cannot reach the server through a DNS name that points here).
//! * A request with an `Origin` header is refused unless the origin is a
//!   browser extension (`chrome-extension://...`) or was given with
//!   `--allow-origin` (for the local app's development server). Only those
//!   get CORS headers.
//! * Names to look up travel in a POST body, never in the URL, and are never
//!   printed or logged: the log has the method, path, status and time only.
//! * The built web app (`web/dist`) is served from the same address. Its
//!   pages hold no secret: the browser signs in by opening the login link
//!   printed at start (`/login?code=`, a code made fresh for each run), which
//!   sets the token as an `HttpOnly`, `SameSite=Strict` cookie. The cookie is
//!   accepted only on requests from the app's own origin, never from another
//!   site or an extension. Pages get a strict Content-Security-Policy.
//!
//! Plain HTTP/1.1 on `std::net`, one connection per request, a few worker
//! threads each with its own database connection (ADR 0001: no new
//! dependency).

use oe_alerts::render::{json_number, json_string};
use oe_store::{Lookup, PersonCard, Receipt};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 8730;
/// Where `npm run build` puts the web app.
const DEFAULT_APP: &str = "web/dist";
const TOKEN_FILE: &str = "data/serve/token.txt";
const WORKERS: usize = 4;
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 4 * 1024;
const MAX_NAME: usize = 200;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Whole-ledger answers (status, daily flow) take seconds; they are kept
/// this long.
const CACHE_FOR: Duration = Duration::from_secs(600);
const LOOKUP_LIMIT: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    pub token: String,
    /// Web origins allowed besides browser extensions, exactly as sent.
    pub allowed_origins: Vec<String>,
    /// The built web app to serve, if any.
    pub app_dir: Option<PathBuf>,
    /// Opens `/login` for this run (32 hex characters).
    pub login_code: String,
}

const COOKIE: &str = "oe_token";
/// Thirty days; the token itself lasts until `--new-token`.
const COOKIE_MAX_AGE: u32 = 30 * 86_400;
const MAX_FILE: u64 = 25 * 1024 * 1024;
const APP_CSP: &str =
    "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; \
frame-ancestors 'none'";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The path without its query string.
    pub path: String,
    /// The query string, undecoded (never logged).
    pub query: String,
    /// Header names in lower case, in order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Reads the request line and headers (`head` ends before the blank line).
pub fn parse_head(head: &str) -> Result<Request, &'static str> {
    let mut lines = head.split("\r\n");
    let first = lines.next().ok_or("empty request")?;
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("malformed request line");
    };
    if !version.starts_with("HTTP/1.") {
        return Err("only HTTP/1.x");
    }
    if !target.starts_with('/') {
        return Err("only origin-form request targets");
    }
    let target = target.split('#').next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (path, query) = (path.to_string(), query.to_string());
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("malformed header")?;
        if name.is_empty() || name.contains(' ') {
            return Err("malformed header");
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok(Request {
        method: method.to_string(),
        path,
        query,
        headers,
        body: Vec::new(),
    })
}

/// `%41` -> `A`, `+` -> space. `None` for a bad escape or text that is not
/// UTF-8.
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// The query string's parameters, decoded. A name given twice keeps its
/// last value.
pub fn query_params(query: &str) -> Result<HashMap<String, String>, &'static str> {
    let mut out = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let k = percent_decode(k).ok_or("bad escape in the query string")?;
        let v = percent_decode(v).ok_or("bad escape in the query string")?;
        out.insert(k, v);
    }
    Ok(out)
}

fn is_date(s: &str) -> bool {
    s.len() == 10 && s.parse::<oe_core::Date>().is_ok()
}

/// The `/v1/events` filter from the query string, checked.
pub fn event_filter(query: &str) -> Result<oe_store::EventFilter, String> {
    let params = query_params(query)?;
    let mut f = oe_store::EventFilter {
        limit: 50,
        ..Default::default()
    };
    for (k, v) in &params {
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        match k.as_str() {
            "since" | "until" => {
                if !is_date(v) {
                    return Err(format!("{k} must be a date YYYY-MM-DD"));
                }
                if k == "since" {
                    f.since = Some(v.to_string());
                } else {
                    f.until = Some(v.to_string());
                }
            }
            "rule" => {
                if v.len() > 60
                    || !v.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b"._@".contains(&b)
                    })
                {
                    return Err(
                        "rule must look like insider.cluster_buy or insider.cluster_buy@1".into(),
                    );
                }
                f.rule = Some(v.to_string());
            }
            "ticker" => {
                if v.len() > 12
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                {
                    return Err("ticker must be letters, digits, '.' or '-'".into());
                }
                f.ticker = Some(v.to_string());
            }
            "cik" => f.issuer_cik = Some(v.parse().map_err(|_| "cik must be a number")?),
            "min_amount" => {
                let m: f64 = v.parse().map_err(|_| "min_amount must be a number")?;
                if !m.is_finite() {
                    return Err("min_amount must be a number".into());
                }
                f.min_amount = Some(m);
            }
            "q" => {
                if v.chars().count() > 100 {
                    return Err("q must be at most 100 characters".into());
                }
                f.text = Some(v.to_string());
            }
            "limit" => {
                f.limit = v
                    .parse()
                    .ok()
                    .filter(|n| (1..=500).contains(n))
                    .ok_or("limit must be 1 to 500")?
            }
            "offset" => {
                f.offset = v
                    .parse()
                    .ok()
                    .filter(|n| (0..=1_000_000).contains(n))
                    .ok_or("offset must be 0 to 1000000")?
            }
            other => return Err(format!("unknown parameter {other:?}")),
        }
    }
    Ok(f)
}

/// What to do with a request before routing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// A CORS preflight from an allowed origin.
    Preflight(String),
    /// Route it; CORS headers for this origin, if it sent one.
    Handle(Option<String>),
    /// A page or file of the web app, or the login link: no token needed.
    App,
    Reject(u16, &'static str),
}

/// Equal in time whatever the content (the token compare).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// The app's own origins (the pages `oe serve` serves itself).
fn own_origin(origin: &str, cfg: &Config) -> bool {
    origin == format!("http://127.0.0.1:{}", cfg.port)
        || origin == format!("http://localhost:{}", cfg.port)
}

/// The token from the `oe_token` cookie, if the request sent one.
fn cookie_token(req: &Request) -> Option<&str> {
    req.header("cookie")?
        .split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.trim())
}

pub fn origin_allowed(origin: &str, cfg: &Config) -> bool {
    let is_extension = ["chrome-extension://", "moz-extension://"]
        .iter()
        .any(|p| origin.len() > p.len() && origin.starts_with(p));
    is_extension || cfg.allowed_origins.iter().any(|o| o == origin)
}

pub fn check(req: &Request, cfg: &Config) -> Decision {
    let host_ok = req.header("host").is_some_and(|h| {
        h.eq_ignore_ascii_case(&format!("127.0.0.1:{}", cfg.port))
            || h.eq_ignore_ascii_case(&format!("localhost:{}", cfg.port))
    });
    if !host_ok {
        return Decision::Reject(403, "the Host header must be 127.0.0.1:<port>");
    }
    // The app's own pages send their origin on POST; they need no CORS.
    let same_origin = match req.header("origin") {
        None => true,
        Some(o) => own_origin(o, cfg),
    };
    let origin = match req.header("origin") {
        Some(o) if own_origin(o, cfg) => None,
        Some(o) if origin_allowed(o, cfg) => Some(o.to_string()),
        Some(_) => return Decision::Reject(403, "this origin may not use the API"),
        None => None,
    };
    if !req.path.starts_with("/v1/") && req.method != "OPTIONS" {
        if req.method != "GET" {
            return Decision::Reject(405, "method not allowed");
        }
        if cfg.app_dir.is_none() && req.path != "/login" {
            return Decision::Reject(
                404,
                "no web app is served (build it in web/ and start oe serve again)",
            );
        }
        return Decision::App;
    }
    if req.method == "OPTIONS" {
        return match origin {
            Some(o) => Decision::Preflight(o),
            None => Decision::Reject(400, "a preflight needs an Origin"),
        };
    }
    if req.method == "GET" && req.path == "/v1/health" {
        return Decision::Handle(origin);
    }
    let bearer_ok = req
        .header("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| same(t.trim(), &cfg.token));
    // The cookie counts only for the app's own pages (SameSite=Strict keeps
    // other sites from sending it; this keeps extensions from using it).
    let cookie_ok = same_origin && cookie_token(req).is_some_and(|t| same(t, &cfg.token));
    let token_ok = bearer_ok || cookie_ok;
    if !token_ok {
        return Decision::Reject(401, "missing or wrong token (see `oe serve`)");
    }
    Decision::Handle(origin)
}

/// A 64-character token from the operating system's randomness (the keys of
/// `RandomState` come from it), hashed.
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut seed = String::new();
    for i in 0..8u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        seed.push_str(&format!("{:016x}", h.finish()));
    }
    oe_core::sha256_hex(seed.as_bytes())
}

fn is_token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn load_or_make_token(path: &Path, renew: bool) -> Result<String, String> {
    if !renew {
        if let Ok(t) = std::fs::read_to_string(path) {
            if is_token(t.trim()) {
                return Ok(t.trim().to_string());
            }
        }
    }
    let t = new_token();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, format!("{t}\n")).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// Responses

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// Extra headers; `Cache-Control: no-store` unless one is given here.
    pub headers: Vec<(&'static str, String)>,
}

impl Response {
    fn json(status: u16, body: String) -> Response {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
            headers: Vec::new(),
        }
    }
    fn error(status: u16, message: &str) -> Response {
        Response::json(status, format!("{{\"error\":{}}}", json_string(message)))
    }
    fn text(status: u16, message: &str) -> Response {
        Response {
            status,
            content_type: "text/plain; charset=utf-8",
            body: message.as_bytes().to_vec(),
            headers: Vec::new(),
        }
    }
    fn with(mut self, name: &'static str, value: String) -> Response {
        self.headers.push((name, value));
        self
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The file under `root` that `path` names, or `None` if it would leave the
/// folder or does not exist. `/` is `index.html`.
pub fn app_file(root: &Path, path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(path)?;
    let rel = decoded.strip_prefix('/')?;
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let bad = rel.contains(['\\', '\0', ':'])
        || rel
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..");
    if bad {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let file = root.join(rel).canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

/// A page or file of the web app, or the login link.
fn app_response(req: &Request, cfg: &Config) -> Response {
    if req.path == "/login" {
        let code = query_params(&req.query)
            .ok()
            .and_then(|p| p.get("code").cloned())
            .unwrap_or_default();
        if !same(&code, &cfg.login_code) {
            return Response::text(
                403,
                "This login link is old or wrong. Use the one the oe serve window printed.",
            );
        }
        return Response::text(303, "signed in")
            .with("Location", "/".into())
            .with(
                "Set-Cookie",
                format!(
                    "{COOKIE}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={COOKIE_MAX_AGE}",
                    cfg.token
                ),
            );
    }
    let Some(root) = &cfg.app_dir else {
        return Response::text(404, "no web app is served");
    };
    let Some(file) = app_file(root, &req.path) else {
        return Response::text(404, "not found");
    };
    match std::fs::metadata(&file) {
        Ok(m) if m.len() <= MAX_FILE => {}
        _ => return Response::text(404, "not found"),
    }
    let Ok(body) = std::fs::read(&file) else {
        return Response::text(404, "not found");
    };
    let ct = content_type(&file);
    let mut r = Response {
        status: 200,
        content_type: ct,
        body,
        headers: Vec::new(),
    };
    if req.path.starts_with("/assets/") {
        // Vite names these by their content: a new build gets new names.
        r = r.with(
            "Cache-Control",
            "public, max-age=31536000, immutable".into(),
        );
    }
    if ct.starts_with("text/html") {
        r = r
            .with("Content-Security-Policy", APP_CSP.into())
            .with("X-Frame-Options", "DENY".into())
            .with("Referrer-Policy", "no-referrer".into());
    }
    r
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        303 => "See Other",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn write_response(
    stream: &mut TcpStream,
    r: &Response,
    origin: Option<&str>,
    preflight: bool,
) -> std::io::Result<()> {
    let status = r.status;
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        reason(status)
    );
    if !r.headers.iter().any(|(n, _)| *n == "Cache-Control") {
        head.push_str("Cache-Control: no-store\r\n");
    }
    for (name, value) in &r.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !preflight {
        head.push_str(&format!(
            "Content-Type: {}\r\nContent-Length: {}\r\n",
            r.content_type,
            r.body.len()
        ));
    } else {
        head.push_str("Content-Length: 0\r\n");
    }
    if let Some(o) = origin {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\n"
        ));
        if preflight {
            head.push_str(
                "Access-Control-Allow-Methods: GET, POST\r\n\
                 Access-Control-Allow-Headers: Authorization, Content-Type\r\n\
                 Access-Control-Max-Age: 600\r\n",
            );
        }
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if !preflight {
        stream.write_all(&r.body)?;
    }
    stream.flush()
}

fn read_request(stream: &mut TcpStream) -> Result<Request, (u16, &'static str)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err((400, "headers too large"));
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|_| (400, "could not read the request"))?;
        if n == 0 {
            return Err((400, "incomplete request"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| (400, "headers not UTF-8"))?;
    let mut req = parse_head(head).map_err(|m| (400, m))?;
    let length: usize = match req.header("content-length") {
        Some(v) => v.parse().map_err(|_| (400, "bad Content-Length"))?,
        None => 0,
    };
    if req.header("transfer-encoding").is_some() {
        return Err((400, "send a Content-Length, not chunked"));
    }
    if length > MAX_BODY {
        return Err((413, "body too large"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream
            .read(&mut chunk)
            .map_err(|_| (400, "could not read the body"))?;
        if n == 0 {
            return Err((400, "incomplete body"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    req.body = body;
    Ok(req)
}

// ---------------------------------------------------------------------------
// The API

struct Shared {
    cfg: Config,
    database_url: Option<String>,
    cache: Mutex<HashMap<&'static str, (Instant, String)>>,
}

struct Worker {
    shared: Arc<Shared>,
    client: Option<oe_store::Client>,
}

impl Worker {
    fn db(&mut self) -> Result<&mut oe_store::Client, Response> {
        if self.client.as_mut().is_some_and(|c| c.is_closed()) {
            self.client = None;
        }
        if self.client.is_none() {
            let url = self
                .shared
                .database_url
                .as_deref()
                .ok_or_else(|| Response::error(503, "DATABASE_URL is not set"))?;
            self.client = Some(
                oe_store::connect(url)
                    .map_err(|_| Response::error(503, "the database is not reachable"))?,
            );
        }
        Ok(self.client.as_mut().expect("connected above"))
    }

    fn cached(
        &mut self,
        key: &'static str,
        make: impl FnOnce(&mut oe_store::Client) -> Result<String, oe_store::StoreError>,
    ) -> Response {
        if let Some((at, body)) = self.shared.cache.lock().unwrap().get(key) {
            if at.elapsed() < CACHE_FOR {
                return Response::json(200, body.clone());
            }
        }
        let client = match self.db() {
            Ok(c) => c,
            Err(r) => return r,
        };
        match make(client) {
            Ok(body) => {
                self.shared
                    .cache
                    .lock()
                    .unwrap()
                    .insert(key, (Instant::now(), body.clone()));
                Response::json(200, body)
            }
            Err(e) => self.store_error(e),
        }
    }

    fn store_error(&mut self, e: oe_store::StoreError) -> Response {
        // A broken connection is made again on the next request.
        self.client = None;
        eprintln!("database error: {e}");
        Response::error(500, "database error (see the oe serve window)")
    }

    fn route(&mut self, req: &Request) -> Response {
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/v1/health") => Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"service\":\"ownership-engine\",\"version\":{}}}",
                    json_string(env!("CARGO_PKG_VERSION"))
                ),
            ),
            ("POST", "/v1/lookup") => self.lookup(req),
            ("GET", "/v1/status") => {
                self.cached("status", |c| oe_store::status(c).map(status_json))
            }
            ("GET", "/v1/flow") => self.cached("flow", |c| {
                oe_store::flow_by_day(c).map(|(days, left)| flow_json(&days, left))
            }),
            ("GET", "/v1/events") => self.events(req),
            ("GET", "/v1/company") => self.company(req),
            ("GET", "/v1/companies") => self.companies(req),
            ("GET", "/v1/events/counts") => {
                let client = match self.db() {
                    Ok(c) => c,
                    Err(r) => return r,
                };
                match oe_store::event_counts(client) {
                    Ok(rows) => Response::json(200, counts_json(&rows)),
                    Err(e) => self.store_error(e),
                }
            }
            (
                _,
                "/v1/lookup" | "/v1/status" | "/v1/flow" | "/v1/health" | "/v1/events"
                | "/v1/events/counts" | "/v1/company" | "/v1/companies",
            ) => Response::error(405, "method not allowed"),
            _ => Response::error(404, "no such endpoint"),
        }
    }

    fn events(&mut self, req: &Request) -> Response {
        let filter = match event_filter(&req.query) {
            Ok(f) => f,
            Err(m) => return Response::error(400, &m),
        };
        let client = match self.db() {
            Ok(c) => c,
            Err(r) => return r,
        };
        match oe_store::query_events(client, &filter) {
            Ok(page) => Response::json(
                200,
                format!(
                    "{{\"total\":{},\"limit\":{},\"offset\":{},\"built_at\":{},\"events\":[{}]}}",
                    page.total,
                    filter.limit,
                    filter.offset,
                    opt_str(page.built_at.as_deref()),
                    page.events.join(",")
                ),
            ),
            Err(e) => self.store_error(e),
        }
    }

    fn company(&mut self, req: &Request) -> Response {
        let cik = match query_params(&req.query) {
            Ok(p) => p.get("cik").and_then(|c| c.trim().parse::<i64>().ok()),
            Err(_) => None,
        };
        let Some(cik) = cik.filter(|c| *c > 0) else {
            return Response::error(400, "expected ?cik=<the company's CIK>");
        };
        let client = match self.db() {
            Ok(c) => c,
            Err(r) => return r,
        };
        match oe_store::company(client, cik) {
            Ok(Some(c)) => Response::json(200, company_json(&c)),
            Ok(None) => Response::error(404, "no Form 3/4/5 filing names this company"),
            Err(e) => self.store_error(e),
        }
    }

    fn companies(&mut self, req: &Request) -> Response {
        let q = query_params(&req.query)
            .ok()
            .and_then(|p| p.get("q").cloned())
            .unwrap_or_default();
        if q.trim().is_empty() || q.chars().count() > 100 {
            return Response::error(400, "expected ?q= with 1 to 100 characters");
        }
        let client = match self.db() {
            Ok(c) => c,
            Err(r) => return r,
        };
        match oe_store::find_companies(client, &q, 10) {
            Ok(rows) => {
                let items: Vec<String> = rows
                    .iter()
                    .map(|(cik, name, ticker)| {
                        format!(
                            "{{\"cik\":{cik},\"name\":{},\"ticker\":{}}}",
                            json_string(name),
                            opt_str(ticker.as_deref())
                        )
                    })
                    .collect();
                Response::json(200, format!("{{\"companies\":[{}]}}", items.join(",")))
            }
            Err(e) => self.store_error(e),
        }
    }

    fn lookup(&mut self, req: &Request) -> Response {
        let json_type = req
            .header("content-type")
            .is_some_and(|t| t.split(';').next().unwrap_or("").trim() == "application/json");
        if !json_type {
            return Response::error(415, "send Content-Type: application/json");
        }
        let Ok(text) = std::str::from_utf8(&req.body) else {
            return Response::error(400, "body not UTF-8");
        };
        let Ok(body) = oe_edgar::json::parse(text) else {
            return Response::error(400, "body is not JSON");
        };
        let Some(name) = body.get("name").and_then(|n| n.as_str()) else {
            return Response::error(400, "expected {\"name\": \"...\"}");
        };
        if name.trim().is_empty() || name.chars().count() > MAX_NAME {
            return Response::error(400, "the name must be 1 to 200 characters");
        }
        let client = match self.db() {
            Ok(c) => c,
            Err(r) => return r,
        };
        match oe_store::lookup(client, name, LOOKUP_LIMIT) {
            Ok(l) => Response::json(200, lookup_json(&l)),
            Err(e) => self.store_error(e),
        }
    }
}

fn handle(worker: &mut Worker, mut stream: TcpStream) {
    let started = Instant::now();
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let req = match read_request(&mut stream) {
        Ok(r) => r,
        Err((status, msg)) => {
            let _ = write_response(&mut stream, &Response::error(status, msg), None, false);
            log("?", "?", status, started);
            return;
        }
    };
    let status = match check(&req, &worker.shared.cfg) {
        Decision::Reject(status, msg) => {
            let _ = write_response(&mut stream, &Response::error(status, msg), None, false);
            status
        }
        Decision::Preflight(o) => {
            let _ = write_response(&mut stream, &Response::text(204, ""), Some(&o), true);
            204
        }
        Decision::App => {
            let r = app_response(&req, &worker.shared.cfg);
            let _ = write_response(&mut stream, &r, None, false);
            r.status
        }
        Decision::Handle(origin) => {
            let r = worker.route(&req);
            let _ = write_response(&mut stream, &r, origin.as_deref(), false);
            r.status
        }
    };
    log(&req.method, &req.path, status, started);
}

/// Method, path, status and time only: never a body or a query string.
fn log(method: &str, path: &str, status: u16, started: Instant) {
    let path: String = path.chars().take(40).collect();
    let t = oe_alerts::select::now_unix().rem_euclid(86_400);
    eprintln!(
        "{:02}:{:02}:{:02} UTC {method} {path} {status} {} ms",
        t / 3600,
        t / 60 % 60,
        t % 60,
        started.elapsed().as_millis()
    );
}

/// Accepts connections on `listener` until the process ends.
fn run(listener: TcpListener, shared: Arc<Shared>) {
    let (tx, rx) = mpsc::channel::<TcpStream>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..WORKERS {
        let rx = Arc::clone(&rx);
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            let mut worker = Worker {
                shared,
                client: None,
            };
            loop {
                let next = rx.lock().unwrap().recv();
                match next {
                    Ok(stream) => handle(&mut worker, stream),
                    Err(_) => return,
                }
            }
        });
    }
    for stream in listener.incoming().flatten() {
        if tx.send(stream).is_err() {
            return;
        }
    }
}

pub fn serve(argv: &[String]) -> Result<(), String> {
    let mut port = DEFAULT_PORT;
    let mut allowed_origins = Vec::new();
    let mut renew = false;
    let mut token_file = PathBuf::from(TOKEN_FILE);
    let mut app_dir: Option<PathBuf> =
        Some(PathBuf::from(DEFAULT_APP)).filter(|d| d.join("index.html").is_file());
    let mut open = false;
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => {
                port = it
                    .next()
                    .and_then(|p| p.parse().ok())
                    .filter(|p| *p > 0)
                    .ok_or("--port needs a number from 1 to 65535")?
            }
            "--allow-origin" => {
                let o = it.next().ok_or("--allow-origin needs an origin")?;
                let local = ["http://localhost:", "http://127.0.0.1:"]
                    .iter()
                    .any(|p| o.starts_with(p) && o[p.len()..].parse::<u16>().is_ok());
                if !local {
                    return Err(format!(
                        "--allow-origin takes a local development origin such as http://localhost:5173, not {o:?}"
                    ));
                }
                allowed_origins.push(o.clone());
            }
            "--new-token" => renew = true,
            "--app" => {
                let d = super::args::folder(&mut it, "--app")?;
                if !d.join("index.html").is_file() {
                    return Err(format!(
                        "{} has no index.html (build the app: cd web; npm run build)",
                        d.display()
                    ));
                }
                app_dir = Some(d);
            }
            "--no-app" => app_dir = None,
            "--open" => open = true,
            "--token-file" => token_file = super::args::folder(&mut it, "--token-file")?,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let database_url = std::env::var("DATABASE_URL").ok();
    if database_url.is_none() {
        return Err("DATABASE_URL is not set (see `oe db-status`)".into());
    }
    let token = load_or_make_token(&token_file, renew)?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let login_code = new_token()[..32].to_string();
    let login = format!("http://127.0.0.1:{port}/login?code={login_code}");
    println!("oe serve: http://127.0.0.1:{port}/v1/ (this computer only)");
    match &app_dir {
        Some(d) => println!(
            "web app ({}): open {login} (signs this browser in; the link works until oe serve stops)",
            d.display()
        ),
        None => println!("no web app found (build it: cd web; npm run build)"),
    }
    println!(
        "token (kept in {}; paste it into the extension's options): {token}",
        token_file.display()
    );
    println!(
        "endpoints: GET /v1/health, POST /v1/lookup {{\"name\": ...}}, GET /v1/status, GET /v1/flow, GET /v1/events, GET /v1/events/counts, GET /v1/company?cik=, GET /v1/companies?q="
    );
    println!("Ctrl+C stops it. Looked-up names are never logged.");
    if open && app_dir.is_some() {
        open_browser(&login);
    }
    let shared = Arc::new(Shared {
        cfg: Config {
            port,
            token,
            allowed_origins,
            app_dir,
            login_code,
        },
        database_url,
        cache: Mutex::new(HashMap::new()),
    });
    warm_up(Arc::clone(&shared));
    run(listener, shared);
    Ok(())
}

/// Works out the two whole-ledger answers (status, daily flow) once in the
/// background, so the app's first page does not wait seconds for them.
fn warm_up(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut w = Worker {
            shared,
            client: None,
        };
        let started = Instant::now();
        let a = w.cached("status", |c| oe_store::status(c).map(status_json));
        let b = w.cached("flow", |c| {
            oe_store::flow_by_day(c).map(|(days, left)| flow_json(&days, left))
        });
        if a.status == 200 && b.status == 200 {
            eprintln!(
                "ready: status and flow worked out in {:.1} s",
                started.elapsed().as_secs_f64()
            );
        }
    });
}

/// Opens `url` in the default browser; prints a note if it cannot.
fn open_browser(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if r.is_err() {
        println!("could not open a browser; open the link above yourself");
    }
}

// ---------------------------------------------------------------------------
// JSON bodies

fn opt_str(v: Option<&str>) -> String {
    v.map_or_else(|| "null".to_string(), json_string)
}

fn str_array(v: &[String]) -> String {
    let items: Vec<String> = v.iter().map(|s| json_string(s)).collect();
    format!("[{}]", items.join(","))
}

fn receipt_json(r: &Receipt) -> String {
    format!(
        "{{\"accession\":{},\"url\":{},\"known_on\":{},\"filing_date_only\":{}}}",
        json_string(&r.accession),
        json_string(&r.url),
        json_string(&r.known_on),
        r.filing_date_only
    )
}

fn card_json(c: &PersonCard) -> String {
    let roles: Vec<String> = c
        .roles
        .iter()
        .map(|r| {
            format!(
                "{{\"issuer_cik\":{},\"issuer_name\":{},\"ticker\":{},\"director\":{},\"officer\":{},\"ten_percent_owner\":{},\"other\":{},\"titles\":{},\"first_on\":{},\"last_on\":{},\"filings\":{}}}",
                r.issuer_cik,
                json_string(&r.issuer_name),
                opt_str(r.ticker.as_deref()),
                r.director,
                r.officer,
                r.ten_percent_owner,
                r.other,
                str_array(&r.titles),
                json_string(&r.first_on),
                json_string(&r.last_on),
                r.filings
            )
        })
        .collect();
    let trades: Vec<String> = c
        .trades
        .iter()
        .map(|t| {
            let value = match (t.shares, t.price) {
                (Some(s), Some(p)) if oe_core::plausible_amounts(Some(p), Some(s * p)) => {
                    Some(s * p)
                }
                _ => None,
            };
            format!(
                "{{\"transaction_date\":{},\"code\":{},\"shares\":{},\"price\":{},\"value\":{},\"issuer_name\":{},\"ticker\":{},\"receipt\":{}}}",
                json_string(&t.transaction_date),
                json_string(&t.code),
                json_number(t.shares),
                json_number(t.price),
                json_number(value),
                json_string(&t.issuer_name),
                opt_str(t.ticker.as_deref()),
                receipt_json(&t.receipt)
            )
        })
        .collect();
    let holdings: Vec<String> = c
        .holdings
        .iter()
        .map(|h| {
            format!(
                "{{\"issuer_cik\":{},\"issuer_name\":{},\"class_title\":{},\"submission_type\":{},\"percent\":{},\"receipt\":{}}}",
                h.issuer_cik,
                json_string(&h.issuer_name),
                json_string(&h.class_title),
                json_string(&h.submission_type),
                json_number(h.percent),
                receipt_json(&h.receipt)
            )
        })
        .collect();
    let contests: Vec<String> = c
        .contests
        .iter()
        .map(|k| {
            format!(
                "{{\"subject_name\":{},\"form\":{},\"receipt\":{}}}",
                json_string(&k.subject_name),
                json_string(&k.form),
                receipt_json(&k.receipt)
            )
        })
        .collect();
    let official = match &c.official {
        None => "null".to_string(),
        Some(o) => {
            let committees: Vec<String> = o
                .committees
                .iter()
                .map(|(name, title, as_of)| {
                    format!(
                        "{{\"name\":{},\"title\":{},\"as_of\":{}}}",
                        json_string(name),
                        opt_str(title.as_deref()),
                        json_string(as_of)
                    )
                })
                .collect();
            let recent: Vec<String> = o
                .recent
                .iter()
                .map(|(filed, doc, scanned, url)| {
                    format!(
                        "{{\"filed_on\":{},\"doc_id\":{},\"scanned\":{scanned},\"url\":{}}}",
                        json_string(filed),
                        json_string(doc),
                        json_string(url)
                    )
                })
                .collect();
            format!(
                "{{\"bioguide\":{},\"name\":{},\"terms\":{},\"committees\":[{}],\"reports\":{},\"scanned_reports\":{},\"first_filed\":{},\"last_filed\":{},\"recent_reports\":[{}],\"use_limits\":{}}}",
                json_string(&o.bioguide),
                json_string(&o.name),
                str_array(&o.terms),
                committees.join(","),
                o.reports,
                o.scanned_reports,
                opt_str(o.first_filed.as_deref()),
                opt_str(o.last_filed.as_deref()),
                recent.join(","),
                json_string(
                    "House financial disclosure reports may not be used commercially (5 U.S.C. 13107)."
                )
            )
        }
    };
    format!(
        "{{\"key\":{},\"display_name\":{},\"kinds\":{},\"other_names\":{},\"filings\":{},\"first_on\":{},\"last_on\":{},\"exact\":{},\"roles\":[{}],\"trades\":[{}],\"holdings\":[{}],\"contests\":[{}],\"official\":{}}}",
        json_string(&c.key),
        json_string(&c.display_name),
        str_array(&c.kinds),
        str_array(&c.other_names),
        c.filings,
        opt_str(c.first_on.as_deref()),
        opt_str(c.last_on.as_deref()),
        c.exact,
        roles.join(","),
        trades.join(","),
        holdings.join(","),
        contests.join(","),
        official
    )
}

pub fn lookup_json(l: &Lookup) -> String {
    match l {
        Lookup::NeedsAName => {
            "{\"result\":\"needs_a_name\",\"message\":\"give at least one full word of the name\",\"candidates\":[]}"
                .to_string()
        }
        Lookup::TooBroad(n) => format!(
            "{{\"result\":\"too_broad\",\"count\":{n},\"message\":\"add a first name or initial\",\"candidates\":[]}}"
        ),
        Lookup::Found(cards) => {
            let items: Vec<String> = cards.iter().map(card_json).collect();
            let message = if cards.is_empty() {
                "No public ownership role found. (This says nothing about the person.)"
            } else {
                ""
            };
            format!(
                "{{\"result\":\"found\",\"count\":{},\"message\":{},\"candidates\":[{}]}}",
                cards.len(),
                json_string(message),
                items.join(",")
            )
        }
    }
}

fn status_json(s: oe_store::Status) -> String {
    format!(
        "{{\"filings\":{},\"bulk_filings\":{},\"transactions\":{},\"transactions_known_now\":{},\"form144_notices\":{},\"schedule13_filings\":{},\"contest_filings\":{},\"trading_arrangements\":{},\"earliest_accepted\":{},\"latest_accepted\":{},\"migrations\":{}}}",
        s.filings,
        s.bulk_filings,
        s.transactions,
        s.transactions_known_now,
        s.form144_notices,
        s.schedule13_filings,
        s.contest_filings,
        s.trading_arrangements,
        opt_str(s.earliest_accepted.as_deref()),
        opt_str(s.latest_accepted.as_deref()),
        str_array(&s.migrations)
    )
}

fn company_json(c: &oe_store::Company) -> String {
    let insiders: Vec<String> = c
        .insiders
        .iter()
        .map(|i| {
            format!(
                "{{\"owner_cik\":{},\"name\":{},\"director\":{},\"officer\":{},\"ten_percent_owner\":{},\"other\":{},\"titles\":{},\"first_on\":{},\"last_on\":{},\"filings\":{}}}",
                i.owner_cik,
                json_string(&i.name),
                i.director,
                i.officer,
                i.ten_percent_owner,
                i.other,
                str_array(&i.titles),
                json_string(&i.first_on),
                json_string(&i.last_on),
                i.filings
            )
        })
        .collect();
    let trades: Vec<String> = c
        .trades
        .iter()
        .map(|t| {
            let value = match (t.shares, t.price) {
                (Some(s), Some(p)) if oe_core::plausible_amounts(Some(p), Some(s * p)) => {
                    Some(s * p)
                }
                _ => None,
            };
            format!(
                "{{\"transaction_date\":{},\"code\":{},\"owner_name\":{},\"shares\":{},\"price\":{},\"value\":{},\"receipt\":{}}}",
                json_string(&t.transaction_date),
                json_string(&t.code),
                json_string(&t.owner_name),
                json_number(t.shares),
                json_number(t.price),
                json_number(value),
                receipt_json(&t.receipt)
            )
        })
        .collect();
    let days: Vec<String> = c
        .flow
        .iter()
        .map(|(day, bv, b, sv, s)| {
            format!(
                "{{\"day\":{},\"buy_value\":{},\"buys\":{b},\"sell_value\":{},\"sells\":{s}}}",
                json_string(day),
                json_number(Some(*bv)),
                json_number(Some(*sv))
            )
        })
        .collect();
    format!(
        "{{\"cik\":{},\"name\":{},\"ticker\":{},\"filings\":{},\"first_on\":{},\"last_on\":{},\"insiders\":[{}],\"trades\":[{}],\"flow\":{{\"days\":[{}],\"implausible_rows_left_out\":0}}}}",
        c.cik,
        json_string(&c.name),
        opt_str(c.ticker.as_deref()),
        c.filings,
        opt_str(c.first_on.as_deref()),
        opt_str(c.last_on.as_deref()),
        insiders.join(","),
        trades.join(","),
        days.join(",")
    )
}

fn counts_json(rows: &[(String, String, i64)]) -> String {
    let items: Vec<String> = rows
        .iter()
        .map(|(day, rule, n)| {
            format!(
                "{{\"day\":{},\"rule_id\":{},\"events\":{n}}}",
                json_string(day),
                json_string(rule)
            )
        })
        .collect();
    format!("{{\"counts\":[{}]}}", items.join(","))
}

fn flow_json(days: &[oe_store::FlowDay], left_out: i64) -> String {
    let items: Vec<String> = days
        .iter()
        .map(|d| {
            format!(
                "{{\"day\":{},\"buy_value\":{},\"buys\":{},\"sell_value\":{},\"sells\":{}}}",
                json_string(&d.day),
                json_number(Some(d.buy_value)),
                d.buys,
                json_number(Some(d.sell_value)),
                d.sells
            )
        })
        .collect();
    format!(
        "{{\"days\":[{}],\"implausible_rows_left_out\":{left_out}}}",
        items.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            port: 8730,
            token: "a".repeat(64),
            allowed_origins: vec!["http://localhost:5173".into()],
            app_dir: None,
            login_code: "b".repeat(32),
        }
    }

    fn req(head: &str) -> Request {
        parse_head(head).unwrap()
    }

    const AUTH: &str =
        "Authorization: Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn parses_the_request_line_and_headers_without_the_query() {
        let r = req("POST /v1/lookup?x=secret HTTP/1.1\r\nHost: 127.0.0.1:8730\r\nContent-Type: application/json");
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/v1/lookup"));
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert!(parse_head("GET http://evil/ HTTP/1.1").is_err());
        assert!(parse_head("GET / HTTP/2").is_err());
        assert!(parse_head("GET / HTTP/1.1\r\nno colon").is_err());
        assert!(parse_head("GET /  HTTP/1.1").is_err());
    }

    #[test]
    fn query_strings_are_decoded_and_event_filters_checked() {
        assert_eq!(
            percent_decode("a%20b+c%C3%A9").as_deref(),
            Some("a b c\u{e9}")
        );
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%4"), None);
        let f = event_filter("since=2026-11-01&rule=insider.cluster_buy&ticker=BRK.B&min_amount=1e6&q=elliott+mgmt&limit=10&offset=20").unwrap();
        assert_eq!(f.since.as_deref(), Some("2026-11-01"));
        assert_eq!(f.rule.as_deref(), Some("insider.cluster_buy"));
        assert_eq!(f.ticker.as_deref(), Some("BRK.B"));
        assert_eq!(f.min_amount, Some(1e6));
        assert_eq!(f.text.as_deref(), Some("elliott mgmt"));
        assert_eq!((f.limit, f.offset), (10, 20));
        assert_eq!(event_filter("").unwrap().limit, 50);
        assert_eq!(event_filter("since=").unwrap().since, None);
        for bad in [
            "since=2026-13-01",
            "since=yesterday",
            "rule=Insider;DROP",
            "ticker=A%27B",
            "limit=0",
            "limit=501",
            "offset=-1",
            "cik=abc",
            "min_amount=NaN",
            "colour=red",
        ] {
            assert!(event_filter(bad).is_err(), "{bad}");
        }
        let r = req("GET /v1/events?since=2026-11-01 HTTP/1.1\r\nHost: 127.0.0.1:8730");
        assert_eq!(
            (r.path.as_str(), r.query.as_str()),
            ("/v1/events", "since=2026-11-01")
        );
    }

    #[test]
    fn health_needs_no_token_everything_else_does() {
        let c = cfg();
        assert_eq!(
            check(&req("GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:8730"), &c),
            Decision::Handle(None)
        );
        assert_eq!(
            check(&req("GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1:8730"), &c),
            Decision::Reject(401, "missing or wrong token (see `oe serve`)")
        );
        let wrong = format!(
            "GET /v1/status HTTP/1.1\r\nHost: localhost:8730\r\nAuthorization: Bearer {}",
            "b".repeat(64)
        );
        assert!(matches!(check(&req(&wrong), &c), Decision::Reject(401, _)));
        let right = format!("GET /v1/status HTTP/1.1\r\nHost: localhost:8730\r\n{AUTH}");
        assert_eq!(check(&req(&right), &c), Decision::Handle(None));
    }

    #[test]
    fn other_hosts_and_web_origins_are_refused() {
        let c = cfg();
        for host in ["evil.example:8730", "127.0.0.1:9999", "127.0.0.1"] {
            let r = format!("GET /v1/health HTTP/1.1\r\nHost: {host}");
            assert!(
                matches!(check(&req(&r), &c), Decision::Reject(403, _)),
                "{host}"
            );
        }
        let no_host = req("GET /v1/health HTTP/1.1");
        assert!(matches!(check(&no_host, &c), Decision::Reject(403, _)));
        let web = format!("POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:8730\r\nOrigin: https://news.example\r\n{AUTH}");
        assert!(matches!(check(&req(&web), &c), Decision::Reject(403, _)));
        let ext = format!("POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:8730\r\nOrigin: chrome-extension://abcdef\r\n{AUTH}");
        assert_eq!(
            check(&req(&ext), &c),
            Decision::Handle(Some("chrome-extension://abcdef".into()))
        );
        assert!(!origin_allowed("chrome-extension://", &c));
        assert!(origin_allowed("http://localhost:5173", &c));
        assert!(!origin_allowed("http://localhost:5174", &c));
    }

    #[test]
    fn preflights_are_answered_only_for_allowed_origins() {
        let c = cfg();
        let ok = req(
            "OPTIONS /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:8730\r\nOrigin: http://localhost:5173",
        );
        assert_eq!(
            check(&ok, &c),
            Decision::Preflight("http://localhost:5173".into())
        );
        let bad = req(
            "OPTIONS /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:8730\r\nOrigin: https://evil.example",
        );
        assert!(matches!(check(&bad, &c), Decision::Reject(403, _)));
    }

    #[test]
    fn tokens_are_64_hex_characters_and_differ() {
        let (a, b) = (new_token(), new_token());
        assert!(is_token(&a) && is_token(&b));
        assert_ne!(a, b);
        let dir = std::env::temp_dir().join(format!("oe-serve-token-{}", std::process::id()));
        let path = dir.join("token.txt");
        let first = load_or_make_token(&path, false).unwrap();
        assert_eq!(load_or_make_token(&path, false).unwrap(), first, "kept");
        assert_ne!(load_or_make_token(&path, true).unwrap(), first, "renewed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lookup_json_says_what_kind_of_answer_it_is() {
        assert!(lookup_json(&Lookup::NeedsAName).contains("\"needs_a_name\""));
        assert!(lookup_json(&Lookup::TooBroad(43)).contains("\"count\":43"));
        let empty = lookup_json(&Lookup::Found(Vec::new()));
        assert!(empty.contains("\"count\":0") && empty.contains("No public ownership role"));
        let parsed = oe_edgar::json::parse(&empty).unwrap();
        assert_eq!(parsed.get("result").and_then(|r| r.as_str()), Some("found"));
    }

    /// The real server on a free port, without a database: what a browser
    /// or another program would get.
    #[test]
    fn the_server_answers_health_and_refuses_without_a_token() {
        let app = std::env::temp_dir().join(format!("oe-serve-app-{}", std::process::id()));
        std::fs::create_dir_all(app.join("assets")).unwrap();
        std::fs::write(app.join("index.html"), "<!doctype html><title>t</title>").unwrap();
        std::fs::write(app.join("assets/app-1.js"), "console.log(1)").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Shared {
            cfg: Config {
                port,
                token: "c".repeat(64),
                allowed_origins: Vec::new(),
                app_dir: Some(app.clone()),
                login_code: "d".repeat(32),
            },
            database_url: None,
            cache: Mutex::new(HashMap::new()),
        });
        std::thread::spawn(move || run(listener, shared));
        let ask = |raw: String| -> String {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(raw.as_bytes()).unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let health = ask(format!(
            "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        ));
        assert!(health.starts_with("HTTP/1.1 200 OK"), "{health}");
        assert!(health.ends_with("}") && health.contains("\"ok\":true"));
        let body = r#"{"name":"x y"}"#;
        let no_token = ask(format!(
            "POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        assert!(no_token.starts_with("HTTP/1.1 401"), "{no_token}");
        let no_db = ask(format!(
            "POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            "c".repeat(64),
            body.len()
        ));
        assert!(no_db.starts_with("HTTP/1.1 503"), "{no_db}");
        let wrong_type = ask(format!(
            "POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Length: 0\r\n\r\n",
            "c".repeat(64)
        ));
        assert!(wrong_type.starts_with("HTTP/1.1 415"), "{wrong_type}");
        let preflight = ask(format!(
            "OPTIONS /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: chrome-extension://abc\r\n\r\n"
        ));
        assert!(preflight.starts_with("HTTP/1.1 204"), "{preflight}");
        assert!(preflight.contains("Access-Control-Allow-Origin: chrome-extension://abc"));
        let too_big = ask(format!(
            "POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 99999\r\n\r\n"
        ));
        assert!(too_big.starts_with("HTTP/1.1 413"), "{too_big}");

        // The app's page: no token needed, strict headers, no secret in it.
        let page = ask(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"));
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(page.contains("Content-Type: text/html"));
        assert!(page.contains("Content-Security-Policy: default-src 'self'"));
        assert!(page.contains("X-Frame-Options: DENY"));
        assert!(
            !page.contains(&"c".repeat(64)),
            "the token is never in a page"
        );
        let asset = ask(format!(
            "GET /assets/app-1.js HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
        ));
        assert!(asset.contains("Content-Type: text/javascript"), "{asset}");
        assert!(asset.contains("immutable"));
        for bad in [
            "/../Cargo.toml",
            "/assets/../../x",
            "/%2e%2e/x",
            "/assets\\x.js",
            "/nope.js",
        ] {
            let r = ask(format!(
                "GET {bad} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
            ));
            assert!(r.starts_with("HTTP/1.1 404"), "{bad}: {r}");
        }

        // The login link sets the cookie; a wrong code does not.
        let wrong = ask(format!(
            "GET /login?code={} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n",
            "e".repeat(32)
        ));
        assert!(
            wrong.starts_with("HTTP/1.1 403") && !wrong.contains("Set-Cookie"),
            "{wrong}"
        );
        let login = ask(format!(
            "GET /login?code={} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n",
            "d".repeat(32)
        ));
        assert!(login.starts_with("HTTP/1.1 303"), "{login}");
        assert!(login.contains(&format!(
            "Set-Cookie: oe_token={}; HttpOnly; SameSite=Strict; Path=/",
            "c".repeat(64)
        )));
        assert!(login.contains("Location: /"));

        // With the cookie, the app's own requests work; from elsewhere they do not.
        let cookie = format!("Cookie: theme=x; oe_token={}", "c".repeat(64));
        let own = ask(format!(
            "POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\n{cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        assert!(
            own.starts_with("HTTP/1.1 503"),
            "authorised, then no database: {own}"
        );
        let ext = ask(format!(
            "POST /v1/lookup HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: chrome-extension://abc\r\n{cookie}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        assert!(
            ext.starts_with("HTTP/1.1 401"),
            "an extension needs the token itself: {ext}"
        );
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn the_cookie_counts_only_for_the_apps_own_origin() {
        let c = cfg();
        let with = |origin: &str| {
            let o = if origin.is_empty() {
                String::new()
            } else {
                format!("\r\nOrigin: {origin}")
            };
            req(&format!(
                "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1:8730{o}\r\nCookie: oe_token={}",
                "a".repeat(64)
            ))
        };
        assert_eq!(check(&with(""), &c), Decision::Handle(None));
        assert_eq!(
            check(&with("http://127.0.0.1:8730"), &c),
            Decision::Handle(None)
        );
        assert_eq!(
            check(&with("http://localhost:8730"), &c),
            Decision::Handle(None)
        );
        assert!(matches!(
            check(&with("chrome-extension://abc"), &c),
            Decision::Reject(401, _)
        ));
        assert!(matches!(
            check(&with("http://localhost:5173"), &c),
            Decision::Reject(401, _)
        ));
        assert!(matches!(
            check(&with("https://evil.example"), &c),
            Decision::Reject(403, _)
        ));
        let wrong = req("GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1:8730\r\nCookie: oe_token=zz");
        assert!(matches!(check(&wrong, &c), Decision::Reject(401, _)));
    }

    #[test]
    fn pages_need_no_token_but_still_the_right_host() {
        let mut c = cfg();
        let page = req("GET / HTTP/1.1\r\nHost: 127.0.0.1:8730");
        assert!(
            matches!(check(&page, &c), Decision::Reject(404, _)),
            "no app built"
        );
        assert_eq!(
            check(
                &req("GET /login?code=x HTTP/1.1\r\nHost: 127.0.0.1:8730"),
                &c
            ),
            Decision::App
        );
        c.app_dir = Some(PathBuf::from("web/dist"));
        assert_eq!(check(&page, &c), Decision::App);
        let post = req("POST / HTTP/1.1\r\nHost: 127.0.0.1:8730");
        assert!(matches!(check(&post, &c), Decision::Reject(405, _)));
        let rebound = req("GET / HTTP/1.1\r\nHost: evil.example:8730");
        assert!(matches!(check(&rebound, &c), Decision::Reject(403, _)));
    }

    #[test]
    fn app_files_stay_inside_the_folder() {
        let root = std::env::temp_dir().join(format!("oe-serve-files-{}", std::process::id()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), "x").unwrap();
        std::fs::write(root.join("assets/a.css"), "x").unwrap();
        assert!(app_file(&root, "/").is_some_and(|p| p.ends_with("index.html")));
        assert!(app_file(&root, "/assets/a.css").is_some());
        assert!(
            app_file(&root, "/assets/a%2ecss").is_some(),
            "escapes are decoded first"
        );
        for bad in [
            "/../x",
            "/assets/../../x",
            "/%2e%2e/x",
            "/a\\b",
            "//x",
            "/assets/",
            "/c:/x",
            "/missing",
            "relative",
        ] {
            assert!(app_file(&root, bad).is_none(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
