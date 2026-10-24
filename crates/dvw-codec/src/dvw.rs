//! Typed view of a `.dvw` file's header sections.
//!
//! Every typed section keeps its rows as decoded `;`-separated fields
//! ([`Row`]), so writing back is lossless by construction: accessors
//! interpret fields, they never normalise them. Column meanings follow
//! openvolley (`datavolley/R/meta.R`) unless `docs/FORMAT-NOTES.md` says
//! otherwise. Unknown sections stay as raw bytes.

use crate::Error;
use crate::date::{Date, Written};
use crate::encoding::TextEncoding;
use crate::scout::ScoutLine;
use crate::sections::{Line, LineEnding, RawFile};
use crate::text::Row;

#[derive(Debug, Clone, PartialEq)]
pub struct Dvw {
    raw: RawFile,
    pub encoding: TextEncoding,
    pub header: Header,
    pub match_info: MatchInfo,
    pub teams: Vec<Team>,
    pub more: More,
    pub comments: Vec<Row>,
    pub sets: Vec<SetResult>,
    pub players_home: Vec<Player>,
    pub players_visiting: Vec<Player>,
    pub attack_combinations: Vec<AttackCombination>,
    pub setter_calls: Vec<SetterCall>,
    pub winning_symbols: WinningSymbols,
    pub reserve: Vec<Row>,
    pub video: Vec<VideoEntry>,
    pub scout: Vec<ScoutLine>,
}

impl Dvw {
    /// Parses with the encoding named in `[3MATCH]`. Without one, or with
    /// code page `1` (Windows' "PC default", written by DataVolley 3.x),
    /// the encoding is guessed from the bytes ([`TextEncoding::guess`]);
    /// `courtside validate` says which.
    pub fn parse(input: &[u8]) -> Result<Self, Error> {
        let raw = RawFile::parse(input);
        check_format(&raw)?;
        let encoding = match declared_codepage(&raw) {
            None => TextEncoding::guess(input),
            Some(cp) if cp.trim().is_empty() || cp == "1" => TextEncoding::guess(input),
            Some(cp) => TextEncoding::from_codepage(&cp).ok_or(Error::UnknownCodepage(cp))?,
        };
        Self::from_raw(raw, encoding)
    }

    /// Parses with an explicit encoding, ignoring the declared one.
    pub fn parse_with_encoding(input: &[u8], encoding: TextEncoding) -> Result<Self, Error> {
        let raw = RawFile::parse(input);
        check_format(&raw)?;
        Self::from_raw(raw, encoding)
    }

    fn from_raw(raw: RawFile, encoding: TextEncoding) -> Result<Self, Error> {
        let lines = |name: &str| -> Result<Vec<String>, Error> {
            let Some(section) = raw.section(name) else {
                return Ok(Vec::new());
            };
            section
                .lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    encoding
                        .decode(&line.content)
                        .ok_or_else(|| Error::Undecodable {
                            encoding: encoding.name(),
                            section: name.to_owned(),
                            line: i + 1,
                        })
                })
                .collect()
        };
        let rows = |name: &str| -> Result<Vec<Row>, Error> {
            Ok(lines(name)?.iter().map(|l| Row::parse(l)).collect())
        };
        Ok(Dvw {
            encoding,
            header: Header {
                lines: lines(HEADER)?,
            },
            match_info: MatchInfo { rows: rows(MATCH)? },
            teams: rows(TEAMS)?.into_iter().map(Team).collect(),
            more: More { rows: rows(MORE)? },
            comments: rows(COMMENTS)?,
            sets: rows(SET)?.into_iter().map(SetResult).collect(),
            players_home: rows(PLAYERS_H)?.into_iter().map(Player).collect(),
            players_visiting: rows(PLAYERS_V)?.into_iter().map(Player).collect(),
            attack_combinations: rows(ATTACKS)?.into_iter().map(AttackCombination).collect(),
            setter_calls: rows(SETTER_CALLS)?.into_iter().map(SetterCall).collect(),
            winning_symbols: WinningSymbols {
                lines: lines(WINNING_SYMBOLS)?,
            },
            reserve: rows(RESERVE)?,
            video: lines(VIDEO)?.into_iter().map(VideoEntry).collect(),
            scout: rows(SCOUT)?.into_iter().map(ScoutLine).collect(),
            raw,
        })
    }

    /// The file as bytes: typed sections re-encoded in place, everything else
    /// as read. Typed data for a section the file lacks is not written.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut raw = self.raw.clone();
        // Only the first section of each name is typed; later duplicates stay raw.
        let mut seen = std::collections::HashSet::new();
        for section in &mut raw.sections {
            if !seen.insert(section.name.clone()) {
                continue;
            }
            let Some(texts) = self.typed_lines(&section.name) else {
                continue;
            };
            let lines = texts
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    self.encoding
                        .encode(text)
                        .ok_or_else(|| Error::Unencodable {
                            encoding: self.encoding.name(),
                            section: section.name.clone(),
                            line: i + 1,
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let endings = endings_for(&section.lines, section.header.ending, lines.len());
            section.lines = lines
                .into_iter()
                .zip(endings)
                .map(|(content, ending)| Line { content, ending })
                .collect();
        }
        Ok(raw.to_bytes())
    }

    /// Roster entry for shirt `number` on the home (`home == true`) or
    /// visiting team.
    pub fn player(&self, home: bool, number: u8) -> Option<&Player> {
        let roster = if home {
            &self.players_home
        } else {
            &self.players_visiting
        };
        roster.iter().find(|p| p.number() == Some(number))
    }

    /// The file as read; typed fields above take precedence on write.
    pub fn raw(&self) -> &RawFile {
        &self.raw
    }

    /// Match date: the date serial when present, else the written
    /// `[3MATCH]` date. The day/month order of a written date is the
    /// writing PC's setting, not the program's, and can differ between the
    /// dates in one file (FORMAT-NOTES), so an ambiguous date is settled
    /// only when exactly one reading is within two weeks of another date in
    /// the file that is itself unambiguous. Otherwise `None`.
    pub fn match_date(&self) -> Option<(Date, DateSource)> {
        let anchors: Vec<Date> = ["GENERATOR-DAY", "LASTCHANGE-DAY"]
            .iter()
            .filter_map(|k| self.header.get(k)?.split_whitespace().next())
            .filter_map(|t| match Date::parse_written(t)? {
                Written::One(d) => Some(d),
                Written::Two { .. } => None,
            })
            .collect();
        self.match_info.date(&anchors)
    }

    fn typed_lines(&self, name: &str) -> Option<Vec<String>> {
        let rows = |rows: &[Row]| rows.iter().map(Row::to_line).collect();
        let wrapped = |rows: Vec<&Row>| rows.into_iter().map(Row::to_line).collect();
        Some(match name {
            HEADER => self.header.lines.clone(),
            MATCH => rows(&self.match_info.rows),
            TEAMS => wrapped(self.teams.iter().map(|t| &t.0).collect()),
            MORE => rows(&self.more.rows),
            COMMENTS => rows(&self.comments),
            SET => wrapped(self.sets.iter().map(|s| &s.0).collect()),
            PLAYERS_H => wrapped(self.players_home.iter().map(|p| &p.0).collect()),
            PLAYERS_V => wrapped(self.players_visiting.iter().map(|p| &p.0).collect()),
            ATTACKS => wrapped(self.attack_combinations.iter().map(|a| &a.0).collect()),
            SETTER_CALLS => wrapped(self.setter_calls.iter().map(|s| &s.0).collect()),
            WINNING_SYMBOLS => self.winning_symbols.lines.clone(),
            RESERVE => rows(&self.reserve),
            VIDEO => self.video.iter().map(|v| v.0.clone()).collect(),
            SCOUT => wrapped(self.scout.iter().map(|s| &s.0).collect()),
            _ => return None,
        })
    }
}

const HEADER: &str = "3DATAVOLLEYSCOUT";
const MATCH: &str = "3MATCH";
const TEAMS: &str = "3TEAMS";
const MORE: &str = "3MORE";
const COMMENTS: &str = "3COMMENTS";
const SET: &str = "3SET";
const PLAYERS_H: &str = "3PLAYERS-H";
const PLAYERS_V: &str = "3PLAYERS-V";
const ATTACKS: &str = "3ATTACKCOMBINATION";
const SETTER_CALLS: &str = "3SETTERCALL";
const WINNING_SYMBOLS: &str = "3WINNINGSYMBOLS";
const RESERVE: &str = "3RESERVE";
const VIDEO: &str = "3VIDEO";
const SCOUT: &str = "3SCOUT";

/// Refuses files that can't be DataVolley 2007-format text: a `[Match]`
/// first line (the pre-2007 layout) or no `[3…]` section at all (encrypted
/// files, other formats). Anything else is read leniently; missing
/// sections are for the validator to report.
fn check_format(raw: &RawFile) -> Result<(), Error> {
    if raw.sections.iter().any(|s| s.name.starts_with('3')) {
        return Ok(());
    }
    let first = raw
        .preamble
        .iter()
        .chain(raw.sections.iter().map(|s| &s.header))
        .map(|l| l.content.trim_ascii())
        .find(|c| !c.is_empty());
    match first {
        None => Ok(()),
        Some(c) if c.len() >= 7 && c[..7].eq_ignore_ascii_case(b"[match]") => {
            Err(Error::LegacyFormat)
        }
        Some(_) => Err(Error::NotDataVolley),
    }
}

/// `[3MATCH]` field 9 of the first line, read as ASCII before decoding.
fn declared_codepage(raw: &RawFile) -> Option<String> {
    let line = raw.section(MATCH)?.lines.first()?;
    let field = line.content.split(|b| *b == b';').nth(8)?;
    let cp = std::str::from_utf8(field).ok()?.trim();
    (!cp.is_empty()).then(|| cp.to_owned())
}

/// Line endings for `n` rewritten lines: reuse the originals by position,
/// keep the section's final ending on the new last line, and use the
/// header's ending for any added lines.
fn endings_for(original: &[Line], header: LineEnding, n: usize) -> Vec<LineEnding> {
    let fill = match header {
        LineEnding::None => LineEnding::CrLf,
        e => e,
    };
    let last = original.last().map_or(fill, |l| l.ending);
    (0..n)
        .map(|i| {
            if i + 1 == n {
                last
            } else if i + 1 < original.len() {
                original[i].ending
            } else {
                fill
            }
        })
        .collect()
}

/// `[3DATAVOLLEYSCOUT]`: `KEY: value` lines.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Header {
    pub lines: Vec<String>,
}

impl Header {
    /// Value for `key`, without the single space after the colon.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k == key).then(|| v.strip_prefix(' ').unwrap_or(v))
        })
    }
}

/// How a match date was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSource {
    /// The date serial on the second `[3MATCH]` line.
    Serial,
    /// The written date, which only one order makes valid.
    Unambiguous,
    /// The written date, in the one order that puts it near another
    /// unambiguous date in the file.
    NearOtherDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regulation {
    IndoorSideout,
    IndoorRallyPoint,
    BeachRallyPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CourtCoding {
    Zones,
    Cones,
}

/// `[3MATCH]`. Line 1: date; time; season; league; phase; home/away;
/// day number; match number; code page; regulation; zones/cones; ...;
/// league, phase and home/away as embedded Unicode (fields 12–14).
/// Line 2 (DataVolley only): federation match id; competition id; date serial; ...
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchInfo {
    pub rows: Vec<Row>,
}

impl MatchInfo {
    fn line(&self, i: usize) -> &Row {
        static EMPTY: Row = Row(Vec::new());
        self.rows.get(i).unwrap_or(&EMPTY)
    }
    pub fn date_text(&self) -> &str {
        self.line(0).get(0)
    }
    pub fn time(&self) -> &str {
        self.line(0).get(1)
    }
    pub fn season(&self) -> &str {
        self.line(0).get(2)
    }
    pub fn league(&self) -> String {
        self.line(0).text(3, 12)
    }
    pub fn phase(&self) -> String {
        self.line(0).text(4, 13)
    }
    pub fn home_away(&self) -> String {
        self.line(0).text(5, 14)
    }
    pub fn day_number(&self) -> &str {
        self.line(0).get(6)
    }
    pub fn match_number(&self) -> &str {
        self.line(0).get(7)
    }
    pub fn codepage(&self) -> &str {
        self.line(0).get(8)
    }
    pub fn regulation(&self) -> Option<Regulation> {
        match self.line(0).int::<u8>(9)? {
            0 => Some(Regulation::IndoorSideout),
            1 => Some(Regulation::IndoorRallyPoint),
            2 => Some(Regulation::BeachRallyPoint),
            _ => None,
        }
    }
    pub fn court_coding(&self) -> Option<CourtCoding> {
        match self.line(0).get(10) {
            "Z" => Some(CourtCoding::Zones),
            "C" => Some(CourtCoding::Cones),
            _ => None,
        }
    }
    /// The date serial on line 2. `12345` is a known placeholder.
    pub fn date_serial(&self) -> Option<i64> {
        self.line(1).int::<i64>(2).filter(|&n| n != 12_345)
    }

    /// See [`Dvw::match_date`], which supplies the `anchors` (other
    /// unambiguous dates in the file).
    pub fn date(&self, anchors: &[Date]) -> Option<(Date, DateSource)> {
        if let Some(date) = self.date_serial().and_then(Date::from_serial) {
            return Some((date, DateSource::Serial));
        }
        match Date::parse_written(self.date_text())? {
            Written::One(d) => Some((d, DateSource::Unambiguous)),
            Written::Two {
                month_first,
                day_first,
            } => {
                let near = |d: Date| {
                    anchors
                        .iter()
                        .any(|a| (a.to_unix_days() - d.to_unix_days()).abs() <= 14)
                };
                match (near(month_first), near(day_first)) {
                    (true, false) => Some((month_first, DateSource::NearOtherDate)),
                    (false, true) => Some((day_first, DateSource::NearOtherDate)),
                    _ => None,
                }
            }
        }
    }
}

/// `[3TEAMS]` row: id; name; sets won; coach; assistant; shirt colour;
/// name, coach, assistant as embedded Unicode (fields 6–8). Row 1 is home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team(pub Row);

impl Team {
    pub fn id(&self) -> &str {
        self.0.get(0)
    }
    pub fn name(&self) -> String {
        self.0.text(1, 6)
    }
    pub fn sets_won(&self) -> Option<u8> {
        self.0.int(2)
    }
    pub fn coach(&self) -> String {
        self.0.text(3, 7)
    }
    pub fn assistant(&self) -> String {
        self.0.text(4, 8)
    }
    /// Colour as a Windows `COLORREF` integer (0x00BBGGRR).
    pub fn shirt_colour(&self) -> Option<u32> {
        self.0.int(5)
    }
}

/// `[3MORE]`. Line 1: referees; spectators; receipts; city; arena; scout;
/// referees, city, arena, scout as embedded Unicode (fields 6–9).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct More {
    pub rows: Vec<Row>,
}

impl More {
    fn first(&self) -> &Row {
        static EMPTY: Row = Row(Vec::new());
        self.rows.first().unwrap_or(&EMPTY)
    }
    pub fn referees(&self) -> String {
        self.first().text(0, 6)
    }
    pub fn spectators(&self) -> Option<u32> {
        self.first().int(1)
    }
    pub fn receipts(&self) -> Option<u32> {
        self.first().int(2)
    }
    pub fn city(&self) -> String {
        self.first().text(3, 7)
    }
    pub fn arena(&self) -> String {
        self.first().text(4, 8)
    }
    pub fn scout(&self) -> String {
        self.first().text(5, 9)
    }
}

/// `[3SET]` row: played; score at 8, 16 and 21 points; final score;
/// duration in minutes. Unplayed sets are `True;;;;;;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetResult(pub Row);

impl SetResult {
    pub fn played(&self) -> Option<bool> {
        self.0.bool(0)
    }
    /// Score when the leader reached 8, 16 or 21 (`checkpoint` 0, 1, 2).
    pub fn checkpoint_score(&self, checkpoint: usize) -> Option<(u8, u8)> {
        (checkpoint < 3).then(|| score(self.0.get(1 + checkpoint)))?
    }
    pub fn final_score(&self) -> Option<(u8, u8)> {
        score(self.0.get(4))
    }
    pub fn duration_minutes(&self) -> Option<u16> {
        self.0.int(5)
    }
}

/// `"25-19"`, `" 7- 8"`, `"8 -7"` → home, visiting.
fn score(field: &str) -> Option<(u8, u8)> {
    let (home, visiting) = field.split_once('-')?;
    Some((home.trim().parse().ok()?, visiting.trim().parse().ok()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Libero,
    Outside,
    Opposite,
    Middle,
    Setter,
    /// Role 6; openvolley calls it "unknown".
    Unknown,
}

/// `[3PLAYERS-H]`/`[3PLAYERS-V]` row: team (0 home, 1 visiting); shirt
/// number; index; starting zone in sets 1–5; player id; last name; first
/// name; nickname; special role; role; foreign; ...; last, first and
/// nickname as embedded Unicode (fields 17–19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player(pub Row);

impl Player {
    pub fn number(&self) -> Option<u8> {
        self.0.int(1)
    }
    /// Starting zone for `set` 1–5: `1`–`6`, `*` for came on as a
    /// substitute, empty for did not play.
    pub fn starting_zone(&self, set: usize) -> Option<&str> {
        (1..=5).contains(&set).then(|| self.0.get(2 + set))
    }
    pub fn id(&self) -> &str {
        self.0.get(8)
    }
    pub fn last_name(&self) -> String {
        self.0.text(9, 17)
    }
    pub fn first_name(&self) -> String {
        self.0.text(10, 18)
    }
    pub fn nickname(&self) -> String {
        self.0.text(11, 19)
    }
    /// `L` libero, `C` captain, empty otherwise.
    pub fn special_role(&self) -> &str {
        self.0.get(12)
    }
    /// `None` for an empty field or VolleyStation's `NaN`.
    pub fn role(&self) -> Option<Role> {
        match self.0.int::<u8>(13)? {
            1 => Some(Role::Libero),
            2 => Some(Role::Outside),
            3 => Some(Role::Opposite),
            4 => Some(Role::Middle),
            5 => Some(Role::Setter),
            6 => Some(Role::Unknown),
            _ => None,
        }
    }
    pub fn foreign(&self) -> Option<bool> {
        self.0.bool(14)
    }
}

/// `[3ATTACKCOMBINATION]` row: code; start zone; side; tempo; description;
/// ?; colour; start coordinate; target attacker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackCombination(pub Row);

impl AttackCombination {
    pub fn code(&self) -> &str {
        self.0.get(0)
    }
    pub fn start_zone(&self) -> Option<u8> {
        self.0.int(1)
    }
    pub fn side(&self) -> &str {
        self.0.get(2)
    }
    /// Attack tempo/type letter (`Q`, `H`, `T`, `O`, ...).
    pub fn tempo(&self) -> &str {
        self.0.get(3)
    }
    pub fn description(&self) -> &str {
        self.0.get(4)
    }
    pub fn colour(&self) -> Option<u32> {
        self.0.int(6)
    }
    pub fn start_coordinate(&self) -> Option<u32> {
        self.0.int(7)
    }
    /// Set target (`F` front, `B` back, `C` centre, `P` pipe, `S` setter).
    pub fn target_attacker(&self) -> &str {
        self.0.get(8)
    }
}

/// `[3SETTERCALL]` row: code; ?; description; ?; colour; start, mid and end
/// coordinates; path; path colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetterCall(pub Row);

impl SetterCall {
    pub fn code(&self) -> &str {
        self.0.get(0)
    }
    pub fn description(&self) -> &str {
        self.0.get(2)
    }
    pub fn colour(&self) -> Option<u32> {
        self.0.int(4)
    }
}

/// `[3WINNINGSYMBOLS]`: one 56-character line, 8 per skill in the order
/// S R A B D E F; the first 4 are losing evaluations, the last 4 winning,
/// padded with `~`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WinningSymbols {
    pub lines: Vec<String>,
}

impl WinningSymbols {
    /// `(losing, winning)` evaluation symbols for `skill`.
    pub fn for_skill(&self, skill: char) -> Option<(Vec<char>, Vec<char>)> {
        let i = "SRABDEF".find(skill)?;
        let chars: Vec<char> = self.lines.first()?.chars().collect();
        if chars.len() != 56 {
            return None;
        }
        let symbols = |s: &[char]| s.iter().copied().filter(|&c| c != '~').collect();
        let block = &chars[i * 8..i * 8 + 8];
        Some((symbols(&block[..4]), symbols(&block[4..])))
    }
}

/// `[3VIDEO]` line: `Camera0=<path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoEntry(pub String);

impl VideoEntry {
    pub fn camera(&self) -> &str {
        self.0.split_once('=').map_or(self.0.as_str(), |(c, _)| c)
    }
    pub fn path(&self) -> &str {
        self.0.split_once('=').map_or("", |(_, p)| p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small synthetic file in DataVolley 4 layout. Names are made up.
    const SAMPLE: &[u8] = b"[3DATAVOLLEYSCOUT]\r\n\
FILEFORMAT: 2.0\r\n\
GENERATOR-PRG: Data Volley\r\n\
GENERATOR-REL: \r\n\
[3MATCH]\r\n\
03/04/2026;19.30.00;2025/2026;Test League;Regular;;1;7;1252;1;Z;0;\x0f254657374204C6561677565;;;\r\n\
1;2;46115;;;;L;R;X;1;\r\n\
[3TEAMS]\r\n\
T1;Home Club;3;Coach One;;255;\x0f2486F6D6520436C7562;;;\r\n\
T2;Away Club;1;Coach Two;;0;;;;\r\n\
[3MORE]\r\n\
Ref A, Ref B;120;;Cork;Main Hall;Scout X;;;;;\r\n\
;0;0;\r\n\
[3COMMENTS]\r\n\
[3SET]\r\n\
True;8 -7;16-14;21-17;25-19;22;\r\n\
True;;;;;;\r\n\
[3PLAYERS-H]\r\n\
0;9;1;1;*;;;;H09;Byrne;Si\xe1n;;C;5;False;H09;;\x0f24279726E65;\x0f25369E16E;\x0f2;;;\r\n\
[3PLAYERS-V]\r\n\
1;4;1;2;;;;;V04;Ng;Ana;;L;1;;;V04;;;;;2;;;\r\n\
[3ATTACKCOMBINATION]\r\n\
X5;4;R;H;Shoot;;65280;4868;F;;\r\n\
[3SETTERCALL]\r\n\
K1;;Front Quick;;255;;;;;;\r\n\
[3WINNINGSYMBOLS]\r\n\
=~~~#~~~=~~~~~~~=/~~#~~~=/~~#~~~=~~~~~~~=~~~~~~~=~~~~~~~\r\n\
[3RESERVE]\r\n\
[3VIDEO]\r\n\
Camera0=C:\\video\\a=b.mp4\r\n\
[3SCOUT]\r\n\
*09SH#;;;;;;;19.31.02;1;1;1;;;;9;1;2;3;4;5;4;6;7;8;10;11;\r\n";

    #[test]
    fn reads_header_sections() {
        let dvw = Dvw::parse(SAMPLE).unwrap();
        assert_eq!(dvw.encoding, TextEncoding::WINDOWS_1252);
        assert_eq!(dvw.header.get("GENERATOR-PRG"), Some("Data Volley"));
        assert_eq!(dvw.header.get("GENERATOR-REL"), Some(""));

        let m = &dvw.match_info;
        assert_eq!(m.league(), "Test League");
        assert_eq!(m.regulation(), Some(Regulation::IndoorRallyPoint));
        assert_eq!(m.court_coding(), Some(CourtCoding::Zones));
        assert_eq!(
            dvw.match_date(),
            Some((Date::new(2026, 4, 3).unwrap(), DateSource::Serial))
        );

        assert_eq!(dvw.teams[0].name(), "Home Club");
        assert_eq!(dvw.teams[0].sets_won(), Some(3));
        assert_eq!(dvw.teams[1].coach(), "Coach Two");
        assert_eq!(dvw.more.spectators(), Some(120));
        assert_eq!(dvw.more.receipts(), None);
        assert_eq!(dvw.more.city(), "Cork");

        assert_eq!(dvw.sets[0].checkpoint_score(0), Some((8, 7)));
        assert_eq!(dvw.sets[0].final_score(), Some((25, 19)));
        assert_eq!(dvw.sets[0].duration_minutes(), Some(22));
        assert_eq!(dvw.sets[1].final_score(), None);

        let p = &dvw.players_home[0];
        assert_eq!(p.number(), Some(9));
        assert_eq!(p.first_name(), "Sián");
        assert_eq!(p.starting_zone(1), Some("1"));
        assert_eq!(p.starting_zone(2), Some("*"));
        assert_eq!(p.role(), Some(Role::Setter));
        assert_eq!(p.special_role(), "C");
        assert_eq!(p.foreign(), Some(false));
        assert_eq!(dvw.players_visiting[0].role(), Some(Role::Libero));
        assert_eq!(
            dvw.player(true, 9).map(|p| p.last_name()).as_deref(),
            Some("Byrne")
        );
        assert!(dvw.player(false, 9).is_none());

        assert_eq!(dvw.attack_combinations[0].code(), "X5");
        assert_eq!(dvw.attack_combinations[0].target_attacker(), "F");
        assert_eq!(dvw.setter_calls[0].description(), "Front Quick");
        assert_eq!(
            dvw.winning_symbols.for_skill('A'),
            Some((vec!['=', '/'], vec!['#']))
        );
        assert_eq!(
            dvw.winning_symbols.for_skill('S'),
            Some((vec!['='], vec!['#']))
        );
        assert_eq!(dvw.video[0].camera(), "Camera0");
        assert_eq!(dvw.video[0].path(), "C:\\video\\a=b.mp4");
        assert_eq!(dvw.scout.len(), 1);
        assert_eq!(dvw.scout[0].code(), "*09SH#");
        assert_eq!(dvw.scout[0].home_players()[0], Some(9));
    }

    #[test]
    fn round_trips_unchanged() {
        assert_eq!(Dvw::parse(SAMPLE).unwrap().to_bytes().unwrap(), SAMPLE);
    }

    #[test]
    fn edits_are_written_in_place() {
        let mut dvw = Dvw::parse(SAMPLE).unwrap();
        dvw.teams[1].0.0[1] = "Visitors".into();
        dvw.comments.push(Row::parse("first comment"));
        let out = Dvw::parse(&dvw.to_bytes().unwrap()).unwrap();
        assert_eq!(out.teams[1].name(), "Visitors");
        assert_eq!(out.comments, [Row::parse("first comment")]);
        assert_eq!(out.sets, dvw.sets);
    }

    #[test]
    fn unrepresentable_text_is_an_error() {
        let mut dvw = Dvw::parse(SAMPLE).unwrap();
        dvw.teams[0].0.0[1] = "Łódź".into();
        assert!(matches!(
            dvw.to_bytes(),
            Err(Error::Unencodable { line: 1, .. })
        ));
    }

    #[test]
    fn invalid_utf8_reports_location() {
        let input = b"[3MATCH]\n;;;;;;;;65001;\n[3TEAMS]\nok\nbad\xff\n";
        assert!(matches!(
            Dvw::parse(input),
            Err(Error::Undecodable { ref section, line: 2, .. }) if section == "3TEAMS"
        ));
    }

    #[test]
    fn unknown_codepage_is_an_error() {
        let input = b"[3MATCH]\n;;;;;;;;4242;\n";
        assert!(matches!(Dvw::parse(input), Err(Error::UnknownCodepage(cp)) if cp == "4242"));
        assert!(Dvw::parse_with_encoding(input, TextEncoding::WINDOWS_1252).is_ok());
    }

    #[test]
    fn codepage_1_reads_as_windows_1252() {
        let dvw = Dvw::parse(b"[3MATCH]\n;;;;;;;;1;\n[3TEAMS]\nT1;Caf\xe9;0;\n").unwrap();
        assert_eq!(dvw.encoding, TextEncoding::WINDOWS_1252);
        assert_eq!(dvw.teams[0].name(), "Caf\u{e9}");
        assert_eq!(dvw.match_info.codepage(), "1");
    }

    #[test]
    fn legacy_and_encrypted_files_are_refused() {
        assert!(matches!(
            Dvw::parse(b"[Match]\r\nsomething\r\n"),
            Err(Error::LegacyFormat)
        ));
        assert!(matches!(
            Dvw::parse(b"\x8f\x12\xa0binary\x00\x01"),
            Err(Error::NotDataVolley)
        ));
        assert!(Dvw::parse(b"").is_ok());
    }

    #[test]
    fn date_without_serial() {
        let info = |date: &str| MatchInfo {
            rows: vec![Row::parse(date)],
        };
        let d = |y, m, day| Date::new(y, m, day).unwrap();
        assert_eq!(
            info("23/03/2024").date(&[]),
            Some((d(2024, 3, 23), DateSource::Unambiguous))
        );
        assert_eq!(
            info("2026-04-03").date(&[]),
            Some((d(2026, 4, 3), DateSource::Unambiguous))
        );
        // Ambiguous with nothing to settle it, or both readings near.
        assert_eq!(info("03/04/2026").date(&[]), None);
        assert_eq!(
            info("05/06/2026").date(&[d(2026, 5, 20), d(2026, 5, 22)]),
            None
        );
        // An unambiguous date nearby settles it.
        assert_eq!(
            info("03/04/2026").date(&[d(2026, 4, 5)]),
            Some((d(2026, 4, 3), DateSource::NearOtherDate))
        );
        assert_eq!(info("03/04/2026").date(&[d(2026, 6, 1)]), None);
    }

    #[test]
    fn endings_for_added_and_removed_lines() {
        use LineEnding::*;
        let line = |ending| Line {
            content: Vec::new(),
            ending,
        };
        let original = [line(CrLf), line(None)];
        assert_eq!(endings_for(&original, CrLf, 2), [CrLf, None]);
        assert_eq!(endings_for(&original, CrLf, 3), [CrLf, CrLf, None]);
        assert_eq!(endings_for(&original, CrLf, 1), [None]);
        assert_eq!(endings_for(&[], Lf, 2), [Lf, Lf]);
    }
}
