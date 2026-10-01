//! The nest's Prometheus text, reduced to one number per family.

use std::collections::BTreeMap;

/// One number per metric family.
///
/// Labelled series of one family are summed, as RPC methods are across endpoints. A family that
/// also publishes an unlabelled series is giving its total there and only breaking it down in the
/// labelled ones, so adding the two would count everything twice.
pub fn parse_prometheus(text: &str) -> BTreeMap<String, f64> {
    let mut totals = BTreeMap::new();
    let mut summed: BTreeMap<String, f64> = BTreeMap::new();
    for line in text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        // A label value may hold a space, so the series ends at the closing brace when there is one.
        let split = match line.rfind('}') {
            Some(brace) => line.split_at_checked(brace + 1),
            None => line.split_once(' '),
        };
        let Some((series, rest)) = split else {
            continue;
        };
        // An optional timestamp may follow the value.
        let Some(Ok(value)) = rest.split_whitespace().next().map(str::parse::<f64>) else {
            continue;
        };
        match series.split_once('{') {
            Some((name, _)) => *summed.entry(name.to_owned()).or_default() += value,
            None => {
                totals.insert(series.to_owned(), value);
            }
        }
    }
    summed.extend(totals);
    summed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labelled_series_are_summed() {
        let metrics = parse_prometheus("a{m=\"x\"} 2\na{m=\"y\"} 3\n");
        assert_eq!(metrics["a"], 5.0);
    }

    #[test]
    fn an_unlabelled_total_is_not_added_to_its_breakdown() {
        let metrics = parse_prometheus("a 5\na{m=\"x\"} 2\na{m=\"y\"} 3\n");
        assert_eq!(metrics["a"], 5.0);
    }

    #[test]
    fn a_label_value_with_a_space_keeps_its_number() {
        let metrics = parse_prometheus("a{m=\"eth getLogs\"} 7\n");
        assert_eq!(metrics["a"], 7.0);
    }

    #[test]
    fn a_trailing_timestamp_is_not_the_value() {
        let metrics = parse_prometheus("a 7 1790846568000\nb{m=\"x\"} 2 1790846568000\n");
        assert_eq!(metrics["a"], 7.0);
        assert_eq!(metrics["b"], 2.0);
    }

    #[test]
    fn comments_and_noise_are_skipped() {
        let metrics = parse_prometheus("# HELP a\n# TYPE a counter\n\nnonsense\na NaNope\nb 1\n");
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics["b"], 1.0);
    }
}
