//! UTC formatting without a date crate: file-name stamps and RFC 3339.

use std::time::SystemTime;

struct Civil {
    year: i64,
    month: i64,
    day: i64,
    hour: u64,
    minute: u64,
    second: u64,
}

/// Times before the epoch clamp to it; nothing here records them.
fn civil(t: SystemTime) -> Civil {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Civil {
        year: yoe + era * 400 + i64::from(month <= 2),
        month,
        day,
        hour: rem / 3600,
        minute: rem % 3600 / 60,
        second: rem % 60,
    }
}

/// `YYYYMMDDHHMMSS`, for file names that sort by time.
pub(crate) fn utc_stamp(t: SystemTime) -> String {
    let c = civil(t);
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

/// `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn rfc3339(t: SystemTime) -> String {
    let c = civil(t);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn stamps() {
        assert_eq!(utc_stamp(at(0)), "19700101000000");
        assert_eq!(utc_stamp(at(951_868_800)), "20000301000000");
        assert_eq!(utc_stamp(at(1_709_210_096)), "20240229123456");
        assert_eq!(utc_stamp(at(1_792_265_405)), "20261017193005");
        assert_eq!(
            utc_stamp(SystemTime::UNIX_EPOCH - Duration::from_secs(5)),
            "19700101000000"
        );
    }

    #[test]
    fn rfc3339_forms() {
        assert_eq!(rfc3339(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(at(1_709_210_096)), "2024-02-29T12:34:56Z");
        assert_eq!(rfc3339(at(1_792_265_405)), "2026-10-17T19:30:05Z");
        assert_eq!(
            rfc3339(SystemTime::UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z"
        );
    }
}
