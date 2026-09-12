//! Conversions between seconds and "hh:mm:ss.fff" timecode strings.

/// Formats a duration in seconds as "hh:mm:ss.fff".
pub fn seconds_to_timecode(total_seconds: f64) -> String {
    let total_seconds = total_seconds.max(0.0);
    let total_millis = (total_seconds * 1000.0).round() as i64;

    let hours = total_millis / (3_600_000);
    let minutes = (total_millis / 60_000) % 60;
    let seconds = (total_millis / 1000) % 60;
    let millis = total_millis % 1000;

    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Parses a "hh:mm:ss.fff" (or "mm:ss", "ss") timecode string into seconds.
pub fn timecode_to_seconds(timecode: &str) -> Option<f64> {
    let timecode = timecode.trim();
    if timecode.is_empty() {
        return None;
    }

    let (main, millis) = match timecode.split_once('.') {
        Some((m, f)) => {
            let mut frac = f.to_string();
            frac.truncate(3);
            while frac.len() < 3 {
                frac.push('0');
            }
            (m, frac.parse::<f64>().unwrap_or(0.0) / 1000.0)
        }
        None => (timecode, 0.0),
    };

    let parts: Vec<&str> = main.split(':').collect();
    let seconds = match parts.as_slice() {
        [h, m, s] => {
            let h: f64 = h.parse().ok()?;
            let m: f64 = m.parse().ok()?;
            let s: f64 = s.parse().ok()?;
            h * 3600.0 + m * 60.0 + s
        }
        [m, s] => {
            let m: f64 = m.parse().ok()?;
            let s: f64 = s.parse().ok()?;
            m * 60.0 + s
        }
        [s] => s.parse().ok()?,
        _ => return None,
    };

    Some(seconds + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_basic_values() {
        assert_eq!(seconds_to_timecode(0.0), "00:00:00.000");
        assert_eq!(seconds_to_timecode(61.5), "00:01:01.500");
        assert_eq!(seconds_to_timecode(3661.25), "01:01:01.250");
    }

    #[test]
    fn parses_full_timecode() {
        assert_eq!(timecode_to_seconds("00:01:01.500"), Some(61.5));
        assert_eq!(timecode_to_seconds("01:01:01.250"), Some(3661.25));
    }

    #[test]
    fn parses_short_forms() {
        assert_eq!(timecode_to_seconds("61.5"), Some(61.5));
        assert_eq!(timecode_to_seconds("01:01.5"), Some(61.5));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(timecode_to_seconds(""), None);
        assert_eq!(timecode_to_seconds("not a timecode"), None);
    }
}
