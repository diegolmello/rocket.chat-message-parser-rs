use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeDelta, TimeZone};

use super::chars::is;
use super::memo::memoized;
use super::{Flags, Parsed, Parser};
use crate::ast::{plain, timestamp, Node};

const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const MAXIMUM_TIME_VALUE: i64 = 8_640_000_000_000_000;

#[derive(Default)]
struct DateParts {
    year: i64,
    month: i64,
    day: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
    timezone: Option<(i64, i64, i64)>,
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn local_offset_milliseconds(local_as_utc: i64) -> i64 {
    let Some(naive) =
        DateTime::from_timestamp_millis(local_as_utc).map(|moment| moment.naive_utc())
    else {
        return 0;
    };
    let offset_at_instant = |utc: NaiveDateTime| {
        i64::from(Local.offset_from_utc_datetime(&utc).local_minus_utc()) * 1000
    };
    let candidates = match Local.from_local_datetime(&naive) {
        LocalResult::Single(local) => vec![local],
        LocalResult::Ambiguous(first, second) => vec![first, second],
        LocalResult::None => Vec::new(),
    };
    let earliest_matching_instant = candidates
        .into_iter()
        .map(|local| local.timestamp_millis())
        .filter(|&instant| {
            DateTime::from_timestamp_millis(instant)
                .is_some_and(|utc| utc.with_timezone(&Local).naive_local() == naive)
        })
        .min();
    match earliest_matching_instant {
        Some(instant) => local_as_utc - instant,
        None => offset_at_instant(naive - TimeDelta::days(1)),
    }
}

fn epoch_milliseconds(parts: &DateParts) -> Option<i64> {
    let is_midnight_end =
        parts.hours == 24 && parts.minutes == 0 && parts.seconds == 0 && parts.milliseconds == 0;
    let valid = (1..=12).contains(&parts.month)
        && (1..=31).contains(&parts.day)
        && (parts.hours <= 23 || is_midnight_end)
        && parts.minutes <= 59
        && parts.seconds <= 59
        && parts
            .timezone
            .is_none_or(|(_, hours, minutes)| hours <= 23 && minutes <= 59);
    if !valid {
        return None;
    }
    let days = days_from_civil(parts.year, parts.month, parts.day);
    let time =
        ((parts.hours * 60 + parts.minutes) * 60 + parts.seconds) * 1000 + parts.milliseconds;
    let wall_clock = days * MILLISECONDS_PER_DAY + time;
    let utc = match parts.timezone {
        Some((sign, hours, minutes)) => wall_clock - sign * (hours * 60 + minutes) * 60_000,
        None => wall_clock - local_offset_milliseconds(wall_clock),
    };
    (utc.abs() <= MAXIMUM_TIME_VALUE).then_some(utc)
}

fn to_int32_seconds(milliseconds: Option<i64>) -> String {
    let Some(milliseconds) = milliseconds else {
        return "0".into();
    };
    let truncated = (milliseconds as f64 / 1000.0).trunc() as i64;
    let wrapped = truncated.rem_euclid(1 << 32);
    let signed = if wrapped >= 1 << 31 {
        wrapped - (1 << 32)
    } else {
        wrapped
    };
    signed.to_string()
}

fn utc_civil_date(now: SystemTime) -> (i64, i64, i64) {
    let milliseconds = match now.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    };
    let days = milliseconds.div_euclid(MILLISECONDS_PER_DAY);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

impl Parser<'_> {
    fn number(&mut self, start: usize, end: usize) -> i64 {
        self.text(start, end).parse().unwrap_or(0)
    }

    fn two_digits(&mut self, position: usize) -> Parsed<i64> {
        let end = self.exact_digits(position, 2)?;
        Some((end, self.number(position, end)))
    }

    fn timezone(&mut self, position: usize) -> Parsed<(i64, i64, i64)> {
        let sign = match self.at(position)? {
            unit if is(unit, b'+') => 1,
            unit if is(unit, b'-') => -1,
            _ => return None,
        };
        let (after_hours, hours) = self.two_digits(position + 1)?;
        let after_colon = self.char(after_hours, b':')?;
        let (end, minutes) = self.two_digits(after_colon)?;
        Some((end, (sign, hours, minutes)))
    }

    fn optional_timezone(&mut self, position: usize) -> (usize, Option<(i64, i64, i64)>) {
        match self.timezone(position) {
            Some((end, timezone)) => (end, Some(timezone)),
            None => (position, None),
        }
    }

    fn iso_date(&mut self, position: usize, with_milliseconds: bool) -> Parsed<String> {
        let year_end = self.exact_digits(position, 4)?;
        let year = self.number(position, year_end);
        let after_dash = self.char(year_end, b'-')?;
        let (month_end, month) = self.two_digits(after_dash)?;
        let after_dash = self.char(month_end, b'-')?;
        let (day_end, day) = self.two_digits(after_dash)?;
        let after_separator = self.char(day_end, b'T')?;
        let (hours_end, hours) = self.two_digits(after_separator)?;
        let after_colon = self.char(hours_end, b':')?;
        let (minutes_end, minutes) = self.two_digits(after_colon)?;
        let after_colon = self.char(minutes_end, b':')?;
        let (mut cursor, seconds) = self.two_digits(after_colon)?;
        let mut milliseconds = 0;
        if with_milliseconds {
            let after_dot = self.char(cursor, b'.')?;
            let milliseconds_end = self.exact_digits(after_dot, 3)?;
            milliseconds = self.number(after_dot, milliseconds_end);
            cursor = milliseconds_end;
        }
        let (end, timezone) = self.optional_timezone(cursor);
        let parts = DateParts {
            year,
            month,
            day,
            hours,
            minutes,
            seconds,
            milliseconds,
            timezone,
        };
        Some((end, to_int32_seconds(epoch_milliseconds(&parts))))
    }

    fn time_of_day(&mut self, position: usize, with_seconds: bool) -> Parsed<String> {
        let (hours_end, hours) = self.two_digits(position)?;
        let after_colon = self.char(hours_end, b':')?;
        let (mut cursor, minutes) = self.two_digits(after_colon)?;
        let mut seconds = 0;
        if with_seconds {
            let after_colon = self.char(cursor, b':')?;
            let (seconds_end, value) = self.two_digits(after_colon)?;
            seconds = value;
            cursor = seconds_end;
        }
        let (end, timezone) = self.optional_timezone(cursor);
        let (year, month, day) = utc_civil_date(self.now);
        let parts = DateParts {
            year,
            month,
            day,
            hours,
            minutes,
            seconds,
            milliseconds: 0,
            timezone,
        };
        Some((end, to_int32_seconds(epoch_milliseconds(&parts))))
    }

    fn timestamp_date(&mut self, position: usize) -> Parsed<String> {
        if let Some(end) = self.exact_digits(position, 10) {
            return Some((end, self.text(position, end)));
        }
        self.iso_date(position, true)
            .or_else(|| self.iso_date(position, false))
            .or_else(|| self.time_of_day(position, true))
            .or_else(|| self.time_of_day(position, false))
    }

    fn timestamp_format(&mut self, position: usize) -> Parsed<char> {
        let unit = self.at(position)?;
        let format =
            char::from_u32(u32::from(unit)).filter(|format| "tTdDfFR".contains(*format))?;
        Some((position + 1, format))
    }

    pub(super) fn timestamp_rules(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, timestamp_rules, position, Flags::NONE, {
            let date_start = self.literal(position, "<t:")?;
            let (date_end, date) = self.timestamp_date(date_start)?;
            let formatted = self.char(date_end, b':').and_then(|after_colon| {
                let (format_end, format) = self.timestamp_format(after_colon)?;
                Some((self.char(format_end, b'>')?, format))
            });
            let (end, format) = match formatted {
                Some((end, format)) => (end, Some(format)),
                None => (self.char(date_end, b'>')?, None),
            };
            Some((end, timestamp(date, format, Some([position, end]))))
        })
    }

    pub(super) fn escaped_timestamp_rules(&mut self, position: usize) -> Parsed<Node> {
        memoized!(self, escaped_timestamp_rules, position, Flags::NONE, {
            let after_backslash = self.char(position, b'\\')?;
            let (end, _) = self.timestamp_rules(after_backslash)?;
            Some((end, plain(self.text(after_backslash, end))))
        })
    }
}
