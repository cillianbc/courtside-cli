//! Phase 2 task 7: the validator against openvolley's `dv_validate` on a
//! bad-file set (terms in CONTEXT.md).
//!
//! `testdata/bad/recipes.toml` (local, like the corpus) lists the bad
//! files: a corpus source file, a slip class and a seed (or a `[3SCOUT]`
//! file line). `bad_files_are_generated` plants the slips and writes
//! `target/bad/<class>-<n>.dvw` plus `labels.toml`; `tools/parity/run.sh
//! validate` runs `dv_validate` on them and on the clean sources;
//! `validator_beats_dv_validate` compares.
//!
//! Issues already reported at the mapped line of the clean source (same
//! rule for Courtside, same message for `dv_validate`) are ignored. Of the
//! rest, a hit is a Warning or Error from the slip's line up to the first
//! serve of the next rally; anything else is noise. Gate (FORMAT-NOTES "Decisions"): recall at least
//! `dv_validate`'s in every slip class and higher overall, and no more
//! noise.
//!
//! Skips without the corpus unless `COURTSIDE_REQUIRE_CORPUS=1`, and the
//! comparison skips without the oracle output unless
//! `COURTSIDE_REQUIRE_PARITY=1`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod slips;

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use dvw_codec::Dvw;
use serde::Deserialize;
use slips::{Planted, SlipClass, plant};
use vb_engine::validate::{Issue, Severity, validate};
use vb_engine::{IdGen, Match, fold};
use vb_grammar::{Code, Skill};

#[derive(Deserialize)]
struct Recipes {
    bad: Vec<Recipe>,
}

#[derive(Deserialize)]
struct Recipe {
    /// Corpus path, relative to `testdata/`.
    source: String,
    slip: SlipClass,
    #[serde(default)]
    seed: u64,
    /// File line of the slip's site in the source, instead of a seed.
    line: Option<usize>,
}

struct Bad {
    name: String,
    recipe: Recipe,
    planted: Planted,
    bytes: Vec<u8>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn out_dir() -> PathBuf {
    root().join("target/bad")
}

/// The bad-file set, or `None` without the corpus.
fn bad_files() -> Option<Vec<Bad>> {
    let path = root().join("testdata/bad/recipes.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        assert!(
            std::env::var("COURTSIDE_REQUIRE_CORPUS").as_deref() != Ok("1"),
            "corpus required but {} is missing",
            path.display()
        );
        eprintln!("skipping: no testdata/bad/recipes.toml");
        return None;
    };
    let recipes: Recipes = toml::from_str(&text).expect("parse recipes");
    let mut counts: BTreeMap<SlipClass, usize> = BTreeMap::new();
    let mut out = Vec::new();
    let mut failures = Vec::new();
    for recipe in recipes.bad {
        let n = counts.entry(recipe.slip).or_default();
        *n += 1;
        let name = format!("{}-{n}", recipe.slip.name());
        let bytes =
            std::fs::read(root().join("testdata").join(&recipe.source)).expect("read source");
        let source = Dvw::parse(&bytes).expect("parse source");
        let index = recipe.line.map(|l| l - first_scout_line(&source));
        match plant(&source, recipe.slip, recipe.seed, index) {
            Ok(planted) => {
                let bytes = planted.dvw.to_bytes().expect("write bad file");
                out.push(Bad {
                    name,
                    recipe,
                    planted,
                    bytes,
                });
            }
            Err(e) => failures.push(format!("{name} ({}): {e}", recipe.source)),
        }
    }
    assert!(
        failures.is_empty(),
        "recipes failed:\n{}",
        failures.join("\n")
    );
    Some(out)
}

/// File line (1-based) of the first `[3SCOUT]` line.
fn first_scout_line(dvw: &Dvw) -> usize {
    dvw.raw().header_line("3SCOUT").expect("[3SCOUT]") + 1
}

#[test]
fn bad_files_are_generated() {
    let Some(bad) = bad_files() else { return };
    let dir = out_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create target/bad");
    let mut labels = String::new();
    for b in &bad {
        std::fs::write(dir.join(format!("{}.dvw", b.name)), &b.bytes).expect("write");
        let dvw = Dvw::parse(&b.bytes).expect("reparse bad file");
        let line = b.planted.slip + first_scout_line(&dvw);
        let _ = writeln!(
            labels,
            "[{}]\nsource = {:?}\nslip = {:?}\nline = {line}\ncode = {:?}\nexpected_rule = \"{:?}\"\n",
            b.name,
            b.recipe.source,
            b.recipe.slip.name(),
            dvw.scout[b.planted.slip].code(),
            b.recipe.slip.expected_rule(),
        );
    }
    std::fs::write(dir.join("labels.toml"), labels).expect("write labels");
    eprintln!("{} bad files in {}", bad.len(), dir.display());
}

/// One validator's issues on one file, as (`[3SCOUT]` index, key, counts
/// as a hit/noise, message). The key is the rule (Courtside) or message
/// (openvolley) used to match issues against the clean source.
type Found = Vec<(Option<usize>, String, bool, String)>;

fn ours(bytes: &[u8]) -> Found {
    let dvw = Dvw::parse(bytes).expect("parse");
    let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
    validate(&m)
        .into_iter()
        .map(|i: Issue| {
            (
                Some(i.index),
                format!("{:?}", i.rule),
                i.severity >= Severity::Warning,
                i.message,
            )
        })
        .collect()
}

#[derive(Deserialize)]
struct OracleRow {
    file_line_number: Option<usize>,
    message: String,
}

/// `dv_validate`'s issues, or `None` if the oracle hasn't been run.
fn theirs(csv_path: &Path, bytes: &[u8]) -> Option<Found> {
    let mut reader = csv::Reader::from_path(csv_path).ok()?;
    let dvw = Dvw::parse(bytes).expect("parse");
    let first = first_scout_line(&dvw);
    Some(
        reader
            .deserialize::<OracleRow>()
            .map(|r| {
                let r = r.expect("oracle row");
                let index = r
                    .file_line_number
                    .and_then(|l| l.checked_sub(first))
                    .filter(|&i| i < dvw.scout.len());
                (index, r.message.clone(), true, r.message)
            })
            .collect(),
    )
}

/// `[3SCOUT]` indices `[slip, first serve of the next rally)`.
fn window(bytes: &[u8], slip: usize) -> std::ops::Range<usize> {
    let dvw = Dvw::parse(bytes).expect("parse");
    let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
    let events = m.log.visible();
    let f = fold(&m);
    let from = f.states[slip]
        .rally
        .map_or(slip + 1, |r| f.rallies[r].last + 1);
    let end = (from..events.len())
        .find(|&i| matches!(events[i].code(), Some(Code::Touch(t)) if t.skill == Skill::Serve))
        .unwrap_or(events.len());
    slip..end.max(slip + 1)
}

#[derive(Default)]
struct Score {
    files: usize,
    caught: usize,
    noise: usize,
    rule_matched: usize,
}

/// The first hit's key (`None` if the slip was missed), and how many
/// issues are noise.
fn score(
    found: &Found,
    clean: &Found,
    planted: &Planted,
    window: &std::ops::Range<usize>,
) -> (Option<String>, usize) {
    let clean: HashSet<(Option<usize>, &str)> =
        clean.iter().map(|(i, k, _, _)| (*i, k.as_str())).collect();
    let mut caught = None;
    let mut noise = 0;
    for (index, key, counts, message) in found {
        if !counts {
            continue;
        }
        let source = match index {
            Some(i) => planted.source_line.get(*i).copied().flatten(),
            None => None,
        };
        let in_clean = match (index, source) {
            (None, _) => clean.contains(&(None, key.as_str())),
            (Some(_), Some(s)) => clean.contains(&(Some(s), key.as_str())),
            (Some(_), None) => false,
        };
        if in_clean {
            continue;
        }
        if index.is_some_and(|i| window.contains(&i)) {
            caught.get_or_insert_with(|| key.clone());
        } else {
            noise += 1;
            if std::env::var_os("COURTSIDE_BAD_DETAIL").is_some() {
                eprintln!("    noise at {index:?}: {message}");
            }
        }
    }
    (caught, noise)
}

fn csv_name(rel: &str) -> String {
    format!("{}.csv", rel.replace('/', "__"))
}

#[test]
fn validator_beats_dv_validate() {
    let Some(bad) = bad_files() else { return };
    let oracle = root().join("target/parity/oracle/validate");
    let require = std::env::var("COURTSIDE_REQUIRE_PARITY").as_deref() == Ok("1");
    let mut ours_by: BTreeMap<SlipClass, Score> = BTreeMap::new();
    let mut theirs_by: BTreeMap<SlipClass, Score> = BTreeMap::new();
    let mut have_oracle = true;
    let mut detail = String::new();
    for b in &bad {
        let source = std::fs::read(root().join("testdata").join(&b.recipe.source)).expect("read");
        if std::env::var_os("COURTSIDE_BAD_DETAIL").is_some() {
            eprintln!("{} (Courtside, then dv_validate):", b.name);
        }
        let w = window(&b.bytes, b.planted.slip);
        let found = ours(&b.bytes);
        let (hit, noise) = score(&found, &ours(&source), &b.planted, &w);
        let caught = hit.is_some();
        let expected = format!("{:?}", b.recipe.slip.expected_rule());
        let s = ours_by.entry(b.recipe.slip).or_default();
        s.files += 1;
        s.caught += usize::from(caught);
        s.noise += noise;
        s.rule_matched += usize::from(
            found
                .iter()
                .any(|(i, k, c, _)| *c && *k == expected && i.is_some_and(|i| w.contains(&i))),
        );
        let _ = write!(
            detail,
            "{:28} ours {:14} noise {noise:3}",
            b.name,
            hit.as_deref().unwrap_or("MISS")
        );

        // The oracle must have run on these exact bytes.
        let on_disk = std::fs::read(out_dir().join(format!("{}.dvw", b.name))).ok();
        let theirs_bad = theirs(
            &oracle.join("bad").join(format!("{}.dvw.csv", b.name)),
            &b.bytes,
        );
        let theirs_clean = theirs(
            &oracle.join("corpus").join(csv_name(&b.recipe.source)),
            &source,
        );
        match (
            on_disk.as_deref() == Some(&b.bytes[..]),
            theirs_bad,
            theirs_clean,
        ) {
            (true, Some(tb), Some(tc)) => {
                let (hit, noise) = score(&tb, &tc, &b.planted, &w);
                let caught = hit.is_some();
                let s = theirs_by.entry(b.recipe.slip).or_default();
                s.files += 1;
                s.caught += usize::from(caught);
                s.noise += noise;
                let _ = write!(
                    detail,
                    " | dv_validate noise {noise:3} {}",
                    hit.as_deref().unwrap_or("MISS")
                );
            }
            _ => have_oracle = false,
        }
        detail.push('\n');
    }
    eprintln!("{detail}");

    let mut table = String::from(
        "| Slip class | Files | Courtside recall | dv_validate recall | Courtside noise | dv_validate noise | Rule matched |\n|---|---|---|---|---|---|---|\n",
    );
    let total = |m: &BTreeMap<SlipClass, Score>| {
        m.values().fold((0, 0, 0), |(f, c, n), s| {
            (f + s.files, c + s.caught, n + s.noise)
        })
    };
    for (class, s) in &ours_by {
        let t = theirs_by.get(class);
        let _ = writeln!(
            table,
            "| {} | {} | {}/{} | {} | {} | {} | {}/{} |",
            class.name(),
            s.files,
            s.caught,
            s.files,
            t.map_or("–".to_owned(), |t| format!("{}/{}", t.caught, t.files)),
            s.noise,
            t.map_or("–".to_owned(), |t| t.noise.to_string()),
            s.rule_matched,
            s.files,
        );
    }
    let (of, oc, on) = total(&ours_by);
    let (tf, tc, tn) = total(&theirs_by);
    let _ = writeln!(
        table,
        "| **Total** | {of} | {oc}/{of} | {tc}/{tf} | {on} | {tn} | |"
    );
    eprintln!("{table}");

    if !have_oracle {
        assert!(
            !require,
            "dv_validate output missing or stale: run the bad_files_are_generated test, then tools/parity/run.sh validate"
        );
        eprintln!("skipping the comparison: run tools/parity/run.sh validate");
        return;
    }
    let mut failures = Vec::new();
    for (class, s) in &ours_by {
        let t = &theirs_by[class];
        if s.caught < t.caught {
            failures.push(format!(
                "{}: recall {} < dv_validate {}",
                class.name(),
                s.caught,
                t.caught
            ));
        }
    }
    if oc <= tc {
        failures.push(format!("total recall {oc} not above dv_validate {tc}"));
    }
    if on > tn {
        failures.push(format!("noise {on} above dv_validate {tn}"));
    }
    assert!(failures.is_empty(), "gate:\n{}", failures.join("\n"));
}
