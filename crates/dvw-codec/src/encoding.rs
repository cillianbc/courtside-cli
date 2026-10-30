//! Text encodings of `.dvw` files.
//!
//! `[3MATCH]` field 9 holds a Windows code page number (`1252` in every
//! corpus file so far). Decoding must be reversible: a file only parses if
//! re-encoding the decoded text gives back the same bytes.

use encoding_rs::Encoding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEncoding(&'static Encoding);

impl TextEncoding {
    pub const WINDOWS_1252: TextEncoding = TextEncoding(encoding_rs::WINDOWS_1252);
    pub const UTF_8: TextEncoding = TextEncoding(encoding_rs::UTF_8);

    /// From a Windows code page number as written in `[3MATCH]`.
    pub fn from_codepage(codepage: &str) -> Option<Self> {
        let encoding = match codepage.trim() {
            "1250" => encoding_rs::WINDOWS_1250,
            "1251" => encoding_rs::WINDOWS_1251,
            "1252" => encoding_rs::WINDOWS_1252,
            "1253" => encoding_rs::WINDOWS_1253,
            "1254" => encoding_rs::WINDOWS_1254,
            "1255" => encoding_rs::WINDOWS_1255,
            "1256" => encoding_rs::WINDOWS_1256,
            "1257" => encoding_rs::WINDOWS_1257,
            "1258" => encoding_rs::WINDOWS_1258,
            "874" => encoding_rs::WINDOWS_874,
            "932" => encoding_rs::SHIFT_JIS,
            "936" => encoding_rs::GBK,
            "949" => encoding_rs::EUC_KR,
            "950" => encoding_rs::BIG5,
            "28592" => encoding_rs::ISO_8859_2,
            "28605" => encoding_rs::ISO_8859_15,
            "65001" => encoding_rs::UTF_8,
            _ => return None,
        };
        Some(TextEncoding(encoding))
    }

    /// The likeliest encoding of a file that doesn't name one (code page
    /// empty, or `1`, Windows' "PC default" that DataVolley 3.x writes):
    /// UTF-8 when the bytes are valid UTF-8 (VolleyMetrics Portal files),
    /// Shift_JIS when many high bytes are in 0x81–0x9F (lead bytes, rare in
    /// Western text) and it decodes cleanly, Windows-1251 when most high
    /// bytes are letters in whole-word runs of three or more (Cyrillic
    /// names), Windows-1250 when a byte inside a word is a Central European
    /// letter there but a symbol or nothing in 1252 (`Ź` 0x8F, `ł` `³`,
    /// `ż` `¿`, …) or a word ends in `Æ`/`æ` (the `-IĆ` of a surname), else
    /// Windows-1252.
    pub fn guess(bytes: &[u8]) -> Self {
        let high = bytes.iter().filter(|b| **b >= 0x80).count();
        if high == 0 {
            return Self::WINDOWS_1252;
        }
        if std::str::from_utf8(bytes).is_ok() {
            return Self::UTF_8;
        }
        let leads = bytes.iter().filter(|b| (0x81..=0x9f).contains(*b)).count();
        let sjis = TextEncoding(encoding_rs::SHIFT_JIS);
        if leads * 4 >= high && sjis.decode(bytes).is_some() {
            return sjis;
        }
        let mut in_words = 0;
        for run in bytes.split(|b| *b < 0x80) {
            if run.len() >= 3 && run.iter().all(|b| *b >= 0xc0) {
                in_words += run.len();
            }
        }
        if in_words * 2 > high {
            return TextEncoding(encoding_rs::WINDOWS_1251);
        }
        let cp1250 = TextEncoding(encoding_rs::WINDOWS_1250);
        if central_european(bytes) && cp1250.decode(bytes).is_some() {
            return cp1250;
        }
        Self::WINDOWS_1252
    }

    /// From a WHATWG label such as `windows-1250` or `iso-8859-2`.
    pub fn from_label(label: &str) -> Option<Self> {
        Encoding::for_label(label.as_bytes()).map(TextEncoding)
    }

    pub fn name(self) -> &'static str {
        self.0.name()
    }

    /// Decodes `bytes`; `None` unless encoding the result reproduces them.
    pub fn decode(self, bytes: &[u8]) -> Option<String> {
        let text = self
            .0
            .decode_without_bom_handling_and_without_replacement(bytes)?
            .into_owned();
        (self.encode(&text)? == bytes).then_some(text)
    }

    /// Encodes `text`; `None` if the encoding cannot represent it.
    pub fn encode(self, text: &str) -> Option<Vec<u8>> {
        let (bytes, _, unmappable) = self.0.encode(text);
        (!unmappable).then(|| bytes.into_owned())
    }
}

/// Bytes that are letters in Windows-1250 but symbols, or unassigned, in
/// Windows-1252: Ť Ź ť Ł Ą Ż ł ą Ľ ľ ż.
const LETTERS_ONLY_IN_1250: [u8; 11] = [
    0x8d, 0x8f, 0x9d, 0xa3, 0xa5, 0xaf, 0xb3, 0xb9, 0xbc, 0xbe, 0xbf,
];

/// Whether the text reads as Central European: a 1250-only letter next to
/// another letter, or a word ending in 0xC6/0xE6 (`Ć`/`ć` in 1250, `Æ`/`æ`
/// in 1252, which hardly ends a word).
fn central_european(bytes: &[u8]) -> bool {
    let letter = |b: u8| b.is_ascii_alphabetic() || b >= 0xc0;
    bytes.windows(3).any(|w| {
        let [before, b, after] = [w[0], w[1], w[2]];
        (LETTERS_ONLY_IN_1250.contains(&b) && (letter(before) || letter(after)))
            || (matches!(b, 0xc6 | 0xe6) && letter(before) && !letter(after))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made-up names in each script, as the files that don't say their
    /// code page hold them.
    #[test]
    fn guesses_an_undeclared_encoding() {
        let enc = |e: &'static Encoding, s: &str| e.encode(s).0.into_owned();
        assert_eq!(
            TextEncoding::guess(b"Smith;Jones"),
            TextEncoding::WINDOWS_1252
        );
        assert_eq!(
            TextEncoding::guess("Müller;Bäcker".as_bytes()),
            TextEncoding::UTF_8
        );
        let ja = enc(encoding_rs::SHIFT_JIS, "0;12;山田;花子;\r\n0;7;佐藤;太郎;");
        assert_eq!(TextEncoding::guess(&ja).name(), "Shift_JIS");
        let ru = enc(
            encoding_rs::WINDOWS_1251,
            "0;5;Иванова;Мария;\r\n0;9;Петрова;Анна;",
        );
        assert_eq!(TextEncoding::guess(&ru).name(), "windows-1251");
        let fr = enc(
            encoding_rs::WINDOWS_1252,
            "0;5;Lefèvre;Hélène;\r\n0;9;Müller;Zoë;",
        );
        assert_eq!(TextEncoding::guess(&fr), TextEncoding::WINDOWS_1252);
        let hr = enc(
            encoding_rs::WINDOWS_1250,
            "0;5;BRODARIĆ;Tiha;\r\n0;9;Kovel;Ostrina;",
        );
        assert_eq!(TextEncoding::guess(&hr).name(), "windows-1250");
        let pl = enc(
            encoding_rs::WINDOWS_1250,
            "0;5;Wróbel;Łucja;\r\n0;9;Kowalczyk;Źaneta;",
        );
        assert_eq!(TextEncoding::guess(&pl).name(), "windows-1250");
        // Danish and Norwegian letters stay 1252.
        let da = enc(
            encoding_rs::WINDOWS_1252,
            "0;5;Sæther;Bjørn;\r\n0;9;Løkke;Åse;",
        );
        assert_eq!(TextEncoding::guess(&da), TextEncoding::WINDOWS_1252);
    }

    #[test]
    fn windows_1252_round_trips_every_byte() {
        let bytes: Vec<u8> = (0..=255).collect();
        let text = TextEncoding::WINDOWS_1252.decode(&bytes).unwrap();
        assert_eq!(TextEncoding::WINDOWS_1252.encode(&text).unwrap(), bytes);
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert!(TextEncoding::UTF_8.decode(&[0x41, 0xED, 0x6E]).is_none());
    }

    #[test]
    fn rejects_unrepresentable_text() {
        assert!(TextEncoding::WINDOWS_1252.encode("Łódź").is_none());
        assert!(
            TextEncoding::from_codepage("1250")
                .unwrap()
                .encode("Łódź")
                .is_some()
        );
    }

    #[test]
    fn unknown_codepage() {
        assert_eq!(TextEncoding::from_codepage("9999"), None);
    }
}
