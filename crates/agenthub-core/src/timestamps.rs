use jiff::Timestamp;

/// Preserve the fractional precision and UTC suffix in native session summaries.
pub(crate) fn format_utc(timestamp: Timestamp, zulu: bool) -> String {
    let nanos = timestamp.subsec_nanosecond();
    let precision = if nanos == 0 {
        0
    } else if nanos % 1_000_000 == 0 {
        3
    } else if nanos % 1_000 == 0 {
        6
    } else {
        9
    };
    if zulu {
        format!("{timestamp:.precision$}")
    } else {
        format!(
            "{:.precision$}",
            timestamp.display_with_offset(jiff::tz::Offset::UTC)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_utc_seconds_milliseconds_and_native_fractional_precision() {
        for (source, zulu, offset) in [
            (
                "2026-01-02T03:04:05+08:00",
                "2026-01-01T19:04:05Z",
                "2026-01-01T19:04:05+00:00",
            ),
            (
                "2026-01-02T03:04:05.120Z",
                "2026-01-02T03:04:05.120Z",
                "2026-01-02T03:04:05.120+00:00",
            ),
            (
                "1969-12-31T23:59:59.999999Z",
                "1969-12-31T23:59:59.999999Z",
                "1969-12-31T23:59:59.999999+00:00",
            ),
            (
                "2026-01-02T03:04:05.120000007Z",
                "2026-01-02T03:04:05.120000007Z",
                "2026-01-02T03:04:05.120000007+00:00",
            ),
        ] {
            let timestamp = source.parse().unwrap();
            assert_eq!(format_utc(timestamp, true), zulu);
            assert_eq!(format_utc(timestamp, false), offset);
        }
    }
}
