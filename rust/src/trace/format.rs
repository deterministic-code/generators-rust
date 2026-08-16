#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceTier {
    Route,
    Service,
    Datasource,
}

impl TraceTier {
    pub fn as_str(self) -> &'static str {
        match self {
            TraceTier::Route => "route",
            TraceTier::Service => "service",
            TraceTier::Datasource => "datasource",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracePhase {
    Start,
    Finish,
    Error,
}

impl TracePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            TracePhase::Start => "Start",
            TracePhase::Finish => "Finish",
            TracePhase::Error => "Error",
        }
    }
}

pub fn format_trace_line(
    iso_date_time: &str,
    tier: TraceTier,
    phase: TracePhase,
    method: &str,
    suffix: Option<&str>,
) -> String {
    let head = format!(
        "[{}]-[{}][{}]-[{}]",
        iso_date_time,
        tier.as_str(),
        phase.as_str(),
        method,
    );
    match suffix {
        Some(s) if !s.is_empty() => format!("{} {}", head, s),
        _ => head,
    }
}

pub fn format_elapsed_ms(elapsed_ms: f64) -> String {
    if !elapsed_ms.is_finite() || elapsed_ms <= 0.0 {
        return "0ns".to_string();
    }
    if elapsed_ms >= 1.0 {
        return format!("{}ms", elapsed_ms.round() as i64);
    }
    if elapsed_ms >= 0.001 {
        return format!("{}μs", (elapsed_ms * 1000.0).round() as i64);
    }
    format!("{}ns", (elapsed_ms * 1_000_000.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_trace_line_emits_head_with_no_suffix() {
        let line = format_trace_line(
            "2026-06-01T12:34:56.789Z",
            TraceTier::Route,
            TracePhase::Start,
            "GET /api/users/42",
            None,
        );
        assert_eq!(
            line,
            "[2026-06-01T12:34:56.789Z]-[route][Start]-[GET /api/users/42]",
        );
    }

    #[test]
    fn format_trace_line_appends_suffix_with_single_leading_space() {
        let line = format_trace_line(
            "2026-06-01T12:34:56.802Z",
            TraceTier::Route,
            TracePhase::Finish,
            "GET /api/users/42",
            Some("200 13ms"),
        );
        assert_eq!(
            line,
            "[2026-06-01T12:34:56.802Z]-[route][Finish]-[GET /api/users/42] 200 13ms",
        );
    }

    #[test]
    fn format_trace_line_supports_datasource_tier_and_error_phase() {
        let line = format_trace_line(
            "2026-06-01T12:34:56.797Z",
            TraceTier::Datasource,
            TracePhase::Error,
            "SELECT",
            Some("connection refused 2ms"),
        );
        assert_eq!(
            line,
            "[2026-06-01T12:34:56.797Z]-[datasource][Error]-[SELECT] connection refused 2ms",
        );
    }

    #[test]
    fn format_trace_line_supports_service_tier() {
        let line = format_trace_line(
            "2026-06-01T12:34:56.795Z",
            TraceTier::Service,
            TracePhase::Start,
            "UsersService.findById",
            None,
        );
        assert_eq!(
            line,
            "[2026-06-01T12:34:56.795Z]-[service][Start]-[UsersService.findById]",
        );
    }

    #[test]
    fn format_trace_line_omits_suffix_when_empty_string_passed() {
        let line = format_trace_line(
            "2026-06-01T00:00:00.000Z",
            TraceTier::Service,
            TracePhase::Finish,
            "X.y",
            Some(""),
        );
        assert_eq!(line, "[2026-06-01T00:00:00.000Z]-[service][Finish]-[X.y]");
    }

    #[test]
    fn format_elapsed_ms_formats_at_or_above_one_ms_as_rounded_ms() {
        assert_eq!(format_elapsed_ms(1.0), "1ms");
        assert_eq!(format_elapsed_ms(42.5), "43ms");
        assert_eq!(format_elapsed_ms(15.0), "15ms");
        assert_eq!(format_elapsed_ms(1234.7), "1235ms");
    }

    #[test]
    fn format_elapsed_ms_formats_sub_millisecond_as_microseconds() {
        assert_eq!(format_elapsed_ms(0.234), "234μs");
        assert_eq!(format_elapsed_ms(0.9999), "1000μs");
        assert_eq!(format_elapsed_ms(0.001), "1μs");
        assert_eq!(format_elapsed_ms(0.5), "500μs");
    }

    #[test]
    fn format_elapsed_ms_formats_sub_microsecond_as_nanoseconds() {
        assert_eq!(format_elapsed_ms(0.0009), "900ns");
        assert_eq!(format_elapsed_ms(0.000123), "123ns");
        assert_eq!(format_elapsed_ms(0.000001), "1ns");
    }

    #[test]
    fn format_elapsed_ms_formats_zero_as_0ns() {
        assert_eq!(format_elapsed_ms(0.0), "0ns");
    }

    #[test]
    fn format_elapsed_ms_clamps_negative_clock_skew_to_0ns() {
        assert_eq!(format_elapsed_ms(-0.5), "0ns");
    }
}
