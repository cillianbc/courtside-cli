//! Validation (`courtside validate`): file-level checks (sections, team sheets,
//! round trip), then vb-engine's match rules on the `[3SCOUT]` events.

use std::fmt;
use std::path::Path;

use dvw_codec::date::{Date, Written};
use dvw_codec::text::decode_embedded;
use dvw_codec::{Dvw, RawVsm};
use vb_engine::validate::{Rule, Severity};
use vb_engine::{IdGen, Match};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// The match rule, for issues from vb-engine; `None` for file-level ones.
    pub rule: Option<Rule>,
    /// 1-based file line, when the issue has one.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "{line}: ")?;
        }
        write!(f, "{}: ", severity_name(self.severity))?;
        if let Some(rule) = self.rule {
            write!(f, "{}: ", rule_name(rule))?;
        }
        f.write_str(&self.message)
    }
}

pub fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

pub fn rule_name(r: Rule) -> &'static str {
    match r {
        Rule::Score => "score",
        Rule::Rotation => "rotation",
        Rule::BackRow => "back row",
        Rule::Substitution => "substitution",
        Rule::ServeReception => "serve/reception",
        Rule::Code => "code",
        Rule::Roles => "roles",
    }
}

/// Issues as a JSON array of `{severity, rule, line, message}`; `rule` and
/// `line` are `null` when the issue has none.
pub fn to_json(issues: &[Issue]) -> serde_json::Value {
    issues
        .iter()
        .map(|i| {
            serde_json::json!({
                "severity": severity_name(i.severity),
                "rule": i.rule.map(rule_name),
                "line": i.line,
                "message": i.message,
            })
        })
        .collect()
}

/// Whether a name's plain column matches its Unicode copy. A letter the
/// file's code page can't hold DataVolley writes as its base letter in
/// the plain column (`C` for `Č`, `e` for `ě`), or as `?`, and the true
/// letter only in the copy.
fn same_name(plain: &str, copy: &str, encoding: dvw_codec::TextEncoding) -> bool {
    use unicode_normalization::UnicodeNormalization;
    if plain == copy {
        return true;
    }
    let (p, c): (Vec<char>, Vec<char>) = (plain.chars().collect(), copy.chars().collect());
    p.len() == c.len()
        && p.iter().zip(&c).all(|(p, c)| {
            p == c
                || (encoding.encode(&c.to_string()).is_none()
                    && (*p == '?' || c.nfd().next() == Some(*p)))
        })
}

fn issue(severity: Severity, line: Option<usize>, message: impl Into<String>) -> Issue {
    Issue {
        severity,
        rule: None,
        line,
        message: message.into(),
    }
}

pub fn validate(path: &Path, bytes: &[u8]) -> Vec<Issue> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("dvw") => validate_dvw(bytes),
        Some("vsm") => validate_vsm(bytes),
        _ => vec![issue(
            Severity::Error,
            None,
            "unsupported file type; expected .dvw or .vsm",
        )],
    }
}

fn validate_vsm(bytes: &[u8]) -> Vec<Issue> {
    match RawVsm::parse(bytes) {
        Err(e) => vec![issue(Severity::Error, None, e.to_string())],
        Ok(vsm) if vsm.to_bytes() != bytes => vec![issue(
            Severity::Warning,
            None,
            "not in VolleyStation's compact JSON layout; re-saving will change formatting",
        )],
        Ok(_) => Vec::new(),
    }
}

fn validate_dvw(bytes: &[u8]) -> Vec<Issue> {
    let dvw = match Dvw::parse(bytes) {
        Ok(d) => d,
        Err(e) => return vec![issue(Severity::Error, None, e.to_string())],
    };
    let mut issues = Vec::new();
    let raw = dvw.raw();

    for name in [
        "3DATAVOLLEYSCOUT",
        "3MATCH",
        "3TEAMS",
        "3PLAYERS-H",
        "3PLAYERS-V",
        "3SCOUT",
    ] {
        if raw.section(name).is_none() {
            issues.push(issue(
                Severity::Error,
                None,
                format!("missing [{name}] section"),
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for section in &raw.sections {
        if !seen.insert(section.name.as_str()) {
            issues.push(issue(
                Severity::Warning,
                raw.header_line(&section.name),
                format!(
                    "[{}] appears more than once; only the first is read",
                    section.name
                ),
            ));
        }
    }

    match dvw.to_bytes() {
        Ok(out) if out == bytes => {}
        Ok(_) => issues.push(issue(
            Severity::Error,
            None,
            "file does not round-trip through Courtside; please report it with the file",
        )),
        Err(e) => issues.push(issue(Severity::Error, None, e.to_string())),
    }

    if dvw.teams.len() != 2 {
        issues.push(issue(
            Severity::Error,
            raw.header_line("3TEAMS"),
            format!("expected 2 teams, found {}", dvw.teams.len()),
        ));
    }
    let match_line = raw.header_line("3MATCH").map(|l| l + 1);
    if dvw.match_date().is_none() {
        let text = dvw.match_info.date_text();
        let message = match Date::parse_written(text) {
            Some(Written::Two {
                month_first,
                day_first,
            }) => format!(
                "match date {text:?} is {month_first} or {day_first}; nothing else in the file settles the day/month order"
            ),
            _ => format!("cannot determine the match date from {text:?}"),
        };
        issues.push(issue(Severity::Warning, match_line, message));
    }
    if dvw.match_info.codepage().trim() == "1" {
        issues.push(issue(
            Severity::Warning,
            match_line,
            format!(
                "code page 1 (the PC's default, written by DataVolley 3.x) read as {}, guessed from the text; check accented names",
                dvw.encoding.name()
            ),
        ));
    }

    for (section, roster) in [
        ("3PLAYERS-H", &dvw.players_home),
        ("3PLAYERS-V", &dvw.players_visiting),
    ] {
        let first = raw.header_line(section).map_or(0, |l| l + 1);
        let mut numbers = std::collections::HashSet::new();
        for (i, p) in roster.iter().enumerate() {
            let line = Some(first + i);
            match p.number() {
                None => issues.push(issue(
                    Severity::Error,
                    line,
                    "player without a shirt number",
                )),
                Some(n) if !numbers.insert(n) => issues.push(issue(
                    Severity::Error,
                    line,
                    format!("shirt number {n} listed twice"),
                )),
                Some(_) => {}
            }
            for (plain, embedded, what) in [
                (9, 17, "last name"),
                (10, 18, "first name"),
                (11, 19, "nickname"),
            ] {
                if let Some(text) = decode_embedded(p.0.get(embedded))
                    && !text.is_empty()
                    && !same_name(p.0.get(plain), &text, dvw.encoding)
                {
                    issues.push(issue(
                        Severity::Warning,
                        line,
                        format!(
                            "{what} differs from its Unicode copy (file edited by another program?)"
                        ),
                    ));
                }
            }
        }
    }

    // Match rules. Each event comes from one [3SCOUT] line.
    let first = raw.header_line("3SCOUT").map(|l| l + 1);
    let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
    let events = m.log.visible();
    let file_line = |index: usize| {
        events
            .get(index)
            .and_then(|e| e.source_line)
            .zip(first)
            .map(|(n, first)| first + n)
    };
    for i in vb_engine::validate::validate(&m) {
        let message = match i.cause.and_then(file_line) {
            Some(cause) => format!("{} (follows from line {cause})", i.message),
            None => i.message,
        };
        issues.push(Issue {
            severity: i.severity,
            rule: Some(i.rule),
            line: file_line(i.index),
            message,
        });
    }
    issues.sort_by_key(|i| (i.line.unwrap_or(0), std::cmp::Reverse(i.severity)));
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[u8] = b"[3DATAVOLLEYSCOUT]\nGENERATOR-PRG: Data Volley\n[3MATCH]\n04/23/2026;;;;;;;;1252;1;Z;\n[3TEAMS]\nT1;Home;0;;;;\nT2;Away;0;;;;\n[3PLAYERS-H]\n0;1;1;;;;;;H1;One;A;;;2;;\n[3PLAYERS-V]\n1;2;1;;;;;;V2;Two;B;;;2;;\n[3SCOUT]\n*01SH#;;;;;;;;;;;;;;;;;;;;;;;;;;\n";

    #[test]
    fn clean_file_has_no_issues() {
        assert_eq!(validate(Path::new("m.dvw"), GOOD), []);
    }

    #[test]
    fn reports_problems_with_lines() {
        let bad = b"[3DATAVOLLEYSCOUT]\n\
GENERATOR-PRG: Data Volley\n\
[3MATCH]\n\
04/23/2026;;;;;;;;1252;1;Z;\n\
[3TEAMS]\n\
T1;Home;0;;;;\n\
T2;Away;0;;;;\n\
[3PLAYERS-H]\n\
0;1;1;;;;;;H1;One;A;;;2;;\n\
0;1;2;;;;;;H2;Dup;C;;;2;;;;\x0f24F6E65;;\n\
[3PLAYERS-V]\n\
1;2;1;;;;;;V2;Two;B;;;2;;\n\
[3SCOUT]\n\
*01SH#;;;;;;;;;;;;;;;;;;;;;;;;;;\n\
*10X\n";
        let text: Vec<String> = validate(Path::new("m.DVW"), bad)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            text,
            [
                "10: error: shirt number 1 listed twice",
                "10: warning: last name differs from its Unicode copy (file edited by another program?)",
                "15: warning: code: unrecognised code \"*10X\"",
            ]
        );
    }

    /// A letter the code page can't hold: DataVolley writes its base
    /// letter in the plain column and the true one in the Unicode copy.
    #[test]
    fn a_best_fit_letter_matches_its_unicode_copy() {
        let file = b"[3DATAVOLLEYSCOUT]\n\
GENERATOR-PRG: Data Volley\n\
[3MATCH]\n\
04/23/2026;;;;;;;;1252;1;Z;\n\
[3TEAMS]\n\
T1;Home;0;;;;\n\
T2;Away;0;;;;\n\
[3PLAYERS-H]\n\
0;1;1;;;;;;H1;Cech;A;;;2;;;;\x0f4010C006500630068;;\n\
0;2;2;;;;;;H2;Novak;B;;;2;;;;\x0f4004E006F0076006B;;\n\
[3PLAYERS-V]\n\
1;2;1;;;;;;V2;Two;B;;;2;;\n\
[3SCOUT]\n\
*01SH#;;;;;;;;;;;;;;;;;;;;;;;;;;\n";
        let text: Vec<String> = validate(Path::new("m.dvw"), file)
            .iter()
            .map(ToString::to_string)
            .filter(|t| t.contains("Unicode copy"))
            .collect();
        // `Čech` for Cech is the code page's best fit; `Novk` for Novak isn't.
        assert_eq!(
            text,
            [
                "10: warning: last name differs from its Unicode copy (file edited by another program?)"
            ]
        );
    }

    #[test]
    fn ambiguous_date_and_codepage_1() {
        let file = String::from_utf8(GOOD.to_vec())
            .unwrap()
            .replace("04/23/2026;;;;;;;;1252", "03/04/2026;;;;;;;;1");
        let text: Vec<String> = validate(Path::new("m.dvw"), file.as_bytes())
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            text,
            [
                "4: warning: match date \"03/04/2026\" is 2026-03-04 or 2026-04-03; nothing else in the file settles the day/month order",
                "4: warning: code page 1 (the PC's default, written by DataVolley 3.x) read as windows-1252, guessed from the text; check accented names",
            ]
        );
        let legacy = validate(Path::new("m.dvw"), b"[Match]\n");
        assert_eq!(
            legacy[0].to_string(),
            "error: legacy DataVolley file (pre-2007 [Match] format); not supported"
        );
    }

    #[test]
    fn missing_sections_and_bad_types() {
        let issues = validate(Path::new("m.dvw"), b"[3MATCH]\n;;;;;;;;1252;\n");
        assert!(
            issues
                .iter()
                .any(|i| i.message == "missing [3SCOUT] section")
        );
        assert_eq!(
            validate(Path::new("m.txt"), b"")[0].severity,
            Severity::Error
        );
        assert!(validate(Path::new("m.vsm"), b"{\"a\":1}").is_empty());
        assert_eq!(
            validate(Path::new("m.vsm"), b"{\"a\": 1}")[0].severity,
            Severity::Warning
        );
        assert_eq!(
            validate(Path::new("m.vsm"), b"{")[0].severity,
            Severity::Error
        );
    }

    /// Every corpus file passes the file-level checks. Match-rule issues
    /// are expected (real scouting slips; see FORMAT-NOTES "Validator"),
    /// so for those this only checks that the engine runs and every issue
    /// has a file line.
    /// File-level findings in corpus files checked by hand and right
    /// (FORMAT-NOTES "Encodings", CORPUS.md intake 2026-10-19): (start of
    /// the path under testdata, start of the message). Any other is a
    /// problem. A path is cut short where the file name has players' names.
    const KNOWN: &[(&str, &str)] = &[
        // The day/month order really is ambiguous in these.
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        // Code page 1: the encoding is guessed, as the warning says.
        ("corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        (
            "corpus-file",
            "code page 1",
        ),
        ("corpus-file", "code page 1"),
        ("dvw/datavolley3/corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        (
            "corpus-file",
            "code page 1",
        ),
        ("corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        // The file really lists home shirt 1 twice.
        ("corpus-file", "shirt number 1 listed twice"),
        // UTF-8 names in a file that says 1252: the plain columns are
        // mojibake, the Unicode copies are right.
        ("corpus-file", "first name differs"),
        ("corpus-file", "last name differs"), // VolleyStation's date order is a season setting; these dates and
        // their export stamps have day and month both ≤ 12 (2026-10-27).
        ("corpus-file", "match date"),
        (
            "corpus-file",
            "match date",
        ),
        (
            "corpus-file",
            "match date",
        ),
        ("corpus-file", "match date"),
        (
            "corpus-file",
            "match date",
        ),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        ("corpus-file", "match date"),
        // Code page 1 (PC default), read by the guess (2026-10-27 batch).
        (
            "corpus-file",
            "code page 1",
        ),
        ("corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        ("corpus-file", "code page 1"),
        (
            "corpus-file",
            "code page 1",
        ),
        // The files really list visiting shirt 0 twice.
        (
            "corpus-file",
            "shirt number 0 listed twice",
        ),
        (
            "corpus-file",
            "shirt number 0 listed twice",
        ),
        // The plain column drops a diacritic its Unicode copy has (D/Đ).
        (
            "corpus-file",
            "last name differs",
        ),
    ];

    #[test]
    fn corpus_has_no_file_level_issues() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
        if !root.is_dir() {
            assert!(
                std::env::var("COURTSIDE_REQUIRE_CORPUS").as_deref() != Ok("1"),
                "corpus required but missing"
            );
            return;
        }
        let mut stack = vec![root.clone()];
        let mut checked = 0;
        let mut problems = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                // Doctored fixtures aren't matches (CORPUS.md).
                if path.is_dir() && path.file_name().is_some_and(|n| n == "malformed") {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("dvw" | "vsm")
                ) {
                    checked += 1;
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    for i in validate(&path, &std::fs::read(&path).unwrap()) {
                        let known = KNOWN
                            .iter()
                            .any(|(p, m)| rel.starts_with(p) && i.message.starts_with(m));
                        if (i.rule.is_none() || i.line.is_none()) && !known {
                            problems.push(format!("{}:{i}", path.display()));
                        }
                    }
                }
            }
        }
        assert!(checked > 0);
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
