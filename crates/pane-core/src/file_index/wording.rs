//! Counts, sizes and spans of time as file search says them to people: on
//! the File search page, in the index's status, and in Search Files'
//! Metadata.

use std::time::Duration;

/// `count` for people: "5 million", "1,000", "12,345".
pub fn count_words(count: u64) -> String {
    if count >= 1_000_000 && count.is_multiple_of(1_000_000) {
        return format!("{} million", count / 1_000_000);
    }
    let digits = count.to_string();
    let mut grouped = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// A size for people, in the decimal units macOS's Finder and the Linux
/// file managers use for documents (1 KB is 1,000 bytes), the one
/// convention Pane writes sizes in, Search Files' Metadata and the File
/// search page alike: "0 bytes", "1 byte", "532 bytes", "12.3 KB",
/// "1.2 MB", "1 GB", "3.4 GB".
pub fn size_words(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    match bytes {
        1 => return "1 byte".into(),
        0..1000 => return format!("{bytes} bytes"),
        _ => {}
    }
    let mut value = bytes as f64 / 1000.;
    let mut unit = 0;
    while value >= 999.95 && unit + 1 < UNITS.len() {
        value /= 1000.;
        unit += 1;
    }
    let shown = if value >= 100. {
        format!("{value:.0}")
    } else {
        let tenths = format!("{value:.1}");
        tenths
            .strip_suffix(".0")
            .map(str::to_owned)
            .unwrap_or(tenths)
    };
    format!("{shown} {}", UNITS[unit])
}

/// `span` for people: "1 minute", "10 seconds", "200 ms".
pub(crate) fn span_words(span: Duration) -> String {
    let plural = |count: u64, unit: &str| {
        if count == 1 {
            format!("1 {unit}")
        } else {
            format!("{count} {unit}s")
        }
    };
    let seconds = span.as_secs();
    if span.subsec_nanos() == 0 && seconds >= 60 && seconds.is_multiple_of(60) {
        plural(seconds / 60, "minute")
    } else if span.subsec_nanos() == 0 && seconds > 0 {
        plural(seconds, "second")
    } else {
        format!("{} ms", span.as_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_the_file_managers_say_them() {
        assert_eq!(size_words(0), "0 bytes");
        assert_eq!(size_words(1), "1 byte");
        assert_eq!(size_words(532), "532 bytes");
        assert_eq!(size_words(1000), "1 KB");
        assert_eq!(size_words(12_345), "12.3 KB");
        assert_eq!(size_words(1_200_000), "1.2 MB");
        assert_eq!(size_words(999_999), "1 MB");
        assert_eq!(size_words(345_000_000), "345 MB");
        assert_eq!(size_words(1_000_000_000), "1 GB");
        assert_eq!(size_words(3_400_000_000), "3.4 GB");
    }

    #[test]
    fn counts_and_spans_read_as_people_say_them() {
        assert_eq!(count_words(5_000_000), "5 million");
        assert_eq!(count_words(12_345), "12,345");
        assert_eq!(count_words(999), "999");
        assert_eq!(span_words(Duration::from_secs(60)), "1 minute");
        assert_eq!(span_words(Duration::from_secs(10)), "10 seconds");
        assert_eq!(span_words(Duration::from_millis(200)), "200 ms");
    }
}
