//! Team files: DataVolley's `.sq` (tab-separated, `DV-Team-2`, also read
//! and written by VolleyStation) and VolleyStation's `.vst` (JSON), as the
//! files themselves lay them out. Only reading for now.

use serde_json::Value;

use crate::date::{Date, Written};
use crate::encoding::TextEncoding;
use crate::text::decode_embedded;

/// A team as a team file holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamFile {
    pub code: String,
    pub name: String,
    pub short_code: String,
    pub staff: Staff,
    /// The team's home hall (`.vst` `homeCourtName`; `.sq` has none).
    pub home_court: String,
    pub players: Vec<TeamFilePlayer>,
}

/// A staff member's name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Person {
    pub last_name: String,
    pub first_name: String,
}

impl Person {
    /// From "Last First", as `.sq` and `.dvw` write a coach: the first word
    /// is the last name, as VolleyStation reads it.
    pub fn from_last_first(s: &str) -> Self {
        let s = s.trim();
        let (last, first) = s.split_once(' ').unwrap_or((s, ""));
        Person {
            last_name: last.to_owned(),
            first_name: first.trim().to_owned(),
        }
    }

    /// "Last First", or whichever of the two is given.
    pub fn last_first(&self) -> String {
        [self.last_name.trim(), self.first_name.trim()]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn is_empty(&self) -> bool {
        self.last_name.trim().is_empty() && self.first_name.trim().is_empty()
    }
}

/// A team's staff, as VolleyStation keeps it. `.sq` and `.dvw` hold only
/// the coach and the first assistant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Staff {
    pub coach: Person,
    pub assistant: Person,
    pub assistant2: Person,
    pub physio: Person,
    pub doctor: Person,
}

/// `.vst` `staff` keys, in VolleyStation's order.
const STAFF_KEYS: [&str; 5] = [
    "coach",
    "assistantCoach",
    "assistantCoach2",
    "therapist",
    "medicalDoctor",
];

impl Staff {
    fn people(&self) -> [&Person; 5] {
        [
            &self.coach,
            &self.assistant,
            &self.assistant2,
            &self.physio,
            &self.doctor,
        ]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamFilePlayer {
    pub number: u8,
    pub code: String,
    pub last_name: String,
    pub first_name: String,
    pub nickname: String,
    /// `.dvw` role: 1 libero, 2 outside, 3 opposite, 4 middle, 5 setter.
    pub role: Option<u8>,
    pub libero: bool,
    pub captain: bool,
    pub foreign: bool,
    /// ISO `YYYY-MM-DD`, when the file gives one that reads one way only.
    pub birth_date: Option<String>,
    pub height_cm: Option<u16>,
    /// `.vst` only.
    pub weight_kg: Option<u16>,
    /// Attack reach (`.vst` `spikeHeight`).
    pub reach_cm: Option<u16>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TeamFileError {
    #[error("not a team file: the first line isn't DV-Team-2")]
    NotSq,
    #[error("the team line is missing or too short")]
    NoTeam,
    #[error("can't read the file as text in code page {0}")]
    Undecodable(String),
    #[error("not a VolleyStation team file: {0}")]
    BadVst(String),
}

/// Reads a `.sq` file: decoded with the code page its team line names
/// (column 5), else Windows-1252; embedded Unicode copies win over the
/// plain columns, as in `.dvw`.
pub fn parse_sq(bytes: &[u8]) -> Result<TeamFile, TeamFileError> {
    // The header and the code page column are ASCII in any code page.
    let ascii = String::from_utf8_lossy(bytes);
    let mut lines = ascii.lines();
    if !lines
        .next()
        .is_some_and(|l| l.trim_start_matches('\u{feff}').starts_with("DV-Team-2"))
    {
        return Err(TeamFileError::NotSq);
    }
    let codepage = lines
        .next()
        .and_then(|l| l.split('\t').nth(5))
        .map(|c| c.trim().to_owned())
        .unwrap_or_default();
    let encoding = if std::str::from_utf8(bytes).is_ok() && !bytes.is_ascii() {
        // VolleyStation re-encodes to the season's code page, but a file
        // saved as UTF-8 by another tool is UTF-8.
        None
    } else {
        Some(TextEncoding::from_codepage(&codepage).unwrap_or(TextEncoding::WINDOWS_1252))
    };
    let text = match encoding {
        None => String::from_utf8_lossy(bytes).into_owned(),
        Some(e) => e
            .decode(bytes)
            .ok_or_else(|| TeamFileError::Undecodable(codepage.clone()))?,
    };
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).skip(1);
    let team: Vec<&str> = lines
        .next()
        .ok_or(TeamFileError::NoTeam)?
        .split('\t')
        .collect();
    if team.len() < 2 {
        return Err(TeamFileError::NoTeam);
    }
    let col = |row: &[&str], i: usize| row.get(i).map_or("", |s| s.trim()).to_owned();
    // Column `plain`, or its embedded Unicode copy when that has text.
    let text_col = |row: &[&str], plain: usize, embedded: usize| {
        row.get(embedded)
            .and_then(|f| decode_embedded(f))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| col(row, plain))
    };
    let mut file = TeamFile {
        code: col(&team, 0),
        name: text_col(&team, 1, 6),
        staff: Staff {
            coach: Person::from_last_first(&text_col(&team, 2, 7)),
            assistant: Person::from_last_first(&text_col(&team, 3, 8)),
            ..Staff::default()
        },
        short_code: text_col(&team, 4, 9),
        home_court: String::new(),
        players: Vec::new(),
    };
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let row: Vec<&str> = line.split('\t').collect();
        let Some(number) = col(&row, 0).parse::<u8>().ok().filter(|n| *n <= 99) else {
            continue;
        };
        let special = col(&row, 6).to_ascii_uppercase();
        let libero = special == "L";
        let role = col(&row, 9)
            .parse::<u8>()
            .ok()
            .filter(|r| (1..=6).contains(r))
            .or(libero.then_some(1));
        let date = |s: String| match Date::parse_written(&s) {
            Some(Written::One(d)) => Some(d.to_string()),
            _ => None,
        };
        file.players.push(TeamFilePlayer {
            number,
            code: col(&row, 1),
            last_name: text_col(&row, 2, 14),
            first_name: text_col(&row, 8, 15),
            nickname: text_col(&row, 10, 16),
            role,
            libero: libero || role == Some(1),
            captain: special == "C" || special == "CAPTAIN",
            foreign: col(&row, 12).eq_ignore_ascii_case("true"),
            // Column 13 (a fixed format) wins over 3 (the PC's locale).
            birth_date: date(col(&row, 13)).or_else(|| date(col(&row, 3))),
            height_cm: col(&row, 4)
                .parse()
                .ok()
                .filter(|h| (100..=250).contains(h)),
            weight_kg: None,
            reach_cm: None,
        });
    }
    Ok(file)
}

/// Reads a `.vst` file (VolleyStation's team JSON).
pub fn parse_vst(bytes: &[u8]) -> Result<TeamFile, TeamFileError> {
    let v: Value =
        serde_json::from_slice(bytes).map_err(|e| TeamFileError::BadVst(e.to_string()))?;
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    let person = |key: &str| {
        let p = v.get("staff").and_then(|st| st.get(key));
        p.map_or(Person::default(), |p| Person {
            last_name: s(p, "lastName"),
            first_name: s(p, "firstName"),
        })
    };
    let measure = |p: &Value, k: &str, range: std::ops::RangeInclusive<u16>| {
        p.get(k)
            .and_then(Value::as_u64)
            .and_then(|x| u16::try_from(x).ok())
            .filter(|x| range.contains(x))
    };
    let name = s(&v, "name");
    if name.is_empty() && s(&v, "code").is_empty() {
        return Err(TeamFileError::BadVst("no team name or code".into()));
    }
    let captain = v.get("captain").and_then(Value::as_u64);
    let players = v
        .get("players")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let number = u8::try_from(p.get("shirtNumber")?.as_u64()?).ok().filter(|n| *n <= 99)?;
            // VolleyStation positions are the .dvw role minus one.
            let role = p
                .get("position")
                .and_then(Value::as_u64)
                .and_then(|x| u8::try_from(x + 1).ok())
                .filter(|r| (1..=5).contains(r));
            Some(TeamFilePlayer {
                number,
                code: s(p, "code"),
                last_name: s(p, "lastName"),
                first_name: s(p, "firstName"),
                nickname: s(p, "nickname"),
                role,
                libero: role == Some(1),
                captain: captain == Some(u64::from(number)),
                foreign: p.get("isForeign").and_then(Value::as_bool).unwrap_or(false),
                birth_date: p
                    .get("birthDate")
                    .and_then(Value::as_str)
                    .and_then(|d| d.get(..10))
                    .filter(|d| {
                        let mut it = d.split('-');
                        matches!(
                            (it.next(), it.next(), it.next()),
                            (Some(y), Some(m), Some(dd)) if y.len() == 4 && m.len() == 2 && dd.len() == 2
                        )
                    })
                    .map(str::to_owned),
                height_cm: measure(p, "height", 100..=250),
                weight_kg: measure(p, "weight", 30..=200),
                reach_cm: measure(p, "spikeHeight", 150..=450),
            })
        })
        .collect();
    Ok(TeamFile {
        code: s(&v, "code"),
        short_code: s(&v, "shortCode"),
        name,
        staff: Staff {
            coach: person(STAFF_KEYS[0]),
            assistant: person(STAFF_KEYS[1]),
            assistant2: person(STAFF_KEYS[2]),
            physio: person(STAFF_KEYS[3]),
            doctor: person(STAFF_KEYS[4]),
        },
        home_court: s(&v, "homeCourtName"),
        players,
    })
}

/// Writes a `.sq` in DataVolley's full layout, which VolleyStation reads
/// too: tab-separated, a trailing tab on each row, CRLF, the code page in
/// the team row, and DataVolley's Unicode copies of every name. The plain
/// columns use the first Windows code page that can hold all the text
/// (`?` for what none can; the Unicode copies keep it). Birth dates are
/// not written: DataVolley reads them in the PC's date format.
pub fn write_sq(team: &TeamFile) -> Vec<u8> {
    let coach = team.staff.coach.last_first();
    let assistant = team.staff.assistant.last_first();
    let texts = std::iter::once([&team.name, &coach, &assistant, &team.short_code])
        .flat_map(|x| x.into_iter().cloned())
        .chain(team.players.iter().flat_map(|p| {
            [
                p.last_name.clone(),
                p.first_name.clone(),
                p.nickname.clone(),
                p.code.clone(),
            ]
        }))
        .collect::<Vec<_>>()
        .join("\t");
    let (codepage, encoding) = ["1252", "1250", "1251", "1253", "1254", "1257"]
        .into_iter()
        .find_map(|cp| {
            let e = TextEncoding::from_codepage(cp)?;
            e.encode(&texts).map(|_| (cp, e))
        })
        .unwrap_or(("1252", TextEncoding::WINDOWS_1252));
    // A field can't hold a tab or a line break.
    let plain = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let embedded = |s: &str| crate::text::encode_embedded(&plain(s));
    let mut lines = vec!["DV-Team-2".to_owned()];
    lines.push(
        [
            plain(&team.code),
            plain(&team.name),
            plain(&coach),
            plain(&assistant),
            plain(&team.short_code),
            codepage.to_owned(),
            embedded(&team.name),
            embedded(&coach),
            embedded(&assistant),
            embedded(&team.short_code),
            String::new(),
        ]
        .join("\t"),
    );
    let mut players: Vec<&TeamFilePlayer> = team.players.iter().collect();
    players.sort_by_key(|p| p.number);
    for p in players {
        let special = if p.libero || p.role == Some(1) {
            "L"
        } else if p.captain {
            "C"
        } else {
            ""
        };
        let flag = |b: bool| if b { "True" } else { "False" }.to_owned();
        lines.push(
            [
                p.number.to_string(),
                plain(&p.code),
                plain(&p.last_name),
                String::new(),
                p.height_cm.map_or(String::new(), |h| h.to_string()),
                String::new(),
                special.to_owned(),
                plain(&p.code),
                plain(&p.first_name),
                p.role.map_or(String::new(), |r| r.to_string()),
                plain(&p.nickname),
                flag(false),
                flag(p.foreign),
                String::new(),
                embedded(&p.last_name),
                embedded(&p.first_name),
                embedded(&p.nickname),
                String::new(),
            ]
            .join("\t"),
        );
    }
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    encoding.encode(&text).unwrap_or_else(|| {
        // Some character no code page holds: `?` in the plain text.
        let safe: String = text
            .chars()
            .map(|c| {
                if TextEncoding::WINDOWS_1252.encode(&c.to_string()).is_some() {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        let (bytes, _, _) = encoding_rs::WINDOWS_1252.encode(&safe);
        bytes.into_owned()
    })
}

/// Writes a `.vst`: VolleyStation's team JSON (no version field; absent
/// values left out).
pub fn write_vst(team: &TeamFile) -> Vec<u8> {
    use serde_json::{Map, json};
    let mut staff = Map::new();
    for (key, p) in STAFF_KEYS.into_iter().zip(team.staff.people()) {
        let mut o = Map::new();
        for (k, v) in [("firstName", &p.first_name), ("lastName", &p.last_name)] {
            if !v.trim().is_empty() {
                o.insert(k.into(), json!(v.trim()));
            }
        }
        if !o.is_empty() {
            staff.insert(key.into(), Value::Object(o));
        }
    }
    let mut obj = Map::new();
    obj.insert("code".into(), json!(team.code));
    if !team.short_code.is_empty() {
        obj.insert(
            "shortCode".into(),
            json!(team.short_code.chars().take(5).collect::<String>()),
        );
    }
    obj.insert("name".into(), json!(team.name));
    if !staff.is_empty() {
        obj.insert("staff".into(), Value::Object(staff));
    }
    if let Some(c) = team.players.iter().find(|p| p.captain) {
        obj.insert("captain".into(), json!(c.number));
    }
    if !team.home_court.trim().is_empty() {
        obj.insert("homeCourtName".into(), json!(team.home_court.trim()));
    }
    let players: Vec<Value> = team
        .players
        .iter()
        .map(|p| {
            let mut o = Map::new();
            o.insert("code".into(), json!(p.code));
            o.insert("firstName".into(), json!(p.first_name));
            o.insert("lastName".into(), json!(p.last_name));
            if !p.nickname.is_empty() {
                o.insert("nickname".into(), json!(p.nickname));
            }
            o.insert("isForeign".into(), json!(p.foreign));
            o.insert("shirtNumber".into(), json!(p.number));
            let role = if p.libero {
                Some(1)
            } else {
                p.role.filter(|r| (1..=5).contains(r))
            };
            if let Some(r) = role {
                o.insert("position".into(), json!(r - 1));
            }
            if let Some(d) = &p.birth_date {
                o.insert("birthDate".into(), json!(format!("{d}T00:00:00.000Z")));
            }
            for (k, v) in [
                ("height", p.height_cm),
                ("weight", p.weight_kg),
                ("spikeHeight", p.reach_cm),
            ] {
                if let Some(v) = v {
                    o.insert(k.into(), json!(v));
                }
            }
            Value::Object(o)
        })
        .collect();
    obj.insert("players".into(), Value::Array(players));
    serde_json::to_vec(&Value::Object(obj)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made-up examples in each file's layout.
    fn datavolley_sq() -> Vec<u8> {
        let rows = [
            "DV-Team-2".to_owned(),
            "HCV\tHarbour City\tMarlow Dennis\tOkafor Tess\tHCV\t1252\t\u{0F}2486172626F75722043697479\t\u{0F}24D61726C6F772044656E6E6973\t\u{0F}24F6B61666F722054657373\t\u{0F}2484356\t".to_owned(),
            "3\tHCV-ARD\tArdent\t14/03/2001\t192\t\tC\tHCV-ARD\tJonah\t2\t\tFalse\tFalse\t\t\u{0F}2417264656E74\t\u{0F}24A6F6E6168\t\u{0F}2\t".to_owned(),
            "7\tHCV-QUI\tQuill\t\t178\t\tL\tHCV-QUI\tMaya\t1\t\tFalse\tTrue\t\t\u{0F}25175696C6C\t\u{0F}24D617961\t\u{0F}2\t".to_owned(),
            "11\tHCV-BRA\tBrannigan\t\t\t\t\tHCV-BRA\tNell\t5\t\tFalse\tFalse\t\t\u{0F}24272616E6E6967616E\t\u{0F}24E656C6C\t\u{0F}2\t".to_owned(),
        ];
        let mut s = rows.join("\r\n");
        s.push_str("\r\n");
        s.into_bytes()
    }

    #[test]
    fn reads_a_datavolley_sq() {
        let t = parse_sq(&datavolley_sq()).expect("sq");
        assert_eq!(
            (
                t.code.as_str(),
                t.name.as_str(),
                t.staff.coach.last_first().as_str(),
                t.short_code.as_str()
            ),
            ("HCV", "Harbour City", "Marlow Dennis", "HCV")
        );
        assert_eq!(t.players.len(), 3);
        let a = &t.players[0];
        assert_eq!(
            (
                a.number,
                a.code.as_str(),
                a.last_name.as_str(),
                a.first_name.as_str()
            ),
            (3, "HCV-ARD", "Ardent", "Jonah")
        );
        assert!(a.captain && !a.libero);
        assert_eq!(
            (a.role, a.height_cm, a.birth_date.as_deref()),
            (Some(2), Some(192), Some("2001-03-14"))
        );
        let q = &t.players[1];
        assert!(q.libero && q.foreign);
        assert_eq!(q.role, Some(1));
        assert_eq!(t.players[2].role, Some(5));
    }

    #[test]
    fn reads_a_volleystation_sq() {
        let text = "DV-Team-2\nHCV\tHarbour City\tMarlow Dennis\tOkafor Tess\tHCV\t1252\n\
                    3\tHCV-ARD\tArdent\t\t\t\t\t\tJonah\t2\t\t\tFalse\n\
                    7\tHCV-QUI\tQuill\t\t\t\tL\t\tMaya\t1\t\t\tTrue";
        let t = parse_sq(text.as_bytes()).expect("sq");
        assert_eq!(t.players.len(), 2);
        assert!(t.players[1].libero && t.players[1].foreign);
        assert_eq!(t.players[0].birth_date, None);
    }

    #[test]
    fn decodes_the_named_code_page() {
        // "Łuk" in Windows-1250: Ł = 0xA3.
        let mut bytes = b"DV-Team-2\r\nPOL\tKlub\t\t\tPOL\t1250\r\n4\tLUK\t".to_vec();
        bytes.extend([0xA3, b'u', b'k']);
        bytes.extend(b"\t\t\t\t\t\tJan\t2\t\tFalse\tFalse\r\n");
        let t = parse_sq(&bytes).expect("sq");
        assert_eq!(t.players[0].last_name, "Łuk");
    }

    #[test]
    fn rejects_other_files() {
        assert_eq!(
            parse_sq(b"[3DATAVOLLEYSCOUT]\r\n"),
            Err(TeamFileError::NotSq)
        );
        assert_eq!(parse_sq(b"DV-Team-2\r\n"), Err(TeamFileError::NoTeam));
        assert!(parse_vst(b"not json").is_err());
    }

    fn sample() -> TeamFile {
        parse_sq(&datavolley_sq()).expect("sq")
    }

    #[test]
    fn written_sq_reads_back() {
        let mut t = sample();
        t.players[0].birth_date = None; // not written to .sq
        let bytes = write_sq(&t);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with(
            "DV-Team-2\r\nHCV\tHarbour City\tMarlow Dennis\tOkafor Tess\tHCV\t1252\t\u{0F}2"
        ));
        assert!(text.ends_with("\t\r\n"));
        assert_eq!(parse_sq(&bytes).expect("read back"), t);
        let row = text.lines().nth(3).expect("player row");
        assert_eq!(row.split('\t').count(), 18, "17 columns and a trailing tab");
    }

    #[test]
    fn written_sq_picks_a_code_page_or_keeps_unicode_copies() {
        let mut t = sample();
        t.players[0].last_name = "Łukasz".into();
        let bytes = write_sq(&t);
        assert!(String::from_utf8_lossy(&bytes).contains("\t1250\t"));
        assert_eq!(
            parse_sq(&bytes).expect("read").players[0].last_name,
            "Łukasz"
        );
        t.players[0].last_name = "Łódź Ђ".into(); // no single code page holds both
        let back = parse_sq(&write_sq(&t)).expect("read");
        assert_eq!(
            back.players[0].last_name, "Łódź Ђ",
            "the Unicode copy keeps it"
        );
    }

    #[test]
    fn written_vst_reads_back() {
        let t = sample();
        let back = parse_vst(&write_vst(&t)).expect("read back");
        assert_eq!(back.name, t.name);
        assert_eq!(back.staff.coach.last_first(), "Marlow Dennis");
        assert_eq!(
            back.players,
            t.players
                .iter()
                .map(|p| TeamFilePlayer {
                    nickname: p.nickname.clone(),
                    ..p.clone()
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn reads_a_vst() {
        let json = r#"{"code":"HCV","shortCode":"HCV","name":"Harbour City",
            "staff":{"coach":{"firstName":"Dennis","lastName":"Marlow"},"assistantCoach":{"firstName":"Tess","lastName":"Okafor"},"therapist":{"lastName":"Vance"}},
            "captain":3,"homeCourtName":"Quayside Arena",
            "players":[
              {"code":"HCV-ARD","firstName":"Jonah","lastName":"Ardent","isForeign":false,"shirtNumber":3,"position":1,"birthDate":"2001-03-14T00:00:00.000Z","height":192,"weight":84,"spikeHeight":338},
              {"code":"HCV-QUI","firstName":"Maya","lastName":"Quill","isForeign":true,"shirtNumber":7,"position":0},
              {"code":"HCV-BRA","firstName":"Nell","lastName":"Brannigan","isForeign":false,"shirtNumber":11,"position":4}]}"#;
        let t = parse_vst(json.as_bytes()).expect("vst");
        assert_eq!(
            (
                t.name.as_str(),
                t.staff.coach.last_first().as_str(),
                t.staff.assistant.first_name.as_str(),
                t.staff.physio.last_name.as_str(),
                t.home_court.as_str()
            ),
            (
                "Harbour City",
                "Marlow Dennis",
                "Tess",
                "Vance",
                "Quayside Arena"
            )
        );
        assert_eq!(
            (t.players[0].weight_kg, t.players[0].reach_cm),
            (Some(84), Some(338))
        );
        // Everything a .vst holds comes back from one Courtside writes.
        assert_eq!(parse_vst(&write_vst(&t)).expect("read back"), t);
        let a = &t.players[0];
        assert!(a.captain);
        assert_eq!(
            (a.role, a.birth_date.as_deref(), a.height_cm),
            (Some(2), Some("2001-03-14"), Some(192))
        );
        assert!(t.players[1].libero && t.players[1].foreign);
        assert_eq!(t.players[2].role, Some(5));
    }
}
