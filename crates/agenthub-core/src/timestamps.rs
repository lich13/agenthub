use jiff::Timestamp;

/// Parse an optional timestamp string into a unified UTC nanosecond sort key.
/// Ensures `.123Z` vs `Z` and `+08:00` vs `Z` sort chronologically rather than lexicographically.
pub(crate) fn sort_key_nanos(raw: Option<&str>) -> Option<i128> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed.starts_with("0000-") || trimmed.starts_with("0001-") {
        return None;
    }
    if let Ok(ts) = trimmed.parse::<Timestamp>() {
        return Some(ts.as_nanosecond());
    }
    if let Ok(num) = trimmed.parse::<i128>() {
        if num <= 0 {
            return None;
        }
        return Some(if num > 1_000_000_000_000_000 {
            num
        } else if num > 1_000_000_000_000 {
            num * 1_000_000
        } else {
            num * 1_000_000_000
        });
    }
    None
}

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

    #[test]
    fn sort_key_nanos_orders_fractional_and_offset_timestamps_chronologically() {
        let with_ms = sort_key_nanos(Some("2026-01-02T03:04:05.120Z")).unwrap();
        let whole_sec = sort_key_nanos(Some("2026-01-02T03:04:05Z")).unwrap();
        assert!(with_ms > whole_sec);

        let offset_later = sort_key_nanos(Some("2026-01-02T12:00:00+08:00")).unwrap();
        let utc_earlier = sort_key_nanos(Some("2026-01-02T03:59:59Z")).unwrap();
        assert!(offset_later > utc_earlier);

        assert_eq!(
            sort_key_nanos(Some("1700000000")),
            Some(1_700_000_000_000_000_000)
        );
        assert_eq!(
            sort_key_nanos(Some("1700000000123")),
            Some(1_700_000_000_123_000_000)
        );
        assert_eq!(sort_key_nanos(Some("")), None);
        assert_eq!(sort_key_nanos(Some("0001-01-01T00:00:00Z")), None);
        assert_eq!(sort_key_nanos(None), None);
    }
}
