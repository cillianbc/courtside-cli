//! Parity with openvolley on parsed code fields (PRD I-03): team, player,
//! skill, evaluation, combinations, zones, custom code, and how each line
//! is classified (point, timeout, substitution, set end).
//!
//! Reads `target/parity/oracle/` from `tools/parity/run.sh`; skips when it
//! is absent unless `COURTSIDE_REQUIRE_PARITY=1`.

#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use dvw_codec::Dvw;
use dvw_codec::dvw::CourtCoding;
use vb_grammar::{Code, Player, Skill, Team};

/// (corpus path, file line, column) differences checked by hand and recorded
/// in FORMAT-NOTES.md, where openvolley is wrong, from the corpus's own `expected.toml` (rows for
/// this crate), so no file name is in the code.
fn accepted() -> Vec<(String, usize, String)> {
    expected("parity_accepted")
        .iter()
        .filter(|r| r.get("crate").and_then(|v| v.as_str()) == Some(env!("CARGO_PKG_NAME")))
        .filter_map(|r| {
            let path = r.get("path")?.as_str()?.to_owned();
            let line = usize::try_from(r.get("line")?.as_integer()?).ok()?;
            Some((path, line, r.get("column")?.as_str()?.to_owned()))
        })
        .collect()
}

/// One section of `testdata/expected.toml` (findings checked by hand; it
/// lives with the private corpus). Empty without it.
fn expected(section: &str) -> Vec<toml::Table> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/expected.toml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let table: toml::Table = text.parse().expect("expected.toml");
    table
        .get(section)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_table().cloned())
        .collect()
}

fn oracle_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/parity/oracle");
    if dir.is_dir() {
        return Some(dir);
    }
    assert!(
        std::env::var("COURTSIDE_REQUIRE_PARITY").as_deref() != Ok("1"),
        "parity required but {} is missing; run tools/parity/run.sh",
        dir.display()
    );
    eprintln!("skipping: no oracle output at {}", dir.display());
    None
}

fn skill_name(skill: Skill) -> &'static str {
    match skill {
        Skill::Serve => "Serve",
        Skill::Reception => "Reception",
        Skill::Set => "Set",
        Skill::Attack => "Attack",
        Skill::Block => "Block",
        Skill::Dig => "Dig",
        Skill::Freeball => "Freeball",
        Skill::Green => "",
    }
}

fn digit(c: Option<char>) -> String {
    c.and_then(|c| c.to_digit(10))
        .map(|d| d.to_string())
        .unwrap_or_default()
}

fn flag(b: bool) -> String {
    if b { "TRUE" } else { "FALSE" }.to_owned()
}

/// Our fields for one code, in openvolley's presentation. Columns left out
/// are not compared for this kind of code.
fn ours(
    code: &Code,
    team_names: &[String; 2],
    coding: Option<CourtCoding>,
) -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    let team = |t: Team| match t {
        Team::Home => team_names[0].clone(),
        Team::Visiting => team_names[1].clone(),
    };
    m.insert("team", code.team().map(team).unwrap_or_default());
    m.insert("timeout", flag(matches!(code, Code::Timeout { .. })));
    m.insert("point", flag(matches!(code, Code::Point { .. })));
    m.insert("end_of_set", flag(matches!(code, Code::SetEnd { .. })));
    // openvolley counts a setter change without `>LUp` as a substitution.
    let substitution = match code {
        Code::Substitution { .. } => true,
        Code::SetterPlayer { note, .. } => !note.to_ascii_lowercase().contains(">lup"),
        _ => false,
    };
    m.insert("substitution", flag(substitution));

    if let Code::Touch(t) = code
        && t.skill != Skill::Green
    {
        let combination = t.combination().unwrap_or_default();
        let only = |skill: Skill, v: String| if t.skill == skill { v } else { String::new() };
        m.insert(
            "player_number",
            match &t.player {
                Player::Number(d) => d.value().map(|v| v.to_string()).unwrap_or_default(),
                Player::Unknown => String::new(),
            },
        );
        m.insert("skill", skill_name(t.skill).to_owned());
        m.insert(
            "evaluation_code",
            t.evaluation().map(String::from).unwrap_or_default(),
        );
        m.insert("attack_code", only(Skill::Attack, combination.clone()));
        m.insert("set_code", only(Skill::Set, combination));
        m.insert(
            "set_type",
            only(Skill::Set, t.target().map(String::from).unwrap_or_default()),
        );
        m.insert("start_zone", digit(t.start_zone()));
        let cones = t.skill == Skill::Attack && coding == Some(CourtCoding::Cones);
        m.insert(
            "end_zone",
            if cones {
                String::new()
            } else {
                digit(t.end_zone())
            },
        );
        m.insert(
            "end_subzone",
            t.end_subzone().map(String::from).unwrap_or_default(),
        );
        m.insert("custom_code", t.custom().to_owned());
    }
    m
}

#[test]
fn parsed_codes_match_openvolley() {
    let accepted = accepted();
    let Some(oracle) = oracle_dir() else { return };
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let mut report = Vec::new();
    let mut compared = 0usize;
    for entry in std::fs::read_dir(&oracle).expect("read oracle dir") {
        let csv_path = entry.expect("dir entry").path();
        let Some(rel) = csv_path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".csv"))
            .map(|r| r.replace("__", "/"))
        else {
            continue;
        };
        let dvw = Dvw::parse(&std::fs::read(corpus.join(&rel)).expect("read corpus file"))
            .expect("parse corpus file");
        let names = [0, 1].map(|i| dvw.teams.get(i).map(|t| t.name()).unwrap_or_default());
        let compare_teams = openvolley_names(&rel, names.clone());
        let names = compare_teams.clone().unwrap_or(names);
        let coding = dvw.match_info.court_coding();
        let first_line = dvw.raw().header_line("3SCOUT").expect("[3SCOUT]") + 1;

        let mut reader = csv::Reader::from_path(&csv_path).expect("open oracle csv");
        let headers = reader.headers().expect("csv headers").clone();
        let rows: HashMap<usize, csv::StringRecord> = reader
            .records()
            .map(|r| {
                let r = r.expect("csv record");
                (r[0].parse().expect("file_line_number"), r)
            })
            .collect();

        let mut diffs: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
        for (i, line) in dvw.scout.iter().enumerate() {
            let file_line = first_line + i;
            let Some(theirs) = rows.get(&file_line) else {
                continue;
            };
            let mut mine = ours(&Code::parse(line.code()), &names, coding);
            if compare_teams.is_none() {
                mine.remove("team");
            }
            for (col, value) in headers.iter().zip(theirs.iter()) {
                let Some(my_value) = mine.get(col) else {
                    continue;
                };
                compared += 1;
                if my_value != value
                    && !accepted
                        .iter()
                        .any(|(p, l, c)| *p == rel && *l == file_line && c == col)
                {
                    let d = diffs.entry(col.to_owned()).or_default();
                    d.0 += 1;
                    if d.1.len() < 3 {
                        d.1.push(format!(
                            "line {file_line} {:?}: ours {my_value:?}, openvolley {value:?}",
                            line.code()
                        ));
                    }
                }
            }
        }
        for (col, (n, examples)) in diffs {
            report.push(format!(
                "{rel}: {col}: {n} diffs\n    {}",
                examples.join("\n    ")
            ));
        }
    }
    eprintln!("compared {compared} values");
    assert!(compared > 0, "nothing compared");
    assert!(report.is_empty(), "parity diffs:\n{}", report.join("\n"));
}

/// Team names as openvolley reports them: trimmed, and told apart with
/// " (home)" / " (visiting)" when both teams have the same name. `None`
/// for a file whose names neither reader can decode (double-encoded:
/// Windows-1250 read as 1252 and saved as UTF-8), so they aren't compared.
fn openvolley_names(rel: &str, names: [String; 2]) -> Option<[String; 2]> {
    let undecodable = expected("undecodable_names");
    if undecodable
        .iter()
        .any(|r| r.get("path").and_then(|v| v.as_str()) == Some(rel))
    {
        return None;
    }
    let [home, visiting] = names.map(|n| n.trim().to_owned());
    Some(if home == visiting {
        [format!("{home} (home)"), format!("{visiting} (visiting)")]
    } else {
        [home, visiting]
    })
}
