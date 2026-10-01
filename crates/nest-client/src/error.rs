use std::fmt;

/// What went wrong with one request. The caller names the endpoint; this says only what failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The nest answered with a status that is not success and gave no reason of its own.
    Status(u16),
    /// The nest refused the request and said why. The text is the nest's, unedited.
    Refused(String),
    /// No answer within the client's timeout.
    Timeout,
    /// Nothing is listening, or the connection could not be made.
    Connect,
    /// The body was not what this endpoint serves.
    Unreadable,
    /// The body was larger than the cap for this endpoint, in bytes.
    TooLarge(usize),
    /// The request could not be built or sent for another reason.
    Failed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(code) => write!(f, "HTTP {code}"),
            Self::Refused(reason) => f.write_str(reason),
            Self::Timeout => f.write_str("timed out"),
            Self::Connect => f.write_str("cannot connect"),
            Self::Unreadable => f.write_str("unreadable response"),
            Self::TooLarge(limit) => write!(f, "response larger than {limit} bytes"),
            Self::Failed => f.write_str("request failed"),
        }
    }
}

impl std::error::Error for Error {}

impl From<&reqwest::Error> for Error {
    // The error's own text is not used: reqwest puts the full URL in it, query string included.
    fn from(error: &reqwest::Error) -> Self {
        if let Some(status) = error.status() {
            Self::Status(status.as_u16())
        } else if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connect
        } else if error.is_decode() || error.is_body() {
            Self::Unreadable
        } else {
            Self::Failed
        }
    }
}
