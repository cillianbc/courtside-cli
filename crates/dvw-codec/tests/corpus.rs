//! Golden-corpus tests (PRD I-01, I-02) over `testdata/`.
//!
//! The corpus is private and may be absent locally. Tests skip when it is
//! missing unless `COURTSIDE_REQUIRE_CORPUS=1`, which CI will set
//! once `testdata/` is a submodule.

#![allow(clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dvw_codec::date::{Date, Written};
use dvw_codec::dvw::DateSource;
use dvw_codec::{Dvw, RawFile, RawVsm};
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    file: Vec<Entry>,
}

#[derive(Deserialize)]
#[allow(dead_code)] // fields are checked for presence; later tests will use them
struct Entry {
    path: String,
    generator: String,
    locale: String,
    encoding: String,
    source: String,
    #[serde(default)]
    notes: String,
}

fn corpus_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    if root.join("manifest.toml").is_file() {
        return Some(root);
    }
    assert!(
        std::env::var("COURTSIDE_REQUIRE_CORPUS").as_deref() != Ok("1"),
        "corpus required but {} has no manifest.toml",
        root.display()
    );
    eprintln!("skipping: no corpus at {}", root.display());
    None
}

fn manifest(root: &Path) -> Manifest {
    let text = std::fs::read_to_string(root.join("manifest.toml")).expect("read manifest");
    toml::from_str(&text).expect("parse manifest")
}

fn corpus_files(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("read corpus dir") {
        let path = entry.expect("dir entry").path();
        // Doctored fixtures (openvolley's dud, truncated, …) have their own
        // manifest and aren't matches: the corpus gates leave them out.
        if path.is_dir() && path.file_name().is_some_and(|n| n == "malformed") {
            continue;
        }
        if path.is_dir() {
            corpus_files(&path, root, out);
        } else if matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("dvw" | "vsm")
        ) {
            let rel = path.strip_prefix(root).expect("under root");
            out.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn manifest_matches_files_on_disk() {
    let Some(root) = corpus_root() else { return };
    let listed: BTreeSet<String> = manifest(&root).file.into_iter().map(|e| e.path).collect();
    let mut on_disk = BTreeSet::new();
    corpus_files(&root, &root, &mut on_disk);
    let unlisted: Vec<_> = on_disk.difference(&listed).collect();
    let missing: Vec<_> = listed.difference(&on_disk).collect();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "not in manifest: {unlisted:#?}\nin manifest but missing: {missing:#?}"
    );
}

fn is_vsm(entry: &Entry) -> bool {
    entry.path.to_ascii_lowercase().ends_with(".vsm")
}

/// I-02, stricter than the PRD's normalised diff: both raw layers must be
/// byte-exact.
#[test]
fn raw_round_trip_byte_exact() {
    let Some(root) = corpus_root() else { return };
    let mut failures = Vec::new();
    for entry in manifest(&root).file {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let out = if is_vsm(&entry) {
            match RawVsm::parse(&bytes) {
                Ok(vsm) => vsm.to_bytes(),
                Err(e) => {
                    failures.push(format!("{}: {e}", entry.path));
                    continue;
                }
            }
        } else {
            RawFile::parse(&bytes).to_bytes()
        };
        if out != bytes {
            let at = out.iter().zip(&bytes).position(|(a, b)| a != b);
            failures.push(format!("{} (first diff at byte {at:?})", entry.path));
        }
    }
    assert!(
        failures.is_empty(),
        "round-trip diffs:\n{}",
        failures.join("\n")
    );
}

/// Every .dvw so far starts with [3DATAVOLLEYSCOUT] and has a [3SCOUT]
/// section. If a new file breaks this, record it in FORMAT-NOTES.md.
#[test]
fn every_dvw_has_header_and_scout_sections() {
    let Some(root) = corpus_root() else { return };
    let mut failures = Vec::new();
    for entry in manifest(&root).file.into_iter().filter(|e| !is_vsm(e)) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let file = RawFile::parse(&bytes);
        let first = file.sections.first().map(|s| s.name.as_str());
        if !file.preamble.is_empty() || first != Some("3DATAVOLLEYSCOUT") {
            failures.push(format!("{}: starts with {first:?}", entry.path));
        }
        if file.section("3SCOUT").is_none() {
            failures.push(format!("{}: no [3SCOUT]", entry.path));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn dvw_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    manifest(root)
        .file
        .into_iter()
        .filter(|e| !is_vsm(e))
        .map(|e| {
            let bytes = std::fs::read(root.join(&e.path)).expect("read corpus file");
            (e.path, bytes)
        })
        .collect()
}

/// I-02 through the typed header model: every typed section is rewritten
/// from its decoded fields and must come out byte-exact.
#[test]
fn typed_round_trip_byte_exact() {
    let Some(root) = corpus_root() else { return };
    let mut failures = Vec::new();
    for (path, bytes) in dvw_files(&root) {
        match Dvw::parse(&bytes).and_then(|d| d.to_bytes()) {
            Ok(out) if out == bytes => {}
            Ok(out) => {
                let at = out.iter().zip(&bytes).position(|(a, b)| a != b);
                failures.push(format!("{path}: first diff at byte {at:?}"));
            }
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Header fields every seed file has. Loosen with a FORMAT-NOTES entry if a
/// new corpus file legitimately breaks one.
#[test]
fn typed_headers_are_sane() {
    let Some(root) = corpus_root() else { return };
    let mut failures = Vec::new();
    for (path, bytes) in dvw_files(&root) {
        let mut fail = |msg: String| failures.push(format!("{path}: {msg}"));
        let dvw = match Dvw::parse(&bytes) {
            Ok(d) => d,
            Err(e) => {
                fail(e.to_string());
                continue;
            }
        };
        if dvw.teams.len() != 2 || dvw.teams.iter().any(|t| t.name().trim().is_empty()) {
            fail(format!("expected two named teams, got {}", dvw.teams.len()));
        }
        match dvw.match_date() {
            None => fail(format!("no date from {:?}", dvw.match_info.date_text())),
            Some((date, DateSource::Serial)) => {
                // The serial must be one reading of the written date.
                let text = dvw.match_info.date_text();
                let agrees = match Date::parse_written(text) {
                    Some(Written::One(d)) => d == date,
                    Some(Written::Two {
                        month_first,
                        day_first,
                    }) => date == month_first || date == day_first,
                    None => false,
                };
                if !agrees {
                    fail(format!("serial date {date} disagrees with {text:?}"));
                }
            }
            Some(_) => {}
        }
        for (side, players) in [("H", &dvw.players_home), ("V", &dvw.players_visiting)] {
            if players.is_empty() {
                fail(format!("no players in [3PLAYERS-{side}]"));
            }
            for p in players {
                // Role may be empty (DataVolley) or `NaN` (VolleyStation), nothing else.
                let role_ok = p.role().is_some() || matches!(p.0.get(13), "" | "NaN");
                if p.number().is_none() || !role_ok || p.last_name().is_empty() {
                    fail(format!(
                        "bad player row in [3PLAYERS-{side}]: {:?}",
                        p.0.to_line()
                    ));
                }
                // Where DataVolley wrote an embedded Unicode copy, it must match the plain text.
                for (plain, embedded) in [(9, 17), (10, 18), (11, 19)] {
                    if let Some(text) = dvw_codec::text::decode_embedded(p.0.get(embedded)) {
                        if text != p.0.get(plain) {
                            fail(format!(
                                "embedded text differs in [3PLAYERS-{side}] column {plain}"
                            ));
                        }
                    }
                }
            }
        }
        let played = dvw
            .sets
            .iter()
            .filter(|s| s.final_score().is_some())
            .count();
        let won: u32 = dvw
            .teams
            .iter()
            .filter_map(|t| t.sets_won())
            .map(u32::from)
            .sum();
        if played == 0 || played as u32 != won {
            fail(format!("{played} scored sets but teams won {won}"));
        }
        if dvw.winning_symbols.for_skill('A').is_none() {
            fail("unreadable [3WINNINGSYMBOLS]".into());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
