use crate::error::{Error, Result};
use chrono::NaiveDateTime;

/// Parsed 5-field cron: minute hour day-of-month month day-of-week.
pub struct CronSchedule {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days_of_month: Vec<u32>,
    months: Vec<u32>,
    days_of_week: Vec<u32>, // 0=Sunday
}

impl CronSchedule {
    /// Supports: `*`, `*/N`, `N`, `N-M`, `N,M,O`.
    pub fn parse(expr: &str) -> Result<Self> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(Error::Config(format!(
                "cron expression must have 5 fields, got {}",
                fields.len()
            )));
        }

        let minutes = parse_field(fields[0], 0, 59)?;
        let hours = parse_field(fields[1], 0, 23)?;
        let days_of_month = parse_field(fields[2], 1, 31)?;
        let months = parse_field(fields[3], 1, 12)?;
        let days_of_week = parse_field(fields[4], 0, 6)?;

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
        })
    }

    /// Next matching datetime after `from`. Gives up after 366 days.
    pub fn next_from(&self, from: NaiveDateTime) -> Option<NaiveDateTime> {
        use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};

        // Always start from the next minute — we don't re-trigger the current one
        let mut current = from.date().and_time(NaiveTime::from_hms_opt(
            from.time().hour(),
            from.time().minute(),
            0,
        )?) + Duration::minutes(1);

        let limit = from + Duration::days(366);

        while current <= limit {
            let month = current.month();
            let day = current.day();
            let hour = current.hour();
            let minute = current.minute();
            let weekday = current.weekday().num_days_from_sunday(); // 0=Sunday

            if !self.months.contains(&month) {
                if let Some(next_month) = next_matching(&self.months, month) {
                    if next_month > month {
                        if let Some(date) = NaiveDate::from_ymd_opt(current.year(), next_month, 1) {
                            current = date.and_hms_opt(0, 0, 0)?;
                            continue;
                        }
                    }
                }
                // No matching month left this year — roll over
                if let Some(&first_month) = self.months.first() {
                    if let Some(date) = NaiveDate::from_ymd_opt(current.year() + 1, first_month, 1)
                    {
                        current = date.and_hms_opt(0, 0, 0)?;
                        continue;
                    }
                }
                return None;
            }

            // Both day-of-month and day-of-week must match (AND, not OR)
            if !self.days_of_month.contains(&day) || !self.days_of_week.contains(&weekday) {
                current += Duration::days(1);
                current = current.date().and_hms_opt(
                    *self.hours.first().unwrap_or(&0),
                    *self.minutes.first().unwrap_or(&0),
                    0,
                )?;
                continue;
            }

            if !self.hours.contains(&hour) {
                if let Some(next_hour) = next_matching(&self.hours, hour) {
                    if next_hour > hour {
                        current = current.date().and_hms_opt(
                            next_hour,
                            *self.minutes.first().unwrap_or(&0),
                            0,
                        )?;
                        continue;
                    }
                }
                current += Duration::days(1);
                current = current.date().and_hms_opt(
                    *self.hours.first().unwrap_or(&0),
                    *self.minutes.first().unwrap_or(&0),
                    0,
                )?;
                continue;
            }

            if !self.minutes.contains(&minute) {
                if let Some(next_min) = next_matching(&self.minutes, minute) {
                    if next_min > minute {
                        current = current.date().and_hms_opt(hour, next_min, 0)?;
                        continue;
                    }
                }
                let next_hour = if let Some(nh) = next_matching(&self.hours, hour + 1) {
                    if nh > hour {
                        current = current.date().and_hms_opt(
                            nh,
                            *self.minutes.first().unwrap_or(&0),
                            0,
                        )?;
                        continue;
                    }
                    nh
                } else {
                    *self.hours.first().unwrap_or(&0)
                };
                current += Duration::days(1);
                current = current.date().and_hms_opt(
                    next_hour,
                    *self.minutes.first().unwrap_or(&0),
                    0,
                )?;
                continue;
            }

            // All fields match
            return Some(current);
        }

        None
    }
}

fn next_matching(values: &[u32], after: u32) -> Option<u32> {
    values.iter().copied().find(|&v| v >= after)
}

/// Expand a cron field (`*`, `*/N`, `N-M`, `N,M`) into a sorted list of values.
fn parse_field(field: &str, min: u32, max: u32) -> Result<Vec<u32>> {
    let mut result = Vec::new();

    for part in field.split(',') {
        if part == "*" {
            return Ok((min..=max).collect());
        } else if let Some(step_str) = part.strip_prefix("*/") {
            let step: u32 = step_str
                .parse()
                .map_err(|_| Error::Config(format!("invalid step value: '{step_str}'")))?;
            if step == 0 {
                return Err(Error::Config("step value cannot be 0".into()));
            }
            let mut val = min;
            while val <= max {
                result.push(val);
                val += step;
            }
        } else if part.contains('-') {
            let bounds: Vec<&str> = part.split('-').collect();
            if bounds.len() != 2 {
                return Err(Error::Config(format!("invalid range: '{part}'")));
            }
            let start: u32 = bounds[0]
                .parse()
                .map_err(|_| Error::Config(format!("invalid range start: '{}'", bounds[0])))?;
            let end: u32 = bounds[1]
                .parse()
                .map_err(|_| Error::Config(format!("invalid range end: '{}'", bounds[1])))?;
            if start < min || end > max || start > end {
                return Err(Error::Config(format!(
                    "range {start}-{end} out of bounds ({min}-{max})"
                )));
            }
            for val in start..=end {
                result.push(val);
            }
        } else {
            let val: u32 = part
                .parse()
                .map_err(|_| Error::Config(format!("invalid cron value: '{part}'")))?;
            if val < min || val > max {
                return Err(Error::Config(format!(
                    "value {val} out of bounds ({min}-{max})"
                )));
            }
            result.push(val);
        }
    }

    result.sort();
    result.dedup();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_parse_simple() {
        let cron = CronSchedule::parse("0 8 * * *").unwrap();
        assert_eq!(cron.minutes, vec![0]);
        assert_eq!(cron.hours, vec![8]);
        assert_eq!(cron.days_of_month, (1..=31).collect::<Vec<_>>());
        assert_eq!(cron.months, (1..=12).collect::<Vec<_>>());
        assert_eq!(cron.days_of_week, (0..=6).collect::<Vec<_>>());
    }

    #[test]
    fn test_parse_step() {
        let cron = CronSchedule::parse("*/5 * * * *").unwrap();
        assert_eq!(
            cron.minutes,
            vec![0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]
        );
    }

    #[test]
    fn test_parse_range() {
        let cron = CronSchedule::parse("0 9-17 * * *").unwrap();
        assert_eq!(cron.hours, vec![9, 10, 11, 12, 13, 14, 15, 16, 17]);
    }

    #[test]
    fn test_parse_list() {
        let cron = CronSchedule::parse("0 8,12,18 * * *").unwrap();
        assert_eq!(cron.hours, vec![8, 12, 18]);
    }

    #[test]
    fn test_next_daily() {
        let cron = CronSchedule::parse("0 8 * * *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_every_5min() {
        let cron = CronSchedule::parse("*/5 * * * *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(10, 3, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(10, 5, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_same_hour() {
        let cron = CronSchedule::parse("30 10 * * *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(10, 0, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    // --- parse_field edge cases ---

    #[test]
    fn test_parse_field_out_of_bounds_below_min() {
        let err = parse_field("0", 1, 31).unwrap_err();
        assert!(err.to_string().contains("out of bounds"));
    }

    #[test]
    fn test_parse_field_out_of_bounds_above_max() {
        let err = parse_field("60", 0, 59).unwrap_err();
        assert!(err.to_string().contains("out of bounds"));
    }

    #[test]
    fn test_parse_field_reversed_range() {
        let err = parse_field("20-10", 0, 59).unwrap_err();
        assert!(err.to_string().contains("out of bounds"));
    }

    #[test]
    fn test_parse_field_range_start_below_min() {
        let err = parse_field("0-5", 1, 31).unwrap_err();
        assert!(err.to_string().contains("out of bounds"));
    }

    #[test]
    fn test_parse_field_range_end_above_max() {
        let err = parse_field("50-65", 0, 59).unwrap_err();
        assert!(err.to_string().contains("out of bounds"));
    }

    #[test]
    fn test_parse_field_step_zero() {
        let err = parse_field("*/0", 0, 59).unwrap_err();
        assert!(err.to_string().contains("step value cannot be 0"));
    }

    #[test]
    fn test_parse_field_invalid_step() {
        let err = parse_field("*/abc", 0, 59).unwrap_err();
        assert!(err.to_string().contains("invalid step"));
    }

    #[test]
    fn test_parse_field_single_valid_value() {
        let result = parse_field("5", 0, 59).unwrap();
        assert_eq!(result, vec![5]);
    }

    #[test]
    fn test_parse_field_comma_list_with_duplicates() {
        let result = parse_field("1,3,1,5,3", 0, 59).unwrap();
        assert_eq!(result, vec![1, 3, 5]);
    }

    #[test]
    fn test_parse_field_large_step() {
        let result = parse_field("*/30", 0, 59).unwrap();
        assert_eq!(result, vec![0, 30]);
    }

    // --- CronSchedule::parse edge cases ---

    #[test]
    fn test_parse_too_few_fields() {
        let result = CronSchedule::parse("0 8 *");
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("must have 5 fields"));
    }

    #[test]
    fn test_parse_too_many_fields() {
        let result = CronSchedule::parse("0 8 * * * *");
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("must have 5 fields"));
    }

    #[test]
    fn test_parse_empty_string() {
        let result = CronSchedule::parse("");
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("must have 5 fields"));
    }

    // --- next_from edge cases ---

    #[test]
    fn test_next_from_year_boundary() {
        let cron = CronSchedule::parse("0 8 1 1 *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 12, 31)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_from_month_boundary() {
        let cron = CronSchedule::parse("0 8 1 * *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 31)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 2, 1)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_from_leap_year_feb_29() {
        let cron = CronSchedule::parse("0 8 29 2 *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 2, 29)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_from_non_leap_year_feb_29() {
        // 2025 is not a leap year; next leap year is 2028 — but that's
        // beyond the 366-day search limit, so we expect None.
        let cron = CronSchedule::parse("0 8 29 2 *").unwrap();
        let from = NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let next = cron.next_from(from);
        assert!(next.is_none());
    }

    #[test]
    fn test_next_from_specific_day_of_week() {
        // 2024-01-01 is a Monday. After 09:00, the next Monday 08:00 is Jan 8.
        let cron = CronSchedule::parse("0 8 * * 1").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 1, 8)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_from_same_minute_skips_current() {
        // */5 at 10:05 should give 10:10 (not 10:05, since it adds 1 minute first)
        let cron = CronSchedule::parse("*/5 * * * *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(10, 5, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(10, 10, 0)
            .unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_from_near_midnight() {
        let cron = CronSchedule::parse("0 0 * * *").unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(23, 59, 0)
            .unwrap();
        let next = cron.next_from(from).unwrap();
        let expected = NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        assert_eq!(next, expected);
    }
}
