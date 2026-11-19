//! Blocking HTTP client for EDGAR (built with the `net` feature).

use crate::ratelimit::RateLimiter;
use crate::{validate_user_agent, UserAgentError, REQUESTS_PER_SECOND};
use std::fmt;
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

    pub fn from_env() -> Result<Self, EdgarError> {
        let ua = crate::user_agent_from_env().map_err(EdgarError::UserAgent)?;
        Self::new(&ua)
    }

    /// Fetches a URL as text, waiting for a rate-limit slot first.
    pub fn get_text(&self, url: &str) -> Result<String, EdgarError> {
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
        response.into_string().map_err(|e| EdgarError::Body {
            url: url.to_string(),
            message: e.to_string(),
        })
    }
}
