//! Blocking HTTP client for EDGAR (built with the `net` feature).

use crate::ratelimit::RateLimiter;
use crate::{validate_user_agent, UserAgentError, REQUESTS_PER_SECOND};
use std::fmt;
use std::io::Read;
use std::time::Duration;

#[derive(Debug)]
pub enum EdgarError {
    UserAgent(UserAgentError),
    /// The server answered with an error status (403 often means a missing
    /// or rejected User-Agent; 429 means too many requests).
    Status {
        url: String,
        code: u16,
    },
    Transport {
        url: String,
        message: String,
    },
    Body {
        url: String,
        message: String,
    },
}

impl fmt::Display for EdgarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EdgarError::UserAgent(e) => write!(f, "{e}"),
            EdgarError::Status { url, code } => write!(f, "HTTP {code} from {url}"),
            EdgarError::Transport { url, message } => {
                write!(f, "request to {url} failed: {message}")
            }
            EdgarError::Body { url, message } => write!(f, "could not read {url}: {message}"),
        }
    }
}

impl std::error::Error for EdgarError {}

pub struct EdgarClient {
    agent: ureq::Agent,
    limiter: RateLimiter,
}

impl EdgarClient {
    /// `user_agent` must name you and give an email, as the SEC requires.
    pub fn new(user_agent: &str) -> Result<Self, EdgarError> {
        let ua = validate_user_agent(user_agent).map_err(EdgarError::UserAgent)?;
        let agent = ureq::AgentBuilder::new()
            .user_agent(&ua)
            .timeout(Duration::from_secs(30))
            .build();
        Ok(EdgarClient {
            agent,
            limiter: RateLimiter::per_second(REQUESTS_PER_SECOND),
        })
    }

    /// A client for a public site other than the SEC (the House Clerk's
    /// disclosures): its own rate limit, and a plain product name as the
    /// User-Agent, with no personal details (those are for the SEC only).
    pub fn other_site(requests_per_second: u32) -> Self {
        let agent = ureq::AgentBuilder::new()
            .user_agent(concat!(
                "OwnershipEngine/",
                env!("CARGO_PKG_VERSION"),
                " (open-source research tool)"
            ))
            .timeout(Duration::from_secs(60))
            .build();
        EdgarClient {
            agent,
            limiter: RateLimiter::per_second(requests_per_second.max(1)),
        }
    }

    pub fn from_env() -> Result<Self, EdgarError> {
        let ua = crate::user_agent_from_env().map_err(EdgarError::UserAgent)?;
        Self::new(&ua)
    }

    /// Fetches a URL as text, waiting for a rate-limit slot first.
    pub fn get_text(&self, url: &str) -> Result<String, EdgarError> {
        Ok(crate::decode_body(self.get_bytes(url)?))
    }

    /// Fetches a URL as raw bytes (zip files, PDFs), waiting for a
    /// rate-limit slot first.
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>, EdgarError> {
        let mut bytes = Vec::new();
        self.open(url)?
            .read_to_end(&mut bytes)
            .map_err(|e| EdgarError::Body {
                url: url.to_string(),
                message: e.to_string(),
            })?;
        Ok(bytes)
    }

    /// Streams a URL into `path` without holding it in memory. The body goes
    /// to `<path>.part` first and is renamed when complete, so an interrupted
    /// download never leaves a truncated file under the final name. Returns
    /// the number of bytes written.
    pub fn save_to_file(&self, url: &str, path: &std::path::Path) -> Result<u64, EdgarError> {
        let body_err = |message: String| EdgarError::Body {
            url: url.to_string(),
            message,
        };
        let mut reader = self.open(url)?;
        let part = path.with_extension(match path.extension() {
            Some(e) => format!("{}.part", e.to_string_lossy()),
            None => "part".to_string(),
        });
        let mut file = std::fs::File::create(&part)
            .map_err(|e| body_err(format!("{}: {e}", part.display())))?;
        let n = std::io::copy(&mut reader, &mut file).map_err(|e| body_err(e.to_string()))?;
        drop(file);
        std::fs::rename(&part, path).map_err(|e| body_err(format!("{}: {e}", path.display())))?;
        Ok(n)
    }

    fn open(&self, url: &str) -> Result<Box<dyn Read + Send + Sync + 'static>, EdgarError> {
        self.limiter.acquire();
        let response = self.agent.get(url).call().map_err(|e| match e {
            ureq::Error::Status(code, _) => EdgarError::Status {
                url: url.to_string(),
                code,
            },
            other => EdgarError::Transport {
                url: url.to_string(),
                message: other.to_string(),
            },
        })?;
        Ok(response.into_reader())
    }
}
