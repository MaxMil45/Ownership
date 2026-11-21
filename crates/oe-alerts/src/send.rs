//! HTTP delivery. Webhook URLs carry secrets (a Discord webhook URL is its own
//! password), so they come from environment variables and are never printed,
//! not even in error messages.

use crate::render;
use crate::Alert;
use std::time::Duration;

/// Discord webhook URL (`https://discord.com/api/webhooks/...`).
pub const DISCORD_ENV: &str = "OE_DISCORD_WEBHOOK_URL";
/// Any HTTPS endpoint that accepts a JSON POST (see `render::json`).
pub const WEBHOOK_ENV: &str = "OE_ALERT_WEBHOOK_URL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Discord,
    Webhook,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Destination {
    pub kind: Kind,
    url: String,
}

// Hand-written so the URL cannot leak through `{:?}`.
impl std::fmt::Debug for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Destination({})", self.name())
    }
}

impl Destination {
    /// Checks the URL: HTTPS, or plain HTTP to this machine only (for tests
    /// and local relays).
    pub fn new(kind: Kind, url: &str) -> Result<Destination, String> {
        let url = url.trim();
        let local = ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"];
        if !(url.starts_with("https://") || local.iter().any(|p| url.starts_with(p))) {
            return Err(format!(
                "the {} URL must start with https:// (plain http is allowed only to this machine)",
                kind_name(&kind)
            ));
        }
        Ok(Destination {
            kind,
            url: url.to_string(),
        })
    }

    /// The name recorded in the sent log.
    pub fn name(&self) -> &'static str {
        kind_name(&self.kind)
    }
}

fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Discord => "discord",
        Kind::Webhook => "webhook",
    }
}

/// The destinations configured in the environment, in a fixed order.
pub fn destinations_from_env() -> Result<Vec<Destination>, String> {
    let mut out = Vec::new();
    for (env, kind) in [(DISCORD_ENV, Kind::Discord), (WEBHOOK_ENV, Kind::Webhook)] {
        if let Ok(url) = std::env::var(env) {
            if !url.trim().is_empty() {
                out.push(Destination::new(kind, &url).map_err(|e| format!("{env}: {e}"))?);
            }
        }
    }
    Ok(out)
}

/// Pause between two messages to one destination. Discord allows about five
/// webhook messages per two seconds.
pub const PACE: Duration = Duration::from_millis(500);

pub struct Sender {
    agent: ureq::Agent,
}

impl Default for Sender {
    fn default() -> Self {
        Sender::new()
    }
}

impl Sender {
    pub fn new() -> Sender {
        Sender {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .user_agent("ownership-engine-alerts")
                .build(),
        }
    }

    /// Delivers one alert. On HTTP 429 it waits as long as the server asks
    /// (at most 10 s) and tries once more.
    pub fn send(&self, to: &Destination, alert: &Alert) -> Result<(), String> {
        let body = match to.kind {
            Kind::Discord => render::discord_payload(alert),
            Kind::Webhook => render::json(alert),
        };
        for attempt in 0..2 {
            let result = self
                .agent
                .post(&to.url)
                .set("Content-Type", "application/json")
                .send_string(&body);
            match result {
                Ok(_) => return Ok(()),
                Err(ureq::Error::Status(429, resp)) if attempt == 0 => {
                    let wait = resp
                        .header("Retry-After")
                        .and_then(|v| v.trim().parse::<f64>().ok())
                        .unwrap_or(2.0)
                        .clamp(0.0, 10.0);
                    std::thread::sleep(Duration::from_secs_f64(wait));
                }
                Err(ureq::Error::Status(code, _)) => {
                    return Err(format!("{} answered HTTP {code}", to.name()))
                }
                // The transport error's text can contain the URL: give only its kind.
                Err(ureq::Error::Transport(t)) => {
                    return Err(format!("{} could not be reached ({})", to.name(), t.kind()))
                }
            }
        }
        Err(format!(
            "{} kept answering HTTP 429 (too many requests)",
            to.name()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Alert;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn alert() -> Alert {
        Alert {
            key: "k".into(),
            rule_id: "activist.new_13d@1",
            amount_usd: None,
            percent: None,
            known_on: None,
            issuer_cik: None,
            issuer_name: None,
            ticker: None,
            headline: "New 13D at Example Corp".into(),
            details: vec![],
            people: vec![],
            receipts: vec![],
        }
    }

    /// A one-request HTTP server on this machine. Answers each request with
    /// the next status in `statuses` and returns what it received.
    fn server(statuses: Vec<u16>) -> (String, std::thread::JoinHandle<Vec<(String, String)>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/hook/secret-token",
            listener.local_addr().unwrap().port()
        );
        let handle = std::thread::spawn(move || {
            let mut got = Vec::new();
            for status in statuses {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut length = 0usize;
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).unwrap();
                let mut stream = reader.into_inner();
                let extra = if status == 429 {
                    "Retry-After: 0\r\n"
                } else {
                    ""
                };
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                got.push((
                    request_line.trim().to_string(),
                    String::from_utf8(body).unwrap(),
                ));
            }
            got
        });
        (url, handle)
    }

    #[test]
    fn posts_discord_and_webhook_bodies_over_real_http() {
        let (url, h) = server(vec![204, 200]);
        let sender = Sender::new();
        sender
            .send(&Destination::new(Kind::Discord, &url).unwrap(), &alert())
            .unwrap();
        sender
            .send(&Destination::new(Kind::Webhook, &url).unwrap(), &alert())
            .unwrap();
        let got = h.join().unwrap();
        assert_eq!(got[0].0, "POST /hook/secret-token HTTP/1.1");
        assert!(got[0]
            .1
            .starts_with("{\"content\":\"**New 13D at Example Corp**"));
        assert!(got[1]
            .1
            .starts_with("{\"key\":\"k\",\"rule_id\":\"activist.new_13d@1\""));
    }

    #[test]
    fn retries_once_after_429_and_reports_errors_without_the_url() {
        let (url, h) = server(vec![429, 204]);
        let to = Destination::new(Kind::Discord, &url).unwrap();
        Sender::new().send(&to, &alert()).unwrap();
        assert_eq!(h.join().unwrap().len(), 2);

        let (url, h) = server(vec![500]);
        let to = Destination::new(Kind::Webhook, &url).unwrap();
        let err = Sender::new().send(&to, &alert()).unwrap_err();
        h.join().unwrap();
        assert_eq!(err, "webhook answered HTTP 500");
        assert!(!format!("{to:?}").contains("secret"));

        // Nothing listens on port 9 here: a transport error, still no URL.
        let to = Destination::new(Kind::Webhook, "http://127.0.0.1:9/secret-token").unwrap();
        let err = Sender::new().send(&to, &alert()).unwrap_err();
        assert!(err.starts_with("webhook could not be reached"), "{err}");
        assert!(!err.contains("secret"));
    }

    #[test]
    fn only_https_or_this_machine() {
        assert!(Destination::new(Kind::Discord, "https://discord.com/api/webhooks/1/x").is_ok());
        assert!(Destination::new(Kind::Webhook, "http://example.com/hook").is_err());
        assert!(Destination::new(Kind::Webhook, "ftp://x").is_err());
        assert!(Destination::new(Kind::Webhook, "http://localhost:8080/x").is_ok());
    }
}
