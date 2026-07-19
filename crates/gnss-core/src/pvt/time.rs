const GPS_WEEK_SECONDS: f64 = 604_800.0;
const GPS_EPOCH_UNIX_SECONDS: f64 = 315_964_800.0;

const LEAP_SECONDS: &[(i32, u8, u8, i32)] = &[
    (2017, 1, 1, 18),
    (2015, 7, 1, 17),
    (2012, 7, 1, 16),
    (2009, 1, 1, 15),
    (2006, 1, 1, 14),
    (1999, 1, 1, 13),
    (1997, 7, 1, 12),
    (1996, 1, 1, 11),
    (1994, 7, 1, 10),
    (1993, 7, 1, 9),
    (1992, 7, 1, 8),
    (1991, 1, 1, 7),
    (1990, 1, 1, 6),
    (1988, 1, 1, 5),
    (1985, 7, 1, 4),
    (1983, 7, 1, 3),
    (1982, 7, 1, 2),
    (1981, 7, 1, 1),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpsTime {
    pub week: u16,
    pub sow: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UtcDateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: f64,
}

pub fn normalize_gps_time(week: u16, sow: f64) -> Option<GpsTime> {
    if !sow.is_finite() {
        return None;
    }
    let week_delta = (sow / GPS_WEEK_SECONDS).floor() as i64;
    let normalized_week = i64::from(week) + week_delta;
    if !(0..=i64::from(u16::MAX)).contains(&normalized_week) {
        return None;
    }
    Some(GpsTime {
        week: normalized_week as u16,
        sow: sow - week_delta as f64 * GPS_WEEK_SECONDS,
    })
}

pub fn gps_time_to_utc(week: u16, sow: f64) -> Option<(f64, UtcDateTime)> {
    let gps = normalize_gps_time(week, sow)?;
    let gps_unix_seconds =
        GPS_EPOCH_UNIX_SECONDS + f64::from(gps.week) * GPS_WEEK_SECONDS + gps.sow;
    let leap_seconds = LEAP_SECONDS
        .iter()
        .find_map(|&(year, month, day, offset)| {
            let transition_utc = unix_seconds_at_utc_midnight(year, month, day) as f64;
            (gps_unix_seconds >= transition_utc + f64::from(offset)).then_some(offset)
        })
        .unwrap_or(0);
    let utc_unix_seconds = gps_unix_seconds - f64::from(leap_seconds);
    Some((utc_unix_seconds, utc_date_time_from_unix(utc_unix_seconds)))
}

fn unix_seconds_at_utc_midnight(year: i32, month: u8, day: u8) -> i64 {
    days_from_civil(year, month, day) * 86_400
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let adjusted_year = i64::from(year) - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn utc_date_time_from_unix(unix_seconds: f64) -> UtcDateTime {
    let whole_seconds = unix_seconds.floor() as i64;
    let fractional_second = unix_seconds - whole_seconds as f64;
    let days = whole_seconds.div_euclid(86_400);
    let seconds_of_day = whole_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    UtcDateTime {
        year,
        month,
        day,
        hour: (seconds_of_day / 3_600) as u8,
        minute: ((seconds_of_day % 3_600) / 60) as u8,
        second: (seconds_of_day % 60) as f64 + fractional_second,
    }
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u8, u8) {
    let shifted_days = days_since_unix_epoch + 719_468;
    let era = shifted_days.div_euclid(146_097);
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cttc_gps_time_converts_to_utc_with_2013_leap_offset() {
        let (unix_seconds, utc) = gps_time_to_utc(1734, 368_663.0).expect("valid GPS time");

        assert_eq!(unix_seconds, 1_365_056_647.0);
        assert_eq!(utc.year, 2013);
        assert_eq!(utc.month, 4);
        assert_eq!(utc.day, 4);
        assert_eq!(utc.hour, 6);
        assert_eq!(utc.minute, 24);
        assert_eq!(utc.second, 7.0);
    }

    #[test]
    fn clock_correction_normalizes_across_gps_week_boundary() {
        assert_eq!(
            normalize_gps_time(1734, -0.25),
            Some(GpsTime {
                week: 1733,
                sow: 604_799.75,
            })
        );
    }
}
