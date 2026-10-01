//! One method per endpoint the client reads.

use std::collections::BTreeMap;

use crate::{
    Client, Error,
    metrics::parse_prometheus,
    types::{
        Identity, NestDocument, QueriesDocument, Ready, RootDocument, Roster, SqlAccess, Tables,
    },
};

/// An endpoint and what went wrong with it.
pub type Problem = (&'static str, Error);

impl Client {
    /// `GET /ready`.
    ///
    /// A stalled or quarantined nest answers 503 with the full document, and that document is
    /// exactly what the operator needs to see, so a 503 is read rather than reported.
    pub fn ready(&self, base: &str) -> Result<Ready, Error> {
        let (status, body) = self.get(&format!("{base}/ready"), &[], self.limits().body_bytes)?;
        if !(200..300).contains(&status) && status != 503 {
            return Err(Error::Status(status));
        }
        parse_ready(&body)
    }

    /// `GET /metrics`, one number per family.
    pub fn metrics(&self, base: &str) -> Result<BTreeMap<String, f64>, Error> {
        self.get_ok(&format!("{base}/metrics"), &[])
            .map(|text| parse_prometheus(&text))
    }

    /// Rows in the nest's hot store, from the root document. `None` where it does not say.
    pub fn hot_rows(&self, base: &str) -> Result<Option<u64>, Error> {
        self.get_json::<RootDocument>(&format!("{base}/"), &[])
            .map(|root| root.entities)
    }

    /// `GET /nests`, which only a runtime's root serves.
    pub fn roster(&self, base: &str) -> Result<Roster, Error> {
        self.get_json(&format!("{base}/nests"), &[])
    }

    /// The catalogue, the nest's name and chain, and what its SQL surface accepts.
    pub fn identity(&self, base: &str) -> Result<Identity, Problem> {
        let tables: Tables = self
            .get_json(&format!("{base}/tables"), &[])
            .map_err(|error| ("/tables", error))?;
        let (nest_name, chain) = match self.get_json::<NestDocument>(&format!("{base}/nest"), &[]) {
            Ok(nest) => (nest.name.filter(|name| !name.is_empty()), nest.chain),
            // A nest too old to serve `/nest` names itself only in the prose of `/schema`.
            Err(_) => (
                self.get_ok(&format!("{base}/schema"), &[])
                    .ok()
                    .and_then(|schema| nest_name_from_schema(&schema)),
                None,
            ),
        };
        // Absent on a nest too old to serve it, which is also a nest too old to close SQL.
        let sql = self
            .get_json::<QueriesDocument>(&format!("{base}/queries"), &[])
            .map_or(SqlAccess::Open, sql_access);
        Ok(Identity {
            nest_name,
            chain,
            tables,
            sql,
        })
    }
}

pub(crate) fn parse_ready(body: &str) -> Result<Ready, Error> {
    serde_json::from_str(body).map_err(|_| Error::Unreadable)
}

pub(crate) fn sql_access(queries: QueriesDocument) -> SqlAccess {
    if queries.free_form && matches!(queries.sql.as_str(), "open" | "") {
        SqlAccess::Open
    } else {
        SqlAccess::Closed {
            mode: queries.sql,
            named: queries
                .queries
                .into_iter()
                .map(|query| query.name)
                .collect(),
        }
    }
}

fn nest_name_from_schema(schema: &str) -> Option<String> {
    let line = schema.lines().find(|line| line.starts_with("The `"))?;
    let rest = line.strip_prefix("The `")?;
    let (name, _) = rest.split_once("` nest on ")?;
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nest_name_is_read_from_the_schema_prose() {
        assert_eq!(
            nest_name_from_schema("# Schema\n\nThe `usdc` nest on mainnet.\n").as_deref(),
            Some("usdc")
        );
        assert_eq!(nest_name_from_schema("The `` nest on mainnet."), None);
        assert_eq!(nest_name_from_schema("nothing of the sort"), None);
    }
}
