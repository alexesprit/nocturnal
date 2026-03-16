use chrono::DateTime;

pub fn format_date(s: &str) -> askama::Result<String> {
    if s.is_empty() {
        return Ok(String::new());
    }
    match DateTime::parse_from_rfc3339(s) {
        Ok(dt) => Ok(dt.format("%b %-d, %Y").to_string()),
        Err(_) => Ok(s.to_string()),
    }
}

pub fn format_datetime(s: &str) -> askama::Result<String> {
    if s.is_empty() {
        return Ok(String::new());
    }
    match DateTime::parse_from_rfc3339(s) {
        Ok(dt) => Ok(dt.format("%b %-d, %Y %H:%M").to_string()),
        Err(_) => Ok(s.to_string()),
    }
}
