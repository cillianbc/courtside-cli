//! Field-level helpers shared by the typed sections.

/// One `;`-separated row, split losslessly: `fields.join(";")` is the line.
/// A trailing `;` gives a final empty field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row(pub Vec<String>);

impl Row {
    pub fn parse(line: &str) -> Self {
        Row(line.split(';').map(str::to_owned).collect())
    }

    pub fn to_line(&self) -> String {
        self.0.join(";")
    }

    /// Field `i`, or `""` if the row is shorter.
    pub fn get(&self, i: usize) -> &str {
        self.0.get(i).map_or("", String::as_str)
    }

    /// Field `i` parsed as a number, ignoring surrounding spaces.
    pub fn int<T: std::str::FromStr>(&self, i: usize) -> Option<T> {
        self.get(i).trim().parse().ok()
    }

    /// Field `i` as `True`/`False`.
    pub fn bool(&self, i: usize) -> Option<bool> {
        match self.get(i) {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        }
    }

    /// Plain text in field `plain`, unless field `embedded` holds a
    /// non-empty DataVolley 4 Unicode copy (see [`decode_embedded`]), which
    /// wins. Same precedence as openvolley.
    pub fn text(&self, plain: usize, embedded: usize) -> String {
        decode_embedded(self.get(embedded))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.get(plain).to_owned())
    }
}

/// Decodes DataVolley 4's embedded Unicode text: `\x0F`, a digit `n` (2 or
/// 4), then `n` hex digits per character as a Unicode code point. `None`
/// if `field` is not in that form.
pub fn decode_embedded(field: &str) -> Option<String> {
    let rest = field.strip_prefix('\u{0F}')?;
    let mut chars = rest.chars();
    let width = chars.next()?.to_digit(10)? as usize;
    let hex = chars.as_str();
    if !matches!(width, 2 | 4) || hex.len() % width != 0 || !hex.is_ascii() {
        return None;
    }
    (0..hex.len())
        .step_by(width)
        .map(|i| {
            u32::from_str_radix(&hex[i..i + width], 16)
                .ok()
                .and_then(char::from_u32)
        })
        .collect()
}

/// The inverse of [`decode_embedded`]: two hex digits per character when
/// every code point fits (as DataVolley writes), else four. Characters
/// beyond four hex digits become `?`.
pub fn encode_embedded(text: &str) -> String {
    let wide = text.chars().any(|c| u32::from(c) > 0xFF);
    let mut out = String::from(if wide { "\u{0F}4" } else { "\u{0F}2" });
    for c in text.chars() {
        let n = u32::from(c);
        let n = if n > 0xFFFF { u32::from('?') } else { n };
        if wide {
            out.push_str(&format!("{n:04X}"));
        } else {
            out.push_str(&format!("{n:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_is_lossless() {
        for line in ["", ";", "a;b;", " 7- 8;x\t;;", "no separators"] {
            assert_eq!(Row::parse(line).to_line(), line);
        }
    }

    #[test]
    fn row_accessors() {
        let row = Row::parse("True; 8 ;x;");
        assert_eq!(row.bool(0), Some(true));
        assert_eq!(row.int::<u8>(1), Some(8));
        assert_eq!(row.int::<u8>(2), None);
        assert_eq!(row.get(3), "");
        assert_eq!(row.get(99), "");
    }

    #[test]
    fn embedded_text() {
        assert_eq!(
            decode_embedded("\u{0F}253696F6268E16E").as_deref(),
            Some("Siobhán")
        );
        assert_eq!(decode_embedded("\u{0F}40141015B").as_deref(), Some("Łś"));
        assert_eq!(decode_embedded("\u{0F}2").as_deref(), Some(""));
        assert_eq!(decode_embedded("plain"), None);
        assert_eq!(decode_embedded("\u{0F}3ABC"), None);
        assert_eq!(decode_embedded("\u{0F}2ABC"), None);
        for text in ["Siobhán", "Łś", "", "Plain"] {
            assert_eq!(
                decode_embedded(&encode_embedded(text)).as_deref(),
                Some(text)
            );
        }
        assert_eq!(encode_embedded("Siobhán"), "\u{0F}253696F6268E16E");
    }

    #[test]
    fn text_prefers_embedded_copy() {
        let row = Row::parse("Siobh?n;\u{0F}253696F6268E16E");
        assert_eq!(row.text(0, 1), "Siobhán");
        assert_eq!(Row::parse("Plain;").text(0, 1), "Plain");
        assert_eq!(Row::parse("Plain;\u{0F}2").text(0, 1), "Plain");
    }
}
