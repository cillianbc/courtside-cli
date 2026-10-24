//! Phase 2 task 3: the fold reproduces the `[3SCOUT]` columns DataVolley
//! and VolleyStation derive themselves (set 8, setter positions 9–10,
//! players on court 14–25) on every line of the corpus, except lines where
//! the file contradicts itself (listed in `ACCEPTED`, explained in
//! FORMAT-NOTES "Engine fold").
//!
//! Skips when `testdata/` is absent unless `COURTSIDE_REQUIRE_CORPUS=1`.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use dvw_codec::Dvw;
use serde::Deserialize;
use vb_engine::{EventKind, IdGen, Match, fold};

#[derive(Deserialize)]
struct Manifest {
    file: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
}

/// (file name prefix, column group, number of lines) where the file's own
/// columns contradict its codes or player table; see FORMAT-NOTES.
const ACCEPTED: &[(&str, &str, usize)] = &[
    // Setter position written as 0 for ten lines with no code changing it.
    ("&corpus-file", "setter position", 10),
    // Home court columns don't match the player table's set 1 lineup.
    ("&corpus-file", "home court", 274),
];

fn corpus() -> Option<(std::path::PathBuf, Manifest)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let Ok(text) = std::fs::read_to_string(root.join("manifest.toml")) else {
        assert!(
            std::env::var("COURTSIDE_REQUIRE_CORPUS").as_deref() != Ok("1"),
            "corpus required but missing"
        );
        eprintln!("skipping: no corpus");
        return None;
    };
    Some((root, toml::from_str(&text).expect("parse manifest")))
}

#[test]
fn fold_reproduces_the_files_own_columns() {
    let Some((root, manifest)) = corpus() else {
        return;
    };
    let mut mismatches: BTreeMap<(String, &str), Vec<String>> = BTreeMap::new();
    let mut checked = 0usize;
    for entry in manifest.file.iter().filter(|e| e.path.ends_with(".dvw")) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        let volleystation = dvw.header.get("GENERATOR-PRG") == Some("VolleyStation");
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let events = m.log.visible();
        let folded = fold(&m);
        let name = entry
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&entry.path)
            .to_owned();
        for (i, line) in dvw.scout.iter().enumerate() {
            checked += 1;
            let mut st = &folded.states[i];
            // VolleyStation writes a rotation line with the substitutions
            // that follow it already applied.
            if volleystation && events[i].kind() == EventKind::SetterPosition {
                let mut j = i;
                while events
                    .get(j + 1)
                    .is_some_and(|e| e.kind() == EventKind::Substitution)
                {
                    j += 1;
                }
                st = &folded.states[j];
            }
            let mut check = |group: &'static str, ok: bool, detail: String| {
                if !ok {
                    mismatches
                        .entry((name.clone(), group))
                        .or_default()
                        .push(format!("line {i} {:?}: {detail}", line.code()));
                }
            };
            check(
                "set",
                line.set_number() == Some(st.set),
                format!("file {:?} engine {}", line.set_number(), st.set),
            );
            let file_sp = (line.home_setter_position(), line.visiting_setter_position());
            let ours = (st.setter_position.home, st.setter_position.visiting);
            let comparable = |f: Option<u8>, o: Option<u8>| o.is_none() || f == o;
            check(
                "setter position",
                comparable(file_sp.0, ours.0) && comparable(file_sp.1, ours.1),
                format!("file {file_sp:?} engine {ours:?}"),
            );
            check(
                "home court",
                line.home_players() == st.court.home,
                format!("file {:?} engine {:?}", line.home_players(), st.court.home),
            );
            check(
                "visiting court",
                line.visiting_players() == st.court.visiting,
                format!(
                    "file {:?} engine {:?}",
                    line.visiting_players(),
                    st.court.visiting
                ),
            );
        }
    }
    assert!(checked > 0, "no scout lines");
    let mut unexpected = Vec::new();
    for ((file, group), lines) in &mismatches {
        let accepted = ACCEPTED
            .iter()
            .any(|(f, g, n)| file.starts_with(f) && g == group && *n == lines.len());
        eprintln!(
            "{file} {group}: {} lines{}",
            lines.len(),
            if accepted { " (accepted)" } else { "" }
        );
        if !accepted {
            unexpected.push(format!(
                "{file} {group}: {} lines, first: {}",
                lines.len(),
                lines
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
    }
    eprintln!("{checked} lines checked");
    assert!(
        unexpected.is_empty(),
        "fold differs from the file:\n{}",
        unexpected.join("\n")
    );
}

/// PRD: a full match (~1,500 events) folds in under 5 ms. Timed only in
/// release builds (`cargo test --release`).
#[test]
fn fold_is_fast_enough_to_run_on_every_change() {
    if cfg!(debug_assertions) {
        eprintln!("skipping: debug build");
        return;
    }
    let Some((root, manifest)) = corpus() else {
        return;
    };
    let mut slowest = std::time::Duration::ZERO;
    for entry in manifest.file.iter().filter(|e| e.path.ends_with(".dvw")) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let folded = fold(&m);
            slowest = slowest.max(start.elapsed());
            assert_eq!(folded.states.len(), dvw.scout.len());
        }
    }
    eprintln!("slowest fold: {slowest:?}");
    assert!(slowest < std::time::Duration::from_millis(5), "{slowest:?}");
}

/// Phase 2 task 5: `.dvw` → engine → `.dvw` gives back the same bytes,
/// headers on every file,
/// except on lines where the file's own derived columns contradict it
/// (see `ACCEPTED` above and the VolleyStation rotation-line quirk), which
/// the engine writes corrected. Only derived columns may differ.
#[test]
fn dvw_round_trips_through_the_engine() {
    let Some((root, manifest)) = corpus() else {
        return;
    };
    let mut report = Vec::new();
    let mut files = 0;
    for entry in manifest.file.iter().filter(|e| e.path.ends_with(".dvw")) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let out = m.to_dvw(&dvw);
        files += 1;
        assert_eq!(out.scout.len(), dvw.scout.len(), "{}", entry.path);
        // Everything outside [3SCOUT], including the regenerated [3SET]
        // scores and [3TEAMS] sets won, is byte-exact on every file.
        let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
        let mut headers = out.clone();
        headers.scout = dvw.scout.clone();
        let changed = player_rows_changed(&bytes, &headers.to_bytes().expect("write"));
        if changed != Some(allowed(HEADER_REWRITES, name)) {
            report.push(format!("{}: headers differ: {changed:?}", entry.path));
        }
        // And they really are regenerated: blank set 1's scores and
        // duration, both teams' sets won and every `*` (came on as a
        // substitute) in the template, and the engine writes them back.
        // VolleyStation's durations are kept as written, so not blanked.
        let volleystation = dvw.header.get("GENERATOR-PRG") == Some("VolleyStation");
        let mut blank = dvw.clone();
        let blanked = if volleystation { 1..5 } else { 1..6 };
        for f in &mut blank.sets[0].0.0[blanked] {
            f.clear();
        }
        for t in &mut blank.teams {
            t.0.0[2].clear();
        }
        for p in blank
            .players_home
            .iter_mut()
            .chain(&mut blank.players_visiting)
        {
            for f in p.0.0.iter_mut().skip(3).take(5) {
                if f.trim() == "*" {
                    f.clear();
                }
            }
        }
        let regen_match = Match::from_dvw(&blank, &mut IdGen::new(0));
        let mut regen = regen_match.to_dvw(&blank);
        regen.scout = dvw.scout.clone();
        let changed = player_rows_changed(&bytes, &regen.to_bytes().expect("write"));
        let expected = allowed(HEADER_REWRITES, name) + allowed(SUBSTITUTES_ONLY_ON_SHEET, name);
        if changed != Some(expected) {
            report.push(format!(
                "{}: regenerated headers differ: {changed:?}",
                entry.path
            ));
        }
        let mut changed = 0;
        for (i, (a, b)) in dvw.scout.iter().zip(&out.scout).enumerate() {
            let (a, b) = (&a.0.0, &b.0.0);
            if a == b {
                continue;
            }
            changed += 1;
            let cols: Vec<usize> = (0..a.len().max(b.len()))
                .filter(|&c| a.get(c) != b.get(c))
                .collect();
            if cols.iter().any(|c| !(8..=25).contains(c)) {
                report.push(format!(
                    "{} line {i}: non-derived columns {cols:?}",
                    entry.path
                ));
            }
        }
        eprintln!("{name}: {changed} lines rewritten");
        if changed == 0 {
            let written = out.to_bytes().expect("write");
            if written != bytes {
                report.push(format!(
                    "{}: scout lines equal but bytes differ",
                    entry.path
                ));
            }
        } else if !ROUND_TRIP_REWRITES
            .iter()
            .any(|(f, n)| name.starts_with(f) && *n == changed)
        {
            report.push(format!("{}: {changed} lines rewritten", entry.path));
        }
    }
    assert!(files > 0, "no .dvw files");
    assert!(report.is_empty(), "round trip:\n{}", report.join("\n"));
}

/// Lines outside `[3SCOUT]` that differ, if all are player-table rows
/// (`Some(n)`); `None` if any other line differs or the line count changes.
fn player_rows_changed(a: &[u8], b: &[u8]) -> Option<usize> {
    let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    if a.len() != b.len() {
        return None;
    }
    let mut section = "";
    let mut n = 0;
    for (x, y) in a.iter().zip(&b) {
        if x.starts_with('[') {
            section = x;
        }
        if x != y {
            if !section.starts_with("[3PLAYERS-") {
                return None;
            }
            n += 1;
        }
    }
    Some(n)
}

fn allowed(list: &[(&str, usize)], name: &str) -> usize {
    list.iter()
        .filter(|(f, _)| name.starts_with(f))
        .map(|(_, n)| n)
        .sum()
}

/// Player rows the engine rewrites: a libero who touches the ball in set 1
/// is marked as having played it, though the team sheet says not (the file
/// whose set 1 sheet is known to be wrong, see `ACCEPTED`).
const HEADER_REWRITES: &[(&str, usize)] = &[("&corpus-file", 1)];

/// Player rows whose `*` (came on as a substitute) only the team sheet
/// knows: no substitution code brings the player on (FORMAT-NOTES
/// "Validator": missing substitution codes), so blanking it loses it.
const SUBSTITUTES_ONLY_ON_SHEET: &[(&str, usize)] =
    &[("&corpus-file", 1), ("&corpus-file", 1), ("&corpus-file", 2)];

/// Files whose derived columns the engine rewrites, and how many lines:
/// the `ACCEPTED` glitches plus VolleyStation rotation lines written with
/// the following substitution already applied.
const ROUND_TRIP_REWRITES: &[(&str, usize)] =
    &[("&corpus-file", 10), ("&corpus-file", 274), ("2024-03-23 corpus-file", 7)];

/// The validator runs on every corpus file. Prints issue counts per rule
/// and message so false positives show up in review; real scouting slips
/// are expected, so nothing is asserted beyond running cleanly.
#[test]
fn validator_runs_on_the_corpus() {
    let Some((root, manifest)) = corpus() else {
        return;
    };
    // COURTSIDE_VALIDATOR_DETAIL=<text> also prints every issue whose
    // message contains <text>, with its file line and the codes around it.
    let detail = std::env::var("COURTSIDE_VALIDATOR_DETAIL").ok();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in manifest.file.iter().filter(|e| e.path.ends_with(".dvw")) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let first_line = dvw.raw().header_line("3SCOUT").expect("[3SCOUT]") + 1;
        for issue in vb_engine::validate::validate(&m) {
            if let Some(pat) = &detail
                && issue.message.contains(pat.as_str())
            {
                let line = issue.index + first_line;
                let from = issue.index.saturating_sub(4);
                let context: Vec<&str> = (from..=issue.index + 1)
                    .filter_map(|i| dvw.scout.get(i).map(|l| l.code()))
                    .collect();
                eprintln!(
                    "{} line {line}: {}\n    {}",
                    entry.path,
                    issue.message,
                    context.join("  ")
                );
            }
            let shape: String = issue
                .message
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect();
            *counts
                .entry(format!("{:?} {:?}: {shape}", issue.severity, issue.rule))
                .or_default() += 1;
        }
    }
    for (k, n) in &counts {
        eprintln!("{n:6} {k}");
    }
}
