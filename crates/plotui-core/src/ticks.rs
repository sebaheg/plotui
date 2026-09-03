//! "Nice ticks": pick round tick positions (1/2/5 × 10^k steps) for an axis
//! range, and format tick labels with just enough precision for that step.

/// Tick positions covering `[lo, hi]` with roughly `target` steps, each a
/// multiple of a 1/2/5 × 10^k step. Returns `(ticks, step)`.
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, f64) {
    if !lo.is_finite() || !hi.is_finite() {
        return (Vec::new(), 1.0);
    }
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let span = hi - lo;
    if span <= 0.0 {
        return (vec![lo], 1.0);
    }
    let raw = span / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = mag
        * if norm <= 1.5 {
            1.0
        } else if norm <= 3.0 {
            2.0
        } else if norm <= 7.0 {
            5.0
        } else {
            10.0
        };
    // Index-based so long ranges don't accumulate float error.
    let i0 = (lo / step - 1e-9).ceil() as i64;
    let i1 = (hi / step + 1e-9).floor() as i64;
    let ticks = (i0..=i1)
        .map(|i| {
            let v = i as f64 * step;
            if v == 0.0 {
                0.0 // normalize -0.0
            } else {
                v
            }
        })
        .collect();
    (ticks, step)
}

/// Format a tick value with precision implied by the tick step.
pub fn format_tick(v: f64, step: f64) -> String {
    if v != 0.0 && (v.abs() >= 1e6 || v.abs() < 1e-4) {
        return format!("{v:.1e}");
    }
    let decimals =
        if step >= 1.0 { 0 } else { (-step.log10().floor() as i32).clamp(0, 6) as usize };
    format!("{v:.decimals$}")
}

const DAY: f64 = 86_400.0;

/// Days since 1970-01-01 for a civil date (proleptic Gregorian, UTC).
/// Howard Hinnant's `days_from_civil` — exact over the whole i64 range that
/// matters here, no dependencies.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = u64::from((m + 9) % 12); // Mar=0 .. Feb=11
    let doy = (153 * mp + 2) / 5 + u64::from(d) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

/// Civil date `(year, month, day)` for days since 1970-01-01 — the inverse
/// of [`days_from_civil`].
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    ((y + i64::from(m <= 2)) as i32, m, d)
}

/// Split an epoch-seconds timestamp into `(year, month, day, second-of-day)`.
fn civil_of(ts: f64) -> (i32, u32, u32, i64) {
    let t = ts.floor() as i64;
    let days = t.div_euclid(86_400);
    let sod = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, sod)
}

/// Date-aware ticks over `[lo, hi]` epoch seconds (UTC), roughly `target`
/// steps: positions land on calendar boundaries (round seconds and minutes,
/// hours, midnights, month firsts, January firsts) and each carries its own
/// label — `HH:MM:SS` / `HH:MM` for sub-day steps (midnights show `MM-DD`),
/// `MM-DD` for day steps (`YYYY-MM-DD` on a year change), `YYYY-MM` for
/// months, `YYYY` for years. Returns `(positions, labels)`.
pub fn date_ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, Vec<String>) {
    if !lo.is_finite() || !hi.is_finite() {
        return (Vec::new(), Vec::new());
    }
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let span = hi - lo;
    if span <= 0.0 {
        let (y, m, d, _) = civil_of(lo);
        return (vec![lo], vec![format!("{y:04}-{m:02}-{d:02}")]);
    }
    let raw = span / target.max(1) as f64;

    if raw <= 26.0 * DAY {
        // Fixed-length steps, all dividing (or being whole multiples of) a
        // day, so index-multiples stay on calendar boundaries.
        #[rustfmt::skip]
        const LADDER: [f64; 21] = [
            1.0, 2.0, 5.0, 10.0, 15.0, 30.0,
            60.0, 120.0, 300.0, 600.0, 900.0, 1800.0,
            3600.0, 7200.0, 10800.0, 21600.0, 43200.0,
            DAY, 2.0 * DAY, 7.0 * DAY, 14.0 * DAY,
        ];
        let step = LADDER.iter().copied().find(|s| *s >= raw).unwrap_or(14.0 * DAY);
        let i0 = (lo / step - 1e-9).ceil() as i64;
        let i1 = (hi / step + 1e-9).floor() as i64;
        let mut ticks = Vec::new();
        let mut labels = Vec::new();
        let mut prev_year: Option<i32> = None;
        for i in i0..=i1 {
            let ts = i as f64 * step;
            let (y, m, d, sod) = civil_of(ts);
            let label = if step >= DAY {
                // Day-grain: a year change (or the first tick) carries it.
                let l = if prev_year != Some(y) {
                    format!("{y:04}-{m:02}-{d:02}")
                } else {
                    format!("{m:02}-{d:02}")
                };
                prev_year = Some(y);
                l
            } else if sod == 0 {
                format!("{m:02}-{d:02}") // midnight anchors the day
            } else if step < 60.0 {
                format!("{:02}:{:02}:{:02}", sod / 3600, (sod / 60) % 60, sod % 60)
            } else {
                format!("{:02}:{:02}", sod / 3600, (sod / 60) % 60)
            };
            ticks.push(ts);
            labels.push(label);
        }
        return (ticks, labels);
    }

    if raw <= 300.0 * DAY {
        // Calendar months are uneven, so month steps walk civil dates. The
        // absolute month index is aligned to the step, putting quarters on
        // Jan/Apr/Jul/Oct and half-years on Jan/Jul.
        let k = if raw <= 45.0 * DAY {
            1i64
        } else if raw <= 135.0 * DAY {
            3
        } else {
            6
        };
        let (y, m, _, _) = civil_of(lo);
        let mut mi = i64::from(y) * 12 + i64::from(m) - 1;
        mi = mi.div_euclid(k) * k;
        let mut ticks = Vec::new();
        let mut labels = Vec::new();
        loop {
            let (ty, tm) = (mi.div_euclid(12) as i32, (mi.rem_euclid(12) + 1) as u32);
            let ts = days_from_civil(ty, tm, 1) as f64 * DAY;
            if ts > hi + 1e-9 {
                break;
            }
            if ts >= lo - 1e-9 {
                ticks.push(ts);
                labels.push(format!("{ty:04}-{tm:02}"));
            }
            mi += k;
        }
        return (ticks, labels);
    }

    // Years, on the 1/2/5 ladder over the year count.
    let step = {
        let raw_years = raw / (365.2425 * DAY);
        let mag = 10f64.powf(raw_years.log10().floor());
        let norm = raw_years / mag;
        (mag * if norm <= 1.5 {
            1.0
        } else if norm <= 3.0 {
            2.0
        } else if norm <= 7.0 {
            5.0
        } else {
            10.0
        })
        .max(1.0) as i64
    };
    let (ylo, ..) = civil_of(lo);
    let mut y = i64::from(ylo).div_euclid(step) * step;
    let mut ticks = Vec::new();
    let mut labels = Vec::new();
    loop {
        let ts = days_from_civil(y as i32, 1, 1) as f64 * DAY;
        if ts > hi + 1e-9 {
            break;
        }
        if ts >= lo - 1e-9 {
            ticks.push(ts);
            labels.push(format!("{y:04}"));
        }
        y += step;
    }
    (ticks, labels)
}

/// `YYYY-MM-DD HH:MM:SS` for an epoch-seconds timestamp (UTC), the time part
/// dropped at exact midnight — the crosshair readout's x header on time axes.
pub fn format_datetime(ts: f64) -> String {
    if !ts.is_finite() {
        return format!("{ts}");
    }
    let (y, m, d, sod) = civil_of(ts);
    if sod == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", sod / 3600, (sod / 60) % 60, sod % 60)
    }
}

/// Tick positions and labels for a log₁₀ axis over the *data* range
/// `[lo, hi]`, in data units — the map applies the transform, so an axis's
/// ticks are the same shape whatever its scale.
///
/// Powers of ten are the ladder, thinned by a whole power when more decades
/// fit than `target` asks for. A range spanning a decade or two would show
/// one or two ticks that way, so there the 1/2/5 subdivisions come in;
/// under a single decade the axis is close enough to linear that
/// [`nice_ticks`] labels it better than any power-of-ten rule can, and
/// deferring to it also keeps a log axis zoomed all the way in from going
/// blank.
pub fn log_ticks(lo: f64, hi: f64, target: usize) -> (Vec<f64>, Vec<String>) {
    if !lo.is_finite() || !hi.is_finite() || hi <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    // A non-positive low end cannot be a log coordinate; the visible range
    // still has to be labelled, so it starts a decade under the top instead.
    let lo = if lo > 0.0 { lo } else { hi / 10.0 };
    let (l0, l1) = (lo.log10(), hi.log10());
    let decades = l1 - l0;
    let target = target.max(1);
    if decades < 1.0 {
        let (t, step) = nice_ticks(lo, hi, target);
        let labels = t.iter().map(|v| format_tick(*v, step)).collect();
        return (t, labels);
    }
    // Whole-decade stride: as many powers as `target` will take.
    let stride = ((decades / target as f64).ceil() as i64).max(1);
    // Subdivisions only where the decades alone would leave the axis nearly
    // unlabelled, and only when they all fit.
    let mantissas: &[f64] =
        if stride == 1 && decades <= 2.0 && target >= 4 { &[1.0, 2.0, 5.0] } else { &[1.0] };
    let (k0, k1) = (l0.floor() as i64, l1.ceil() as i64);
    let eps = 1e-9;
    let mut ticks = Vec::new();
    for k in k0..=k1 {
        if k.rem_euclid(stride) != 0 {
            continue;
        }
        let decade = 10f64.powi(k as i32);
        for m in mantissas {
            let v = m * decade;
            if v >= lo * (1.0 - eps) && v <= hi * (1.0 + eps) {
                ticks.push(v);
            }
        }
    }
    let labels = ticks.iter().map(|v| format_log_tick(*v)).collect();
    (ticks, labels)
}

/// Format a log-axis tick. Its own precision comes from its own magnitude —
/// there is no single step to read it from, the way [`format_tick`] does.
pub fn format_log_tick(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 {
        return "0".to_string();
    }
    let e = a.log10().floor();
    if !(1e-4..1e6).contains(&a) {
        let m = v / 10f64.powf(e);
        let e = e as i32;
        // The mantissa is 1, 2 or 5 by construction, so it never needs a
        // decimal — "1e6" rather than "1.0e6".
        return if (m - 1.0).abs() < 1e-9 { format!("1e{e}") } else { format!("{m:.0}e{e}") };
    }
    let decimals = (-e as i32).clamp(0, 6) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_range_gets_round_ticks() {
        let (t, step) = nice_ticks(0.0, 10.0, 5);
        assert_eq!(step, 2.0);
        assert_eq!(t, vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
    }

    #[test]
    fn fractional_range() {
        let (t, step) = nice_ticks(0.0, 1.0, 5);
        assert_eq!(step, 0.2);
        assert_eq!(t.len(), 6);
        assert!((t[1] - 0.2).abs() < 1e-12);
    }

    #[test]
    fn range_crossing_zero_includes_zero() {
        let (t, _) = nice_ticks(-3.0, 7.0, 5);
        assert!(t.contains(&0.0));
        assert!(t.iter().all(|v| *v >= -3.0 && *v <= 7.0));
    }

    #[test]
    fn degenerate_and_invalid_ranges_do_not_panic() {
        assert_eq!(nice_ticks(5.0, 5.0, 5).0, vec![5.0]);
        assert!(nice_ticks(f64::NAN, 1.0, 5).0.is_empty());
        // Inverted input is normalized.
        let (t, _) = nice_ticks(10.0, 0.0, 5);
        assert_eq!(t.first(), Some(&0.0));
    }

    #[test]
    fn labels_match_step_precision() {
        assert_eq!(format_tick(4.0, 2.0), "4");
        assert_eq!(format_tick(0.2, 0.2), "0.2");
        assert_eq!(format_tick(0.0, 0.05), "0.00");
        assert_eq!(format_tick(2_500_000.0, 500_000.0), "2.5e6");
        assert_eq!(format_tick(0.00002, 0.00001), "2.0e-5");
    }

    // --- date ticks ---

    fn ts(y: i32, m: u32, d: u32) -> f64 {
        days_from_civil(y, m, d) as f64 * 86_400.0
    }

    #[test]
    fn civil_roundtrip_across_leap_years() {
        // 2000 (leap, century-divisible-by-400), 2024 (leap), 2100 (not).
        for days in [-1, 0, 1, 11_016, 19_782, 19_783, 47_540, 47_541, -719_468] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "roundtrip failed for day {days}");
        }
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(days_from_civil(2100, 3, 1) - days_from_civil(2100, 2, 28), 1, "2100 not leap");
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2, "2000 leap");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn day_ticks_land_on_midnights() {
        let (t, l) = date_ticks(ts(2026, 3, 10), ts(2026, 3, 20), 8);
        assert!(!t.is_empty());
        for v in &t {
            assert_eq!((*v as i64).rem_euclid(86_400), 0, "tick {v} not at midnight");
        }
        assert_eq!(l[0], "2026-03-10", "first tick carries the year");
        assert!(l[1..].iter().all(|s| s.len() == 5), "later day labels are MM-DD: {l:?}");
        // Step alignment picks a day parity; either mid-window date is fine.
        assert!(l.contains(&"03-14".into()) || l.contains(&"03-15".into()), "labels: {l:?}");
    }

    #[test]
    fn month_ticks_land_on_the_first() {
        let (t, l) = date_ticks(ts(2025, 11, 15), ts(2026, 8, 3), 8);
        assert!(!t.is_empty());
        for v in &t {
            let (_, _, d) = civil_from_days((*v as i64).div_euclid(86_400));
            assert_eq!(d, 1, "month tick {v} not on the 1st");
        }
        assert!(l.contains(&"2025-12".into()) && l.contains(&"2026-06".into()), "labels: {l:?}");
    }

    #[test]
    fn year_span_gets_year_labels() {
        let (t, l) = date_ticks(ts(2010, 6, 1), ts(2026, 2, 1), 8);
        assert!(!t.is_empty());
        for (v, s) in t.iter().zip(&l) {
            let (y, m, d) = civil_from_days((*v as i64).div_euclid(86_400));
            assert_eq!((m, d), (1, 1), "year tick {v} not on Jan 1");
            assert_eq!(s, &format!("{y:04}"));
        }
        // Quarter alignment for a mid-length span.
        let (t, _) = date_ticks(ts(2025, 2, 20), ts(2026, 4, 10), 6);
        for v in &t {
            let (_, m, _) = civil_from_days((*v as i64).div_euclid(86_400));
            assert!([1, 4, 7, 10].contains(&m), "quarter tick landed on month {m}");
        }
    }

    #[test]
    fn hour_and_minute_label_formats() {
        // Six hours: hour steps, HH:MM labels, midnight shows the date.
        let (t, l) = date_ticks(ts(2026, 3, 10) - 3.0 * 3600.0, ts(2026, 3, 10) + 3.0 * 3600.0, 6);
        assert!(l.contains(&"03-10".into()), "midnight anchors the day: {l:?}");
        assert!(l.iter().any(|s| s == "22:00" || s == "23:00"), "labels: {l:?}");
        // Half a minute: second steps, HH:MM:SS labels.
        let base = ts(2026, 3, 10) + 12.0 * 3600.0;
        let (t2, l2) = date_ticks(base, base + 30.0, 6);
        assert!(!t2.is_empty());
        assert!(l2.iter().all(|s| s.len() == 8 && s.starts_with("12:00:")), "labels: {l2:?}");
        // Every returned tick stays inside the requested range.
        for (lo, hi) in [(base, base + 30.0), (ts(2020, 1, 1), ts(2026, 1, 1))] {
            let (ticks, labels) = date_ticks(lo, hi, 8);
            assert_eq!(ticks.len(), labels.len());
            assert!(ticks.iter().all(|v| *v >= lo - 1e-6 && *v <= hi + 1e-6));
        }
        let _ = t;
    }

    #[test]
    fn log_ticks_walk_the_decades() {
        let (t, labels) = log_ticks(1.0, 10_000.0, 6);
        assert_eq!(labels, vec!["1", "10", "100", "1000", "10000"]);
        assert!(t.iter().zip(&labels).all(|(v, _)| *v > 0.0));

        // Too many decades for the target: whole powers are skipped.
        let (t, labels) = log_ticks(1.0, 1e12, 4);
        assert!(t.len() <= 6, "thinned to {t:?}");
        assert!(labels.contains(&"1".to_string()));
        assert!(labels.iter().any(|l| l.contains('e')), "big decades go exponential: {labels:?}");
    }

    #[test]
    fn a_short_log_range_subdivides_then_goes_linear() {
        // One decade would be two lonely ticks, so 2 and 5 come in.
        let (_, labels) = log_ticks(1.0, 10.0, 5);
        assert_eq!(labels, vec!["1", "2", "5", "10"]);

        // Under a decade the axis is near enough linear to label as one.
        let (t, _) = log_ticks(3.0, 8.0, 5);
        assert!(t.len() >= 3, "a sub-decade range still gets a ladder: {t:?}");
        assert!(t.iter().all(|v| *v >= 3.0 && *v <= 8.0));
    }

    #[test]
    fn log_tick_labels_read_by_magnitude() {
        assert_eq!(format_log_tick(0.001), "0.001");
        assert_eq!(format_log_tick(0.2), "0.2");
        assert_eq!(format_log_tick(1.0), "1");
        assert_eq!(format_log_tick(50.0), "50");
        assert_eq!(format_log_tick(1e6), "1e6");
        assert_eq!(format_log_tick(2e-5), "2e-5");
    }
}
