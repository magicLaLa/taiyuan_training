/// 将自纪元以来的天数转换为年份（1970-2100 范围）
pub fn days_since_epoch_to_year(days: i64) -> i64 {
    let mut remaining = days;
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
        if year > 2100 {
            break;
        }
    }
    year
}

/// 判断闰年
pub fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// 获取指定年份起始的 Unix 时间戳（秒）
pub fn year_start_timestamp(year: i64) -> i64 {
    let mut days = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    days * 86400
}
