//! Dates the way people say them.

use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Utc};

/// A short, relative time for list rows: "now", "5m", "3h", "Yesterday",
/// a weekday within the week, then "Mar 4", then "Mar 4, 2023".
pub fn short<Tz: TimeZone>(time: DateTime<Utc>, now: DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = time.with_timezone(&now.timezone());
    let age = now.clone().with_timezone(&Utc) - time;
    if age < Duration::minutes(1) {
        return "now".into();
    }
    if age < Duration::hours(1) {
        return format!("{}m", age.num_minutes());
    }
    let days = (now.date_naive() - local.date_naive()).num_days();
    if days == 0 {
        return format!("{}h", age.num_hours().max(1));
    }
    if days == 1 {
        return "Yesterday".into();
    }
    if days < 7 {
        return local.format("%A").to_string();
    }
    if local.year() == now.year() {
        local.format("%b %-d").to_string()
    } else {
        local.format("%b %-d, %Y").to_string()
    }
}

/// A full date for the reader's header: "March 4, 2025".
pub fn long<Tz: TimeZone>(time: DateTime<Utc>, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    time.with_timezone(zone).format("%B %-d, %Y").to_string()
}

/// The start of `now`'s day, in UTC, for "Today" lists.
pub fn start_of_day<Tz: TimeZone>(now: DateTime<Tz>) -> DateTime<Utc> {
    let midnight = now.date_naive().and_time(NaiveTime::MIN);
    now.timezone()
        .from_local_datetime(&midnight)
        .earliest()
        // Midnight can fall in a DST gap; the day then starts an hour in.
        .unwrap_or_else(|| now.clone())
        .with_timezone(&Utc)
}

/// [`start_of_day`] in the system's time zone.
pub fn local_start_of_today() -> DateTime<Utc> {
    start_of_day(Local::now())
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    fn zone() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<FixedOffset> {
        zone().with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn utc(time: DateTime<FixedOffset>) -> DateTime<Utc> {
        time.with_timezone(&Utc)
    }

    #[test]
    fn short_times() {
        let now = at(2025, 6, 11, 15, 0); // a Wednesday
        let cases = [
            (at(2025, 6, 11, 14, 59), "1m"),
            (at(2025, 6, 11, 14, 59) + Duration::seconds(30), "now"),
            (at(2025, 6, 11, 14, 5), "55m"),
            (at(2025, 6, 11, 9, 0), "6h"),
            (at(2025, 6, 11, 0, 30), "14h"),
            (at(2025, 6, 10, 23, 0), "Yesterday"),
            (at(2025, 6, 8, 12, 0), "Sunday"),
            (at(2025, 6, 4, 12, 0), "Jun 4"),
            (at(2024, 12, 31, 12, 0), "Dec 31, 2024"),
            (at(2025, 6, 11, 16, 0), "now"),
        ];
        for (time, expected) in cases {
            assert_eq!(short(utc(time), now), expected, "{time}");
        }
    }

    #[test]
    fn long_dates() {
        assert_eq!(long(utc(at(2025, 3, 4, 23, 30)), &zone()), "March 4, 2025");
    }

    #[test]
    fn days_start_at_local_midnight() {
        let start = start_of_day(at(2025, 6, 11, 15, 0));
        assert_eq!(start, utc(at(2025, 6, 11, 0, 0)));
        assert_eq!(start.to_rfc3339(), "2025-06-11T05:00:00+00:00");
    }
}
