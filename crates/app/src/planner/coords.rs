//! Coordinates typed or pasted in the ways people write them: decimal degrees (`40.4168, -3.7038`), with
//! hemisphere letters (`40.4168 N 3.7038 W`) or in degrees, minutes and seconds (`40°25'0.5"N 3°42'13.7"W`).

/// Reads a latitude and a longitude from text, or `None` when it is not one.
pub(super) fn parse(text: &str) -> Option<(f64, f64)> {
    let t = text.trim();
    if t.is_empty() || t.len() > 120 {
        return None;
    }
    // split into the two halves: at a comma/semicolon, or after the first hemisphere letter
    let normalized: String = t.replace(['º', '˚'], "°").replace(['′', '’'], "'").replace(['″', '”', '“'], "\"");
    let parts: Vec<&str> = if let Some((a, b)) = normalized.split_once([',', ';']) {
        vec![a, b]
    } else if let Some(i) = normalized.find(['N', 'S', 'n', 's']).filter(|i| *i + 1 < normalized.len()) {
        let (a, b) = normalized.split_at(i + 1);
        vec![a, b]
    } else {
        let words: Vec<&str> = normalized.split_whitespace().collect();
        if words.len() == 2 { words } else { return None }
    };
    let [a, b] = parts[..] else { return None };
    let lat = angle(a, ['N', 'S'])?;
    let lon = angle(b, ['E', 'W'])?;
    (lat.abs() <= 90.0 && lon.abs() <= 180.0).then_some((lat, lon))
}

/// One angle: degrees with optional minutes and seconds and a hemisphere letter (the south and the west are negative).
fn angle(text: &str, hemis: [char; 2]) -> Option<f64> {
    let mut sign: f64 = 1.0;
    let mut numbers: Vec<f64> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, numbers: &mut Vec<f64>| -> Option<()> {
        if !cur.is_empty() {
            numbers.push(cur.replace(',', ".").parse::<f64>().ok()?);
            cur.clear();
        }
        Some(())
    };
    for c in text.chars() {
        match c {
            '0'..='9' | '.' => cur.push(c),
            '-' | '+' if cur.is_empty() && numbers.is_empty() => {
                if c == '-' {
                    sign = -1.0;
                }
            }
            c if c.is_whitespace() || matches!(c, '°' | '\'' | '"') => flush(&mut cur, &mut numbers)?,
            c if c.eq_ignore_ascii_case(&hemis[0]) => flush(&mut cur, &mut numbers)?,
            c if c.eq_ignore_ascii_case(&hemis[1]) => {
                flush(&mut cur, &mut numbers)?;
                sign = -sign.abs();
            }
            // the other axis' letters and the word "O" for oeste (Spanish)
            'O' | 'o' if hemis[1] == 'W' => {
                flush(&mut cur, &mut numbers)?;
                sign = -sign.abs();
            }
            _ => return None,
        }
    }
    flush(&mut cur, &mut numbers)?;
    let (d, m, s) = match numbers[..] {
        [d] => (d, 0.0, 0.0),
        [d, m] => (d, m, 0.0),
        [d, m, s] => (d, m, s),
        _ => return None,
    };
    if !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&s) {
        return None;
    }
    Some(sign * (d + m / 60.0 + s / 3600.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Option<(f64, f64)>, lat: f64, lon: f64) -> bool {
        a.is_some_and(|(x, y)| (x - lat).abs() < 1e-4 && (y - lon).abs() < 1e-4)
    }

    #[test]
    fn reads_the_usual_ways_of_writing_a_place() {
        assert!(close(parse("40.4168, -3.7038"), 40.4168, -3.7038));
        assert!(close(parse("40.4168 N 3.7038 W"), 40.4168, -3.7038));
        assert!(close(parse("40°25'0.5\"N 3°42'13.7\"W"), 40.416_805_6, -3.703_805_6));
        assert!(close(parse("33°52'S, 151°12'E"), -33.866_667, 151.2));
        assert!(close(parse("41.38 2.17"), 41.38, 2.17));
        assert!(close(parse("40° 25' N, 3° 42' O"), 40.416_667, -3.7));
    }

    #[test]
    fn refuses_what_is_not_a_place() {
        for bad in ["", "hello", "91, 0", "0, 181", "40.4", "40°70'N 3°W", "1,2,3", "N N"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert!(parse(&"9".repeat(500)).is_none());
    }
}
