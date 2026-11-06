//! Minimal RFC 3339 parsing for date literals in rules.

/// Accepts `YYYY-MM-DD`, `YYYY-MM-DD[T ]HH:MM:SS[.ffffff][Z]`. Returns µs since epoch, UTC.
pub fn parse_datetime_us(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = b.get(r)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    let mut us = days_from_civil(y, m, d) * 86_400_000_000;
    let rest = &b[10..];
    if rest.is_empty() {
        return Some(us);
    }
    if !matches!(rest[0], b'T' | b' ') || rest.len() < 9 || rest[3] != b':' || rest[6] != b':' {
        return None;
    }
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    us += ((h * 60 + mi) * 60 + sec) * 1_000_000;
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let frac = &s[start..i];
        if frac.is_empty() || frac.len() > 6 {
            return None;
        }
        us += frac.parse::<i64>().ok()? * 10i64.pow(6 - frac.len() as u32);
    }
    match &b[i..] {
        b"" | b"Z" => Some(us),
        _ => None,
    }
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// Howard Hinnant's days_from_civil.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::parse_datetime_us as p;

    #[test]
    fn parses() {
        assert_eq!(p("1970-01-01"), Some(0));
        assert_eq!(p("2000-03-01T00:00:00Z"), Some(951_868_800_000_000));
        assert_eq!(p("2024-02-29 12:30:15.5"), Some(1_709_209_815_500_000));
        for bad in [
            "2023-02-29",
            "2024-13-01",
            "2024-01-01T25:00:00",
            "2024-01-01T00:00:00+01:00",
            "x",
        ] {
            assert_eq!(p(bad), None, "{bad}");
        }
    }
}
