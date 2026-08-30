//! Reading and parsing tabular input: stdin or a file, delimiter sniffing,
//! optional header row, and the column → series mapping.

use std::fs;
use std::io::Read;
use std::path::Path;

/// Parsed input: a shared x column and one or more y series.
#[derive(Debug)]
pub struct Table {
    /// One entry per series (from the header row, else `None`).
    pub names: Vec<Option<String>>,
    pub x: Vec<f32>,
    pub series: Vec<Vec<f32>>,
    /// Set when the x column held ISO-8601 dates: `x` values are seconds
    /// since this UTC epoch base (the first timestamp's midnight), and the
    /// plot gets a calendar x axis.
    pub x_epoch: Option<f64>,
}

/// A strict ISO-8601 timestamp — `YYYY-MM-DD`, optionally `THH:MM[:SS]`
/// (`T` or a space, optional trailing `Z`) — as epoch seconds UTC. Years
/// need four digits, so numeric-ish strings like "1-2-3" never pass.
fn parse_iso(s: &str) -> Option<f64> {
    let s = s.strip_suffix('Z').unwrap_or(s);
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut it = date.split('-');
    let (ys, ms, ds) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || ys.len() != 4 {
        return None;
    }
    let (y, m, d) = (ys.parse::<i32>().ok()?, ms.parse::<u32>().ok()?, ds.parse::<u32>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = 0i64;
    if let Some(t) = time {
        let mut parts = t.split(':');
        let hh: i64 = parts.next()?.parse().ok()?;
        let mm: i64 = parts.next()?.parse().ok()?;
        let ss: i64 = match parts.next() {
            Some(p) => p.parse().ok()?,
            None => 0,
        };
        if parts.next().is_some() || hh > 23 || mm > 59 || ss > 59 {
            return None;
        }
        secs = hh * 3600 + mm * 60 + ss;
    }
    Some(plotui_core::days_from_civil(y, m, d) as f64 * 86_400.0 + secs as f64)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Delim {
    Char(char),
    Whitespace,
}

pub fn load(file: Option<&Path>, delimiter: Option<&str>, header: bool) -> Result<Table, String> {
    let text = read_input(file).map_err(|e| match file {
        Some(p) => format!("{}: {e}", p.display()),
        None => format!("stdin: {e}"),
    })?;
    parse(&text, delimiter, header)
}

fn read_input(file: Option<&Path>) -> std::io::Result<String> {
    match file {
        Some(p) if p.as_os_str() != "-" => fs::read_to_string(p),
        _ => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
    }
}

fn parse_delimiter(spec: &str) -> Result<Delim, String> {
    match spec {
        "tab" | "\t" | "\\t" => Ok(Delim::Char('\t')),
        "space" | " " => Ok(Delim::Whitespace),
        s => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(Delim::Char(c)),
                _ => Err(format!(
                    "unrecognized delimiter {s:?} (use a single character, \"tab\", or \"space\")"
                )),
            }
        }
    }
}

fn sniff(line: &str) -> Delim {
    if line.contains('\t') {
        Delim::Char('\t')
    } else if line.contains(',') {
        Delim::Char(',')
    } else {
        Delim::Whitespace
    }
}

fn fields(line: &str, delim: Delim) -> Vec<&str> {
    match delim {
        Delim::Whitespace => line.split_whitespace().collect(),
        Delim::Char(c) => line.split(c).map(str::trim).collect(),
    }
}

fn parse(text: &str, delimiter: Option<&str>, header: bool) -> Result<Table, String> {
    // 1-based line numbers for error messages; blank lines are skipped but
    // still counted.
    let mut rows: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(_, l)| !l.trim().is_empty())
        .collect();
    if rows.is_empty() {
        return Err("no data on input".into());
    }

    let delim = match delimiter {
        Some(spec) => parse_delimiter(spec)?,
        None => sniff(rows[0].1),
    };

    let mut header_names: Vec<String> = Vec::new();
    if header {
        let (_, line) = rows.remove(0);
        header_names = fields(line, delim).into_iter().map(str::to_owned).collect();
        if rows.is_empty() {
            return Err("no data on input (only a header row)".into());
        }
    }

    let ncols = fields(rows[0].1, delim).len();
    let n_series = if ncols <= 1 { 1 } else { ncols - 1 };
    // A first x cell that is no number but a strict ISO date puts the whole
    // x column on a time axis (values become offsets from an epoch base).
    let first_x = fields(rows[0].1, delim)[0];
    let time_x = ncols > 1 && first_x.parse::<f32>().is_err() && parse_iso(first_x).is_some();
    let mut x_epoch: Option<f64> = None;
    let mut x: Vec<f32> = Vec::with_capacity(rows.len());
    let mut series: Vec<Vec<f32>> = vec![Vec::with_capacity(rows.len()); n_series];

    for (row_idx, (lineno, line)) in rows.iter().enumerate() {
        let f = fields(line, delim);
        if f.len() != ncols {
            return Err(format!("line {lineno}: expected {ncols} fields, found {}", f.len()));
        }
        if time_x {
            let ts = parse_iso(f[0]).ok_or_else(|| {
                format!(
                    "line {lineno}: {:?} is not an ISO-8601 date (mixed date and numeric x?)",
                    f[0]
                )
            })?;
            let base = *x_epoch.get_or_insert_with(|| (ts / 86_400.0).floor() * 86_400.0);
            x.push((ts - base) as f32);
        }
        let mut nums = Vec::with_capacity(ncols);
        for field in &f[if time_x { 1 } else { 0 }..] {
            match field.parse::<f32>() {
                Ok(v) => nums.push(v),
                Err(_) => {
                    let mut msg = format!("line {lineno}: {field:?} is not a number");
                    if row_idx == 0 && !header {
                        msg.push_str("; use -H if the first row is a header");
                    }
                    return Err(msg);
                }
            }
        }
        if ncols == 1 {
            x.push(x.len() as f32);
            series[0].push(nums[0]);
        } else {
            // With a time x, `nums` holds only the series values; otherwise
            // its first entry is x.
            let ys = if time_x {
                &nums[..]
            } else {
                x.push(nums[0]);
                &nums[1..]
            };
            for (s, v) in series.iter_mut().zip(ys) {
                s.push(*v);
            }
        }
    }

    // Header → series names: with a single column the one name labels the one
    // series; otherwise the first name labels x (unused in v1) and the rest
    // label the series.
    let names: Vec<Option<String>> = if header_names.is_empty() {
        vec![None; n_series]
    } else if ncols == 1 {
        vec![header_names.first().cloned()]
    } else {
        (1..ncols).map(|i| header_names.get(i).cloned()).collect()
    };

    Ok(Table { names, x, series, x_epoch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_tab_then_comma_then_whitespace() {
        assert_eq!(sniff("1\t2"), Delim::Char('\t'));
        assert_eq!(sniff("1,2"), Delim::Char(','));
        assert_eq!(sniff("1 2"), Delim::Whitespace);
    }

    #[test]
    fn single_column_auto_indexes_x() {
        let t = parse("5\n7\n9\n", None, false).unwrap();
        assert_eq!(t.x, vec![0.0, 1.0, 2.0]);
        assert_eq!(t.series, vec![vec![5.0, 7.0, 9.0]]);
        assert_eq!(t.names, vec![None]);
    }

    #[test]
    fn two_columns_are_x_and_y() {
        let t = parse("1,10\n2,20\n", None, false).unwrap();
        assert_eq!(t.x, vec![1.0, 2.0]);
        assert_eq!(t.series, vec![vec![10.0, 20.0]]);
    }

    #[test]
    fn multi_series_shares_first_column_as_x() {
        let t = parse("t,a,b\n1,10,100\n2,20,200\n", None, true).unwrap();
        assert_eq!(t.x, vec![1.0, 2.0]);
        assert_eq!(t.series, vec![vec![10.0, 20.0], vec![100.0, 200.0]]);
        assert_eq!(t.names, vec![Some("a".into()), Some("b".into())]);
    }

    #[test]
    fn header_on_single_column_names_the_series() {
        let t = parse("loss\n0.5\n0.4\n", None, true).unwrap();
        assert_eq!(t.names, vec![Some("loss".into())]);
        assert_eq!(t.series, vec![vec![0.5, 0.4]]);
    }

    #[test]
    fn whitespace_runs_collapse() {
        let t = parse("1   10\n2\t 20\n", Some("space"), false).unwrap();
        assert_eq!(t.x, vec![1.0, 2.0]);
    }

    #[test]
    fn non_numeric_first_row_hints_at_header_flag() {
        let err = parse("t,a\n1,2\n", None, false).unwrap_err();
        assert!(err.contains("line 1"), "{err}");
        assert!(err.contains("use -H"), "{err}");
    }

    #[test]
    fn non_numeric_later_row_reports_line_number() {
        let err = parse("1,2\n2,x\n", None, false).unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(!err.contains("use -H"), "{err}");
    }

    #[test]
    fn ragged_row_reports_field_count() {
        let err = parse("1,2\n3,4,5\n", None, false).unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("expected 2 fields, found 3"), "{err}");
    }

    #[test]
    fn blank_lines_are_skipped_but_counted() {
        let t = parse("\n1,10\n\n2,20\n", None, false).unwrap();
        assert_eq!(t.x, vec![1.0, 2.0]);
        let err = parse("\n1,10\n\nx,20\n", None, false).unwrap_err();
        assert!(err.contains("line 4"), "{err}");
    }

    #[test]
    fn empty_input_errors() {
        assert!(parse("", None, false).is_err());
        assert!(parse("a,b\n", None, true).is_err());
    }

    #[test]
    fn explicit_delimiter_overrides_sniffing() {
        let t = parse("1;10\n2;20\n", Some(";"), false).unwrap();
        assert_eq!(t.series, vec![vec![10.0, 20.0]]);
        assert!(parse_delimiter("ab").is_err());
    }

    #[test]
    fn iso_dates_parse_to_epoch_offsets() {
        let t = parse("2026-01-01 1\n2026-01-02 4\n2026-01-03T06:00 2\n", None, false).unwrap();
        let base = plotui_core::days_from_civil(2026, 1, 1) as f64 * 86_400.0;
        assert_eq!(t.x_epoch, Some(base));
        assert_eq!(t.x, vec![0.0, 86_400.0, 2.0 * 86_400.0 + 6.0 * 3600.0]);
        assert_eq!(t.series, vec![vec![1.0, 4.0, 2.0]]);
        // Numeric x stays numeric, and near-date numerics don't trip it.
        assert_eq!(parse("1,10\n2,20\n", None, false).unwrap().x_epoch, None);
        assert!(parse_iso("1-2-3").is_none(), "years need four digits");
        assert_eq!(parse_iso("2026-03-10T12:30:15Z"), parse_iso("2026-03-10 12:30:15"));
    }

    #[test]
    fn mixed_dates_and_numbers_error() {
        let err = parse("2026-01-01,1\n7,2\n", None, false).unwrap_err();
        assert!(err.contains("line 2"), "{err}");
        assert!(err.contains("is not an ISO-8601 date"), "{err}");
    }
}
