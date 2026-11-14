//! Reading the ISO 8601 times `claude` writes in its replies.

/// Unix seconds for `YYYY-MM-DDTHH:MM:SS`, an optional fraction (dropped) and a
/// zone that is `Z` or `±HH:MM`. `None` for anything else, including a time
/// with no zone or a field out of range.
pub fn iso8601_to_unix(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    // `YYYY-MM-DDTHH:MM:SS` is 19 bytes, all ASCII.
    if bytes.len() < 20 || !text.is_char_boundary(19) {
        return None;
    }
    let (clock, rest) = text.split_at(19);
    let shape = clock.as_bytes();
    for (at, separator) in [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')] {
        if shape[at] != separator {
            return None;
        }
    }
    let year = digits(&clock[0..4])?;
    let month = digits(&clock[5..7])?;
    let day = digits(&clock[8..10])?;
    let hour = digits(&clock[11..13])?;
    let minute = digits(&clock[14..16])?;
    let second = digits(&clock[17..19])?;
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    // An optional fraction, then the zone.
    let rest = match rest.strip_prefix('.') {
        Some(fraction) => {
            let end = fraction
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(fraction.len());
            if end == 0 {
                return None;
            }
            &fraction[end..]
        }
        None => rest,
    };
    let offset_seconds = match rest {
        "Z" => 0,
        zone => {
            let sign = match zone.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let zone = zone.get(1..)?;
            if zone.len() != 5 || zone.as_bytes()[2] != b':' {
                return None;
            }
            let (zone_hour, zone_minute) = (digits(&zone[0..2])?, digits(&zone[3..5])?);
            if zone_hour > 23 || zone_minute > 59 {
                return None;
            }
            sign * (zone_hour * 3600 + zone_minute * 60)
        }
    };

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds)
}

/// A number made only of ASCII digits; no sign, no spaces.
fn digits(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to the given date in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times_become_unix_seconds() {
        assert_eq!(
            iso8601_to_unix("2026-11-11T08:19:59.618411+00:00"),
            Some(1794385199)
        );
        assert_eq!(iso8601_to_unix("2026-11-15T20:59:59Z"), Some(1794776399));
        assert_eq!(
            iso8601_to_unix("2026-03-01T00:30:00+02:00"),
            Some(1772317800)
        );
        assert_eq!(iso8601_to_unix("1970-01-01T00:00:00Z"), Some(0));
        for bad in [
            "",
            "soon",
            "2026-13-01T00:00:00Z",
            "2026-11-11",
            "2026-11-11T08:19:59",
        ] {
            assert_eq!(iso8601_to_unix(bad), None, "{bad}");
        }
    }
}
