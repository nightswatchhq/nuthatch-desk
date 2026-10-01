//! The nest's `/sql` surface: free-form statements, and the preview of one table.

use serde::Deserialize;
use serde_json::Value;

use crate::{Client, Error, types::EventTable};

/// Whether `name` is safe to put in a statement this client writes.
///
/// Table and column names come from the nest, which is not trusted. A name that is not a plain
/// identifier is left out of generated SQL rather than escaped and hoped for.
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 128
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// `name` as a quoted SQL identifier.
pub fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// One value of a result, as text that loses nothing.
///
/// Token amounts are 256-bit and a JSON number in most readers is a double, so a number keeps the
/// digits the nest sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// SQL `NULL`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number, as the digits the nest sent.
    Number(String),
    /// A string. A nested array or object arrives here as compact JSON.
    Text(String),
}

impl Cell {
    fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(flag) => Self::Bool(flag),
            Value::Number(number) => Self::Number(number.to_string()),
            Value::String(text) => Self::Text(text),
            nested => Self::Text(nested.to_string()),
        }
    }

    /// The cell as unsigned integer, when it is one.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(digits) => digits.parse().ok(),
            _ => None,
        }
    }
}

/// The answer to one statement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    /// Column names, in the order of the first row as the nest sent it.
    pub columns: Vec<String>,
    /// The rows, each as long as `columns`.
    pub rows: Vec<Vec<Cell>>,
    /// The nest or this client cut the result at a row cap.
    pub truncated: bool,
    /// A sealed segment could not be read, so the answer holds less data than it should.
    pub degraded: bool,
    /// The hot store could not be read, so the answer is sealed history only.
    pub tip_unavailable: bool,
}

#[derive(Debug, Deserialize, Default)]
struct SqlResponse {
    #[serde(default)]
    rows: Vec<Value>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    degraded: bool,
    #[serde(default)]
    tip_unavailable: bool,
}

#[derive(Debug, Deserialize)]
struct Refusal {
    error: String,
}

/// Reads a `/sql` body, keeping at most `row_cap` rows.
pub(crate) fn parse_answer(body: &str, row_cap: usize) -> Result<Answer, Error> {
    let mut response: SqlResponse = serde_json::from_str(body).map_err(|_| Error::Unreadable)?;
    // An older nest ignores `max_rows`, so the cap is applied here as well.
    let truncated = response.truncated || response.rows.len() > row_cap;
    response.rows.truncate(row_cap);
    let columns: Vec<String> = match response.rows.first() {
        Some(Value::Object(first)) => first.keys().cloned().collect(),
        Some(_) => vec!["value".to_owned()],
        None => Vec::new(),
    };
    let rows = response
        .rows
        .into_iter()
        .map(|row| match row {
            Value::Object(mut fields) => columns
                .iter()
                .map(|column| fields.remove(column).map_or(Cell::Null, Cell::from_value))
                .collect(),
            other => vec![Cell::from_value(other)],
        })
        .collect();
    Ok(Answer {
        columns,
        rows,
        truncated,
        degraded: response.degraded,
        tip_unavailable: response.tip_unavailable,
    })
}

/// What a non-success `/sql` answer means: the nest's own reason where it gave one.
pub(crate) fn refusal(status: u16, body: &str) -> Error {
    match serde_json::from_str::<Refusal>(body) {
        Ok(refusal) if !refusal.error.is_empty() => Error::Refused(refusal.error),
        _ => Error::Status(status),
    }
}

impl Answer {
    /// The same answer with its columns in `order`. Columns the answer lacks are left out.
    fn reordered(self, order: &[String]) -> Self {
        let picks: Vec<usize> = order
            .iter()
            .filter_map(|name| self.columns.iter().position(|column| column == name))
            .collect();
        Self {
            columns: picks.iter().map(|&at| self.columns[at].clone()).collect(),
            rows: self
                .rows
                .into_iter()
                .map(|row| picks.iter().map(|&at| row[at].clone()).collect())
                .collect(),
            ..self
        }
    }
}

/// The preview of one table: its newest rows, and how many it holds.
///
/// The catalogue lists only decoded columns, so selecting them by name also leaves out the `_dec`
/// and `_overflow` companions Nuthatch adds to every big integer for arithmetic; the plain column
/// already holds the exact decimal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    table: String,
    columns: Vec<String>,
    has_log_index: bool,
    limit: usize,
}

impl Preview {
    /// The preview of `table`, or `None` when its name is not a plain identifier.
    pub fn new(table: &EventTable, limit: usize) -> Option<Self> {
        if !is_identifier(&table.table) {
            return None;
        }
        let mut columns: Vec<String> = table
            .columns
            .iter()
            .filter(|column| column.sol_type != "implicit" && is_identifier(&column.name))
            .map(|column| column.name.clone())
            .collect();
        if table.is_call() {
            // What the call returned is the point; the calldata is the same on every row.
            columns.sort_by_key(|name| name != "result");
        }
        Some(Self {
            table: table.table.clone(),
            columns,
            has_log_index: table.columns.is_empty()
                || table
                    .columns
                    .iter()
                    .any(|column| column.name == "log_index"),
            limit,
        })
    }

    /// The table this previews.
    pub fn table(&self) -> &str {
        &self.table
    }

    fn selected(&self) -> Vec<String> {
        std::iter::once("block_number".to_owned())
            .chain(self.columns.iter().cloned())
            .collect()
    }

    fn counts_sql(&self) -> String {
        format!(
            "SELECT count(*) AS rows, max(block_number) AS latest_block FROM {}",
            quote_identifier(&self.table)
        )
    }

    fn rows_sql(&self) -> String {
        let columns = if self.columns.is_empty() {
            "*".to_owned()
        } else {
            self.selected()
                .iter()
                .map(|name| quote_identifier(name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let order = if self.has_log_index {
            "block_number DESC, log_index DESC"
        } else {
            "block_number DESC"
        };
        format!(
            "SELECT {columns} FROM {} ORDER BY {order} LIMIT {}",
            quote_identifier(&self.table),
            self.limit
        )
    }
}

/// What a [`Preview`] returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The table previewed.
    pub table: String,
    /// Rows in the table.
    pub rows: Option<u64>,
    /// Highest block with a row in the table.
    pub latest_block: Option<u64>,
    /// The newest rows, newest first.
    pub newest: Answer,
}

impl Client {
    /// Runs one statement on the nest's `/sql`.
    ///
    /// The nest is asked for at most [`Limits::sql_rows`](crate::Limits::sql_rows) rows. A refused
    /// statement comes back as [`Error::Refused`] in the nest's own words.
    pub fn sql(&self, base: &str, statement: &str) -> Result<Answer, Error> {
        let limits = self.limits();
        let max_rows = limits.sql_rows.to_string();
        let (status, body) = self.get(
            &format!("{base}/sql"),
            &[("q", statement), ("max_rows", &max_rows)],
            limits.sql_body_bytes,
        )?;
        if (200..300).contains(&status) {
            parse_answer(&body, limits.sql_rows)
        } else {
            Err(refusal(status, &body))
        }
    }

    /// Runs a [`Preview`].
    pub fn preview(&self, base: &str, preview: &Preview) -> Result<Selection, Error> {
        let counts = self.sql(base, &preview.counts_sql())?;
        let newest = self.sql(base, &preview.rows_sql())?;
        let count = |name: &str| {
            let at = counts.columns.iter().position(|column| column == name)?;
            counts.rows.first()?.get(at)?.as_u64()
        };
        let newest = if preview.columns.is_empty() {
            newest
        } else {
            // The nest sends a row's fields in its own order, not the statement's.
            newest.reordered(&preview.selected())
        };
        Ok(Selection {
            table: preview.table.clone(),
            rows: count("rows"),
            latest_block: count("latest_block"),
            newest: Answer {
                degraded: newest.degraded || counts.degraded,
                tip_unavailable: newest.tip_unavailable || counts.tip_unavailable,
                ..newest
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Column;

    fn column(name: &str, sol_type: &str) -> Column {
        Column {
            name: name.into(),
            sol_type: sol_type.into(),
        }
    }

    fn transfer() -> EventTable {
        EventTable {
            table: "usdc__transfer".into(),
            alias: "usdc".into(),
            kind: String::new(),
            columns: vec![
                column("block_number", "implicit"),
                column("log_index", "implicit"),
                column("from", "address"),
                column("to", "address"),
                column("value", "uint256"),
            ],
        }
    }

    #[test]
    fn identifiers_are_plain_names_only() {
        for good in ["usdc__transfer", "_seq", "a$b", "T1"] {
            assert!(is_identifier(good), "{good}");
        }
        for bad in ["", "1abc", "a b", "a\"b", "a;drop", "a-b", "naïve", &"a".repeat(129)] {
            assert!(!is_identifier(bad), "{bad}");
        }
    }

    #[test]
    fn the_preview_selects_decoded_columns_by_quoted_name() {
        let preview = Preview::new(&transfer(), 50).unwrap();
        assert_eq!(
            preview.rows_sql(),
            "SELECT \"block_number\", \"from\", \"to\", \"value\" FROM \"usdc__transfer\" \
             ORDER BY block_number DESC, log_index DESC LIMIT 50"
        );
    }

    #[test]
    fn a_table_with_a_hostile_name_gets_no_preview() {
        let mut table = transfer();
        table.table = "x\" UNION SELECT * FROM secrets --".into();
        assert_eq!(Preview::new(&table, 50), None);
    }

    #[test]
    fn a_column_with_a_hostile_name_is_left_out() {
        let mut table = transfer();
        table.columns.push(column("v\", (SELECT 1) AS \"x", "uint256"));
        let preview = Preview::new(&table, 50).unwrap();
        assert_eq!(preview.columns, ["from", "to", "value"]);
    }

    #[test]
    fn a_call_table_puts_the_result_first_and_orders_by_block_alone() {
        let table = EventTable {
            table: "supply".into(),
            alias: "supply".into(),
            kind: "call".into(),
            columns: vec![
                column("block_number", "implicit"),
                column("calldata", "bytes"),
                column("result", "uint256"),
            ],
        };
        let preview = Preview::new(&table, 6).unwrap();
        assert_eq!(
            preview.rows_sql(),
            "SELECT \"block_number\", \"result\", \"calldata\" FROM \"supply\" \
             ORDER BY block_number DESC LIMIT 6"
        );
    }

    #[test]
    fn a_number_keeps_every_digit() {
        let big = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        let answer = parse_answer(&format!("{{\"rows\":[{{\"v\":{big}}}]}}"), 10).unwrap();
        assert_eq!(answer.rows[0][0], Cell::Number(big.into()));
    }

    #[test]
    fn rows_past_the_cap_are_cut_and_said_to_be() {
        let answer = parse_answer("{\"rows\":[{\"a\":1},{\"a\":2},{\"a\":3}]}", 2).unwrap();
        assert_eq!(answer.rows.len(), 2);
        assert!(answer.truncated);
    }

    #[test]
    fn a_row_missing_a_column_reads_as_null_there() {
        let answer = parse_answer("{\"rows\":[{\"a\":1,\"b\":2},{\"a\":3}]}", 10).unwrap();
        assert_eq!(answer.rows[1], [Cell::Number("3".into()), Cell::Null]);
    }

    #[test]
    fn nested_values_arrive_as_json_text() {
        let answer = parse_answer("{\"rows\":[{\"a\":[1,\"x\"],\"b\":true}]}", 10).unwrap();
        assert_eq!(
            answer.rows[0],
            [Cell::Text("[1,\"x\"]".into()), Cell::Bool(true)]
        );
    }

    #[test]
    fn a_refusal_without_a_reason_is_its_status() {
        assert_eq!(refusal(502, "<html>bad gateway</html>"), Error::Status(502));
        assert_eq!(refusal(400, "{\"error\":\"\"}"), Error::Status(400));
    }
}
