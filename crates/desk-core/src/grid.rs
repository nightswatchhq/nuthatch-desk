//! A result as rows and columns: how it reads, sorts, copies out and changes between polls.

use std::{borrow::Cow, cmp::Ordering};

use nest_client::sql::{Answer, Cell};

use crate::format::group_digits;

/// A result set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grid {
    /// Column names.
    pub columns: Vec<String>,
    /// Rows, each as long as `columns`.
    pub rows: Vec<Vec<Cell>>,
    /// The result was cut at a row cap.
    pub truncated: bool,
    /// A sealed segment could not be read, so rows are missing.
    pub degraded: bool,
    /// The hot store could not be read, so the newest rows are missing.
    pub tip_unavailable: bool,
}

impl From<Answer> for Grid {
    fn from(answer: Answer) -> Self {
        Self {
            columns: answer.columns,
            rows: answer.rows,
            truncated: answer.truncated,
            degraded: answer.degraded,
            tip_unavailable: answer.tip_unavailable,
        }
    }
}

/// How a cell reads. `NULL` is empty: [`Grid::is_null`] tells it from an empty string.
pub fn display(cell: &Cell) -> Cow<'_, str> {
    match cell {
        Cell::Null => Cow::Borrowed(""),
        Cell::Bool(true) => Cow::Borrowed("true"),
        Cell::Bool(false) => Cow::Borrowed("false"),
        Cell::Number(text) | Cell::Text(text) => Cow::Borrowed(text),
    }
}

/// A plain decimal, split so two of any length compare exactly.
struct Decimal<'a> {
    negative: bool,
    whole: &'a str,
    fraction: &'a str,
}

impl<'a> Decimal<'a> {
    /// `text` as a plain decimal: an optional sign, digits, an optional fraction. No exponent.
    fn parse(text: &'a str) -> Option<Self> {
        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = rest.split_once('.').unwrap_or((rest, ""));
        let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if whole.is_empty() || !digits(whole) || !digits(fraction) {
            return None;
        }
        if rest.contains('.') && fraction.is_empty() {
            return None;
        }
        let whole = whole.trim_start_matches('0');
        let fraction = fraction.trim_end_matches('0');
        Some(Self {
            // Minus zero is zero.
            negative: negative && !(whole.is_empty() && fraction.is_empty()),
            whole,
            fraction,
        })
    }

    fn magnitude_cmp(&self, other: &Self) -> Ordering {
        self.whole
            .len()
            .cmp(&other.whole.len())
            .then_with(|| self.whole.cmp(other.whole))
            .then_with(|| self.fraction.cmp(other.fraction))
    }

    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.magnitude_cmp(other),
            (true, true) => other.magnitude_cmp(self),
        }
    }
}

fn number_text(cell: &Cell) -> Option<&str> {
    match cell {
        Cell::Number(text) => Some(text),
        // Amounts are served as decimal text, because a JSON number would not hold them.
        Cell::Text(text) if Decimal::parse(text).is_some() => Some(text),
        _ => None,
    }
}

/// Orders two cells: `NULL` first, then booleans, then numbers by value, then text.
///
/// Numbers written as plain decimals compare exactly at any length. One written with an exponent
/// is placed as a double. The order is total either way, which a sort needs to be safe.
pub fn compare(a: &Cell, b: &Cell) -> Ordering {
    fn rank(cell: &Cell) -> u8 {
        match cell {
            Cell::Null => 0,
            Cell::Bool(_) => 1,
            _ if number_text(cell).is_some() => 2,
            _ => 3,
        }
    }
    rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
        (Cell::Bool(a), Cell::Bool(b)) => a.cmp(b),
        _ => match (number_text(a), number_text(b)) {
            (Some(a), Some(b)) => {
                // Rounding to a double never reorders two numbers, so it can go first, and the
                // exact comparison then only has to split the ones it could not tell apart.
                // Adding zero turns minus zero into zero, which `total_cmp` would tell apart.
                let double = |text: &str| text.parse::<f64>().unwrap_or(f64::NAN) + 0.0;
                double(a).total_cmp(&double(b)).then_with(|| {
                    match (Decimal::parse(a), Decimal::parse(b)) {
                        (Some(a), Some(b)) => a.cmp(&b),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => a.cmp(b),
                    }
                })
            }
            _ => display(a).cmp(&display(b)),
        },
    })
}

/// How to turn the grid a model is showing into the one that has just arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Patch {
    /// Nothing changed.
    Same,
    /// The new grid is the old one with `fresh` rows put on the front and `dropped` taken off the
    /// end, which is what a feed of newest rows does between polls.
    Slide {
        /// Rows to insert at the top, taken from the top of the new grid.
        fresh: usize,
        /// Rows to remove from the bottom of the old grid.
        dropped: usize,
    },
    /// The two are unrelated: replace everything.
    Reset,
}

/// Larger grids are replaced whole: the search below is quadratic and meant for a feed.
const SLIDE_LIMIT: usize = 1_000;

impl Grid {
    /// Whether the cell is SQL `NULL`. Out of range reads as not null.
    pub fn is_null(&self, row: usize, column: usize) -> bool {
        matches!(
            self.rows.get(row).and_then(|cells| cells.get(column)),
            Some(Cell::Null)
        )
    }

    /// How the cell reads, or `None` out of range.
    pub fn text(&self, row: usize, column: usize) -> Option<Cow<'_, str>> {
        self.rows.get(row)?.get(column).map(display)
    }

    /// Row indices in the order that sorts `column`. Rows that compare equal keep their order.
    pub fn sorted(&self, column: usize, descending: bool) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        let cell = |row: usize| self.rows[row].get(column).unwrap_or(&Cell::Null);
        order.sort_by(|&a, &b| {
            let ordering = compare(cell(a), cell(b));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        order
    }

    /// The grid as CSV, rows in `order`, with a header line.
    ///
    /// A field holding a comma, a quote or a line break is quoted. A text cell that a spreadsheet
    /// would run as a formula is prefixed with an apostrophe: the data comes from a nest, which is
    /// not trusted, and a pasted `=HYPERLINK(...)` is an old trick.
    pub fn csv(&self, order: &[usize]) -> String {
        let mut out = String::new();
        let mut line = |fields: &mut dyn Iterator<Item = Cow<'_, str>>| {
            for (index, field) in fields.enumerate() {
                if index > 0 {
                    out.push(',');
                }
                if field.contains([',', '"', '\n', '\r']) {
                    out.push('"');
                    out.push_str(&field.replace('"', "\"\""));
                    out.push('"');
                } else {
                    out.push_str(&field);
                }
            }
            out.push('\n');
        };
        line(&mut self.columns.iter().map(|name| guard_formula(name)));
        for row in order.iter().filter_map(|&row| self.rows.get(row)) {
            line(&mut row.iter().map(|cell| match cell {
                Cell::Text(text) => guard_formula(text),
                other => display(other),
            }));
        }
        out
    }

    /// What the reader should know about the result beyond its rows, or nothing.
    pub fn notice(&self) -> String {
        let mut notes = Vec::new();
        if self.truncated {
            notes.push(format!(
                "cut at {} rows",
                group_digits(self.rows.len() as u64)
            ));
        }
        if self.degraded {
            notes.push("a sealed segment could not be read: rows are missing".to_owned());
        }
        if self.tip_unavailable {
            notes.push("the hot store could not be read: sealed history only".to_owned());
        }
        notes.join(" · ")
    }

    /// How to turn `self` into `new` with the fewest model changes.
    pub fn patch(&self, new: &Grid) -> Patch {
        if self.columns != new.columns
            || self.rows.len() > SLIDE_LIMIT
            || new.rows.len() > SLIDE_LIMIT
        {
            return Patch::Reset;
        }
        for fresh in 0..new.rows.len() {
            let kept = new.rows.len() - fresh;
            if kept <= self.rows.len() && new.rows[fresh..] == self.rows[..kept] {
                let dropped = self.rows.len() - kept;
                return if fresh == 0 && dropped == 0 {
                    Patch::Same
                } else {
                    Patch::Slide { fresh, dropped }
                };
            }
        }
        if self.rows.is_empty() && new.rows.is_empty() {
            Patch::Same
        } else {
            Patch::Reset
        }
    }
}

fn guard_formula(text: &str) -> Cow<'_, str> {
    let risky = text.starts_with(['=', '+', '-', '@', '\t', '\r']) && Decimal::parse(text).is_none();
    if risky {
        Cow::Owned(format!("'{text}"))
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(digits: &str) -> Cell {
        Cell::Number(digits.into())
    }

    fn t(text: &str) -> Cell {
        Cell::Text(text.into())
    }

    fn grid(rows: &[&[Cell]]) -> Grid {
        Grid {
            columns: (0..rows.first().map_or(1, |row| row.len()))
                .map(|index| format!("c{index}"))
                .collect(),
            rows: rows.iter().map(|row| row.to_vec()).collect(),
            ..Grid::default()
        }
    }

    fn column(cells: &[Cell]) -> Grid {
        Grid {
            columns: vec!["c0".into()],
            rows: cells.iter().map(|cell| vec![cell.clone()]).collect(),
            ..Grid::default()
        }
    }

    fn sorted(cells: &[Cell], descending: bool) -> Vec<Cell> {
        let grid = column(cells);
        grid.sorted(0, descending)
            .into_iter()
            .map(|row| grid.rows[row][0].clone())
            .collect()
    }

    #[test]
    fn amounts_sort_by_value_not_by_text() {
        // As text, "9" sorts after "10"; as doubles, the last two are the same number.
        let cells = [
            t("9"),
            t("10"),
            t("115792089237316195423570985008687907853269984665640564039457584007913129639935"),
            t("115792089237316195423570985008687907853269984665640564039457584007913129639934"),
        ];
        assert_eq!(
            sorted(&cells, false),
            [cells[0].clone(), cells[1].clone(), cells[3].clone(), cells[2].clone()]
        );
    }

    #[test]
    fn negatives_fractions_and_zeros_sort_by_value() {
        let cells = [n("0.5"), n("-2"), n("-10"), n("0"), n("-0.0"), n("007"), n("0.25")];
        assert_eq!(
            sorted(&cells, false),
            [n("-10"), n("-2"), n("0"), n("-0.0"), n("0.25"), n("0.5"), n("007")]
        );
    }

    #[test]
    fn an_exponent_falls_back_to_a_double() {
        assert_eq!(sorted(&[n("1e3"), n("999"), n("2.5e2")], false), [n("2.5e2"), n("999"), n("1e3")]);
    }

    #[test]
    fn nulls_come_first_and_text_last() {
        let cells = [t("0xab"), n("3"), Cell::Null, Cell::Bool(true), t("")];
        assert_eq!(
            sorted(&cells, false),
            [Cell::Null, Cell::Bool(true), n("3"), t(""), t("0xab")]
        );
        assert_eq!(
            sorted(&cells, true),
            [t("0xab"), t(""), n("3"), Cell::Bool(true), Cell::Null]
        );
    }

    #[test]
    fn equal_rows_keep_their_order() {
        let grid = grid(&[&[n("1"), t("a")], &[n("0"), t("b")], &[n("1"), t("c")]]);
        assert_eq!(grid.sorted(0, false), [1, 0, 2]);
        assert_eq!(grid.sorted(0, true), [0, 2, 1]);
    }

    #[test]
    fn sorting_a_column_the_grid_lacks_changes_nothing() {
        let grid = grid(&[&[n("2")], &[n("1")]]);
        assert_eq!(grid.sorted(7, false), [0, 1]);
    }

    #[test]
    fn csv_quotes_what_needs_quoting() {
        let grid = Grid {
            columns: vec!["name".into(), "note, with comma".into()],
            rows: vec![
                vec![t("plain"), t("say \"hi\"")],
                vec![Cell::Null, t("two\nlines")],
                vec![Cell::Bool(false), n("12")],
            ],
            ..Grid::default()
        };
        assert_eq!(
            grid.csv(&[0, 1, 2]),
            "name,\"note, with comma\"\nplain,\"say \"\"hi\"\"\"\n,\"two\nlines\"\nfalse,12\n"
        );
    }

    #[test]
    fn csv_follows_the_sort_and_skips_rows_that_are_not_there() {
        let grid = grid(&[&[n("1")], &[n("2")]]);
        assert_eq!(grid.csv(&[1, 0, 9]), "c0\n2\n1\n");
    }

    #[test]
    fn csv_defuses_formulas_but_leaves_negative_numbers_alone() {
        let grid = column(&[
            t("=HYPERLINK(\"http://evil\",\"x\")"),
            t("+cmd"),
            t("@SUM(A1)"),
            t("-12.5"),
            n("-3"),
            t("-x"),
        ]);
        assert_eq!(
            grid.csv(&[0, 1, 2, 3, 4, 5]),
            "c0\n\"'=HYPERLINK(\"\"http://evil\"\",\"\"x\"\")\"\n'+cmd\n'@SUM(A1)\n-12.5\n-3\n'-x\n"
        );
    }

    #[test]
    fn null_is_told_from_an_empty_string() {
        let grid = grid(&[&[Cell::Null, t("")]]);
        assert!(grid.is_null(0, 0));
        assert!(!grid.is_null(0, 1));
        assert!(!grid.is_null(5, 5));
        assert_eq!(grid.text(0, 0).as_deref(), Some(""));
        assert_eq!(grid.text(0, 2), None);
    }

    #[test]
    fn the_notice_says_what_is_missing() {
        assert_eq!(Grid::default().notice(), "");
        let cut = Grid {
            truncated: true,
            tip_unavailable: true,
            ..column(&[n("1"), n("2")])
        };
        assert_eq!(
            cut.notice(),
            "cut at 2 rows · the hot store could not be read: sealed history only"
        );
    }

    fn feed(blocks: &[u32]) -> Grid {
        column(&blocks.iter().map(|block| n(&block.to_string())).collect::<Vec<_>>())
    }

    /// Applies a patch the way a model does, and checks the result is the new grid.
    fn apply(old: &Grid, new: &Grid) -> Patch {
        let patch = old.patch(new);
        let mut rows = old.rows.clone();
        match patch {
            Patch::Same => {}
            Patch::Reset => rows = new.rows.clone(),
            Patch::Slide { fresh, dropped } => {
                rows.truncate(rows.len() - dropped);
                rows.splice(0..0, new.rows[..fresh].iter().cloned());
            }
        }
        assert_eq!(rows, new.rows, "{patch:?}");
        patch
    }

    #[test]
    fn a_feed_slides_rather_than_resets() {
        // A full window: two new rows arrive, two fall off the end.
        assert_eq!(
            apply(&feed(&[5, 4, 3, 2]), &feed(&[7, 6, 5, 4])),
            Patch::Slide { fresh: 2, dropped: 2 }
        );
        // A window still filling: new rows arrive, none fall off.
        assert_eq!(
            apply(&feed(&[2, 1]), &feed(&[4, 3, 2, 1])),
            Patch::Slide { fresh: 2, dropped: 0 }
        );
        // A reorg took the newest row away.
        assert_eq!(
            apply(&feed(&[5, 4, 3]), &feed(&[4, 3])),
            Patch::Reset
        );
    }

    #[test]
    fn an_unchanged_feed_is_left_alone() {
        assert_eq!(apply(&feed(&[3, 2, 1]), &feed(&[3, 2, 1])), Patch::Same);
        assert_eq!(apply(&feed(&[]), &feed(&[])), Patch::Same);
    }

    #[test]
    fn unrelated_grids_reset() {
        assert_eq!(apply(&feed(&[3, 2, 1]), &feed(&[9, 8, 7])), Patch::Reset);
        assert_eq!(apply(&feed(&[]), &feed(&[1])), Patch::Reset);
        assert_eq!(apply(&feed(&[1]), &feed(&[])), Patch::Reset);
        let renamed = Grid {
            columns: vec!["other".into()],
            ..feed(&[3, 2, 1])
        };
        assert_eq!(apply(&feed(&[3, 2, 1]), &renamed), Patch::Reset);
    }

    #[test]
    fn repeated_rows_still_patch_to_the_right_grid() {
        apply(&feed(&[1, 1, 1]), &feed(&[1, 1, 1, 1]));
        apply(&feed(&[2, 1, 2, 1]), &feed(&[1, 2, 1, 2]));
        apply(&feed(&[1, 1, 2]), &feed(&[1, 2]));
    }
}
