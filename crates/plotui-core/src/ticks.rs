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
}
