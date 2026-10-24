//! Phase 2 exit criterion: touch-level output matches openvolley on the
//! whole corpus. For every skill touch, the fold's set, score, setter
//! positions, rally winner, serving team, phase and winning attack equal
//! openvolley's derived columns, and its rallies and team touches group
//! touches the same way as openvolley's `point_id` and `team_touch_id`.
//!
//! Reads `target/parity/oracle/` from `tools/parity/run.sh`; skips when it
//! is absent unless `COURTSIDE_REQUIRE_PARITY=1`.

#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use dvw_codec::Dvw;
use vb_engine::{IdGen, Match, Phase, fold};
use vb_grammar::Team;

/// (corpus path, file line, column) differences checked by hand and recorded
/// in FORMAT-NOTES.md.
const ACCEPTED: &[(&str, usize, &str)] = &[
    // A serve in the same team touch as a reception (a missing point code
    // before it): openvolley calls it reception phase; a serve is a serve.
    (
        "corpus-file",
        75,
        "phase",
    ),
    (
        "corpus-file",
        681,
        "phase",
    ),
    (
        "corpus-file",
        719,
        "phase",
    ),
    (
        "corpus-file",
        544,
        "phase",
    ),
    (
        "corpus-file",
        683,
        "phase",
    ),
    (
        "corpus-file",
        1009,
        "phase",
    ),
    (
        "corpus-file",
        273,
        "phase",
    ),
];

const SKILLS: &[&str] = &[
    "Serve",
    "Reception",
    "Set",
    "Attack",
    "Block",
    "Dig",
    "Freeball",
];

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

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_default()
}

/// Checks that two id columns split rows into the same groups.
#[derive(Default)]
struct Grouping {
    theirs_to_ours: HashMap<String, usize>,
    ours_to_theirs: HashMap<usize, String>,
}

impl Grouping {
    fn same(&mut self, theirs: &str, ours: Option<usize>) -> bool {
        let Some(ours) = ours else { return false };
        let a = *self.theirs_to_ours.entry(theirs.to_owned()).or_insert(ours);
        let b = self
            .ours_to_theirs
            .entry(ours)
            .or_insert_with(|| theirs.to_owned())
            .clone();
        a == ours && b == theirs
    }
}

#[test]
fn match_state_matches_openvolley() {
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
        let compare_teams = openvolley_names(&rel, names.clone()).is_some();
        let names = openvolley_names(&rel, names.clone()).unwrap_or(names);
        let team_name = |t: Option<Team>| match t {
            Some(Team::Home) => names[0].clone(),
            Some(Team::Visiting) => names[1].clone(),
            None => String::new(),
        };
        let first_line = dvw.raw().header_line("3SCOUT").expect("[3SCOUT]") + 1;
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let folded = fold(&m);

        let mut reader = csv::Reader::from_path(&csv_path).expect("open oracle csv");
        let headers = reader.headers().expect("csv headers").clone();
        let col = |name: &str| {
            headers
                .iter()
                .position(|h| h == name)
                .unwrap_or_else(|| panic!("oracle has no {name} column; rerun tools/parity/run.sh"))
        };
        let (c_line, c_skill, c_point, c_touch) = (
            col("file_line_number"),
            col("skill"),
            col("point_id"),
            col("team_touch_id"),
        );
        let mut points = Grouping::default();
        let mut touches = Grouping::default();
        let mut diffs: BTreeMap<&str, (usize, Vec<String>)> = BTreeMap::new();
        for record in reader.records() {
            let r = record.expect("csv record");
            if !SKILLS.contains(&&r[c_skill]) {
                continue;
            }
            let file_line: usize = r[c_line].parse().expect("file_line_number");
            let i = file_line - first_line;
            let st = &folded.states[i];
            let rally = st.rally.and_then(|k| folded.rallies.get(k));
            let ours: [(&str, String); 11] = [
                ("set_number", st.set.to_string()),
                ("home_team_score", opt(rally.map(|r| r.score_after.home))),
                (
                    "visiting_team_score",
                    opt(rally.map(|r| r.score_after.visiting)),
                ),
                (
                    "home_score_start_of_point",
                    opt(rally.map(|r| r.score_before.home)),
                ),
                (
                    "visiting_score_start_of_point",
                    opt(rally.map(|r| r.score_before.visiting)),
                ),
                ("home_setter_position", opt(st.setter_position.home)),
                ("visiting_setter_position", opt(st.setter_position.visiting)),
                ("point_won_by", team_name(rally.and_then(|r| r.won_by))),
                ("serving_team", team_name(rally.and_then(|r| r.serving))),
                (
                    "phase",
                    match st.phase {
                        Some(Phase::Serve) => "Serve",
                        Some(Phase::Reception) => "Reception",
                        Some(Phase::Transition) => "Transition",
                        None => "",
                    }
                    .to_owned(),
                ),
                (
                    "winning_attack",
                    if st.winning_attack { "TRUE" } else { "FALSE" }.to_owned(),
                ),
            ];
            let mut check = |name: &'static str, ok: bool, detail: String| {
                compared += 1;
                if !ok && !ACCEPTED.contains(&(rel.as_str(), file_line, name)) {
                    let d = diffs.entry(name).or_default();
                    d.0 += 1;
                    if d.1.len() < 3 {
                        d.1.push(format!(
                            "line {file_line} {:?}: {detail}",
                            dvw.scout[i].code()
                        ));
                    }
                }
            };
            for (name, mine) in ours {
                if !compare_teams && matches!(name, "point_won_by" | "serving_team") {
                    continue;
                }
                let theirs = &r[col(name)];
                check(
                    name,
                    mine == theirs,
                    format!("ours {mine:?}, openvolley {theirs:?}"),
                );
            }
            check(
                "point_id",
                points.same(&r[c_point], st.rally),
                format!("rally {:?} vs point_id {}", st.rally, &r[c_point]),
            );
            check(
                "team_touch_id",
                touches.same(&r[c_touch], st.team_touch),
                format!("team touch {:?} vs {}", st.team_touch, &r[c_touch]),
            );
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
    if rel == "corpus-file" {
        return None;
    }
    let [home, visiting] = names.map(|n| n.trim().to_owned());
    Some(if home == visiting {
        [format!("{home} (home)"), format!("{visiting} (visiting)")]
    } else {
        [home, visiting]
    })
}
