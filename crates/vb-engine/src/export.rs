//! [`Match`] to `.dvw`: the `[3SCOUT]` section regenerated from the event
//! log and the fold, everything else from the file the match came from.
//!
//! Each visible event (comments aside) becomes one line. An event imported
//! from the template, or a correction of one, starts from its source line,
//! so columns the engine doesn't model (phases, coordinates, video) are
//! kept as written. Inserted events start from an empty line. The engine
//! then writes the code and the derived columns: set (8), setter positions
//! (9–10, kept as written where the fold doesn't know them) and players on
//! court (14–25).
//!
//! `[3SET]` scores (when a team first reaches 8, 16 and 21, or 5, 10 and
//! 12 in a 15-point set, and the final score), set durations, `[3TEAMS]`
//! sets won and the player tables' starting zones come from the match. A
//! field that already holds the right value is kept as written; a new one
//! follows the template's style (`8 -7` from DataVolley, ` 8- 7` from
//! VolleyStation).
//!
//! - Duration: the set end's own minutes when the scout gave them (#21);
//!   else whole minutes from the first serve to the last point code,
//!   as DataVolley counts them (FORMAT-NOTES "[3SET]"), for a set whose
//!   scores change; a set left as it was keeps the file's duration, empty
//!   or not (DataVolley doesn't always follow the scout clock).
//!   VolleyStation's durations don't follow its clock either, so they are
//!   kept as written.
//! - Sets won: rewritten only when some set's scores change.
//! - Starting zones: the team sheet's, plus `*` for a player a
//!   substitution code brings on in a set they didn't start, and for a
//!   libero who touches the ball in a set.

use dvw_codec::Dvw;
use dvw_codec::dvw::SetResult;
use dvw_codec::scout::ScoutLine;
use dvw_codec::text::Row;

use vb_grammar::{Code, Skill, Team};

use crate::event::{Body, ClockTime, EventKind, MatchEvent, Points};
use crate::fold::{Folded, Score, State, fold_events};
use crate::model::{Match, StartingZone};

/// Fields in a `[3SCOUT]` line, counting the empty one after the trailing `;`.
const WIDTH: usize = 27;

impl Match {
    /// The match as a `.dvw`, with headers from `template` (normally the
    /// file it was imported from).
    pub fn to_dvw(&self, template: &Dvw) -> Dvw {
        let events = self.log.visible();
        let folded = fold_events(self, &events);
        let mut dvw = template.clone();
        dvw.scout = events
            .iter()
            .zip(&folded.states)
            .filter_map(|(e, st)| line(e, st, template))
            .collect();
        let sets = set_scores(self, &events, &folded);
        let volleystation = template.header.get("GENERATOR-PRG") == Some("VolleyStation");
        // A file's own sets won stay as written while no set changes
        // (some leave it empty, count a golden set or write `null`).
        if write_sets(&mut dvw.sets, &sets, !volleystation) {
            write_sets_won(&mut dvw, &sets);
        }
        write_starting_zones(self, &mut dvw, &events, &folded);
        dvw
    }
}

/// What the fold says about one set.
#[derive(Debug, Default)]
struct SetScores {
    /// Score when the leader first reached each checkpoint.
    checkpoints: [Option<Score>; 3],
    last: Option<Score>,
    /// A set end code closed the set.
    ended: bool,
    target: u8,
    /// Played as a golden set.
    golden: bool,
    /// Clock of the first serve and of the last point code.
    first_serve: Option<ClockTime>,
    last_point: Option<ClockTime>,
    /// The set's length as the scout gave it on the set end.
    given: Option<u16>,
}

impl SetScores {
    /// Minutes as DataVolley writes them: rounded up, after taking off one
    /// second (a set of 20:01 is 20 minutes, 20:02 is 21).
    fn duration(&self) -> Option<u32> {
        let secs = |c: ClockTime| {
            u32::from(c.hour) * 3600 + u32::from(c.minute) * 60 + u32::from(c.second)
        };
        let (a, b) = (secs(self.first_serve?), secs(self.last_point?));
        let elapsed = b.checked_sub(a)?;
        Some(elapsed.saturating_sub(1).div_ceil(60))
    }
}

impl SetScores {
    /// The team that won the set, if it is over: closed by a set end code,
    /// or won on points.
    fn winner_is_home(&self) -> Option<bool> {
        let s = self.last?;
        let (hi, lo) = (s.home.max(s.visiting), s.home.min(s.visiting));
        let won = hi >= self.target && hi - lo >= 2;
        ((self.ended || won) && hi != lo).then_some(s.home > s.visiting)
    }
}

fn set_scores(m: &Match, events: &[&MatchEvent], folded: &Folded) -> Vec<SetScores> {
    fn get<'a>(
        m: &Match,
        folded: &Folded,
        sets: &'a mut Vec<SetScores>,
        set: u8,
    ) -> Option<&'a mut SetScores> {
        let i = usize::from(set.checked_sub(1)?);
        if sets.len() <= i {
            sets.resize_with(i + 1, SetScores::default);
        }
        let s = &mut sets[i];
        s.target = folded.set_target(m, set);
        s.golden = folded.golden_set == Some(set);
        Some(s)
    }
    let mut sets: Vec<SetScores> = Vec::new();
    for r in folded.rallies.iter().filter(|r| r.won_by.is_some()) {
        let Some(s) = get(m, folded, &mut sets, r.set) else {
            continue;
        };
        let checkpoints = if s.target == 15 {
            [5, 10, 12]
        } else {
            [8, 16, 21]
        };
        let score = r.score_after;
        for (slot, at) in s.checkpoints.iter_mut().zip(checkpoints) {
            if slot.is_none() && score.home.max(score.visiting) == at {
                *slot = Some(score);
            }
        }
        s.last = Some(score);
        s.last_point = events.get(r.last).and_then(|e| e.clock).or(s.last_point);
        if s.first_serve.is_none() {
            s.first_serve = (r.first..=r.last)
                .find(|&i| matches!(events[i].code(), Some(Code::Touch(t)) if t.skill == Skill::Serve))
                .and_then(|i| events[i].clock);
        }
    }
    for (e, st) in events.iter().zip(&folded.states) {
        if e.kind() == EventKind::SetEnd
            && let Some(s) = st
                .set
                .checked_sub(1)
                .and_then(|set| get(m, folded, &mut sets, set))
        {
            s.ended = true;
            s.given = e.minutes.or(s.given);
        }
    }
    sets
}

/// Each set's length in whole minutes, by set number from 1: the scout's
/// own on its set end, else from the clock (first serve to last point).
pub fn set_minutes(m: &Match) -> Vec<Option<u32>> {
    let events = m.log.visible();
    let folded = fold_events(m, &events);
    set_scores(m, &events, &folded)
        .iter()
        .map(|s| s.given.map(u32::from).or_else(|| s.duration()))
        .collect()
}

/// Writes the sets' scores and durations; whether any set changed.
fn write_sets(rows: &mut Vec<SetResult>, sets: &[SetScores], durations: bool) -> bool {
    // VolleyStation pads both sides (` 7- 8`), DataVolley only the home
    // score (`7 -8`).
    let both_padded = rows
        .iter()
        .flat_map(|r| r.0.0.iter().skip(1).take(4))
        .any(|f| f.starts_with(' ') || f.contains("- "));
    let format = |s: Score| {
        if both_padded {
            format!("{:>2}-{:>2}", s.home, s.visiting)
        } else {
            format!("{:<2}-{}", s.home, s.visiting)
        }
    };
    let mut changed = rows.len() < sets.len();
    while rows.len() < sets.len() {
        rows.push(SetResult(Row::parse("True;;;;;;")));
    }
    for (i, row) in rows.iter_mut().enumerate() {
        let mut row_changed = false;
        let set = sets.get(i);
        let wanted = set.map_or([None; 4], |s| {
            [s.checkpoints[0], s.checkpoints[1], s.checkpoints[2], s.last]
        });
        let fields = &mut row.0.0;
        if fields.len() < 6 {
            fields.resize(6, String::new());
        }
        for (field, want) in fields[1..5].iter_mut().zip(wanted) {
            let have = parse_score(field);
            if have != want.map(|s| (s.home, s.visiting)) {
                *field = want.map(format).unwrap_or_default();
                row_changed = true;
            }
        }
        changed |= row_changed;
        // The scout's minutes always; the clock's only where the scores
        // changed and the template counts them (not VolleyStation).
        let minutes = set.and_then(|s| {
            s.given
                .map(u32::from)
                .or_else(|| (durations && row_changed).then(|| s.duration()).flatten())
        });
        if let Some(minutes) = minutes
            && fields[5].trim().parse::<u32>().ok() != Some(minutes)
        {
            fields[5] = minutes.to_string();
            changed = true;
        }
    }
    changed
}

fn write_sets_won(dvw: &mut Dvw, sets: &[SetScores]) {
    for (team, home) in dvw.teams.iter_mut().zip([true, false]) {
        let won = sets
            .iter()
            // A golden set decides the tie, not the match: not a set won.
            .filter(|s| !s.golden && s.winner_is_home() == Some(home))
            .count();
        if team.sets_won().map(usize::from) != Some(won) {
            let fields = &mut team.0.0;
            if fields.len() < 3 {
                fields.resize(3, String::new());
            }
            fields[2] = won.to_string();
        }
    }
}

/// Starting zones as the import reads them, so a field is only rewritten
/// when its meaning changes.
fn read_zone(field: &str) -> StartingZone {
    match field.trim() {
        "*" => StartingZone::Substitute,
        z => z
            .parse::<u8>()
            .ok()
            .filter(|z| (1..=6).contains(z))
            .map_or(StartingZone::None, StartingZone::Zone),
    }
}

fn write_starting_zones(m: &Match, dvw: &mut Dvw, events: &[&MatchEvent], folded: &Folded) {
    // (team, set, player) brought on by a substitution code, or a libero
    // who touched the ball in the set (no code brings a libero on).
    let mut came_on: Vec<(Team, u8, u8)> = Vec::new();
    // The first six entered in the console per team and set, which
    // replaces the team sheet's starting zones for that set.
    let mut sixes: Vec<(Team, u8, [u8; 6])> = Vec::new();
    for (e, st) in events.iter().zip(&folded.states) {
        if let Body::StartingSix { team, court } = &e.body
            && !sixes.iter().any(|(t, s, _)| t == team && *s == st.set)
        {
            sixes.push((*team, st.set, *court));
        }
        match e.code() {
            Some(Code::Substitution { team, r#in, .. }) => {
                if let Some(n) = r#in.value().and_then(|v| u8::try_from(v).ok()) {
                    came_on.push((*team, st.set, n));
                }
            }
            Some(Code::Touch(t)) => {
                if let vb_grammar::Player::Number(d) = &t.player
                    && let Some(n) = d.value().and_then(|v| u8::try_from(v).ok())
                    && m.teams.get(t.team).is_libero(n)
                {
                    came_on.push((t.team, st.set, n));
                }
            }
            _ => {}
        }
    }
    for (team, rows) in [
        (Team::Home, &mut dvw.players_home),
        (Team::Visiting, &mut dvw.players_visiting),
    ] {
        let sheet = m.teams.get(team);
        for row in rows.iter_mut() {
            let Some(n) = row.number() else { continue };
            let zones = sheet.player(n).map(|p| p.starting_zones);
            for set in 1..=5u8 {
                let six = sixes.iter().find(|(t, s, _)| *t == team && *s == set);
                let from_sheet = match six {
                    Some((.., court)) => court
                        .iter()
                        .position(|&c| c == n)
                        .and_then(|i| u8::try_from(i + 1).ok())
                        .map_or(StartingZone::None, StartingZone::Zone),
                    None => zones.map_or(StartingZone::None, |z| z[usize::from(set) - 1]),
                };
                let want = match from_sheet {
                    StartingZone::None if came_on.contains(&(team, set, n)) => {
                        StartingZone::Substitute
                    }
                    z => z,
                };
                let field = 2 + usize::from(set);
                if read_zone(row.0.get(field)) == want {
                    continue;
                }
                let fields = &mut row.0.0;
                if fields.len() <= field {
                    fields.resize(field + 1, String::new());
                }
                fields[field] = match want {
                    StartingZone::Zone(z) => z.to_string(),
                    StartingZone::Substitute => "*".to_owned(),
                    StartingZone::None => String::new(),
                };
            }
        }
    }
}

fn parse_score(field: &str) -> Option<(u8, u8)> {
    let (home, visiting) = field.split_once('-')?;
    Some((home.trim().parse().ok()?, visiting.trim().parse().ok()?))
}

fn line(e: &MatchEvent, st: &State, template: &Dvw) -> Option<ScoutLine> {
    let Body::Code(code) = &e.body else {
        return None;
    };
    let source = e.source_line.and_then(|i| template.scout.get(i));
    let mut row = match source {
        Some(l) => l.0.0.clone(),
        None => {
            let mut row = vec![String::new(); WIDTH];
            if let Some(c) = e.clock {
                row[7] = format!("{:02}.{:02}.{:02}", c.hour, c.minute, c.second);
            }
            row
        }
    };
    if row.len() < WIDTH {
        row.resize(WIDTH, String::new());
    }
    // Video seconds (whole, as VolleyStation writes them), where the event
    // has a time the source line doesn't already say.
    if let Some(ms) = e.video_ms {
        let secs = ms / 1000;
        if source.and_then(ScoutLine::video_time) != Some(secs as f64) {
            row[12] = secs.to_string();
        }
    }
    // Court positions, where the event's differ from the source line's.
    let from_source = source.map(Points::of_line);
    if from_source.unwrap_or_default() != e.points {
        let cell = |c: Option<u16>| c.map_or_else(|| "-1-1".to_owned(), |c| format!("{c:04}"));
        if e.points.is_empty() {
            row[4..=6].iter_mut().for_each(String::clear);
        } else {
            row[4] = cell(e.points.start);
            row[5] = cell(e.points.mid);
            row[6] = cell(e.points.end);
        }
    }
    let num = |n: Option<u8>| n.map(|n| n.to_string()).unwrap_or_default();
    row[0] = code.to_string();
    row[8] = st.set.to_string();
    for (col, sp) in [
        (9, st.setter_position.home),
        (10, st.setter_position.visiting),
    ] {
        if sp.is_some() || source.is_none() {
            row[col] = num(sp);
        }
    }
    for (i, p) in st.court.home.iter().chain(&st.court.visiting).enumerate() {
        row[14 + i] = num(*p);
    }
    Some(ScoutLine(Row(row)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ClockTime, IdGen, NewEvent, Producer};
    use vb_grammar::Code;

    const FILE: &str = "[3DATAVOLLEYSCOUT]\r\nFILEFORMAT: 2.0\r\n[3PLAYERS-H]\r\n\
        0;8;1;1;;;;;h8;Placeholder;A;;;5;False;h8;;\r\n0;12;2;2;;;;;h12;Placeholder;B;;;2;False;h12;;\r\n\
        [3SCOUT]\r\n*P08>LUp;;;;;;;15.30.00;1;9;;;;;8;12;;;;;;;;;;;\r\n\
        *z1>LUp;;;;;;;15.30.01;1;1;;;;;8;12;;;;;;;;;;;\r\n\
        *08SH-;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;\r\n";

    #[test]
    fn unchanged_match_writes_the_same_file() {
        let dvw = Dvw::parse(FILE.as_bytes()).expect("parse");
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let out = m.to_dvw(&dvw).to_bytes().expect("bytes");
        // The setter position before the first z code is unknown to the
        // engine, so the file's value (9) stays.
        assert_eq!(String::from_utf8_lossy(&out), FILE);
    }

    #[test]
    fn edits_are_written_with_fresh_derived_columns() {
        let dvw = Dvw::parse(FILE.as_bytes()).expect("parse");
        let mut m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let mut ids = IdGen::new(1);
        let serve = m.log.visible()[2].id;
        m.log
            .correct(
                &mut ids,
                serve,
                NewEvent::code(Code::parse("*08SM-"), Producer::Keyboard),
            )
            .expect("correct");
        m.log.append(
            &mut ids,
            NewEvent {
                clock: Some(ClockTime {
                    hour: 15,
                    minute: 30,
                    second: 15,
                }),
                video_ms: Some(12_400),
                ..NewEvent::code(Code::parse("*c12:05"), Producer::Keyboard)
            },
        );
        let out = m.to_dvw(&dvw);
        let lines: Vec<String> = out.scout.iter().map(|l| l.0.to_line()).collect();
        assert_eq!(lines[2], "*08SM-;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;");
        assert_eq!(lines[3], "*c12:05;;;;;;;15.30.15;1;1;;;12;;8;5;;;;;;;;;;;");
    }

    #[test]
    fn video_times_set_later_are_written() {
        let dvw = Dvw::parse(SETS.as_bytes()).expect("parse");
        let mut m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let serve = m.log.visible()[2].id;
        m.log.set_video_ms(serve, Some(83_900)).expect("set");
        let out = m.to_dvw(&dvw);
        assert_eq!(
            out.scout[2].0.to_line(),
            "*08SH#;;;;;;;15.30.10;1;1;;;83;;8;12;;;;;;;;;;;"
        );
        assert_eq!(out.scout[0].0.to_line(), dvw.scout[0].0.to_line());
    }

    const SETS: &str = "[3DATAVOLLEYSCOUT]\r\nFILEFORMAT: 2.0\r\n[3SET]\r\nTrue;;;;;;\r\n\
        [3PLAYERS-H]\r\n0;8;1;1;;;;;h8;Placeholder;A;;;5;False;h8;;\r\n\
        0;12;2;2;;;;;h12;Placeholder;B;;;2;False;h12;;\r\n0;5;3;;;;;;h5;Placeholder;C;;;2;False;h5;;\r\n\
        [3SCOUT]\r\n*P08>LUp;;;;;;;15.30.00;1;1;;;;;8;12;;;;;;;;;;;\r\n\
        *z1>LUp;;;;;;;15.30.01;1;1;;;;;8;12;;;;;;;;;;;\r\n\
        *08SH#;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;\r\n";

    fn with(codes: &[(&str, (u8, u8, u8))]) -> Dvw {
        let dvw = Dvw::parse(SETS.as_bytes()).expect("parse");
        let mut m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let mut ids = IdGen::new(1);
        for (code, (hour, minute, second)) in codes {
            m.log.append(
                &mut ids,
                NewEvent {
                    clock: Some(ClockTime {
                        hour: *hour,
                        minute: *minute,
                        second: *second,
                    }),
                    ..NewEvent::code(Code::parse(code), Producer::Keyboard)
                },
            );
        }
        m.to_dvw(&dvw)
    }

    #[test]
    fn set_duration_rounds_up_after_one_second() {
        // First serve 15:30:10; 20:01 later is 20 minutes, 20:02 is 21.
        let d = with(&[("*p01:00", (15, 50, 11))]);
        assert_eq!(d.sets[0].0.to_line(), "True;;;;1 -0;20;");
        let d = with(&[("*p01:00", (15, 50, 12))]);
        assert_eq!(d.sets[0].0.to_line(), "True;;;;1 -0;21;");
    }

    /// The scout's minutes on a set end win over the clock's, even where
    /// the file had its own (#21).
    #[test]
    fn a_set_ends_own_minutes_are_written() {
        let file = SETS.replace(
            "[3SET]\r\nTrue;;;;;;\r\n",
            "[3TEAMS]\r\nT1;Home;;;;\r\nT2;Away;;;;\r\n[3SET]\r\nTrue;;;;1 -0;31;\r\n",
        )
        .replace(
            "*08SH#;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;\r\n",
            "*08SH#;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;\r\n*p01:00;;;;;;;15.50.11;1;1;;;;;8;12;;;;;;;;;;;\r\n**1set;;;;;;;15.50.20;1;1;;;;;8;12;;;;;;;;;;;\r\n",
        );
        let dvw = Dvw::parse(file.as_bytes()).expect("parse");
        let mut m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let end = m
            .log
            .all()
            .iter()
            .find(|e| e.kind() == EventKind::SetEnd)
            .expect("set end")
            .id;
        m.log.set_minutes(end, Some(27)).expect("minutes");
        assert_eq!(m.to_dvw(&dvw).sets[0].0.to_line(), "True;;;;1 -0;27;");
        assert_eq!(set_minutes(&m), [Some(27)]);
        m.log.set_minutes(end, None).expect("minutes");
        // Without them: the file's own 31 stays (its scores didn't change),
        // and the console offers the clock's 20.
        assert_eq!(m.to_dvw(&dvw).sets[0].0.to_line(), "True;;;;1 -0;31;");
        assert_eq!(set_minutes(&m), [Some(20)]);
    }

    /// A file's own duration and sets won stay while its sets don't change.
    #[test]
    fn unchanged_sets_keep_what_the_file_says() {
        let file = SETS.replace(
            "[3SET]\r\nTrue;;;;;;\r\n",
            "[3TEAMS]\r\nT1;Home;;;;\r\nT2;Away;;;;\r\n[3SET]\r\nTrue;;;;1 -0;31;\r\n",
        )
        .replace(
            "*08SH#;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;\r\n",
            "*08SH#;;;;;;;15.30.10;1;1;;;;;8;12;;;;;;;;;;;\r\n*p01:00;;;;;;;15.50.11;1;1;;;;;8;12;;;;;;;;;;;\r\n",
        );
        let dvw = Dvw::parse(file.as_bytes()).expect("parse");
        let m = Match::from_dvw(&dvw, &mut IdGen::new(0));
        let out = m.to_dvw(&dvw);
        // The clock says 20 minutes and home won the set; the file says 31
        // and leaves sets won empty.
        assert_eq!(out.sets[0].0.to_line(), "True;;;;1 -0;31;");
        assert_eq!(out.teams[0].0.to_line(), "T1;Home;;;;");
    }

    #[test]
    fn substitutes_are_marked_in_the_player_table() {
        let d = with(&[("*p01:00", (15, 31, 0)), ("*c12:05", (15, 31, 5))]);
        let row = |n: u8| {
            d.players_home
                .iter()
                .find(|p| p.number() == Some(n))
                .expect("player")
                .0
                .to_line()
        };
        assert_eq!(row(5), "0;5;3;*;;;;;h5;Placeholder;C;;;2;False;h5;;");
        assert_eq!(row(12), "0;12;2;2;;;;;h12;Placeholder;B;;;2;False;h12;;");
    }
}
