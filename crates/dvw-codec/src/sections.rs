//! Section tokeniser: splits a file into `[3NAME]` sections of raw lines.
//!
//! Works on bytes, before any decoding, so it is lossless for every input:
//! `RawFile::parse(b).to_bytes() == b` for all `b`. Each line keeps its own
//! terminator, so mixed line endings and a missing final newline survive.
//! Sections are kept in file order whether or not later stages know them.

/// How a line ended in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    CrLf,
    Lf,
    Cr,
    /// Last line of a file with no trailing newline.
    None,
}

impl LineEnding {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            LineEnding::CrLf => b"\r\n",
            LineEnding::Lf => b"\n",
            LineEnding::Cr => b"\r",
            LineEnding::None => b"",
        }
    }
}

/// One line without its terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub content: Vec<u8>,
    pub ending: LineEnding,
}

/// A `[3NAME]` header and the lines up to the next header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Name between `[` and `]`, e.g. `3SCOUT`.
    pub name: String,
    pub header: Line,
    pub lines: Vec<Line>,
}

/// A whole file as sections, plus any lines before the first header.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawFile {
    pub preamble: Vec<Line>,
    pub sections: Vec<Section>,
}

impl RawFile {
    pub fn parse(input: &[u8]) -> Self {
        let mut file = RawFile::default();
        for line in split_lines(input) {
            if let Some(name) = section_name(&line.content) {
                file.sections.push(Section {
                    name,
                    header: line,
                    lines: Vec::new(),
                });
            } else if let Some(section) = file.sections.last_mut() {
                section.lines.push(line);
            } else {
                file.preamble.push(line);
            }
        }
        file
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let lines = self.preamble.iter().chain(
            self.sections
                .iter()
                .flat_map(|s| std::iter::once(&s.header).chain(&s.lines)),
        );
        for line in lines {
            out.extend_from_slice(&line.content);
            out.extend_from_slice(line.ending.as_bytes());
        }
        out
    }

    /// 1-based line number of the first `[NAME]` header.
    pub fn header_line(&self, name: &str) -> Option<usize> {
        let mut line = self.preamble.len();
        for section in &self.sections {
            line += 1;
            if section.name == name {
                return Some(line);
            }
            line += section.lines.len();
        }
        None
    }

    /// First section with this name, e.g. `file.section("3SCOUT")`.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }
}

fn split_lines(input: &[u8]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < input.len() {
        let ending = match (input[i], input.get(i + 1)) {
            (b'\r', Some(b'\n')) => LineEnding::CrLf,
            (b'\r', _) => LineEnding::Cr,
            (b'\n', _) => LineEnding::Lf,
            _ => {
                i += 1;
                continue;
            }
        };
        lines.push(Line {
            content: input[start..i].to_vec(),
            ending,
        });
        i += ending.as_bytes().len();
        start = i;
    }
    if start < input.len() {
        lines.push(Line {
            content: input[start..].to_vec(),
            ending: LineEnding::None,
        });
    }
    lines
}

/// A header is a whole line `[3NAME]` where NAME is printable ASCII.
fn section_name(line: &[u8]) -> Option<String> {
    let inner = line.strip_prefix(b"[")?.strip_suffix(b"]")?;
    let printable = |b: &u8| b.is_ascii_graphic() && *b != b'[' && *b != b']';
    if inner.first() != Some(&b'3') || !inner.iter().all(printable) {
        return None;
    }
    String::from_utf8(inner.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn names(file: &RawFile) -> Vec<&str> {
        file.sections.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn splits_sections_in_order() {
        let input = b"[3DATAVOLLEYSCOUT]\r\nFILEFORMAT: 2.0\r\n[3SCOUT]\r\n*13SH#\r\n";
        let file = RawFile::parse(input);
        assert_eq!(names(&file), ["3DATAVOLLEYSCOUT", "3SCOUT"]);
        assert_eq!(file.section("3SCOUT").map(|s| s.lines.len()), Some(1));
        assert_eq!(file.header_line("3DATAVOLLEYSCOUT"), Some(1));
        assert_eq!(file.header_line("3SCOUT"), Some(3));
        assert_eq!(file.header_line("3NOPE"), None);
        assert_eq!(file.to_bytes(), input);
    }

    #[test]
    fn keeps_unknown_sections_and_preamble() {
        let input = b"junk\n[3SCOUT]\na\n[3FUTURETHING]\nx;y\n\n[3VIDEO]\n";
        let file = RawFile::parse(input);
        assert_eq!(file.preamble.len(), 1);
        assert_eq!(names(&file), ["3SCOUT", "3FUTURETHING", "3VIDEO"]);
        assert_eq!(file.to_bytes(), input);
    }

    #[test]
    fn keeps_mixed_endings_and_missing_final_newline() {
        let input = b"[3A]\r\na\nb\rc";
        let file = RawFile::parse(input);
        let endings: Vec<_> = file.sections[0].lines.iter().map(|l| l.ending).collect();
        assert_eq!(endings, [LineEnding::Lf, LineEnding::Cr, LineEnding::None]);
        assert_eq!(file.to_bytes(), input);
    }

    #[test]
    fn non_header_brackets_are_content() {
        for line in [
            &b"[3SCOUT] "[..],
            b" [3SCOUT]",
            b"[SCOUT]",
            b"[3]x",
            b"[3A B]",
        ] {
            assert_eq!(
                section_name(line),
                None,
                "{:?}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn empty_input() {
        assert_eq!(RawFile::parse(b""), RawFile::default());
    }

    proptest! {
        #[test]
        fn round_trips_any_bytes(input in proptest::collection::vec(any::<u8>(), 0..512)) {
            prop_assert_eq!(RawFile::parse(&input).to_bytes(), input);
        }

        #[test]
        fn round_trips_line_soup(
            lines in proptest::collection::vec(
                prop_oneof!["\\[3[A-Z]{1,8}\\]", "[ -~]{0,20}"], 0..40),
            seps in proptest::collection::vec(prop_oneof!["\r\n", "\n", "\r"], 40),
        ) {
            let input: String = lines.iter().zip(&seps).map(|(l, s)| format!("{l}{s}")).collect();
            prop_assert_eq!(RawFile::parse(input.as_bytes()).to_bytes(), input.as_bytes());
        }
    }
}
