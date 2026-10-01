//! The table list, as rows.

use nest_client::types::Identity;

/// One row of the table list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The name to preview and to use in SQL.
    pub table: String,
    /// The heading the table is listed under.
    pub group: String,
    /// The name under that heading.
    pub name: String,
    /// Whether the table holds `eth_call` results.
    pub call: bool,
}

/// The catalogue as rows: event tables by alias in the nest's order, state calls last.
pub fn entries(identity: &Identity) -> Vec<Entry> {
    let mut entries: Vec<Entry> = identity
        .tables
        .tables
        .iter()
        .map(|table| Entry {
            table: table.table.clone(),
            group: table.group().to_owned(),
            name: table.short_name().to_owned(),
            call: table.is_call(),
        })
        .collect();
    // Stable, so tables keep the nest's order within a heading.
    entries.sort_by(|a, b| (a.call, &a.group).cmp(&(b.call, &b.group)));
    entries
}

#[cfg(test)]
mod tests {
    use nest_client::types::{EventTable, SqlAccess, Tables};

    use super::*;

    fn table(name: &str, alias: &str, kind: &str) -> EventTable {
        EventTable {
            table: name.into(),
            alias: alias.into(),
            kind: kind.into(),
            columns: Vec::new(),
        }
    }

    #[test]
    fn tables_are_listed_by_alias_with_calls_last() {
        let identity = Identity {
            nest_name: None,
            chain: None,
            sql: SqlAccess::Open,
            tables: Tables {
                tables: vec![
                    table("usdc__transfer", "usdc", ""),
                    table("total_supply", "total_supply", "call"),
                    table("dai__transfer", "dai", ""),
                    table("usdc__approval", "usdc", ""),
                ],
            },
        };
        let listed: Vec<(String, String)> = entries(&identity)
            .into_iter()
            .map(|entry| (entry.group, entry.name))
            .collect();
        assert_eq!(
            listed,
            [
                ("dai".into(), "transfer".into()),
                ("usdc".into(), "transfer".into()),
                ("usdc".into(), "approval".into()),
                ("calls".into(), "total_supply".into()),
            ]
        );
    }
}
