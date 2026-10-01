use std::{io::Read, time::Duration};

use serde::de::DeserializeOwned;

use crate::Error;

/// Caps on what the client will read from a nest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest body accepted from any endpoint but `/sql`, in bytes.
    pub body_bytes: usize,
    /// Largest body accepted from `/sql`, in bytes.
    pub sql_body_bytes: usize,
    /// Most rows asked of, and kept from, one `/sql` answer.
    pub sql_rows: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            body_bytes: 8 << 20,
            sql_body_bytes: 64 << 20,
            sql_rows: 10_000,
        }
    }
}

/// A blocking HTTP client for one or more nests.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::blocking::Client,
    limits: Limits,
}

impl Client {
    /// A client that gives up on a request after `timeout`.
    pub fn new(timeout: Duration, limits: Limits) -> Result<Self, Error> {
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .connect_timeout(timeout.min(Duration::from_secs(5)))
            .user_agent(concat!("nuthatch-desk/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::from(&error))?;
        Ok(Self { http, limits })
    }

    /// The caps this client reads under.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The status and body of a GET, whatever the status. The body is read up to `cap` bytes.
    pub(crate) fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
        cap: usize,
    ) -> Result<(u16, String), Error> {
        let response = self
            .http
            .get(url)
            .query(query)
            .send()
            .map_err(|error| Error::from(&error))?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        // One byte past the cap is enough to know the body is over it.
        response
            .take(cap as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|_| Error::Unreadable)?;
        if body.len() > cap {
            return Err(Error::TooLarge(cap));
        }
        let body = String::from_utf8(body).map_err(|_| Error::Unreadable)?;
        Ok((status, body))
    }

    pub(crate) fn get_ok(&self, url: &str, query: &[(&str, &str)]) -> Result<String, Error> {
        let (status, body) = self.get(url, query, self.limits.body_bytes)?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(Error::Status(status))
        }
    }

    pub(crate) fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<T, Error> {
        serde_json::from_str(&self.get_ok(url, query)?).map_err(|_| Error::Unreadable)
    }
}
