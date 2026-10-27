use serde::{Deserialize, Serialize};

/// Display units; everything is stored in mm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Mm,
    Inch,
}

const MM_PER_INCH: f64 = 25.4;

pub fn to_display(mm: f64, u: Units) -> f64 {
    match u {
        Units::Mm => mm,
        Units::Inch => mm / MM_PER_INCH,
    }
}

pub fn from_display(v: f64, u: Units) -> f64 {
    match u {
        Units::Mm => v,
        Units::Inch => v * MM_PER_INCH,
    }
}

/// Read-only length in display units: 3 decimals for mm, 4 for inch.
pub fn fmt_len(mm: f64, u: Units) -> String {
    match u {
        Units::Mm => format!("{mm:.3} mm"),
        Units::Inch => format!("{:.4} in", to_display(mm, u)),
    }
}

fn parse_decimal(s: &str) -> Option<f64> {
    let v: f64 = s.replace(',', ".").parse().ok()?;
    v.is_finite().then_some(v)
}

/// "a/b" or a decimal.
fn parse_simple(s: &str) -> Option<f64> {
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b) = (parse_decimal(a)?, parse_decimal(b)?);
            (b != 0.0).then_some(a / b)
        }
        None => parse_decimal(s),
    }
}

/// User length input → mm. Accepts `6.35`, `6,35`, `1/4`, `1 1/2`, with optional `mm`, `in` or `"` suffix.
pub fn parse_length(s: &str, default: Units) -> Option<f64> {
    let lower = s.trim().to_lowercase();
    let (num, units) = if let Some(n) = lower.strip_suffix("mm") {
        (n, Units::Mm)
    } else if let Some(n) = lower.strip_suffix("in").or_else(|| lower.strip_suffix('"')) {
        (n, Units::Inch)
    } else {
        (lower.as_str(), default)
    };
    let parts: Vec<&str> = num.split_whitespace().collect();
    let v = match parts.as_slice() {
        [single] => parse_simple(single)?,
        [whole, frac] if frac.contains('/') => {
            let (w, f) = (parse_decimal(whole)?, parse_simple(frac)?);
            if w < 0.0 || whole.starts_with('-') {
                w - f
            } else {
                w + f
            }
        }
        _ => return None,
    };
    v.is_finite().then(|| from_display(v, units))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negatives_and_spaces() {
        assert_eq!(parse_length(" -2 mm ", Units::Inch), Some(-2.0));
        assert_eq!(parse_length("-1 1/2", Units::Mm), Some(-1.5));
        assert_eq!(parse_length("2 IN", Units::Mm), Some(50.8));
        assert_eq!(parse_length("inf", Units::Mm), None);
        assert_eq!(parse_length("1 2", Units::Mm), None);
        assert_eq!(from_display(1.0, Units::Inch), 25.4);
    }

    #[test]
    fn fmt_len_units() {
        assert_eq!(fmt_len(15.0, Units::Mm), "15.000 mm");
        assert_eq!(fmt_len(25.4, Units::Inch), "1.0000 in");
    }
}
