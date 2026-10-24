//! PRD task 6: `format(parse(s)) == s` for every scout line in the corpus,
//! and every line is a recognised code.
//!
//! Skips when `testdata/` is absent unless `COURTSIDE_REQUIRE_CORPUS=1`.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use dvw_codec::Dvw;
use serde::Deserialize;
use vb_grammar::Code;

#[derive(Deserialize)]
struct Manifest {
    file: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    path: String,
}

fn kind(code: &Code) -> &'static str {
    match code {
        Code::Touch(_) => "touch",
        Code::Point { .. } => "point",
        Code::SetterPosition { .. } => "setter position",
        Code::SetterPlayer { .. } => "setter player",
        Code::Substitution { .. } => "substitution",
        Code::Timeout { .. } => "timeout",
        Code::SetEnd { .. } => "set end",
        Code::Comment(_) => "comment",
        Code::Unrecognised(_) => "unrecognised",
    }
}

#[test]
fn every_scout_code_parses_and_round_trips() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let Ok(text) = std::fs::read_to_string(root.join("manifest.toml")) else {
        assert!(
            std::env::var("COURTSIDE_REQUIRE_CORPUS").as_deref() != Ok("1"),
            "corpus required but missing"
        );
        eprintln!("skipping: no corpus");
        return;
    };
    let manifest: Manifest = toml::from_str(&text).expect("parse manifest");
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for entry in manifest.file.iter().filter(|e| e.path.ends_with(".dvw")) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        let first = dvw.raw().header_line("3SCOUT").expect("[3SCOUT]") + 1;
        for (i, line) in dvw.scout.iter().enumerate() {
            let s = line.code();
            if s.is_empty() {
                continue;
            }
            let code = Code::parse(s);
            *counts.entry(kind(&code)).or_default() += 1;
            let out = code.to_string();
            if matches!(code, Code::Unrecognised(_)) || out != s {
                failures.push(format!(
                    "{} line {}: {s:?} -> {code:?}",
                    entry.path,
                    first + i
                ));
            }
        }
    }
    eprintln!("{counts:?}");
    assert!(counts.values().sum::<usize>() > 0, "no scout codes found");
    assert!(
        failures.is_empty(),
        "{} codes failed:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Typing a complete code gives that code back: expansion only fills what
/// is missing. Checked on every touch, point, setter and substitution code
/// in the corpus that has a type and an evaluation.
#[test]
fn complete_codes_expand_to_themselves() {
    use vb_grammar::expand::{Defaults, expand};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata");
    let Ok(text) = std::fs::read_to_string(root.join("manifest.toml")) else {
        assert!(
            std::env::var("COURTSIDE_REQUIRE_CORPUS").as_deref() != Ok("1"),
            "corpus required but missing"
        );
        eprintln!("skipping: no corpus");
        return;
    };
    let manifest: Manifest = toml::from_str(&text).expect("parse manifest");
    let (mut checked, mut changed) = (0usize, Vec::new());
    for entry in manifest.file.iter().filter(|e| e.path.ends_with(".dvw")) {
        let bytes = std::fs::read(root.join(&entry.path)).expect("read corpus file");
        let dvw = Dvw::parse(&bytes).expect("parse corpus file");
        for line in &dvw.scout {
            let s = line.code();
            let complete = match Code::parse(s) {
                Code::Touch(t) => t.skill_type().is_some() && t.evaluation().is_some(),
                Code::Point { .. } | Code::SetterPlayer { .. } | Code::Substitution { .. } => {
                    !s.contains('>')
                }
                Code::Timeout { .. } | Code::SetEnd { .. } => true,
                _ => false,
            };
            if !complete {
                continue;
            }
            checked += 1;
            let got = expand(s, None, &Defaults::default())
                .map(|e| e.codes.iter().map(ToString::to_string).collect::<Vec<_>>());
            if got.as_deref() != Ok(&[s.to_owned()][..]) {
                changed.push(format!("{} {s:?} -> {got:?}", entry.path));
            }
        }
    }
    assert!(checked > 10_000, "only {checked} codes checked");
    assert!(
        changed.is_empty(),
        "{} of {checked} changed, e.g. {:?}",
        changed.len(),
        &changed[..changed.len().min(10)]
    );
}
