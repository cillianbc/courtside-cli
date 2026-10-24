//! Parity with openvolley on raw [3SCOUT] columns (PRD I-03).
//!
//! `tools/parity/run.sh` writes openvolley's view of each corpus .dvw to
//! `target/parity/oracle/` (Docker, R, pinned datavolley). This test skips
//! when that directory is absent unless `COURTSIDE_REQUIRE_PARITY=1`.
//!
//! Every column is mapped the way openvolley presents it (`p` → `Breakpoint`,
//! `-1-1` → empty, `15.33.42` → `15:33:42`) and must match exactly. A known,
//! checked disagreement goes in docs/FORMAT-NOTES.md and in `ACCEPTED` below.

#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::path::{Path, PathBuf};

use dvw_codec::Dvw;
use dvw_codec::scout::{AttackPhase, PointPhase, ScoutLine};

/// (corpus path, file line, column) differences checked by hand and recorded
/// in FORMAT-NOTES.md, where openvolley is wrong.
const ACCEPTED: &[(&str, usize, &str)] = &[];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oracle_dir() -> Option<PathBuf> {
    let dir = workspace().join("target/parity/oracle");
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

fn opt<T: Display>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_default()
}

/// Our columns, in openvolley's presentation.
fn ours(line: &ScoutLine) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        m.insert(k.to_owned(), v);
    };
    put("code", line.code().to_owned());
    put(
        "point_phase",
        match line.point_phase() {
            Some(PointPhase::Breakpoint) => "Breakpoint".into(),
            Some(PointPhase::Sideout) => "Sideout".into(),
            None => line.0.get(1).to_owned(),
        },
    );
    put(
        "attack_phase",
        match line.attack_phase() {
            Some(AttackPhase::Reception) => "Reception".into(),
            Some(AttackPhase::TransitionSideout) => "Transition sideout".into(),
            Some(AttackPhase::TransitionBreakpoint) => "Transition breakpoint".into(),
            None => line.0.get(2).to_owned(),
        },
    );
    put("start_coordinate", opt(line.start_coordinate()));
    put("mid_coordinate", opt(line.mid_coordinate()));
    put("end_coordinate", opt(line.end_coordinate()));
    put(
        "time",
        opt(line
            .time_hms()
            .map(|(h, m, s)| format!("{h:02}:{m:02}:{s:02}"))),
    );
    put("video_file_number", opt(line.video_file_number()));
    put("video_time", opt(line.video_time()));
    for (i, p) in line.home_players().into_iter().enumerate() {
        put(&format!("home_p{}", i + 1), opt(p));
    }
    for (i, p) in line.visiting_players().into_iter().enumerate() {
        put(&format!("visiting_p{}", i + 1), opt(p));
    }
    m
}

#[test]
fn scout_columns_match_openvolley() {
    let Some(oracle) = oracle_dir() else { return };
    let corpus = workspace().join("testdata");
    let mut report = Vec::new();
    let mut files = 0;
    for entry in std::fs::read_dir(&oracle).expect("read oracle dir") {
        let csv_path = entry.expect("dir entry").path();
        let Some(name) = csv_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(rel) = name.strip_suffix(".csv").map(|r| r.replace("__", "/")) else {
            continue;
        };
        files += 1;
        let bytes = std::fs::read(corpus.join(&rel)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        let first_line = dvw.raw().header_line("3SCOUT").expect("[3SCOUT]") + 1;

        let mut reader = csv::Reader::from_path(&csv_path).expect("open oracle csv");
        let headers = reader.headers().expect("csv headers").clone();
        let mut oracle_rows: HashMap<usize, csv::StringRecord> = HashMap::new();
        for record in reader.records() {
            let record = record.expect("csv record");
            let line: usize = record[0].parse().expect("file_line_number");
            oracle_rows.insert(line, record);
        }

        let mut diffs: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
        for (i, scout) in dvw.scout.iter().enumerate() {
            let file_line = first_line + i;
            let Some(theirs) = oracle_rows.remove(&file_line) else {
                if !scout.code().trim().is_empty() {
                    diffs
                        .entry("<row missing in openvolley>".into())
                        .or_default()
                        .0 += 1;
                }
                continue;
            };
            let mine = ours(scout);
            for (col, value) in headers.iter().zip(theirs.iter()).skip(1) {
                // Parsed-code columns belong to vb-grammar's parity test.
                let Some(my_value) = mine.get(col).map(String::as_str) else {
                    continue;
                };
                if my_value != value && !ACCEPTED.contains(&(rel.as_str(), file_line, col)) {
                    let d = diffs.entry(col.to_owned()).or_default();
                    d.0 += 1;
                    if d.1.len() < 3 {
                        d.1.push(format!(
                            "line {file_line}: ours {my_value:?}, openvolley {value:?}"
                        ));
                    }
                }
            }
        }
        if !oracle_rows.is_empty() {
            diffs
                .entry("<row only in openvolley>".into())
                .or_default()
                .0 += oracle_rows.len();
        }
        for (col, (n, examples)) in diffs {
            report.push(format!(
                "{rel}: {col}: {n} diffs\n    {}",
                examples.join("\n    ")
            ));
        }
    }
    assert!(files > 0, "oracle directory has no CSV files");
    assert!(report.is_empty(), "parity diffs:\n{}", report.join("\n"));
}
